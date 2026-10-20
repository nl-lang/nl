//! The analyzer: types and lints.
//!
//! [`analyze`] walks a resolved file and produces an [`Analysis`]: the
//! type errors, warnings and lints, and the [`Hints`] the lowering
//! consumes. It runs after the resolver and only on files the
//! resolver accepted, so names are resolved and binding kinds decided.
//!
//! # How types are found
//!
//! - A declared type (an annotation on a `let`, a parameter, a field, a
//!   return type) is the binding's type. An unannotated binding
//!   that is never assigned after its declaration has the type of its
//!   initializer; one that is assigned later is `any`, with the type of
//!   the last assignment known along the flow.
//! - Narrowing: `is`, `== nil`, `!= nil`, a truthiness test,
//!   `and`, `or`, `not` and the patterns of `when` and `if let` refine the
//!   type of a local along the branch. The branches of `if` and `case`
//!   merge by union; a branch that leaves (`return`, `break`, `continue`,
//!   `raise`) drops out of the merge.
//! - Functions are analyzed in two passes: the first infers return types
//!   from bodies, the second reports, so that a call to a function
//!   declared later in the file (hoisting) has its type.
//!
//! # What is an error
//!
//! Only what the language defines as one: a value of a definite type stored where a
//! type that does not accept it is declared (`let`, assignment, an
//! argument of a known callee, a struct literal field), assignment to a
//! `fn`-initialized variable, an unknown field in a struct
//! literal, keyed arguments to a callee without `**opts`, a
//! block on a declared Lua function without a `Function` parameter,
//! an argument of `spawn` that cannot be sent, and `as` with a type
//! that cannot be converted to. Where the language is silent the program is
//! accepted, and `any` on either side accepts everything.
//!
//! # Certainty
//!
//! Every expression type carries whether the runtime guarantees it
//! ([`Ty::certain`]): a literal, a struct literal, a value checked at an
//! annotated position, a local narrowed by a test, `self`.
//! Only certain types produce hints, so that generated code with static
//! resolution gives the result of the runtime path it replaces.
//!
//! # Side tables for tools
//!
//! With [`AnalyzeOptions::record`] the reporting pass also fills
//! [`TypeInfo`]: the type of every expression and binding, the signature
//! of every function node, which nominal type each `x.member` was looked
//! up on, and where the file and the declaration files declare each
//! member. The language server reads them for
//! hover, go-to-definition and completion. Off, nothing is recorded.

mod expr;
mod lints;
mod pattern;
mod stmt;
mod typeexpr;

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use nl_syntax::{SyntaxKind, SyntaxNode, TextRange};

use crate::binding::*;
use crate::decl::{DeclId, Declarations};
use crate::hints::*;
use crate::prelude::{self, Prelude};
use crate::types::*;
use crate::util::*;
use crate::ImportProvider;

use SyntaxKind::*;

/// Options of one analysis.
pub struct AnalyzeOptions<'a> {
    /// Record the hints (proven checks, static resolution).
    /// Off, only the member keys of declared names are recorded, which the
    /// lowering needs whatever the mode.
    pub static_resolution: bool,
    /// The declaration files in scope (the built-in prelude ones
    /// included); `None` for none.
    pub decls: Option<&'a Declarations>,
    /// Report lints (naming, shadowing, unused bindings, …).
    pub lints: bool,
    /// How to read the file of `Worker.new 'literal'`.
    pub imports: Option<&'a dyn ImportProvider>,
    /// Fill [`Analysis::info`], the side tables tools read (the language
    /// server). Off for a build, so that it costs
    /// nothing there.
    pub record: bool,
}

impl Default for AnalyzeOptions<'_> {
    /// Static resolution and lints on, no declaration files, no imports,
    /// nothing recorded: `AnalyzeOptions { decls: Some(&decls), ..Default::default() }`.
    fn default() -> Self {
        AnalyzeOptions { static_resolution: true, decls: None, lints: true, imports: None, record: false }
    }
}

/// What the analyzer produced.
#[derive(Debug, Default)]
pub struct Analysis {
    pub diagnostics: Vec<SemaDiagnostic>,
    pub hints: Hints,
    /// The side tables of [`AnalyzeOptions::record`]; `None` otherwise.
    pub info: Option<TypeInfo>,
}

/// What the analyzer knows about a file, for tools (the language
/// server): the types of its expressions and bindings and
/// where the members it uses are declared. Keyed by ranges of the tree,
/// like the [`Resolution`].
#[derive(Debug, Default)]
pub struct TypeInfo {
    /// The nominal types, which render the types below (`TypeTable::show`).
    pub types: TypeTable,
    /// The type of each expression node, by its range, as the second pass
    /// saw it.
    pub expr_types: HashMap<TextRange, Type>,
    /// The type of each binding: declared, inferred from the initializer,
    /// or the signature of a function binding.
    pub binding_types: HashMap<BindingId, Type>,
    /// The signature of each function node (`FN_DECL`, `FN_EXPR`,
    /// `LAMBDA_EXPR`, `TRAILING_BLOCK`), with the inferred return type, by
    /// its range.
    pub fn_types: HashMap<TextRange, Rc<FnType>>,
    /// For each `x.member` (a `FIELD_EXPR`, or a `METHOD_CALL`, by its
    /// range) whose receiver has a nominal type: that type and the member
    /// name, whether or not the member exists.
    pub members: HashMap<TextRange, (NamedId, String)>,
    /// Where this file declares each member of its own types: the range of
    /// the declaring name of a field, variant, method or module function.
    pub member_decls: HashMap<(NamedId, String), TextRange>,
    /// The nominal type of each type-level binding of this file.
    pub named_of: HashMap<BindingId, NamedId>,
    /// The nominal type of each declared name, and of the items nested in
    /// declared modules, with the `DECL_*` node: the top-level item and the
    /// range of the node in its file.
    pub decl_nodes: HashMap<NamedId, (DeclId, TextRange)>,
    /// The nominal types of prelude names (`Number`, `String`, `Exception`,
    /// …), by name.
    pub prelude_named: HashMap<&'static str, NamedId>,
    /// Methods this file defines on `Number`, `String` and `Bool`.
    pub prim_methods: HashMap<&'static str, HashMap<String, Rc<FnType>>>,
}

impl Analysis {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.error)
    }
}

/// Analyzes a resolved file.
pub fn analyze(root: &SyntaxNode, res: &Resolution, opts: &AnalyzeOptions<'_>) -> Analysis {
    // Without declaration files the analyzer reads an empty table, which
    // allocates nothing.
    let none = Declarations::default();
    let mut a = Analyzer::new(res, opts, opts.decls.unwrap_or(&none));
    a.collect(root);
    // Pass 1: infer; pass 2: report.
    a.pass = 1;
    a.run(root);
    a.diags.clear();
    a.hints = Hints::default();
    a.env.clear();
    a.pass = 2;
    a.run(root);
    if opts.lints {
        a.lints(root);
    }
    let info = opts.record.then(|| a.type_info(root));
    let mut diagnostics = a.diags;
    diagnostics.sort_by_key(|d| (!d.error, d.range.start()));
    diagnostics.dedup();
    Analysis { diagnostics, hints: a.hints, info }
}

/// A type with whether the runtime guarantees it (module docs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Ty {
    pub t: Type,
    pub certain: bool,
}

impl Ty {
    pub(crate) fn new(t: Type, certain: bool) -> Ty {
        Ty { t, certain }
    }
    pub(crate) fn certain(t: Type) -> Ty {
        Ty { t, certain: true }
    }
    pub(crate) fn uncertain(t: Type) -> Ty {
        Ty { t, certain: false }
    }
    pub(crate) fn any() -> Ty {
        Ty { t: Type::Any, certain: false }
    }
}

/// The flow types of the bindings along one path.
pub(crate) type Env = HashMap<BindingId, Type>;

/// One function being analyzed.
pub(crate) struct FnCtx {
    /// The node of the function (`FN_DECL`, `FN_EXPR`, `LAMBDA_EXPR`,
    /// `TRAILING_BLOCK`, `ON_EXPR`, `SPAWN_EXPR`); `None` for the file's main
    /// function.
    pub node: Option<TextRange>,
    pub declared_ret: Option<Type>,
    /// `: nil`: the tail value is discarded.
    pub nil_ret: bool,
    /// The types of every `return` and of the tail, for inference.
    pub returns: Vec<Type>,
    /// The type of `self` in a method.
    pub self_type: Option<Type>,
    /// A `spawn do` block.
    pub spawn_block: bool,
    pub loop_depth: u32,
}

/// The analyzer's state.
pub(crate) struct Analyzer<'a> {
    pub(crate) res: &'a Resolution,
    pub(crate) opts: &'a AnalyzeOptions<'a>,
    /// `opts.decls`, or an empty table.
    pub(crate) decls: &'a Declarations,
    pub(crate) types: TypeTable,
    /// The nominal type of each type-level binding of this file.
    pub(crate) named_of: HashMap<BindingId, NamedId>,
    /// The nominal types of prelude names (`Exception`, the library
    /// modules), by name.
    pub(crate) prelude_named: HashMap<&'static str, NamedId>,
    /// The nominal types of declared names.
    pub(crate) decl_named: HashMap<DeclId, NamedId>,
    /// Methods defined on `Number`, `String` and `Bool` in this file.
    pub(crate) prim_methods: HashMap<&'static str, HashMap<String, Rc<FnType>>>,
    /// The signature of each function node, by the node's range, with the
    /// declared return type (or `any` before inference).
    pub(crate) fn_sigs: HashMap<TextRange, Rc<FnType>>,
    /// The signature of each `fn` binding and `fn`-initialized binding.
    pub(crate) binding_sigs: HashMap<BindingId, Rc<FnType>>,
    /// The return type inferred for each function node in pass 1.
    pub(crate) inferred_ret: HashMap<TextRange, Type>,
    /// The declared (or initializer-inferred) type of each binding.
    pub(crate) binding_types: HashMap<BindingId, Ty>,
    /// Bindings assigned somewhere after their declaration.
    pub(crate) assigned: HashSet<BindingId>,
    /// `**opts` parameters: tables of options passed by name, whose
    /// members are their keys, never functions of `Table`.
    pub(crate) keyed_params: HashSet<BindingId>,
    /// The `FIELD_EXPR` target of a compound assignment while its current
    /// value is typed: a place, so `t.count += 1` reads the key `count`,
    /// never `Table.count`.
    pub(crate) place_read: Option<TextRange>,
    /// The function node each binding belongs to.
    pub(crate) binding_fn: HashMap<BindingId, Option<TextRange>>,
    pub(crate) env: Env,
    pub(crate) fns: Vec<FnCtx>,
    /// The current statement leaves its block (`return`, `break`, …).
    pub(crate) diverges: bool,
    pub(crate) diags: Vec<SemaDiagnostic>,
    pub(crate) hints: Hints,
    pub(crate) pass: u8,
    /// Bindings referenced as values (for the unused-binding lint).
    pub(crate) used: HashSet<BindingId>,
    /// The file has top-level statements other than declarations
    /// (a worker that loads it repeats them).
    pub(crate) top_level_effects: Option<TextRange>,
    /// The module each module function belongs to.
    pub(crate) module_of_binding: HashMap<BindingId, NamedId>,
    /// The nominal types of the nested declarations of declared modules,
    /// by node range.
    pub(crate) decl_nested: HashMap<TextRange, NamedId>,
    /// Methods that replace one of the same name, reported
    /// in pass 2.
    pub(crate) pending_replacements: Vec<(TextRange, String, String)>,
    /// The signatures of top-level `declare fn`s, by name.
    pub(crate) decl_fn_sigs: HashMap<String, Rc<FnType>>,
    /// The types of top-level `declare let`/`const`, by name.
    pub(crate) decl_value_types: HashMap<String, Type>,
    /// The Lua key of each declared member, by owner and name.
    pub(crate) decl_keys: typeexpr::DeclKeys,
    /// The operators declared on Lua classes.
    pub(crate) decl_operators: HashMap<NamedId, HashMap<String, Rc<FnType>>>,
    /// The side tables of [`TypeInfo`], filled in pass 2 when
    /// [`AnalyzeOptions::record`] asks for them.
    pub(crate) expr_types: HashMap<TextRange, Type>,
    pub(crate) members: HashMap<TextRange, (NamedId, String)>,
    pub(crate) member_decls: HashMap<(NamedId, String), TextRange>,
    pub(crate) decl_nodes: HashMap<NamedId, (DeclId, TextRange)>,
}

impl<'a> Analyzer<'a> {
    fn new(res: &'a Resolution, opts: &'a AnalyzeOptions<'a>, decls: &'a Declarations) -> Self {
        Analyzer {
            res,
            opts,
            decls,
            types: TypeTable::default(),
            named_of: HashMap::new(),
            prelude_named: HashMap::new(),
            decl_named: HashMap::new(),
            prim_methods: HashMap::new(),
            fn_sigs: HashMap::new(),
            binding_sigs: HashMap::new(),
            inferred_ret: HashMap::new(),
            binding_types: HashMap::new(),
            assigned: HashSet::new(),
            keyed_params: HashSet::new(),
            place_read: None,
            binding_fn: HashMap::new(),
            env: HashMap::new(),
            fns: Vec::new(),
            diverges: false,
            diags: Vec::new(),
            hints: Hints::default(),
            pass: 0,
            used: HashSet::new(),
            top_level_effects: None,
            module_of_binding: HashMap::new(),
            decl_nested: HashMap::new(),
            pending_replacements: Vec::new(),
            decl_fn_sigs: HashMap::new(),
            decl_value_types: HashMap::new(),
            decl_keys: HashMap::new(),
            decl_operators: HashMap::new(),
            expr_types: HashMap::new(),
            members: HashMap::new(),
            member_decls: HashMap::new(),
            decl_nodes: HashMap::new(),
        }
    }

    // ---- the side tables for tools ------------------------------------------------

    /// True while the side tables are being filled: the reporting pass of
    /// an analysis that asked for them.
    pub(crate) fn recording(&self) -> bool {
        self.opts.record && self.pass == 2
    }

    /// Records the type of an expression node (pass 2 only).
    pub(crate) fn record_expr(&mut self, range: TextRange, ty: &Type) {
        if self.recording() {
            self.expr_types.insert(range, ty.clone());
        }
    }

    /// Records that `x.name` at `at` looks `name` up on the nominal type
    /// `owner` (pass 2 only).
    pub(crate) fn record_member(&mut self, at: TextRange, owner: NamedId, name: &str) {
        if self.recording() && !at.is_empty() {
            self.members.insert(at, (owner, name.to_string()));
        }
    }

    /// Records where this file declares a member of one of its types.
    /// Declarations are collected before the passes, so this is kept
    /// whatever the pass and only handed out when asked for.
    pub(crate) fn record_member_decl(&mut self, owner: NamedId, name: &str, range: TextRange) {
        if self.opts.record {
            self.member_decls.insert((owner, name.to_string()), range);
        }
    }

    /// The side tables, after both passes.
    fn type_info(&mut self, root: &SyntaxNode) -> TypeInfo {
        let mut binding_types = HashMap::new();
        for i in 0..self.res.bindings.len() {
            let id = BindingId(u32::try_from(i).unwrap_or(u32::MAX));
            let t = match self.binding_fn_type(id) {
                Some(sig) => Type::Fn(sig),
                None => self.binding_types.get(&id).map(|t| t.t.clone()).unwrap_or(Type::Any),
            };
            binding_types.insert(id, t);
        }
        let mut fn_types = HashMap::new();
        for n in root.descendants().filter(|n| self.fn_sigs.contains_key(&n.text_range())) {
            fn_types.insert(n.text_range(), self.fn_type(&n));
        }
        TypeInfo {
            types: std::mem::take(&mut self.types),
            expr_types: std::mem::take(&mut self.expr_types),
            binding_types,
            fn_types,
            members: std::mem::take(&mut self.members),
            member_decls: std::mem::take(&mut self.member_decls),
            named_of: std::mem::take(&mut self.named_of),
            decl_nodes: std::mem::take(&mut self.decl_nodes),
            prelude_named: std::mem::take(&mut self.prelude_named),
            prim_methods: std::mem::take(&mut self.prim_methods),
        }
    }

    // ---- diagnostics -----------------------------------------------------------

    pub(crate) fn report(&mut self, range: TextRange, code: &'static str, message: impl Into<String>, error: bool, lint: bool) {
        if self.pass != 2 {
            return;
        }
        self.diags.push(SemaDiagnostic { range, code, message: message.into(), error, lint });
    }

    pub(crate) fn error(&mut self, range: TextRange, code: &'static str, message: impl Into<String>) {
        self.report(range, code, message, true, false);
    }

    pub(crate) fn warn(&mut self, range: TextRange, code: &'static str, message: impl Into<String>) {
        self.report(range, code, message, false, false);
    }

    pub(crate) fn lint(&mut self, range: TextRange, code: &'static str, message: impl Into<String>) {
        if self.opts.lints {
            self.report(range, code, message, false, true);
        }
    }

    pub(crate) fn show(&self, t: &Type) -> String {
        self.types.show(t)
    }

    // ---- bindings ---------------------------------------------------------------

    pub(crate) fn binding(&self, id: BindingId) -> Option<&Binding> {
        self.res.binding(id)
    }

    /// The flow type of a binding: narrowed along the path, else declared.
    pub(crate) fn type_of_binding(&self, id: BindingId) -> Ty {
        if let Some(t) = self.env.get(&id) {
            return Ty::new(t.clone(), self.narrowing_is_certain(id));
        }
        self.binding_types.get(&id).cloned().unwrap_or_else(Ty::any)
    }

    /// A narrowed type of a binding is certain when the binding belongs to
    /// the current function and no other function assigns it: only this
    /// function's own statements, which the flow analysis saw, can change
    /// it between the test and the use.
    /// A `global` is never certain: Lua code can assign it at any call.
    pub(crate) fn narrowing_is_certain(&self, id: BindingId) -> bool {
        let current = self.fns.last().and_then(|f| f.node);
        self.binding_fn.get(&id).copied().flatten() == current
            && !self.res.assigned_in_closure(id)
            && self.binding(id).is_none_or(|b| b.decl != DeclKind::Global)
    }

    /// Records the declared type of a binding.
    pub(crate) fn declare_type(&mut self, id: BindingId, ty: Ty) {
        self.binding_types.insert(id, ty);
    }

    /// Sets the flow type after an assignment or a narrowing.
    pub(crate) fn set_flow(&mut self, id: BindingId, t: Type) {
        self.env.insert(id, t);
    }

    pub(crate) fn current_fn(&mut self) -> &mut FnCtx {
        if self.fns.is_empty() {
            self.fns.push(FnCtx { node: None, declared_ret: None, nil_ret: false, returns: Vec::new(), self_type: None, spawn_block: false, loop_depth: 0 });
        }
        let last = self.fns.len() - 1;
        // The vector is not empty here.
        self.fns.get_mut(last).unwrap_or_else(|| unreachable!("a function context was just pushed"))
    }

    pub(crate) fn self_type(&self) -> Option<Type> {
        self.fns.iter().rev().find_map(|f| f.self_type.clone())
    }

    // ---- the nominal types of the file -------------------------------------------

    /// Creates the nominal types of the prelude, the declaration files and
    /// the file's own declarations, with their members, and the signatures
    /// of the file's functions. Runs once, before both passes.
    fn collect(&mut self, root: &SyntaxNode) {
        self.collect_prelude();
        self.collect_decls();
        self.collect_binding_fns(root);
        self.collect_assigned(root);
        self.find_top_level_effects(root);
        // Type-level declarations first (a field may name any of them).
        for n in root.descendants() {
            match n.kind() {
                STRUCT_DECL | ENUM_DECL | INTERFACE_DECL | TYPE_DECL => {
                    let Some(name) = child(&n, NAME) else { continue };
                    let Some(id) = self.res.declaration(name.text_range()) else { continue };
                    let text = name_text(&name).unwrap_or_default();
                    let kind = match n.kind() {
                        STRUCT_DECL => NamedKind::Struct { parent: None, fields: Vec::new(), methods: HashMap::new(), operators: HashMap::new(), interfaces: Vec::new(), type_params: Vec::new(), to: Vec::new(), from: Vec::new() },
                        ENUM_DECL => NamedKind::Enum { variants: Vec::new() },
                        INTERFACE_DECL => NamedKind::Interface { methods: HashMap::new(), fields: Vec::new(), type_params: Vec::new() },
                        _ => NamedKind::Alias(Type::Any),
                    };
                    let nid = self.types.add(&text, kind);
                    self.named_of.insert(id, nid);
                }
                MODULE_DECL => {
                    let Some((id, _reopen)) = self.res.module(n.text_range()) else { continue };
                    if self.named_of.contains_key(&id) {
                        continue;
                    }
                    let text = self.binding(id).map(|b| b.name.clone()).unwrap_or_default();
                    let nid = self.types.add(&text, NamedKind::Module { members: HashMap::new() });
                    self.named_of.insert(id, nid);
                }
                IMPORT_DECL => {
                    for name in n.descendants().filter(|d| d.kind() == NAME) {
                        let Some(id) = self.res.declaration(name.text_range()) else { continue };
                        let Some(b) = self.binding(id).cloned() else { continue };
                        if b.kind == BindingKind::TypeLevel {
                            let kind = match self.res.declared_as(id) {
                                Some(DeclKind::Module | DeclKind::Import) => NamedKind::Module { members: HashMap::new() },
                                _ => NamedKind::Opaque,
                            };
                            let nid = self.types.add(&b.name, kind);
                            self.named_of.insert(id, nid);
                        }
                    }
                }
                _ => {}
            }
        }
        // Type parameters are erased: their bindings have no nominal type.
        // Fill the declarations in.
        for n in root.descendants() {
            match n.kind() {
                STRUCT_DECL => self.collect_struct(&n),
                ENUM_DECL => self.collect_enum(&n),
                INTERFACE_DECL => self.collect_interface(&n),
                TYPE_DECL => self.collect_alias(&n),
                _ => {}
            }
        }
        // Functions: signatures from annotations.
        for n in root.descendants() {
            match n.kind() {
                FN_DECL => self.collect_fn_decl(&n),
                INTERFACE_IMPL => self.collect_impl(&n),
                LET_DECL | CONST_DECL => self.collect_fn_lets(&n),
                _ => {}
            }
        }
    }

    fn collect_prelude(&mut self) {
        let exception = self.types.add("Exception", NamedKind::Struct {
            parent: None,
            fields: vec![
                Field { name: "message".into(), ty: Type::String, has_default: true },
                Field { name: "trace".into(), ty: optional(&self.types, &Type::String), has_default: true },
            ],
            methods: HashMap::new(),
            operators: HashMap::new(),
            interfaces: Vec::new(),
            type_params: Vec::new(),
            to: Vec::new(),
            from: Vec::new(),
        });
        self.prelude_named.insert("Exception", exception);
        for name in ["LuaError", "TypeError", "MatchError", "ConversionError", "RegexError", "EncodingError", "KeyError", "SendError", "WorkerError"] {
            let fields = if name == "LuaError" { vec![Field { name: "value".into(), ty: Type::Any, has_default: true }] } else { Vec::new() };
            let id = self.types.add(name, NamedKind::Struct { parent: Some(exception), fields, methods: HashMap::new(), operators: HashMap::new(), interfaces: Vec::new(), type_params: Vec::new(), to: Vec::new(), from: Vec::new() });
            self.prelude_named.insert(name, id);
        }
        let to = self.types.add("To", NamedKind::Interface { methods: HashMap::new(), fields: Vec::new(), type_params: vec!["T".into()] });
        self.prelude_named.insert("To", to);
        let from = self.types.add("From", NamedKind::Interface { methods: HashMap::new(), fields: Vec::new(), type_params: vec!["T".into()] });
        self.prelude_named.insert("From", from);
        // The library tables and the prelude functions: their members
        // come from the built-in declaration files.
        for name in ["Number", "String", "Table", "IO", "OS", "Coroutine", "Debug", "Bit", "Package", "Jit", "Event", "Worker", "Task", "Regex", "Bool"] {
            let id = self.types.add(name, NamedKind::Module { members: HashMap::new() });
            self.prelude_named.insert(name, id);
        }
        // Library tables whose values are instances over tables, checked
        // by their metatable (runtime/nl_86_time.lua): Lua classes, so that
        // `Time.at(…)` is a function of the library and `t.year` a method
        // of the instance, both from decl/nl.d.nl.
        for name in ["Time", "Duration"] {
            let id = self.types.add(name, NamedKind::LuaClass { fields: Vec::new(), members: HashMap::new() });
            self.prelude_named.insert(name, id);
        }
    }

    /// The nominal types of the declaration files, then their members.
    fn collect_decls(&mut self) {
        let decls = self.decls;
        for (i, item) in decls.items.iter().enumerate() {
            let id = DeclId(u32::try_from(i).unwrap_or(u32::MAX));
            // A declared module with the name of a prelude library fills
            // that library's members (decl/).
            if let Some(&pid) = self.prelude_named.get(item.name.as_str()) {
                self.decl_named.insert(id, pid);
                if self.opts.record {
                    self.decl_nodes.insert(pid, (id, item.node.text_range()));
                }
                continue;
            }
            let kind = match item.kind {
                crate::decl::DeclItemKind::Module => NamedKind::Module { members: HashMap::new() },
                crate::decl::DeclItemKind::Struct => NamedKind::LuaClass { fields: Vec::new(), members: HashMap::new() },
                crate::decl::DeclItemKind::HostType => NamedKind::HostType { parent: None, members: HashMap::new() },
                _ => continue,
            };
            let nid = self.types.add(&item.name, kind);
            self.decl_named.insert(id, nid);
            if self.opts.record {
                self.decl_nodes.insert(nid, (id, item.node.text_range()));
            }
            self.collect_decl_nested(&item.node, nid, id);
        }
        for (i, item) in decls.items.iter().enumerate() {
            let id = DeclId(u32::try_from(i).unwrap_or(u32::MAX));
            let node = item.node.clone();
            match self.decl_named.get(&id) {
                Some(&nid) => self.fill_decl_members(&node, nid),
                // A `declare fn`, `let` or `const`: no nominal type.
                None => self.fill_decl_members(&node, NamedId(u32::MAX)),
            }
        }
    }

    /// Nested modules, structs and types of a declared module get nominal
    /// types of their own; they are members of the parent. `top` is the
    /// declared item they are nested in.
    fn collect_decl_nested(&mut self, node: &SyntaxNode, parent: NamedId, top: DeclId) {
        for c in node.children() {
            let kind = match c.kind() {
                DECL_MODULE => NamedKind::Module { members: HashMap::new() },
                DECL_STRUCT => NamedKind::LuaClass { fields: Vec::new(), members: HashMap::new() },
                DECL_TYPE => NamedKind::HostType { parent: None, members: HashMap::new() },
                _ => continue,
            };
            let name = child(&c, NAME).and_then(|n| name_text(&n)).unwrap_or_default();
            let nid = self.types.add(&name, kind);
            self.decl_nested.insert(c.text_range(), nid);
            if self.opts.record {
                self.decl_nodes.insert(nid, (top, c.text_range()));
            }
            let member = Member::Value(Type::Static(nid));
            self.add_member(parent, &name, member);
            self.collect_decl_nested(&c, nid, top);
        }
    }

    pub(crate) fn add_member(&mut self, owner: NamedId, name: &str, member: Member) {
        if let Some(n) = self.types.get_mut(owner) {
            match &mut n.kind {
                NamedKind::Module { members } | NamedKind::HostType { members, .. } | NamedKind::LuaClass { members, .. } => {
                    members.insert(name.to_string(), member);
                }
                NamedKind::Struct { methods, .. } | NamedKind::Interface { methods, .. } => {
                    if let Member::Function(f) = member {
                        methods.insert(name.to_string(), f);
                    }
                }
                _ => {}
            }
        }
    }

    /// For each declaring name, the function node it belongs to.
    fn collect_binding_fns(&mut self, root: &SyntaxNode) {
        for n in root.descendants() {
            let Some(id) = self.res.declaration(n.text_range()) else { continue };
            let owner = n.ancestors().skip(1).find(|a| is_function_node(a.kind()) && (a.kind() != SPAWN_EXPR || has_token(a, DO_KW))).map(|a| a.text_range());
            self.binding_fn.insert(id, owner);
        }
        // Hidden `self` bindings belong to their method.
        for n in root.descendants() {
            if let Some(id) = self.res.method_self(n.text_range()) {
                self.binding_fn.insert(id, Some(n.text_range()));
            }
        }
    }

    /// The bindings assigned after their declaration (so that their
    /// initializer does not give their type).
    fn collect_assigned(&mut self, root: &SyntaxNode) {
        for n in root.descendants() {
            let targets: Vec<SyntaxNode> = match n.kind() {
                ASSIGN_STMT | COMPOUND_ASSIGN_STMT | MULTI_ASSIGN_STMT => match child(&n, TARGET_LIST) {
                    Some(list) => expr_children(&list).collect(),
                    None => expr_children(&n).take(1).collect(),
                },
                _ => continue,
            };
            for t in targets {
                if t.kind() == NAME_REF {
                    if let Some(Ref::Binding(id)) = self.res.reference(t.text_range()) {
                        self.assigned.insert(*id);
                    }
                }
            }
        }
    }

    fn collect_struct(&mut self, node: &SyntaxNode) {
        let Some(name) = child(node, NAME) else { return };
        let Some(id) = self.res.declaration(name.text_range()) else { return };
        let Some(&nid) = self.named_of.get(&id) else { return };
        let parent = child(node, STRUCT_PARENT).and_then(|p| type_child(&p)).and_then(|t| match self.type_of(&t) {
            Type::Named(p) => Some(p),
            _ => None,
        });
        let type_params: Vec<String> = child(node, TYPE_PARAMS).map(|tp| children(&tp, TYPE_PARAM).filter_map(|p| child(&p, NAME)).filter_map(|n| name_text(&n)).collect()).unwrap_or_default();
        let mut fields = Vec::new();
        for f in children(node, FIELD_DECL) {
            let Some(fname_node) = child(&f, NAME) else { continue };
            let Some(fname) = name_text(&fname_node) else { continue };
            let ty = type_child(&f).map(|t| self.type_of(&t)).unwrap_or(Type::Any);
            self.record_member_decl(nid, &fname, fname_node.text_range());
            fields.push(Field { name: fname, ty, has_default: expr_child(&f).is_some() });
        }
        if let Some(n) = self.types.get_mut(nid) {
            if let NamedKind::Struct { parent: p, fields: fs, type_params: tps, .. } = &mut n.kind {
                *p = parent;
                *fs = fields;
                *tps = type_params;
            }
        }
    }

    fn collect_enum(&mut self, node: &SyntaxNode) {
        let Some(name) = child(node, NAME) else { return };
        let Some(id) = self.res.declaration(name.text_range()) else { return };
        let Some(&nid) = self.named_of.get(&id) else { return };
        let mut variants = Vec::new();
        for v in children(node, VARIANT) {
            let Some(vname_node) = child(&v, NAME) else { continue };
            let Some(vname) = name_text(&vname_node) else { continue };
            self.record_member_decl(nid, &vname, vname_node.text_range());
            let ty = match expr_child(&v) {
                Some(e) => {
                    let lit = if e.kind() == PREFIX_EXPR { expr_child(&e) } else { Some(e) };
                    match lit.and_then(|l| first_token(&l)).map(|t| t.kind()) {
                        Some(NUMBER) => Type::Number,
                        _ => Type::String,
                    }
                }
                None => Type::String,
            };
            variants.push((vname, ty));
        }
        if let Some(n) = self.types.get_mut(nid) {
            if let NamedKind::Enum { variants: vs } = &mut n.kind {
                *vs = variants;
            }
        }
        // An enum of a module body is a member of the module: `M.Inner.X`.
        let module = node.parent().and_then(|b| b.parent()).filter(|m| m.kind() == MODULE_DECL);
        if let Some(m) = module.and_then(|m| self.res.module(m.text_range())).and_then(|(mid, _)| self.named_of.get(&mid).copied()) {
            let text = name_text(&name).unwrap_or_default();
            self.add_member(m, &text, Member::Value(Type::Static(nid)));
        }
    }

    fn collect_interface(&mut self, node: &SyntaxNode) {
        let Some(name) = child(node, NAME) else { return };
        let Some(id) = self.res.declaration(name.text_range()) else { return };
        let Some(&nid) = self.named_of.get(&id) else { return };
        let type_params: Vec<String> = child(node, TYPE_PARAMS).map(|tp| children(&tp, TYPE_PARAM).filter_map(|p| child(&p, NAME)).filter_map(|n| name_text(&n)).collect()).unwrap_or_default();
        let mut methods = HashMap::new();
        let mut fields = Vec::new();
        for m in node.children() {
            match m.kind() {
                IFACE_METHOD => {
                    let Some(mname_node) = child(&m, NAME) else { continue };
                    let Some(mname) = name_text(&mname_node) else { continue };
                    let sig = self.signature_of(&m, true, true, Some(Type::Named(nid)));
                    self.record_member_decl(nid, &mname, mname_node.text_range());
                    methods.insert(mname, Rc::new(sig));
                }
                IFACE_FIELD => {
                    let Some(fname_node) = child(&m, NAME) else { continue };
                    let Some(fname) = name_text(&fname_node) else { continue };
                    let ty = type_child(&m).map(|t| self.type_of(&t)).unwrap_or(Type::Any);
                    self.record_member_decl(nid, &fname, fname_node.text_range());
                    fields.push(Field { name: fname, ty, has_default: false });
                }
                _ => {}
            }
        }
        if let Some(n) = self.types.get_mut(nid) {
            if let NamedKind::Interface { methods: ms, fields: fs, type_params: tps } = &mut n.kind {
                *ms = methods;
                *fs = fields;
                *tps = type_params;
            }
        }
    }

    fn collect_alias(&mut self, node: &SyntaxNode) {
        let Some(name) = child(node, NAME) else { return };
        let Some(id) = self.res.declaration(name.text_range()) else { return };
        let Some(&nid) = self.named_of.get(&id) else { return };
        let ty = type_child(node).map(|t| self.type_of(&t)).unwrap_or(Type::Any);
        if let Some(n) = self.types.get_mut(nid) {
            n.kind = NamedKind::Alias(ty);
        }
    }

    /// The signature of a function-like node from its annotations:
    /// `PARAM_LIST`/`BLOCK_PARAMS`, `REST_PARAM`, `KEYED_PARAM`,
    /// `RETURN_TYPE`. `self_ty` is the type `Self` stands for.
    pub(crate) fn signature_of(&mut self, node: &SyntaxNode, is_method: bool, named: bool, self_ty: Option<Type>) -> FnType {
        let mut params = Vec::new();
        let mut rest = None;
        let mut keyed = None;
        let list = child(node, PARAM_LIST).or_else(|| child(node, BLOCK_PARAMS));
        if let Some(list) = &list {
            for p in list.children() {
                match p.kind() {
                    PARAM => {
                        let optional = expr_child(&p).is_some();
                        let (name, ty) = match pattern_children(&p).next() {
                            Some(pat) => self.param_pattern_type(&pat),
                            None => (String::new(), Type::Any),
                        };
                        params.push(Param { name, ty, optional });
                    }
                    SELF_PARAM => {}
                    REST_PARAM => {
                        rest = Some(type_child(&p).map(|t| self.type_of(&t)).unwrap_or(Type::Any));
                    }
                    KEYED_PARAM => {
                        keyed = Some(type_child(&p).map(|t| self.type_of(&t)));
                    }
                    _ => {}
                }
            }
        }
        let is_async = has_token(node, ASYNC_KW);
        let (ret, nil_ret) = match child(node, RETURN_TYPE).and_then(|r| type_child(&r)) {
            Some(t) => {
                let ty = self.type_of_with_self(&t, self_ty.clone());
                (ty.clone(), ty == Type::Nil)
            }
            None => (Type::Any, false),
        };
        let _ = nil_ret;
        FnType { params, rest, keyed, ret, is_method, is_async, named, declared: false }
    }

    /// The name and declared type of a parameter pattern: `name`, `name:
    /// T`, a destructuring pattern (unnamed, `any`: its parts are checked,
    /// not the whole), `{…} as name: T`.
    fn param_pattern_type(&mut self, pat: &SyntaxNode) -> (String, Type) {
        match pat.kind() {
            BIND_PAT => (child(pat, NAME).and_then(|n| name_text(&n)).unwrap_or_default(), Type::Any),
            TYPED_PAT => {
                let ty = type_child(pat).map(|t| self.type_of(&t)).unwrap_or(Type::Any);
                let inner = pattern_children(pat).next();
                let name = match &inner {
                    Some(i) if i.kind() == BIND_PAT => child(i, NAME).and_then(|n| name_text(&n)).unwrap_or_default(),
                    Some(i) if i.kind() == AS_PAT => child(i, NAME).and_then(|n| name_text(&n)).unwrap_or_default(),
                    _ => String::new(),
                };
                (name, ty)
            }
            AS_PAT => (child(pat, NAME).and_then(|n| name_text(&n)).unwrap_or_default(), Type::Any),
            _ => (String::new(), Type::Any),
        }
    }

    fn collect_fn_decl(&mut self, node: &SyntaxNode) {
        let in_impl = node.parent().is_some_and(|p| p.kind() == INTERFACE_IMPL);
        if in_impl {
            return;
        }
        if let Some(owner) = child(node, QUAL_NAME) {
            // `fn Owner.name`: a method, or an operator.
            let name = child(node, NAME).and_then(|n| name_text(&n));
            let op = child(node, OP_NAME).map(|o| o.text().to_string().split_whitespace().collect::<String>());
            let owner_ty = self.method_owner_type(node, &owner);
            let sig = self.signature_of(node, true, true, owner_ty.clone());
            let sig = Rc::new(sig);
            self.fn_sigs.insert(node.text_range(), sig.clone());
            let name_range = child(node, NAME).map(|n| n.text_range()).unwrap_or(node.text_range());
            match (owner_ty, name, op) {
                (Some(Type::Named(nid)), Some(name), _) => {
                    if self.own_method(nid, &name) {
                        // The second definition replaces the first.
                        self.pending_replacements.push((name_range, name.clone(), self.types.name(nid).to_string()));
                    }
                    self.record_member_decl(nid, &name, name_range);
                    self.add_member(nid, &name, Member::Function(sig));
                }
                (Some(Type::Named(nid)), None, Some(op)) => {
                    if let Some(n) = self.types.get_mut(nid) {
                        if let NamedKind::Struct { operators, .. } = &mut n.kind {
                            operators.insert(op, sig);
                        }
                    }
                }
                (Some(Type::Static(nid)), Some(name), _) => {
                    self.record_member_decl(nid, &name, name_range);
                    self.add_member(nid, &name, Member::Function(sig));
                }
                (Some(prim @ (Type::Number | Type::String | Type::Bool | Type::Table)), name, op) => {
                    let key = match prim {
                        Type::Number => "Number",
                        Type::String => "String",
                        Type::Table => "Table",
                        _ => "Bool",
                    };
                    let member = name.or(op).unwrap_or_default();
                    // For tools, a primitive's methods belong to its library
                    // table (`Number` is `math`), where they are looked up.
                    if let Some(lib) = self.prelude_type_id(key) {
                        self.record_member_decl(lib, &member, name_range);
                    }
                    self.prim_methods.entry(key).or_default().insert(member, sig);
                }
                _ => {}
            }
            return;
        }
        let Some(name) = child(node, NAME) else { return };
        let Some(id) = self.res.declaration(name.text_range()) else { return };
        let b = self.binding(id).cloned();
        let module_owner = b.as_ref().and_then(|b| b.module_owner);
        let self_ty = module_owner.and_then(|m| self.named_of.get(&m).copied()).map(Type::Static);
        let is_method = module_owner.is_some();
        let named = b.as_ref().is_some_and(|b| b.named);
        let sig = Rc::new(self.signature_of(node, is_method, named, self_ty));
        self.fn_sigs.insert(node.text_range(), sig.clone());
        self.binding_sigs.insert(id, sig.clone());
        if let Some(m) = module_owner.and_then(|m| self.named_of.get(&m).copied()) {
            self.module_of_binding.insert(id, m);
            let text = name_text(&name).unwrap_or_default();
            self.record_member_decl(m, &text, name.text_range());
            self.add_member(m, &text, Member::Function(sig));
        }
    }

    /// True when the struct itself (not a parent) defines the method.
    fn own_method(&self, nid: NamedId, name: &str) -> bool {
        matches!(self.types.kind(nid), Some(NamedKind::Struct { methods, .. }) if methods.contains_key(name))
    }

    /// The type `Self` stands for in `fn Owner.name`, as a `Named` for a
    /// struct, a `Static` for a module, or a primitive.
    pub(crate) fn method_owner_type(&self, node: &SyntaxNode, owner: &SyntaxNode) -> Option<Type> {
        match self.res.method_owner(node.text_range())? {
            MethodOwner::Binding { id, path } => {
                let nid = *self.named_of.get(id)?;
                if path.is_empty() {
                    return Some(match self.res.declared_as(*id) {
                        Some(DeclKind::Struct) => Type::Named(nid),
                        _ => Type::Static(nid),
                    });
                }
                // A nested module: find the hidden module binding by its
                // qualified text.
                let text = qual_text(owner);
                let hidden = self.res.bindings.iter().enumerate().find(|(_, b)| b.decl == DeclKind::Module && b.name == text).map(|(i, _)| BindingId(u32::try_from(i).unwrap_or(u32::MAX)));
                hidden.and_then(|h| self.named_of.get(&h).copied()).map(Type::Static)
            }
            MethodOwner::Primitive(p) => Some(match p.name {
                "Number" => Type::Number,
                "String" => Type::String,
                "Table" => Type::Table,
                _ => Type::Bool,
            }),
        }
    }

    fn collect_impl(&mut self, node: &SyntaxNode) {
        let types: Vec<SyntaxNode> = node.children().filter(|c| is_type(c.kind())).collect();
        let (Some(iface_node), Some(target_node)) = (types.first(), types.get(1)) else { return };
        let target = self.type_of(target_node);
        let iface = self.type_of(iface_node);
        let iface_id = match &iface {
            Type::Named(id) => Some(*id),
            _ => None,
        };
        // `To<T>` and `From<S>` keep their argument.
        let arg = child(iface_node, TYPE_ARGS).and_then(|a| a.children().find(|c| is_type(c.kind()))).map(|t| self.type_of(&t));
        let iface_name = iface_id.map(|i| self.types.name(i).to_string()).unwrap_or_default();
        let self_ty = Some(target.clone());
        let mut methods: Vec<(String, Rc<FnType>, TextRange)> = Vec::new();
        for f in children(node, FN_DECL) {
            let Some(name) = child(&f, NAME) else { continue };
            let text = name_text(&name).unwrap_or_default();
            let sig = Rc::new(self.signature_of(&f, true, true, self_ty.clone()));
            self.fn_sigs.insert(f.text_range(), sig.clone());
            methods.push((text, sig, name.text_range()));
        }
        match &target {
            Type::Named(nid) => {
                let nid = *nid;
                // `To<U>` and `From<S>` are kept per type argument: only the
                // same argument again replaces.
                let conversion = matches!(iface_name.as_str(), "To" | "From");
                let same_again = conversion
                    && matches!(self.types.kind(nid), Some(NamedKind::Struct { to, from, .. }) if match (iface_name.as_str(), &arg) {
                        ("To", Some(a)) => to.contains(a),
                        ("From", Some(a)) => from.contains(a),
                        _ => false,
                    });
                for (name, sig, range) in methods {
                    let replaces = if conversion { same_again } else { self.own_method(nid, &name) };
                    if replaces {
                        self.pending_replacements.push((range, name.clone(), self.types.name(nid).to_string()));
                    }
                    self.record_member_decl(nid, &name, range);
                    self.add_member(nid, &name, Member::Function(sig));
                }
                if let Some(n) = self.types.get_mut(nid) {
                    if let NamedKind::Struct { interfaces, to, from, .. } = &mut n.kind {
                        if let Some(i) = iface_id {
                            if !interfaces.contains(&i) {
                                interfaces.push(i);
                            }
                        }
                        match (iface_name.as_str(), arg) {
                            ("To", Some(t)) => to.push(t),
                            ("From", Some(t)) => from.push(t),
                            _ => {}
                        }
                    }
                }
            }
            Type::Number | Type::String | Type::Bool => {
                let key = match target {
                    Type::Number => "Number",
                    Type::String => "String",
                    _ => "Bool",
                };
                for (name, sig, range) in methods {
                    if let Some(lib) = self.prelude_type_id(key) {
                        self.record_member_decl(lib, &name, range);
                    }
                    self.prim_methods.entry(key).or_default().insert(name, sig);
                }
            }
            _ => {}
        }
    }

    /// `let f = fn … end`: the binding has the function's signature.
    fn collect_fn_lets(&mut self, node: &SyntaxNode) {
        let values: Vec<SyntaxNode> = child(node, EXPR_LIST).map(|l| expr_children(&l).collect()).unwrap_or_default();
        for (i, pat) in pattern_children(node).enumerate() {
            let Some(v) = values.get(i) else { continue };
            if !matches!(v.kind(), FN_EXPR | LAMBDA_EXPR) || pat.kind() != BIND_PAT {
                continue;
            }
            let Some(id) = child(&pat, NAME).and_then(|n| self.res.declaration(n.text_range())) else { continue };
            let sig = Rc::new(self.signature_of(v, false, false, None));
            self.fn_sigs.insert(v.text_range(), sig.clone());
            self.binding_sigs.insert(id, sig);
        }
    }

    /// The signature of a function node with its inferred return type
    /// (after pass 1), when declared none.
    pub(crate) fn fn_type(&self, node: &SyntaxNode) -> Rc<FnType> {
        let base = self.fn_sigs.get(&node.text_range()).cloned();
        let Some(base) = base else {
            return Rc::new(FnType::unknown());
        };
        if child(node, RETURN_TYPE).is_some() {
            return base;
        }
        match self.inferred_ret.get(&node.text_range()) {
            Some(r) => {
                let mut f = (*base).clone();
                f.ret = r.clone();
                Rc::new(f)
            }
            None => base,
        }
    }

    /// The signature a binding holds (`fn name`, `let f = fn … end`), with
    /// the inferred return type.
    pub(crate) fn binding_fn_type(&self, id: BindingId) -> Option<Rc<FnType>> {
        let base = self.binding_sigs.get(&id)?.clone();
        let node_range = self.fn_sigs.iter().find(|(_, s)| Rc::ptr_eq(s, &base)).map(|(r, _)| *r)?;
        if base.ret != Type::Any {
            return Some(base);
        }
        match self.inferred_ret.get(&node_range) {
            Some(r) => {
                let mut f = (*base).clone();
                f.ret = r.clone();
                Some(Rc::new(f))
            }
            None => Some(base),
        }
    }

    // ---- running ------------------------------------------------------------------

    fn run(&mut self, root: &SyntaxNode) {
        self.fns.clear();
        self.fns.push(FnCtx { node: None, declared_ret: None, nil_ret: false, returns: Vec::new(), self_type: None, spawn_block: false, loop_depth: 0 });
        self.diverges = false;
        if let Some(b) = block(root) {
            self.block(&b, true);
        }
        if self.pass == 2 {
            let pending = std::mem::take(&mut self.pending_replacements);
            for (range, name, owner) in pending {
                self.warn(range, "method-replaced", format!("`{name}` replaces a method of the same name on `{owner}`"));
            }
        }
    }

    pub(crate) fn prelude_type_id(&self, name: &str) -> Option<NamedId> {
        self.prelude_named.get(name).copied()
    }

    /// The nominal type of a prelude name, as the type it names.
    pub(crate) fn prelude_type(&self, p: &Prelude) -> Type {
        match p.name {
            "Number" => Type::Number,
            "String" => Type::String,
            "Table" => Type::Table,
            "Bool" => Type::Bool,
            "Function" => Type::Function,
            "Userdata" => Type::Userdata,
            "Thread" => Type::Thread,
            "Task" => Type::Task(Rc::new(Type::Any)),
            "Regex" => Type::Regex,
            other => self.prelude_type_id(other).map(Type::Named).unwrap_or(Type::Any),
        }
    }

    /// The value of a prelude name.
    pub(crate) fn prelude_value(&self, p: &Prelude) -> Ty {
        match p.class {
            prelude::PreludeClass::Value | prelude::PreludeClass::ValueAndType => match self.prelude_type_id(p.name) {
                Some(id) if matches!(self.types.kind(id), Some(NamedKind::Module { .. } | NamedKind::LuaClass { .. })) => Ty::certain(Type::Static(id)),
                _ => {
                    // A prelude function: its declaration gives the type.
                    match self.decl_fn_type(p.name) {
                        Some(f) => Ty::certain(Type::Fn(f)),
                        None => Ty::any(),
                    }
                }
            },
            prelude::PreludeClass::Struct => self.prelude_type_id(p.name).map(|id| Ty::certain(Type::Static(id))).unwrap_or_else(Ty::any),
            _ => Ty::any(),
        }
    }

    /// The signature of a top-level `declare fn` by name.
    pub(crate) fn decl_fn_type(&self, name: &str) -> Option<Rc<FnType>> {
        self.decl_fn_sigs.get(name).cloned()
    }
}

/// Function-like nodes: those that open a function scope.
pub(crate) fn is_function_node(k: SyntaxKind) -> bool {
    matches!(k, FN_DECL | FN_EXPR | LAMBDA_EXPR | TRAILING_BLOCK | ON_EXPR | SPAWN_EXPR)
}
