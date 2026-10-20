//! The resolver walk: scopes, hoisting, binding kinds, `Self`, and the
//! checks that need names.
//!
//! The walk visits every node once, in source order. Constructs that open
//! scopes or declare names have their own method; everything else falls
//! through to [`Resolver::walk_children`]. A feature that adds a new node
//! kind adds one arm to [`Resolver::walk`] and, if it declares names, uses
//! [`Resolver::declare_pattern`] or [`Resolver::declare`].

use std::collections::{HashMap, HashSet};

use nl_syntax::{SyntaxKind, SyntaxNode, SyntaxToken, TextRange};

use crate::binding::*;
use crate::decl::{DeclId, DeclItemKind, Declarations};
use crate::prelude::{self, Prelude};
use crate::util::*;
use crate::{ExportInfo, ExtensionInfo, ImportProvider};

use SyntaxKind::*;

#[derive(Default)]
struct Scope {
    names: HashMap<String, BindingId>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FnKind {
    File,
    Decl,
    Method,
    Anon,
    Block,
    Spawn,
    Handler,
}

struct FnCtx {
    index: u32,
    kind: FnKind,
    loop_depth: u32,
    self_binding: Option<BindingId>,
}

/// A module body being walked: its binding, and the function depth of the
/// body (an `fn` at that depth is a method of the module).
struct ModuleCtx {
    binding: BindingId,
    fn_depth: usize,
}

pub(crate) struct Resolver<'a> {
    res: Resolution,
    scopes: Vec<Scope>,
    fns: Vec<FnCtx>,
    next_fn: u32,
    self_targets: Vec<Option<SelfTarget>>,
    modules: Vec<ModuleCtx>,
    imports: &'a dyn ImportProvider,
    /// The declaration files in scope after the prelude.
    decls: &'a Declarations,
    unnamed_handlers: HashSet<String>,
    /// Field names of the structs of this file, and their parents.
    struct_fields: HashMap<BindingId, (Option<BindingId>, Vec<String>)>,
    /// Methods declared on structs of this file (`fn Owner.name`, and the
    /// methods of `interface I for Owner`), for [`Self::check_field_methods`].
    struct_methods: Vec<(BindingId, String, TextRange)>,
    /// While the items of one `when` are declared: the names the first
    /// item bound, which the later items bind again.
    share: Option<HashMap<String, BindingId>>,
    /// While the items of one `when` are declared: the names declared.
    recording: Option<Vec<String>>,
    /// What is known about the functions of each module of this file, by
    /// name, for the block rule.
    module_members: HashMap<BindingId, HashMap<String, FnParams>>,
    /// The exports of each `import from … as Name`.
    import_exports: HashMap<BindingId, Vec<ExportInfo>>,
    /// Trailing blocks on `M.name …` calls: the block, the binding of `M`
    /// and the member name, checked when the whole file is known.
    pending_blocks: Vec<(TextRange, BindingId, String)>,
    /// The variant names of the enums of this file.
    enum_variants: HashMap<BindingId, Vec<String>>,
    /// Each `const` declared at the top of its block (see [`Self::hoist`]),
    /// with the range of its `CONST_DECL`.
    hoisted_consts: HashMap<BindingId, TextRange>,
}

impl<'a> Resolver<'a> {
    pub(crate) fn new(imports: &'a dyn ImportProvider, decls: &'a Declarations) -> Self {
        Resolver {
            res: Resolution::default(),
            scopes: Vec::new(),
            fns: vec![FnCtx { index: 0, kind: FnKind::File, loop_depth: 0, self_binding: None }],
            next_fn: 1,
            self_targets: vec![None],
            modules: Vec::new(),
            imports,
            decls,
            unnamed_handlers: HashSet::new(),
            struct_fields: HashMap::new(),
            struct_methods: Vec::new(),
            share: None,
            recording: None,
            module_members: HashMap::new(),
            import_exports: HashMap::new(),
            pending_blocks: Vec::new(),
            enum_variants: HashMap::new(),
            hoisted_consts: HashMap::new(),
        }
    }

    pub(crate) fn finish(mut self) -> Resolution {
        self.check_pending_blocks();
        self.check_field_methods();
        self.res
    }

    /// Records which parameters each `fn` of the file only calls
    /// (callonly.rs), once the whole file is resolved and before the
    /// exports are collected, which carry the result.
    pub(crate) fn mark_call_only(&mut self, root: &SyntaxNode) {
        crate::callonly::compute(root, &mut self.res);
    }

    /// A struct has one namespace for fields and methods: `x.name` is one
    /// lookup, so a method with a field's name would be hidden
    /// by the field. An error, as TypeScript reports a duplicate member.
    /// Checked once the file is walked, since `fn Owner.name` may stand
    /// before `struct Owner`.
    fn check_field_methods(&mut self) {
        let methods = std::mem::take(&mut self.struct_methods);
        for (id, name, range) in methods {
            let mut fields: Vec<String> = self.inherited_fields(Some(id));
            if let Some((_, own)) = self.struct_fields.get(&id) {
                fields.extend(own.iter().cloned());
            }
            if fields.contains(&name) {
                let owner = self.binding(id).map(|b| b.name.clone()).unwrap_or_default();
                self.error(range, "field-method-clash", format!("`{owner}` has a field named `{name}`, which would hide this method: a struct's fields and methods share one namespace"));
            }
        }
    }

    /// Records the decision of `checks` that a statement's value is
    /// discarded.
    pub(crate) fn mark_discarded(&mut self, stmt: TextRange) {
        self.res.discarded.insert(stmt);
    }

    // ---- diagnostics and scopes ------------------------------------------

    pub(crate) fn error(&mut self, range: TextRange, code: &'static str, message: impl Into<String>) {
        self.res.diagnostics.push(SemaDiagnostic { range, code, message: message.into(), error: true, lint: false });
    }

    /// A lint: the program is accepted.
    pub(crate) fn lint(&mut self, range: TextRange, code: &'static str, message: impl Into<String>) {
        self.res.diagnostics.push(SemaDiagnostic { range, code, message: message.into(), error: false, lint: true });
    }

    fn current_fn(&self) -> u32 {
        self.fns.last().map_or(0, |f| f.index)
    }

    fn at_file_level(&self) -> bool {
        // The file's main function, outside any nested block scope that a
        // module body opens.
        self.fns.len() == 1 && self.modules.is_empty() && self.scopes.len() <= 1
    }

    fn push_scope(&mut self) {
        self.scopes.push(Scope::default());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    #[allow(clippy::too_many_arguments)]
    fn declare(
        &mut self,
        name: &str,
        range: TextRange,
        kind: BindingKind,
        decl: DeclKind,
        mutable: bool,
        annotation: Option<SyntaxNode>,
        params: Option<FnParams>,
    ) -> BindingId {
        if let Some(rec) = self.recording.as_mut() {
            rec.push(name.to_string());
        }
        // A later item of a `when` binds the names of the first item again:
        // the clause's body sees one variable, whichever item matched.
        if let Some(&id) = self.share.as_ref().and_then(|s| s.get(name)) {
            if let Some(scope) = self.scopes.last_mut() {
                scope.names.insert(name.to_string(), id);
            }
            self.res.decls.insert(range, id);
            return id;
        }
        // Declared ahead (a `pub` item in a branch of a file-level `if`,
        // see `hoist_conditional_pubs`): the same binding, now also in
        // this scope.
        if let Some(&id) = self.res.decls.get(&range) {
            if let Some(scope) = self.scopes.last_mut() {
                scope.names.insert(name.to_string(), id);
            }
            return id;
        }
        let id = BindingId(u32::try_from(self.res.bindings.len()).unwrap_or(u32::MAX));
        let file_level = self.at_file_level();
        // Redeclaring a name shadows the earlier one; a lint.
        // Imports are checked by `walk_import`; a hidden binding (`self`)
        // and `_`-prefixed names are not reported.
        if decl != DeclKind::Import && !name.starts_with('_') {
            if let Some(prev) = self.lookup_binding(name) {
                let earlier = self.binding(prev).filter(|b| b.decl != DeclKind::Import && b.decl != DeclKind::SelfParam).map(|b| b.decl);
                if let Some(earlier) = earlier {
                    let what = match earlier {
                        DeclKind::Fn => "function",
                        DeclKind::Param | DeclKind::RestParam | DeclKind::KeyedParam => "parameter",
                        DeclKind::Struct | DeclKind::Enum | DeclKind::Interface | DeclKind::TypeAlias | DeclKind::Module | DeclKind::TypeParam => "type",
                        _ => "variable",
                    };
                    self.lint(range, "shadowing", format!("`{name}` shadows the {what} `{name}` declared earlier"));
                } else if !file_level && self.binding(prev).is_some_and(|b| b.decl == DeclKind::Import) {
                    // At file level the same name is the error
                    // `import-clash`.
                    self.lint(range, "shadows-declared", format!("`{name}` shadows the imported `{name}`; below this, `{name}` is this one"));
                }
            } else if decl != DeclKind::SelfParam && prelude::lookup(name).is_none() && self.shadows_declared_value(name, decl) {
                // A name of the declaration files (looked up after the
                // prelude, so one with a prelude name is not reachable
                // anyway): the local hides the host's value from here on.
                self.lint(range, "shadows-declared", format!("`{name}` shadows the declared `{name}` of the declaration files; below this, `{name}` is this one (rename it, or reach the declared value through its Lua path)"));
            }
        }
        if file_level && decl != DeclKind::Import {
            let imported = self.scopes.last().and_then(|sc| sc.names.get(name)).copied().filter(|b| self.binding(*b).is_some_and(|b| b.decl == DeclKind::Import));
            if imported.is_some() {
                self.import_clash(name, range);
            }
        }
        let module_owner = match decl {
            DeclKind::Fn => self.modules.last().filter(|m| m.fn_depth == self.fns.len()).map(|m| m.binding),
            _ => None,
        };
        let named = decl == DeclKind::Fn && (file_level || module_owner.is_some());
        if let (Some(m), Some(p)) = (module_owner, params) {
            self.module_members.entry(m).or_default().insert(name.to_string(), p);
        }
        self.res.bindings.push(Binding {
            name: name.to_string(),
            kind,
            decl,
            range,
            mutable,
            file_level,
            function: self.current_fn(),
            annotation,
            params,
            module_owner,
            named,
        });
        if let Some(scope) = self.scopes.last_mut() {
            scope.names.insert(name.to_string(), id);
        }
        self.res.decls.insert(range, id);
        id
    }

    /// Lookup finds a file-level declaration before an import, so a
    /// file-level declaration with the name of something the file imports
    /// would hide the import for the whole file: an error at the
    /// declaration (`module Name` reopening an imported module
    /// declares nothing and is not one).
    fn import_clash(&mut self, name: &str, decl_range: TextRange) {
        self.error(
            decl_range,
            "import-clash",
            format!("`{name}` is imported by this file; a file-level declaration cannot have the name of an import, which it would hide"),
        );
    }

    /// A binding that no name refers to (`self`, the body of a nested
    /// module).
    fn hidden(&mut self, name: &str, range: TextRange, decl: DeclKind) -> BindingId {
        let id = BindingId(u32::try_from(self.res.bindings.len()).unwrap_or(u32::MAX));
        self.res.bindings.push(Binding {
            name: name.to_string(),
            kind: BindingKind::Variable,
            decl,
            range,
            mutable: false,
            file_level: self.at_file_level(),
            function: self.current_fn(),
            annotation: None,
            params: None,
            module_owner: None,
            named: false,
        });
        id
    }

    /// Whether a local `name` hides a declared value worth a
    /// `shadows-declared` lint: a declared `let` or `const`, or for any
    /// local but a parameter a declared module. Not a declared function:
    /// a host declares hundreds of lowercase ones (`player`, `color`,
    /// `entity` under gmod), whose names are the natural names of the
    /// values a function works on.
    fn shadows_declared_value(&self, name: &str, decl: DeclKind) -> bool {
        let Some(item) = self.decls.lookup(name).and_then(|id| self.decls.get(id)) else { return false };
        match item.kind {
            DeclItemKind::Let | DeclItemKind::Const => true,
            DeclItemKind::Module => !matches!(decl, DeclKind::Param | DeclKind::RestParam | DeclKind::KeyedParam),
            _ => false,
        }
    }

    fn lookup_binding(&self, name: &str) -> Option<BindingId> {
        self.scopes.iter().rev().find_map(|s| s.names.get(name).copied())
    }

    fn binding(&self, id: BindingId) -> Option<&Binding> {
        self.res.binding(id)
    }

    fn lookup_value(&mut self, name: &str) -> Option<Ref> {
        if let Some(id) = self.lookup_binding(name) {
            return Some(Ref::Binding(id));
        }
        if let Some(p) = prelude::lookup(name).filter(|p| p.is_value()) {
            return Some(Ref::Prelude(p));
        }
        // Declaration files, after the prelude. A host type
        // has no value.
        let id = self.decls.lookup(name)?;
        let item = self.decls.get(id)?;
        if item.kind == DeclItemKind::HostType {
            return None;
        }
        if let Some(root) = &item.root {
            self.res.decl_paths.entry(id).or_insert_with(|| root.clone());
        }
        self.res.decl_kinds.insert(id, item.kind);
        self.res.decl_binding_kinds.insert(id, self.decls.binding_kind(id));
        Some(Ref::Declared(id))
    }

    // ---- the walk ---------------------------------------------------------

    pub(crate) fn walk(&mut self, node: &SyntaxNode) {
        match node.kind() {
            SOURCE_FILE => self.walk_children(node),
            BLOCK => self.walk_block(node),
            LET_DECL => self.walk_let(node, DeclKind::Let),
            CONST_DECL => self.walk_let(node, DeclKind::Const),
            GLOBAL_DECL => self.walk_global(node),
            FN_DECL => self.walk_fn_decl(node),
            FN_EXPR | LAMBDA_EXPR => self.walk_function(node, FnKind::Anon, false),
            TRAILING_BLOCK => self.walk_function(node, FnKind::Block, false),
            ON_EXPR => self.walk_on(node),
            SPAWN_EXPR => self.walk_spawn(node),
            STRUCT_DECL => self.walk_struct(node),
            ENUM_DECL => self.walk_enum(node),
            INTERFACE_DECL => self.walk_interface(node),
            INTERFACE_IMPL => self.walk_interface_impl(node),
            TYPE_DECL => self.walk_type_decl(node),
            MODULE_DECL => self.walk_module(node),
            IMPORT_DECL => self.walk_import(node),
            WHILE_STMT | UNTIL_STMT | LOOP_STMT => self.walk_loop(node),
            FOR_STMT => self.walk_for(node),
            BREAK_STMT | CONTINUE_STMT => self.walk_break(node),
            IF_EXPR => self.walk_if(node),
            CASE_EXPR => self.walk_case(node),
            TRY_EXPR => self.walk_try(node),
            ASSIGN_STMT | COMPOUND_ASSIGN_STMT | MULTI_ASSIGN_STMT => self.walk_assign(node),
            RETURN_STMT => self.walk_return(node),
            NAME_REF => self.walk_name_ref(node),
            SELF_EXPR => self.walk_self(node),
            AT_EXPR => self.walk_at(node),
            LUA_PATH => self.walk_lua_path(node),
            CALL_EXPR | COMMAND_CALL | METHOD_CALL => self.walk_call(node),
            BANG_EXPR => self.walk_bang(node),
            IS_EXPR => self.walk_is_as(node, false),
            AS_EXPR => self.walk_is_as(node, true),
            FIELD_EXPR => self.walk_field(node),
            k if is_type(k) => self.resolve_type(node),
            _ => self.walk_children(node),
        }
    }

    fn walk_children(&mut self, node: &SyntaxNode) {
        for c in node.children() {
            self.walk(&c);
        }
    }

    fn walk_block(&mut self, block: &SyntaxNode) {
        self.push_scope();
        self.hoist(block);
        if block.parent().is_some_and(|p| p.kind() == SOURCE_FILE) {
            self.hoist_conditional_pubs(block);
        }
        for stmt in statements(block) {
            self.walk(&stmt);
        }
        self.pop_scope();
    }

    /// Declares the hoisted names of a block before its statements: `fn`
    /// declarations, type-level declarations, then `const`s.
    fn hoist(&mut self, block: &SyntaxNode) {
        self.hoist_fns_and_types(block);
        self.hoist_consts(block);
    }

    /// A `pub` item directly in a branch of a file-level `if` or `unless`
    /// (`if CLIENT pub fn draw … end end`) is a file-level name: declared
    /// here in the file's scope, and given its value where the branch
    /// runs, so the export table holds it, or `nil` when no branch that
    /// declares it ran. The items of one name in several branches of the
    /// same `if` (one per realm) are one binding. Which branches declare
    /// each one is kept for the exports ([`Resolution::conditional_export`]).
    fn hoist_conditional_pubs(&mut self, file_block: &SyntaxNode) {
        for s in statements(file_block) {
            let Some(construct) = file_branch_construct(&s) else { continue };
            // The binding of each name, and the last branch that declared it.
            let mut by_name: HashMap<String, (BindingId, usize)> = HashMap::new();
            for (clause, b) in branch_blocks(&construct).into_iter().enumerate() {
                for item in statements(&b).filter(is_exported) {
                    for (name, text, kind, decl, ty, params) in pub_item_names(&item) {
                        // Twice in one branch is two items, and the
                        // second export of the name an error.
                        let id = match by_name.get(&text) {
                            Some(&(id, last)) if last != clause => {
                                self.res.decls.insert(name, id);
                                id
                            }
                            _ => self.declare(&text, name, kind, decl, false, ty, params),
                        };
                        by_name.insert(text, (id, clause));
                        let entry = self.res.conditional_exports.entry(id).or_insert_with(|| (construct.clone(), Vec::new()));
                        if !entry.1.contains(&clause) {
                            entry.1.push(clause);
                        }
                    }
                }
            }
        }
    }

    /// A `const` is declared at the top of its block, like `fn`, and gets
    /// its value where it stands: a function of the block, a method of a
    /// module or a closure can use it wherever it stands, and one that
    /// runs before the `const` line reads `nil`, as with a Lua local
    /// declared ahead. `let` is not hoisted, since its value changes. A
    /// read before the line in the block's own straight-line code (the
    /// same function) is an error ([`Self::walk_name_ref`]). A `const`
    /// whose name another statement of the block declares too (a
    /// redeclaration), and a destructuring `const`, are declared in place
    /// as before.
    fn hoist_consts(&mut self, block: &SyntaxNode) {
        let stmts: Vec<SyntaxNode> = statements(block).collect();
        if !stmts.iter().any(|s| s.kind() == CONST_DECL) {
            return;
        }
        let mut counts: HashMap<String, u32> = HashMap::new();
        for s in &stmts {
            for n in statement_names(s) {
                *counts.entry(n).or_default() += 1;
            }
        }
        for stmt in stmts.iter().filter(|s| s.kind() == CONST_DECL) {
            let pats: Vec<SyntaxNode> = pattern_children(stmt).collect();
            let simple: Option<Vec<(SyntaxNode, Option<SyntaxNode>)>> = pats.iter().map(simple_const_pattern).collect();
            let Some(simple) = simple else { continue };
            let names: Vec<String> = simple.iter().map(|(n, _)| name_text(n).unwrap_or_default()).collect();
            if names.iter().any(|n| counts.get(n).copied().unwrap_or(0) != 1) {
                continue;
            }
            let values: Vec<SyntaxNode> = child(stmt, EXPR_LIST).map(|l| expr_children(&l).collect()).unwrap_or_default();
            for (i, ((name, ty), text)) in simple.into_iter().zip(names).enumerate() {
                let func = match values.get(i) {
                    Some(v) if matches!(v.kind(), FN_EXPR | LAMBDA_EXPR) && ty.is_none() => Some(FnParams { has_function_param: has_function_param(v), anonymous: true, call_only: 0 }),
                    _ => None,
                };
                let fn_typed = ty.as_ref().is_some_and(is_function_type);
                let kind = if func.is_some() || fn_typed { BindingKind::Function } else { BindingKind::Variable };
                let id = self.declare(&text, name.text_range(), kind, DeclKind::Const, false, ty, func);
                // A conditional export of one name in several branches is
                // one binding: its first `const` decides what is early.
                self.hoisted_consts.entry(id).or_insert(stmt.text_range());
            }
        }
    }

    fn hoist_fns_and_types(&mut self, block: &SyntaxNode) {
        for stmt in statements(block) {
            match stmt.kind() {
                FN_DECL if child(&stmt, QUAL_NAME).is_none() => {
                    if let Some(name) = child(&stmt, NAME) {
                        let text = name_text(&name).unwrap_or_default();
                        let params = Some(FnParams { has_function_param: has_function_param(&stmt), anonymous: false, call_only: 0 });
                        self.declare(&text, name.text_range(), BindingKind::Function, DeclKind::Fn, false, None, params);
                    }
                }
                STRUCT_DECL | ENUM_DECL | INTERFACE_DECL | TYPE_DECL => {
                    if let Some(name) = child(&stmt, NAME) {
                        let text = name_text(&name).unwrap_or_default();
                        let decl = match stmt.kind() {
                            STRUCT_DECL => DeclKind::Struct,
                            ENUM_DECL => DeclKind::Enum,
                            INTERFACE_DECL => DeclKind::Interface,
                            _ => DeclKind::TypeAlias,
                        };
                        let id = self.declare(&text, name.text_range(), BindingKind::TypeLevel, decl, false, None, None);
                        if decl == DeclKind::Enum {
                            let variants = children(&stmt, VARIANT).filter_map(|v| child(&v, NAME)).filter_map(|n| name_text(&n)).collect();
                            self.enum_variants.insert(id, variants);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // ---- declarations -----------------------------------------------------

    fn walk_let(&mut self, node: &SyntaxNode, decl: DeclKind) {
        let values: Vec<SyntaxNode> = child(node, EXPR_LIST).map(|l| expr_children(&l).collect()).unwrap_or_default();
        for v in &values {
            self.walk(v);
        }
        let mutable = decl == DeclKind::Let;
        if decl == DeclKind::Const {
            // A `const` directly in a module body is a member of the module
            // (`Self.limit`, `Inventory.limit`), as `fn` there is
            // a method; `let` stays private.
            if let Some(m) = self.modules.last().filter(|m| m.fn_depth == self.fns.len()).map(|m| m.binding) {
                self.res.module_consts.insert(node.text_range(), m);
            }
        }
        for (i, pat) in pattern_children(node).enumerate() {
            // Declared ahead by `hoist_consts`: only its type is left.
            let hoisted = simple_const_pattern(&pat).and_then(|(n, _)| self.res.decls.get(&n.text_range()).copied()).is_some_and(|id| self.hoisted_consts.contains_key(&id));
            if hoisted {
                if let Some(t) = type_child(&pat) {
                    self.resolve_type(&t);
                }
                continue;
            }
            let func = match values.get(i) {
                Some(v) if matches!(v.kind(), FN_EXPR | LAMBDA_EXPR) && pat.kind() == BIND_PAT => {
                    Some(FnParams { has_function_param: has_function_param(v), anonymous: true, call_only: 0 })
                }
                _ => None,
            };
            self.declare_pattern(&pat, decl, mutable, func, None);
        }
    }

    /// `return` outside every function: a file returns its export table,
    /// so a top-level `return` has nothing to return from, as
    /// in a TypeScript module.
    fn walk_return(&mut self, node: &SyntaxNode) {
        if self.fns.len() <= 1 && self.modules.is_empty() {
            self.error(trimmed_range(node), "top-level-return", "`return` at the top level of a file: a file returns its export table, and only a function can return a value");
        } else if self.fns.len() <= 1 {
            self.error(trimmed_range(node), "top-level-return", "`return` in a module body: a module body is not a function");
        }
        self.walk_children(node);
    }

    fn walk_global(&mut self, node: &SyntaxNode) {
        if let Some(e) = expr_child(node) {
            self.walk(&e);
        }
        let ty = type_child(node);
        if let Some(t) = &ty {
            self.resolve_type(t);
        }
        if let Some(name) = child(node, NAME) {
            let text = name_text(&name).unwrap_or_default();
            self.declare(&text, name.text_range(), BindingKind::Variable, DeclKind::Global, true, ty, None);
        }
    }

    /// Declares the names a pattern binds and resolves the
    /// types and names it refers to.
    pub(crate) fn declare_pattern(
        &mut self,
        pat: &SyntaxNode,
        decl: DeclKind,
        mutable: bool,
        func: Option<FnParams>,
        annotation: Option<SyntaxNode>,
    ) {
        match pat.kind() {
            BIND_PAT => {
                if let Some(name) = child(pat, NAME) {
                    let text = name_text(&name).unwrap_or_default();
                    let fn_typed = annotation.as_ref().is_some_and(is_function_type);
                    let kind = if func.is_some() || fn_typed { BindingKind::Function } else { BindingKind::Variable };
                    self.declare(&text, name.text_range(), kind, decl, mutable, annotation, func);
                }
            }
            TYPED_PAT => {
                let ty = type_child(pat);
                if let Some(t) = &ty {
                    self.resolve_type(t);
                }
                if let Some(inner) = pattern_children(pat).next() {
                    let ann = if inner.kind() == BIND_PAT { ty } else { None };
                    self.declare_pattern(&inner, decl, mutable, func, ann);
                }
            }
            AS_PAT => {
                if let Some(inner) = pattern_children(pat).next() {
                    self.declare_pattern(&inner, decl, mutable, None, None);
                }
                if let Some(name) = child(pat, NAME) {
                    let text = name_text(&name).unwrap_or_default();
                    self.declare(&text, name.text_range(), BindingKind::Variable, decl, mutable, None, None);
                }
            }
            BRACE_PAT | STRUCT_PAT => {
                if let Some(q) = child(pat, QUAL_NAME) {
                    self.resolve_qual_type(&q);
                }
                for c in pat.children() {
                    match c.kind() {
                        PAT_FIELD => self.declare_pat_field(&c, decl, mutable),
                        k if is_pattern(k) => self.declare_pattern(&c, decl, mutable, None, None),
                        _ => {}
                    }
                }
            }
            BRACKET_PAT => {
                for c in pat.children() {
                    match c.kind() {
                        REST_PAT => {
                            if let Some(name) = child(&c, NAME) {
                                let text = name_text(&name).unwrap_or_default();
                                self.declare(&text, name.text_range(), BindingKind::Variable, decl, mutable, None, None);
                            }
                        }
                        k if is_pattern(k) => self.declare_pattern(&c, decl, mutable, None, None),
                        _ => {}
                    }
                }
            }
            QUALIFIED_PAT => {
                if let Some(q) = child(pat, QUAL_NAME) {
                    self.resolve_qualified_value(&q);
                }
            }
            IS_PAT => {
                if let Some(t) = type_child(pat) {
                    self.resolve_type(&t);
                }
            }
            _ => {}
        }
    }

    fn declare_pat_field(&mut self, field: &SyntaxNode, decl: DeclKind, mutable: bool) {
        if !has_token(field, COLON) {
            // `{name}` shorthand.
            if let Some(name) = child(field, NAME) {
                let text = name_text(&name).unwrap_or_default();
                self.declare(&text, name.text_range(), BindingKind::Variable, decl, mutable, None, None);
            }
            return;
        }
        if let Some(value) = pattern_children(field).next() {
            self.declare_pattern(&value, decl, mutable, None, None);
            // `{key: Mod.Type}` binds `key` like `{key: Type}`; an enum
            // variant only compares.
            let q = child(&value, QUAL_NAME).filter(|_| value.kind() == QUALIFIED_PAT);
            if let Some(q) = q {
                if !self.res.variants.contains(&q.text_range()) {
                    if let Some(key) = name_token(field).filter(|k| k.kind().is_name() || k.kind().is_keyword()) {
                        let text = key.text().to_string();
                        self.declare(&text, key.text_range(), BindingKind::Variable, decl, mutable, None, None);
                    }
                }
            }
        } else if let Some(ty) = type_child(field) {
            // `{key: Type}` binds `key` and requires the type. It is
            // the same as `{key: key: Type}`, so a `Function`
            // type makes the name function-kind as it does there.
            self.resolve_type(&ty);
            if let Some(key) = name_token(field) {
                let text = key.text().to_string();
                let kind = if is_function_type(&ty) { BindingKind::Function } else { BindingKind::Variable };
                self.declare(&text, key.text_range(), kind, decl, mutable, Some(ty), None);
            }
        }
    }

    fn walk_fn_decl(&mut self, node: &SyntaxNode) {
        let in_impl = node.parent().is_some_and(|p| p.kind() == INTERFACE_IMPL);
        if let Some(owner) = child(node, QUAL_NAME) {
            let target = self.resolve_method_owner(node, &owner);
            // A function added to a declared module is a plain function of
            // a Lua library: it has no `self`.
            let method = match self.res.method_owner.get(&node.text_range()) {
                Some(MethodOwner::Declared(id)) => self.decls.get(*id).is_none_or(|i| i.kind != DeclItemKind::Module),
                _ => true,
            };
            self.self_targets.push(target);
            self.walk_function(node, FnKind::Method, method);
            self.self_targets.pop();
        } else if in_impl {
            // Methods of `interface I for T`: Self was pushed by the impl.
            self.walk_function(node, FnKind::Method, true);
        } else {
            if let Some(name) = child(node, NAME) {
                if !self.res.decls.contains_key(&name.text_range()) {
                    let text = name_text(&name).unwrap_or_default();
                    let params = Some(FnParams { has_function_param: has_function_param(node), anonymous: false, call_only: 0 });
                    self.declare(&text, name.text_range(), BindingKind::Function, DeclKind::Fn, false, None, params);
                }
            }
            let module_method = self.modules.last().is_some_and(|m| m.fn_depth == self.fns.len());
            self.walk_function(node, FnKind::Decl, module_method);
        }
    }

    /// Resolves `Owner` in `fn Owner.name`.
    fn resolve_method_owner(&mut self, node: &SyntaxNode, owner: &SyntaxNode) -> Option<SelfTarget> {
        let segs = qual_segments(owner);
        let first = segs.first()?;
        let name = first.text().to_string();
        let path: Vec<String> = segs.iter().skip(1).map(|t| t.text().to_string()).collect();
        match self.lookup_binding(&name) {
            Some(id) => {
                self.res.type_refs.insert(owner.text_range(), Ref::Binding(id));
                // An imported name counts by what declares it in its file;
                // `import from … as Name` binds an export table, which can
                // be extended like a module.
                let ok = matches!(self.res.declared_as(id), Some(DeclKind::Struct | DeclKind::Module | DeclKind::Import));
                if !ok {
                    let what = if self.binding(id).is_some_and(|b| b.decl == DeclKind::Import) { "is imported and is" } else { "is" };
                    self.error(owner.text_range(), "bad-method-owner", format!("`{name}` {what} not a struct or module, so it cannot have methods"));
                }
                if path.is_empty() && self.res.declared_as(id) == Some(DeclKind::Module) {
                    if let Some(word) = child(node, NAME).and_then(|n| name_text(&n)) {
                        let params = FnParams { has_function_param: has_function_param(node), anonymous: false, call_only: 0 };
                        self.module_members.entry(id).or_default().insert(word, params);
                    }
                }
                if path.is_empty() && self.binding(id).is_some_and(|b| b.decl == DeclKind::Struct) {
                    if let Some(n) = child(node, NAME) {
                        self.struct_methods.push((id, name_text(&n).unwrap_or_default(), n.text_range()));
                    }
                }
                self.res.method_owner.insert(node.text_range(), MethodOwner::Binding { id, path: path.clone() });
                if path.is_empty() { Some(SelfTarget::Binding(id)) } else { Some(SelfTarget::Member { id, path }) }
            }
            None => match prelude::lookup(&name) {
                Some(p) if p.takes_methods() && path.is_empty() => {
                    // Plain tables share no metatable, so an operator of
                    // `Table` would have nowhere to live.
                    if p.name == "Table" && child(node, OP_NAME).is_some() {
                        self.error(owner.text_range(), "operator-owner", "operators cannot be defined on `Table`: plain tables share no metatable; define them on a struct");
                    }
                    self.res.type_refs.insert(owner.text_range(), Ref::Prelude(p));
                    self.res.method_owner.insert(node.text_range(), MethodOwner::Primitive(p));
                    Some(SelfTarget::Prelude(p))
                }
                Some(p) => {
                    self.error(owner.text_range(), "bad-method-owner", format!("`{}` cannot have methods; methods are defined on structs, modules, `Number`, `String`, `Bool` and `Table`", p.name));
                    None
                }
                None => match self.decls.lookup(&name) {
                    Some(id) => {
                        self.extend_declared(node, owner, id, &path);
                        None
                    }
                    None => {
                        self.error(owner.text_range(), "undeclared-name", format!("`{name}` is not declared"));
                        None
                    }
                },
            },
        }
    }

    /// `fn Decl.name` where `Decl` is a top-level name of a declaration
    /// file: a declared module gets a plain function, a declared struct
    /// (a Lua class) or host type a method of its values. The metatable of
    /// a struct is its Lua path, that of a host type the one the host
    /// registered under its name. Redefining a member the declaration
    /// has, or one an imported file added, is an error; this file may
    /// define a name again, which replaces it.
    fn extend_declared(&mut self, node: &SyntaxNode, owner: &SyntaxNode, id: DeclId, path: &[String]) {
        let Some(item) = self.decls.get(id) else { return };
        let (kind, decl_name) = (item.kind, item.name.clone());
        let range = owner.text_range();
        let what = match kind {
            DeclItemKind::Module => "module",
            DeclItemKind::Struct => "struct",
            DeclItemKind::HostType => "type",
            DeclItemKind::Fn => "fn",
            DeclItemKind::Let | DeclItemKind::Const => "value",
        };
        if !matches!(kind, DeclItemKind::Module | DeclItemKind::Struct | DeclItemKind::HostType) {
            self.error(range, "bad-method-owner", format!("`{decl_name}` is a declared {what}, not a module or a type, so it cannot have methods"));
            return;
        }
        if !path.is_empty() {
            let full = qual_text(owner);
            let nested = decl_member_node(&item.node, path.first().map(String::as_str).unwrap_or_default());
            let msg = match nested.map(|n| n.kind()) {
                Some(DECL_TYPE | DECL_STRUCT) => format!("`{full}` has no known metatable: a type nested in a declaration has no registered name and no Lua path of its own, so `fn {full}.name` has nowhere to go; declare it at the top level of the declaration file (`declare type`, or `declare struct … = :path`)"),
                Some(DECL_MODULE) => format!("`{full}` is a module nested in a declaration; `fn` extends a top-level declared module, whose Lua table is its path"),
                _ => format!("`{full}` is not a module or type that `{decl_name}` declares"),
            };
            self.error(range, "declared-owner", msg);
            return;
        }
        let Some(word) = child(node, NAME).and_then(|n| name_text(&n)) else {
            // `fn Player.+(x)`: the metatable of a host type is the host's.
            self.error(range, "operator-owner", format!("operators cannot be defined on the declared {what} `{decl_name}`: its metatable belongs to Lua or the host; define them on a struct of NL"));
            return;
        };
        if matches!(word.as_str(), "len" | "coerce" | "iter") {
            self.error(range, "operator-owner", format!("`fn {decl_name}.{word}` would define the operator `{word}`, and operators cannot be defined on the declared {what} `{decl_name}`, whose metatable belongs to Lua or the host"));
            return;
        }
        if let Some(r) = item.root.clone() {
            self.res.decl_paths.entry(id).or_insert(r);
        }
        self.res.decl_kinds.insert(id, kind);
        self.res.decl_binding_kinds.insert(id, self.decls.binding_kind(id));
        self.res.type_refs.insert(range, Ref::Declared(id));
        let name_range = child(node, NAME).map_or(range, |n| n.text_range());
        if let Some(from) = self.declared_member_of(id, &word) {
            self.error(name_range, "declared-member-redefined", format!("cannot redefine {decl_name}.{word}: {from} declares it; a function added to a declared {what} must have a name of its own"));
            return;
        }
        if let Some(e) = self.res.extensions.iter().find(|e| e.owner == decl_name && e.name == word && e.origin.is_some()) {
            let origin = e.origin.clone().unwrap_or_default();
            self.error(name_range, "declared-member-redefined", format!("cannot redefine {decl_name}.{word}: it is already defined by '{origin}', which this file imports"));
            return;
        }
        self.res.method_owner.insert(node.text_range(), MethodOwner::Declared(id));
        let mut params = Vec::new();
        let (mut rest, mut keyed) = (false, false);
        if let Some(list) = child(node, PARAM_LIST) {
            for p in list.children() {
                match p.kind() {
                    PARAM => {
                        let pname = pattern_children(&p).next().and_then(|pat| child(&pat, NAME).or_else(|| pattern_children(&pat).next().and_then(|i| child(&i, NAME)))).and_then(|n| name_text(&n)).unwrap_or_default();
                        params.push((pname, expr_child(&p).is_some()));
                    }
                    REST_PARAM => rest = true,
                    KEYED_PARAM => keyed = true,
                    _ => {}
                }
            }
        }
        let method = kind != DeclItemKind::Module;
        let info = ExtensionInfo { owner: decl_name, name: word, method, params, rest, keyed, is_async: has_token(node, ASYNC_KW), origin: None };
        self.res.extensions.retain(|e| e.origin.is_some() || e.owner != info.owner || e.name != info.name);
        self.res.extensions.push(info);
    }

    /// The declaration file that declares the member `name` of the
    /// declared item `id` or of one of its parents (`declare type Player :
    /// Entity`), if any.
    fn declared_member_of(&self, id: DeclId, name: &str) -> Option<String> {
        let mut cur = Some(id);
        for _ in 0..64 {
            let item = self.decls.get(cur?)?;
            if decl_member_node(&item.node, name).is_some() {
                return Some(item.file.clone());
            }
            let parent = child(&item.node, STRUCT_PARENT).and_then(|p| type_child(&p)).map(|t| t.text().to_string().trim().to_owned());
            cur = parent.and_then(|p| self.decls.lookup(&p));
        }
        None
    }

    /// Walks a function-like node: parameters, return type and body.
    /// `method`: the function receives a hidden `self`.
    fn walk_function(&mut self, node: &SyntaxNode, kind: FnKind, method: bool) {
        let index = self.next_fn;
        self.next_fn += 1;
        // Closures see the enclosing `self`; a method has its
        // own.
        let inherited_self = if method { None } else { self.fns.iter().rev().find_map(|f| f.self_binding) };
        self.fns.push(FnCtx { index, kind, loop_depth: 0, self_binding: inherited_self });
        self.push_scope();
        if method {
            let id = self.hidden("self", node.text_range(), DeclKind::SelfParam);
            self.res.method_self.insert(node.text_range(), id);
            if let Some(f) = self.fns.last_mut() {
                f.self_binding = Some(id);
            }
        }
        if let Some(tp) = child(node, TYPE_PARAMS) {
            self.declare_type_params(&tp);
        }
        let params = child(node, PARAM_LIST).or_else(|| child(node, BLOCK_PARAMS));
        if let Some(pl) = &params {
            self.walk_params(pl);
        }
        if let Some(rt) = child(node, RETURN_TYPE) {
            if let Some(t) = type_child(&rt) {
                self.resolve_type(&t);
            }
        }
        if let Some(b) = block(node) {
            self.walk_block(&b);
        }
        self.pop_scope();
        self.fns.pop();
    }

    fn declare_type_params(&mut self, tp: &SyntaxNode) {
        for p in children(tp, TYPE_PARAM) {
            if let Some(name) = child(&p, NAME) {
                let text = name_text(&name).unwrap_or_default();
                self.declare(&text, name.text_range(), BindingKind::TypeLevel, DeclKind::TypeParam, false, None, None);
            }
        }
    }

    fn walk_params(&mut self, list: &SyntaxNode) {
        crate::checks::check_param_list(self, list);
        // The bindings the parameters declare (patterns walk no
        // expressions, so nothing else is declared meanwhile).
        let mut declared: Vec<BindingId> = Vec::new();
        for p in list.children() {
            match p.kind() {
                PARAM => {
                    // A default may use earlier parameters, not this one.
                    if let Some(default) = expr_child(&p) {
                        self.walk(&default);
                    }
                    if let Some(pat) = pattern_children(&p).next() {
                        let before = self.res.bindings.len();
                        self.declare_pattern(&pat, DeclKind::Param, true, None, None);
                        declared.extend((before..self.res.bindings.len()).filter_map(|i| u32::try_from(i).ok()).map(BindingId));
                    }
                }
                REST_PARAM | KEYED_PARAM => {
                    let ty = type_child(&p);
                    if let Some(t) = &ty {
                        self.resolve_type(t);
                    }
                    if let Some(name) = child(&p, NAME) {
                        let text = name_text(&name).unwrap_or_default();
                        let kind = if p.kind() == REST_PARAM { DeclKind::RestParam } else { DeclKind::KeyedParam };
                        declared.push(self.declare(&text, name.text_range(), BindingKind::Variable, kind, true, None, None));
                    }
                }
                _ => {}
            }
        }
        // A parameter name is declared once.
        let mut seen = HashSet::new();
        for id in declared {
            let Some(b) = self.binding(id) else { continue };
            let (name, range) = (b.name.clone(), b.range);
            if !seen.insert(name.clone()) {
                self.error(range, "duplicate-param", format!("the parameter `{name}` is declared twice in this parameter list"));
            }
        }
    }

    fn walk_on(&mut self, node: &SyntaxNode) {
        if !has_token(node, AS_KW) {
            if let Some(q) = child(node, QUAL_NAME) {
                let event = qual_text(&q);
                if !self.unnamed_handlers.insert(event.clone()) {
                    self.error(
                        trimmed_range(node),
                        "second-unnamed-handler",
                        format!("a second unnamed `on {event}` in one file would replace the first; name one with `as`"),
                    );
                }
            }
        }
        self.walk_function(node, FnKind::Handler, false);
    }

    fn walk_spawn(&mut self, node: &SyntaxNode) {
        if has_token(node, DO_KW) {
            self.walk_function(node, FnKind::Spawn, false);
            return;
        }
        self.walk_children(node);
        // The callee of the call form must be a named function.
        let Some(call) = node.children().find(|c| matches!(c.kind(), CALL_EXPR | COMMAND_CALL)) else { return };
        let Some(callee) = expr_child(&call) else { return };
        if callee.kind() != NAME_REF {
            return;
        }
        let Some(Ref::Binding(id)) = self.res.refs.get(&callee.text_range()).cloned() else { return };
        let Some(b) = self.binding(id).cloned() else { return };
        if b.params.is_some_and(|p| p.anonymous) {
            self.error(callee.text_range(), "spawn-callee", format!("the callee of `spawn` must be a named function; `{}` is an anonymous function", b.name));
        } else if b.decl == DeclKind::Fn && !b.named {
            let place = if b.function == 0 {
                "in a nested block, not at file level or directly in a module body"
            } else {
                "inside another function"
            };
            self.error(callee.text_range(), "spawn-callee", format!("`{}` is declared {place}, so a worker cannot look it up by name", b.name));
        }
        // Any other callee (a parameter, a variable) may hold a named
        // function; the runtime raises SendError when it does not.
    }

    fn walk_struct(&mut self, node: &SyntaxNode) {
        let id = child(node, NAME).and_then(|n| self.res.decls.get(&n.text_range()).copied());
        self.push_scope();
        if let Some(tp) = child(node, TYPE_PARAMS) {
            self.declare_type_params(&tp);
        }
        self.self_targets.push(id.map(SelfTarget::Binding));
        let mut parent = None;
        if let Some(p) = child(node, STRUCT_PARENT) {
            if let Some(t) = type_child(&p) {
                self.resolve_type(&t);
                if let Some(q) = child(&t, QUAL_NAME) {
                    if let Some(Ref::Binding(pid)) = self.res.type_refs.get(&q.text_range()) {
                        parent = Some(*pid);
                    }
                }
            }
        }
        let inherited = self.inherited_fields(parent);
        let mut fields = Vec::new();
        for f in children(node, FIELD_DECL) {
            let Some(name) = child(&f, NAME) else { continue };
            let text = name_text(&name).unwrap_or_default();
            if text == "len" {
                self.error(name.text_range(), "len-field", "`len` is a reserved member name and cannot be a struct field");
            } else if inherited.contains(&text) {
                self.error(name.text_range(), "redeclared-field", format!("`{text}` is a field of the parent struct; a child cannot redeclare a parent's field"));
            } else if fields.contains(&text) {
                self.error(name.text_range(), "duplicate-field", format!("`{text}` is declared twice in this struct"));
            }
            if let Some(t) = type_child(&f) {
                self.resolve_type(&t);
            }
            if let Some(e) = expr_child(&f) {
                self.walk(&e);
            }
            fields.push(text);
        }
        if let Some(id) = id {
            self.struct_fields.insert(id, (parent, fields));
        }
        self.self_targets.pop();
        self.pop_scope();
    }

    /// `enum Name variants end`: a variant is declared once.
    fn walk_enum(&mut self, node: &SyntaxNode) {
        let mut seen = HashSet::new();
        for v in children(node, VARIANT) {
            let Some(name) = child(&v, NAME) else { continue };
            let text = name_text(&name).unwrap_or_default();
            if !seen.insert(text.clone()) {
                self.error(name.text_range(), "duplicate-variant", format!("`{text}` is declared twice in this enum"));
            }
        }
    }

    fn inherited_fields(&self, mut parent: Option<BindingId>) -> Vec<String> {
        let mut out = Vec::new();
        let mut guard = 0;
        while let Some(p) = parent {
            guard += 1;
            if guard > 64 {
                break;
            }
            match self.struct_fields.get(&p) {
                Some((pp, fields)) => {
                    out.extend(fields.iter().cloned());
                    parent = *pp;
                }
                None => break,
            }
        }
        out
    }

    fn walk_interface(&mut self, node: &SyntaxNode) {
        let id = child(node, NAME).and_then(|n| self.res.decls.get(&n.text_range()).copied());
        self.push_scope();
        if let Some(tp) = child(node, TYPE_PARAMS) {
            self.declare_type_params(&tp);
        }
        self.self_targets.push(id.map(SelfTarget::Binding));
        for m in node.children() {
            match m.kind() {
                IFACE_METHOD => {
                    self.push_scope();
                    if let Some(tp) = child(&m, TYPE_PARAMS) {
                        self.declare_type_params(&tp);
                    }
                    for t in m.descendants().filter(|d| is_type(d.kind()) && d.parent().is_some_and(|p| !is_type(p.kind()))) {
                        self.resolve_type(&t);
                    }
                    self.pop_scope();
                }
                IFACE_FIELD => {
                    if let Some(t) = type_child(&m) {
                        self.resolve_type(&t);
                    }
                }
                _ => {}
            }
        }
        self.self_targets.pop();
        self.pop_scope();
    }

    fn walk_interface_impl(&mut self, node: &SyntaxNode) {
        let types: Vec<SyntaxNode> = node.children().filter(|c| is_type(c.kind())).collect();
        for t in &types {
            self.resolve_type(t);
        }
        let target = types.get(1).and_then(|t| child(t, QUAL_NAME)).and_then(|q| self.res.type_refs.get(&q.text_range()).cloned());
        let self_target = match target {
            Some(Ref::Binding(id)) => Some(SelfTarget::Binding(id)),
            Some(Ref::Prelude(p)) => Some(SelfTarget::Prelude(p)),
            _ => None,
        };
        self.push_scope();
        // `interface I for Box` implements `I` for every `Box<T>`: the
        // struct's own type parameters are in scope under their declared
        // names.
        if let Some(Ref::Binding(id)) = target {
            if let Some(tp) = self.struct_type_params(node, id) {
                self.declare_type_params(&tp);
            }
            if self.binding(id).is_some_and(|b| b.decl == DeclKind::Struct) {
                for f in children(node, FN_DECL) {
                    if let Some(n) = child(&f, NAME) {
                        self.struct_methods.push((id, name_text(&n).unwrap_or_default(), n.text_range()));
                    }
                }
            }
        }
        self.self_targets.push(self_target);
        for f in children(node, FN_DECL) {
            self.walk(&f);
        }
        self.self_targets.pop();
        self.pop_scope();
    }

    /// The `TYPE_PARAMS` of the struct declared in this file under `id`.
    fn struct_type_params(&self, at: &SyntaxNode, id: BindingId) -> Option<SyntaxNode> {
        let b = self.binding(id)?;
        if b.decl != DeclKind::Struct {
            return None;
        }
        let root = at.ancestors().last()?;
        root.descendants()
            .filter(|d| d.kind() == STRUCT_DECL)
            .find(|d| child(d, NAME).is_some_and(|n| n.text_range() == b.range))
            .and_then(|d| child(&d, TYPE_PARAMS))
    }

    fn walk_type_decl(&mut self, node: &SyntaxNode) {
        self.push_scope();
        if let Some(tp) = child(node, TYPE_PARAMS) {
            self.declare_type_params(&tp);
        }
        if let Some(t) = type_child(node) {
            self.resolve_type(&t);
        }
        self.pop_scope();
    }

    fn walk_module(&mut self, node: &SyntaxNode) {
        let Some(q) = child(node, QUAL_NAME) else { return };
        let segs = qual_segments(&q);
        let Some(first) = segs.first() else { return };
        let name = first.text().to_string();
        // `pub module` in a branch of a file-level `if`, declared ahead
        // by `hoist_conditional_pubs`: a new module, not a reopening.
        if let Some(id) = self.res.decls.get(&first.text_range()).copied().filter(|_| segs.len() == 1) {
            self.res.modules.insert(node.text_range(), (id, false));
            self.self_targets.push(Some(SelfTarget::Binding(id)));
            self.modules.push(ModuleCtx { binding: id, fn_depth: self.fns.len() });
            if let Some(b) = block(node) {
                self.walk_block(&b);
            }
            self.modules.pop();
            self.self_targets.pop();
            return;
        }
        let existing = self.lookup_binding(&name);
        if segs.len() == 1 {
            // An imported name is reopened only when it is a module, or the
            // export table of `import from … as Name`.
            if let Some(id) = existing {
                let imported = self.binding(id).is_some_and(|b| b.decl == DeclKind::Import);
                if imported && self.res.declared_as(id) != Some(DeclKind::Module) && self.res.imported.contains_key(&id) {
                    self.error(first.text_range(), "reopen-not-module", format!("`{name}` is imported and is not a module, so `module {name}` cannot reopen it"));
                    return;
                }
            }
        }
        let binding = if segs.len() == 1 {
            match existing.filter(|id| matches!(self.res.declared_as(*id), Some(DeclKind::Module | DeclKind::Import))) {
                Some(id) => {
                    self.res.modules.insert(node.text_range(), (id, true));
                    id
                }
                None => {
                    let id = self.declare(&name, first.text_range(), BindingKind::TypeLevel, DeclKind::Module, false, None, None);
                    self.res.modules.insert(node.text_range(), (id, false));
                    id
                }
            }
        } else {
            match existing {
                Some(root) => {
                    self.res.type_refs.insert(q.text_range(), Ref::Binding(root));
                    let id = self.hidden(&qual_text(&q), q.text_range(), DeclKind::Module);
                    self.res.modules.insert(node.text_range(), (id, false));
                    id
                }
                None => {
                    self.error(first.text_range(), "undeclared-name", format!("`{name}` is not declared, so `module {}` has nowhere to go", qual_text(&q)));
                    return;
                }
            }
        };
        self.self_targets.push(Some(SelfTarget::Binding(binding)));
        self.modules.push(ModuleCtx { binding, fn_depth: self.fns.len() });
        if let Some(b) = block(node) {
            self.walk_block(&b);
        }
        self.modules.pop();
        self.self_targets.pop();
    }

    fn walk_import(&mut self, node: &SyntaxNode) {
        let Some(path_tok) = token(node, STRING) else { return };
        let path = decode_plain_string(&path_tok);
        let exports = match self.imports.exports(&path) {
            Ok(e) => Some(e),
            Err(msg) => {
                self.error(path_tok.text_range(), "import-failed", msg);
                None
            }
        };
        let find = |name: &str| -> Option<ExportInfo> { exports.as_ref()?.iter().find(|e| e.name == name).cloned() };
        if has_token(node, AS_KW) {
            if let Some(name) = child(node, NAME) {
                let text = name_text(&name).unwrap_or_default();
                let earlier = self.before_import(&text, &name);
                let id = self.declare(&text, name.text_range(), BindingKind::TypeLevel, DeclKind::Import, false, None, None);
                self.keep_earlier(&text, earlier);
                if let Some(e) = exports {
                    self.import_exports.insert(id, e);
                    self.import_extensions(&path);
                }
            }
            return;
        }
        if exports.is_some() {
            self.import_extensions(&path);
        }
        let names: Vec<SyntaxNode> = match child(node, IMPORT_LIST) {
            Some(list) => children(&list, NAME).collect(),
            None => children(node, NAME).collect(),
        };
        if names.is_empty() {
            // No form of `import` runs a file without binding a
            // name.
            self.error(
                trimmed_range(node),
                "import-nothing",
                "this import binds no name; every import names what it binds; to run a file for its effects, call `include` or `require`",
            );
            return;
        }
        for name in names {
            let text = name_text(&name).unwrap_or_default();
            let (kind, params, decl) = match find(&text) {
                // An export that may be `nil` is a plain value here, which
                // is never called implicitly: `draw` alone does not call
                // it, `draw?.()` and `if draw` work.
                Some(e) if e.optional && e.kind == BindingKind::Function => (BindingKind::Variable, e.params, Some(e.decl)),
                Some(e) => (e.kind, e.params, Some(e.decl)),
                None => {
                    if exports.is_some() {
                        self.error(name.text_range(), "not-exported", format!("`{text}` is not exported by '{path}'"));
                    }
                    let capitalized = text.chars().next().is_some_and(|c| c.is_ascii_uppercase());
                    (if capitalized { BindingKind::TypeLevel } else { BindingKind::Variable }, None, None)
                }
            };
            let earlier = self.before_import(&text, &name);
            let id = self.declare(&text, name.text_range(), kind, DeclKind::Import, false, None, params);
            self.keep_earlier(&text, earlier);
            if let Some(decl) = decl {
                self.res.imported.insert(id, decl);
            }
        }
    }

    /// The functions the imported file adds to declared modules and
    /// types, which running it defines for the whole Lua state: this file
    /// knows them as it knows the declaration's own members, and passes
    /// them on to the files that import it.
    fn import_extensions(&mut self, path: &str) {
        for mut e in self.imports.extensions(path) {
            if e.origin.is_none() {
                e.origin = Some(path.to_string());
            }
            if !self.res.extensions.iter().any(|x| x.owner == e.owner && x.name == e.name) {
                self.res.extensions.push(e);
            }
        }
    }

    /// Before an import declares `text`: what the file scope already binds
    /// to that name. A hoisted file-level declaration (`fn`, `struct`, …)
    /// clashes with the import (see [`Self::import_clash`]); an earlier
    /// import of the same name is an error at this one.
    fn before_import(&mut self, text: &str, name: &SyntaxNode) -> Option<BindingId> {
        let earlier = self.scopes.last().and_then(|sc| sc.names.get(text)).copied()?;
        let (decl, range) = self.binding(earlier).map(|b| (b.decl, b.range))?;
        if decl == DeclKind::Import {
            self.error(name.text_range(), "import-twice", format!("`{text}` is imported twice; an import binds a name once"));
        } else {
            self.import_clash(text, range);
        }
        Some(earlier)
    }

    /// The file-level declaration is found before the import, so
    /// it keeps the name. (An earlier import keeps it too.)
    fn keep_earlier(&mut self, text: &str, earlier: Option<BindingId>) {
        if let (Some(id), Some(scope)) = (earlier, self.scopes.last_mut()) {
            scope.names.insert(text.to_string(), id);
        }
    }

    // ---- control flow -----------------------------------------------------

    fn walk_loop(&mut self, node: &SyntaxNode) {
        for c in node.children() {
            if c.kind() == BLOCK {
                if let Some(f) = self.fns.last_mut() {
                    f.loop_depth += 1;
                }
                self.walk_block(&c);
                if let Some(f) = self.fns.last_mut() {
                    f.loop_depth -= 1;
                }
            } else {
                self.walk(&c);
            }
        }
    }

    fn walk_for(&mut self, node: &SyntaxNode) {
        // The iterable and the step are outside the loop's scope.
        let mut seen_in = false;
        let mut pats = Vec::new();
        for el in node.children_with_tokens() {
            match el {
                rowan::NodeOrToken::Token(t) if t.kind() == IN_KW => seen_in = true,
                rowan::NodeOrToken::Node(n) => {
                    if !seen_in && is_pattern(n.kind()) {
                        pats.push(n);
                    } else if n.kind() == FOR_STEP || (seen_in && is_expr(n.kind())) {
                        self.walk(&n);
                    }
                }
                _ => {}
            }
        }
        self.push_scope();
        for p in &pats {
            self.declare_pattern(p, DeclKind::LoopVar, true, None, None);
        }
        if let Some(b) = block(node) {
            if let Some(f) = self.fns.last_mut() {
                f.loop_depth += 1;
            }
            self.walk_block(&b);
            if let Some(f) = self.fns.last_mut() {
                f.loop_depth -= 1;
            }
        }
        self.pop_scope();
    }

    fn walk_break(&mut self, node: &SyntaxNode) {
        let word = if node.kind() == BREAK_STMT { "break" } else { "continue" };
        let (depth, kind) = self.fns.last().map_or((0, FnKind::File), |f| (f.loop_depth, f.kind));
        if depth == 0 {
            let msg = if kind == FnKind::Spawn {
                format!("`{word}` cannot cross into a `spawn do` block")
            } else {
                format!("`{word}` outside a loop")
            };
            self.error(trimmed_range(node), "break-outside-loop", msg);
        }
        self.walk_children(node);
    }

    fn walk_if(&mut self, node: &SyntaxNode) {
        self.walk_cond_and_block(node);
        for c in node.children() {
            match c.kind() {
                ELSEIF_CLAUSE => self.walk_cond_and_block(&c),
                ELSE_CLAUSE => self.walk_children(&c),
                _ => {}
            }
        }
    }

    /// The condition and block of an `if` or `elseif`; `let` conditions
    /// bind their names for the block only.
    fn walk_cond_and_block(&mut self, node: &SyntaxNode) {
        let body = block(node);
        if let Some(lc) = child(node, LET_CONDITION) {
            if let Some(e) = expr_child(&lc) {
                self.walk(&e);
            }
            self.push_scope();
            if let Some(p) = pattern_children(&lc).next() {
                self.declare_pattern(&p, DeclKind::PatternVar, true, None, None);
            }
            if let Some(b) = &body {
                self.walk_block(b);
            }
            self.pop_scope();
        } else {
            if let Some(e) = expr_child(node) {
                self.walk(&e);
            }
            if let Some(b) = &body {
                self.walk_block(b);
            }
        }
    }

    fn walk_case(&mut self, node: &SyntaxNode) {
        for c in node.children() {
            match c.kind() {
                WHEN_CLAUSE => {
                    self.push_scope();
                    // The items of one `when` are alternatives,
                    // so each must bind the same names, and they bind one
                    // variable per name.
                    let mut first: Option<Vec<String>> = None;
                    let mut shared: HashMap<String, BindingId> = HashMap::new();
                    for item in c.children() {
                        let bound = match item.kind() {
                            k if is_pattern(k) => {
                                self.share = Some(shared.clone());
                                self.recording = Some(Vec::new());
                                self.declare_pattern(&item, DeclKind::PatternVar, true, None, None);
                                self.share = None;
                                let mut names = self.recording.take().unwrap_or_default();
                                for n in &names {
                                    if let Some(id) = self.lookup_binding(n) {
                                        shared.entry(n.clone()).or_insert(id);
                                    }
                                }
                                names.sort();
                                names.dedup();
                                Some(names)
                            }
                            WHEN_GUARD | BLOCK => {
                                self.walk(&item);
                                None
                            }
                            k if is_expr(k) => {
                                self.walk(&item);
                                Some(Vec::new())
                            }
                            _ => None,
                        };
                        let Some(names) = bound else { continue };
                        match &first {
                            None => first = Some(names),
                            Some(f) if *f != names => {
                                let show = |v: &[String]| if v.is_empty() { "no names".to_string() } else { v.iter().map(|n| format!("`{n}`")).collect::<Vec<_>>().join(", ") };
                                let msg = format!(
                                    "every item of a `when` with several items must bind the same names: the first binds {}, this one {}",
                                    show(f),
                                    show(&names)
                                );
                                self.error(trimmed_range(&item), "when-items-bind", msg);
                            }
                            Some(_) => {}
                        }
                    }
                    self.pop_scope();
                }
                _ => self.walk(&c),
            }
        }
    }

    fn walk_try(&mut self, node: &SyntaxNode) {
        for c in node.children() {
            if c.kind() == CATCH_CLAUSE {
                self.push_scope();
                let ty = type_child(&c);
                if let Some(t) = &ty {
                    self.resolve_type(t);
                }
                if let Some(name) = child(&c, NAME) {
                    let text = name_text(&name).unwrap_or_default();
                    self.declare(&text, name.text_range(), BindingKind::Variable, DeclKind::CatchVar, true, None, None);
                }
                if let Some(b) = block(&c) {
                    self.walk_block(&b);
                }
                self.pop_scope();
            } else {
                self.walk(&c);
            }
        }
    }

    // ---- assignment ---------------------------------------------------------

    fn walk_assign(&mut self, node: &SyntaxNode) {
        let targets: Vec<SyntaxNode> = match child(node, TARGET_LIST) {
            Some(list) => expr_children(&list).collect(),
            None => expr_children(node).take(1).collect(),
        };
        // Values: every expression child after the targets, and modifiers.
        for c in node.children() {
            if targets.iter().any(|t| t == &c) || c.kind() == TARGET_LIST {
                continue;
            }
            self.walk(&c);
        }
        for t in &targets {
            self.walk_target(t);
        }
    }

    fn walk_target(&mut self, target: &SyntaxNode) {
        match target.kind() {
            NAME_REF => {
                let name = name_text(target).unwrap_or_default();
                let range = target.text_range();
                match self.lookup_value(&name) {
                    Some(Ref::Binding(id)) => {
                        self.res.refs.insert(range, Ref::Binding(id));
                        let Some(b) = self.binding(id).cloned() else { return };
                        self.res.reassigned.insert(id);
                        if b.function != self.current_fn() {
                            self.res.assigned_in_closure.insert(id);
                        }
                        match b.decl {
                            DeclKind::Const => self.error(range, "assign-const", format!("`{name}` is a constant; a constant is immutable")),
                            // A name declared with `fn` is a named function:
                            // its value is fixed, as a worker
                            // finds it by name. `let f = fn … end` declares
                            // a variable that can be replaced.
                            DeclKind::Fn => self.error(
                                range,
                                "assign-fn",
                                format!("`{name}` is declared with `fn` and cannot be assigned; declare it with `let {name} = fn … end` to replace it later"),
                            ),
                            DeclKind::Struct | DeclKind::Enum | DeclKind::Interface | DeclKind::TypeAlias | DeclKind::Module | DeclKind::TypeParam => {
                                self.error(range, "assign-type", format!("`{name}` is a type-level name; only variables and globals can be assigned"))
                            }
                            DeclKind::Import => self.error(range, "assign-import", format!("`{name}` is imported; only variables and globals can be assigned")),
                            _ => {}
                        }
                        self.check_spawn_capture_write(&b, range);
                    }
                    Some(Ref::Prelude(p)) => {
                        self.error(range, "assign-prelude", format!("`{}` is a prelude name and cannot be assigned; a Lua global is assigned through a Lua path (`:_G.{} = v`)", p.name, p.name));
                    }
                    Some(Ref::Declared(id)) => {
                        self.res.refs.insert(range, Ref::Declared(id));
                        match self.decls.get(id).map(|i| i.kind) {
                            Some(DeclItemKind::Let) => {}
                            Some(DeclItemKind::Const) => self.error(range, "assign-declared-const", format!("`{name}` is declared `const` in a declaration file and cannot be assigned")),
                            _ => self.error(range, "assign-declared", format!("`{name}` is declared in a declaration file as something other than a `let`, and cannot be assigned")),
                        }
                    }
                    _ => self.error(range, "undeclared-name", format!("`{name}` is not declared; assigning to an undeclared name is an error, as reading one is")),
                }
            }
            FIELD_EXPR => {
                if field_name(target).is_some_and(|t| t.text() == "len") {
                    self.error(trimmed_range(target), "len-assign", "`x.len` is the length, not a member, and cannot be assigned; use `t[\"len\"]` for a key of that name");
                }
                self.walk_children(target);
                // `Enum.Variant = v`: a variant is a constant of its enum,
                // as in Rust and TypeScript.
                if let Some(base) = expr_child(target).filter(|b| b.kind() == NAME_REF) {
                    if let Some(Ref::Binding(id)) = self.res.refs.get(&base.text_range()).cloned() {
                        if self.res.declared_as(id) == Some(DeclKind::Enum) {
                            let name = name_text(&base).unwrap_or_default();
                            let member = field_name(target).map(|t| t.text().to_string()).unwrap_or_default();
                            // The variants of an imported enum are not known.
                            let not_variant = self.enum_variants.get(&id).is_some_and(|vs| !vs.contains(&member));
                            if not_variant {
                                self.error(trimmed_range(target), "assign-variant", format!("`{member}` is not a variant of the enum `{name}`"));
                            } else {
                                self.error(trimmed_range(target), "assign-variant", format!("`{name}.{member}` is a variant of the enum `{name}`; variants are constants and cannot be assigned"));
                            }
                        }
                    }
                }
            }
            AT_EXPR => self.walk_at(target),
            _ => self.walk(target),
        }
    }

    /// A local of an enclosing function, captured by a `spawn
    /// do` block, cannot be assigned in it.
    fn check_spawn_capture_write(&mut self, b: &Binding, range: TextRange) {
        if b.file_level || b.decl == DeclKind::Global {
            return;
        }
        let captured = self.fns.iter().any(|f| f.kind == FnKind::Spawn && f.index > b.function);
        if captured {
            self.error(range, "spawn-capture-assign", format!("`{}` is captured by a `spawn do` block; assigning to it would change only the worker's copy", b.name));
        }
    }

    /// The count of a rest parameter of an enclosing function, read in a
    /// `spawn do` block (`args.len`, `*args`): the block takes a copy of
    /// the table, which does not carry the count (the lowering keeps it in
    /// a hidden local, or beside the table), so a trailing nil would be
    /// lost. An error that names the parameter.
    fn check_spawn_rest_count(&mut self, id: BindingId, node: &SyntaxNode) {
        let Some(b) = self.binding(id) else { return };
        if b.decl != DeclKind::RestParam || !self.fns.iter().any(|f| f.kind == FnKind::Spawn && f.index > b.function) {
            return;
        }
        let Some(p) = node.parent() else { return };
        let reads_count = match p.kind() {
            FIELD_EXPR => expr_child(&p).is_some_and(|e| &e == node) && field_name(&p).is_some_and(|t| t.text() == "len"),
            SPREAD_ARG => true,
            _ => false,
        };
        if reads_count {
            let name = b.name.clone();
            self.error(
                node.text_range(),
                "spawn-rest-count",
                format!("this `spawn do` block reads the count of the rest parameter `{name}`, which the block does not take along with its items: make a list of the arguments outside the block (`let items = [*{name}]`) and read that"),
            );
        }
    }

    // ---- names --------------------------------------------------------------

    fn walk_name_ref(&mut self, node: &SyntaxNode) {
        let name = name_text(node).unwrap_or_default();
        let range = node.text_range();
        match self.lookup_value(&name) {
            Some(r) => {
                if let Ref::Binding(id) = r {
                    self.check_const_order(id, &name, range);
                    self.check_spawn_rest_count(id, node);
                }
                self.res.refs.insert(range, r);
            }
            None => {
                let msg = match prelude::lua_library_hint(&name) {
                    Some(hint) => hint.to_string(),
                    None if prelude::lookup(&name).is_some() => format!("`{name}` is a type, not a value"),
                    None => format!("`{name}` is not declared in this file, an import or the prelude; NL has no fallback to Lua globals (use a Lua path, `:{name}`, for one)"),
                };
                self.error(range, "undeclared-name", msg);
            }
        }
    }

    /// A read of a hoisted `const` before its line: an error in the
    /// const's own function, where it runs before the value is assigned;
    /// from another function (which may run later) it is allowed, and the
    /// lowering then declares the local ahead ([`Resolution::forward_consts`]).
    fn check_const_order(&mut self, id: BindingId, name: &str, range: TextRange) {
        let Some(decl) = self.hoisted_consts.get(&id).copied() else { return };
        if range.start() >= decl.end() {
            return;
        }
        let own = self.binding(id).is_some_and(|b| b.function == self.current_fn());
        if own {
            self.error(range, "const-before-definition", format!("`{name}` is read before its `const` declaration, which gives it its value; move the `const` above this line (a function may use it, since it runs later)"));
        } else {
            self.res.forward_consts.insert(id);
        }
    }

    fn self_binding(&self) -> Option<BindingId> {
        self.fns.iter().rev().find_map(|f| f.self_binding)
    }

    fn walk_self(&mut self, node: &SyntaxNode) {
        let range = node.text_range();
        if has_token(node, SELF_TYPE_KW) {
            match self.self_targets.last().cloned().flatten() {
                Some(t) => {
                    self.res.self_refs.insert(range, t);
                }
                None => self.error(range, "self-type-outside", "`Self` is only available in `fn Type.name`, a struct, module or interface body, or an interface implementation"),
            }
        } else {
            match self.self_binding() {
                Some(id) => {
                    self.res.refs.insert(range, Ref::Binding(id));
                }
                None => self.error(range, "self-outside", "`self` is only available in a method"),
            }
        }
    }

    fn walk_at(&mut self, node: &SyntaxNode) {
        let range = node.text_range();
        if token(node, AT_NAME).is_some_and(|t| t.text() == "@len") {
            self.error(range, "len-field", "`len` cannot be a field, so `@len` is an error");
        }
        match self.self_binding() {
            Some(id) => {
                self.res.refs.insert(range, Ref::Binding(id));
            }
            None => self.error(range, "self-outside", "`@name` is `self.name`, and `self` is only available in a method"),
        }
    }

    fn walk_lua_path(&mut self, node: &SyntaxNode) {
        if let Some(root) = lua_path_root(node) {
            let name = root.text().to_string();
            // The first name is a local if one is in scope,
            // otherwise the Lua global.
            let r = match self.lookup_binding(&name) {
                Some(id) => Ref::Binding(id),
                None => Ref::LuaGlobal(name),
            };
            self.res.refs.insert(root.text_range(), r);
        }
        self.walk_children(node);
    }

    fn walk_field(&mut self, node: &SyntaxNode) {
        self.walk_children(node);
    }

    fn walk_bang(&mut self, node: &SyntaxNode) {
        if let Some(inner) = expr_child(node) {
            if inner.kind() == FIELD_EXPR && field_name(&inner).is_some_and(|t| t.text() == "len") {
                self.error(trimmed_range(node), "len-bang", "`x.len!` is an error: `len` is the length, not looked up in the value, so there is no function to take");
            }
        }
        self.walk_children(node);
    }

    fn walk_call(&mut self, node: &SyntaxNode) {
        self.walk_children(node);
        let callee = expr_child(node);
        if let Some(c) = &callee {
            if node.kind() != METHOD_CALL && c.kind() == FIELD_EXPR && field_name(c).is_some_and(|t| t.text() == "len") {
                self.error(trimmed_range(c), "len-call", "`len` is the length, not a callable member");
            }
        }
        let Some(tb) = child(node, TRAILING_BLOCK) else { return };
        if node.kind() == METHOD_CALL {
            return;
        }
        let Some(c) = callee else { return };
        let block_range = token(&tb, DO_KW).map_or_else(|| tb.text_range(), |t| t.text_range());
        match c.kind() {
            NAME_REF => {
                if let Some(Ref::Binding(id)) = self.res.refs.get(&c.text_range()).cloned() {
                    if let Some(b) = self.binding(id) {
                        if b.params.is_some_and(|p| !p.has_function_param) {
                            let name = b.name.clone();
                            self.error(block_range, "block-without-function-param", format!("a block can only be passed to a function that has a `Function` parameter, and `{name}` has none"));
                        }
                    }
                }
            }
            LUA_PATH => {
                self.error(block_range, "block-on-lua-path", "a Lua path that no declaration file describes takes no block; pass `fn ... end` as an argument");
            }
            // `M.name … do … end` with `M` a module of this file or an
            // `import … as M`: the parameters are known once the file is
            // (only callees whose parameter list is declared).
            FIELD_EXPR => {
                let recv = expr_child(&c).filter(|r| r.kind() == NAME_REF);
                let member = field_name(&c).map(|t| t.text().to_string());
                if let (Some(recv), Some(member)) = (recv, member) {
                    if let Some(Ref::Binding(id)) = self.res.refs.get(&recv.text_range()).cloned() {
                        self.pending_blocks.push((block_range, id, member));
                    }
                }
            }
            _ => {}
        }
    }

    /// The trailing blocks on module functions (see [`Self::walk_call`]).
    fn check_pending_blocks(&mut self) {
        for (range, id, member) in std::mem::take(&mut self.pending_blocks) {
            let decl = self.res.declared_as(id);
            let params = match decl {
                Some(DeclKind::Module) => self.module_members.get(&id).and_then(|m| m.get(&member)).copied(),
                Some(DeclKind::Import) => self.import_exports.get(&id).and_then(|e| e.iter().find(|x| x.name == member)).and_then(|x| x.params),
                _ => None,
            };
            if params.is_some_and(|p| !p.has_function_param) {
                let owner = self.binding(id).map(|b| b.name.clone()).unwrap_or_default();
                self.error(range, "block-without-function-param", format!("a block can only be passed to a function that has a `Function` parameter, and `{owner}.{member}` has none"));
            }
        }
    }

    fn walk_is_as(&mut self, node: &SyntaxNode, is_as: bool) {
        for c in node.children() {
            if is_type(c.kind()) {
                self.resolve_type(&c);
                if let Some(at) = self.type_param_in(&c) {
                    if is_as {
                        self.error(at, "as-type-param", "`as` with a type parameter: type parameters do not exist at runtime");
                    } else {
                        self.error(at, "is-type-param", "`is` with a type parameter: the type after `is` must be checked at runtime, and type parameters do not exist at runtime");
                    }
                }
            } else {
                self.walk(&c);
            }
        }
    }

    /// The range of a type parameter that a type tests at runtime: one
    /// that stands for the value or a part of it (a union member, an
    /// optional, a record field), not a type argument, the element type of
    /// `[T]` or `{[K]: V}`, or a part of a function signature, which are
    /// static only.
    fn type_param_in(&self, ty: &SyntaxNode) -> Option<TextRange> {
        match ty.kind() {
            FN_TYPE | ARRAY_TYPE | MAP_TYPE => None,
            NAMED_TYPE => {
                let q = child(ty, QUAL_NAME)?;
                if qual_segments(&q).len() != 1 {
                    return None;
                }
                match self.res.type_refs.get(&q.text_range()) {
                    Some(Ref::Binding(id)) if self.binding(*id).is_some_and(|b| b.decl == DeclKind::TypeParam) => Some(trimmed_range(ty)),
                    _ => None,
                }
            }
            _ => ty.children().find_map(|c| {
                if is_type(c.kind()) {
                    self.type_param_in(&c)
                } else {
                    c.children().filter(|d| is_type(d.kind())).find_map(|d| self.type_param_in(&d))
                }
            }),
        }
    }

    /// Records the file's `pub` items ([`Resolution::exports`]): the name,
    /// binding kind and declaration kind of each, read off the bindings the
    /// walk made. A name exported twice is an error at the second export,
    /// and so is `pub module Outer.Inner`: an import binds a plain name, so
    /// a nested module is exported with the module it belongs to.
    pub(crate) fn collect_exports(&mut self, root: &SyntaxNode) {
        let Some(file_block) = block(root) else { return };
        let mut seen: HashSet<String> = HashSet::new();
        // The file's own items, and those in the branches of its `if`s.
        let mut items = Vec::new();
        for s in statements(&file_block) {
            match file_branch_construct(&s) {
                Some(c) => {
                    for b in branch_blocks(&c) {
                        items.extend(statements(&b));
                    }
                }
                None => items.push(s),
            }
        }
        for s in items {
            if !is_exported(&s) {
                continue;
            }
            let mut found: Vec<(BindingId, TextRange, DeclKind)> = Vec::new();
            let mut declared = |res: &Resolution, name: &SyntaxNode, decl: DeclKind| {
                if let Some(id) = res.decls.get(&name.text_range()) {
                    found.push((*id, name.text_range(), decl));
                }
            };
            match s.kind() {
                FN_DECL if child(&s, QUAL_NAME).is_none() => {
                    if let Some(n) = child(&s, NAME) {
                        declared(&self.res, &n, DeclKind::Fn);
                    }
                }
                CONST_DECL => {
                    for p in pattern_children(&s) {
                        let bind = if p.kind() == TYPED_PAT { pattern_children(&p).next() } else { Some(p) };
                        if let Some(n) = bind.filter(|b| b.kind() == BIND_PAT).and_then(|b| child(&b, NAME)) {
                            declared(&self.res, &n, DeclKind::Const);
                        }
                    }
                }
                STRUCT_DECL | ENUM_DECL | INTERFACE_DECL | TYPE_DECL => {
                    let decl = match s.kind() {
                        STRUCT_DECL => DeclKind::Struct,
                        ENUM_DECL => DeclKind::Enum,
                        INTERFACE_DECL => DeclKind::Interface,
                        _ => DeclKind::TypeAlias,
                    };
                    if let Some(n) = child(&s, NAME) {
                        declared(&self.res, &n, decl);
                    }
                }
                MODULE_DECL => {
                    let Some(q) = child(&s, QUAL_NAME) else { continue };
                    if qual_segments(&q).len() > 1 {
                        let outer = qual_segments(&q).first().map(|t| t.text().to_string()).unwrap_or_default();
                        let kw = if has_token(&s, PUB_KW) { "pub" } else { "global" };
                        self.error(
                            q.text_range(),
                            "pub-nested-module",
                            format!("`{kw} module {}`: an import binds a plain name, so a nested module is exported with the module it belongs to; write `{kw} module {outer}`", qual_text(&q)),
                        );
                        continue;
                    }
                    // A module reopening an imported one is exported as a
                    // module too.
                    if let Some((id, _)) = self.res.modules.get(&s.text_range()) {
                        found.push((*id, q.text_range(), DeclKind::Module));
                    }
                }
                _ => {}
            }
            for (id, range, decl) in found {
                let Some(b) = self.binding(id).cloned() else { continue };
                // One name in several branches of one `if` is one export.
                if self.res.exports.iter().any(|(e, _)| *e == id) {
                    continue;
                }
                if !seen.insert(b.name.clone()) {
                    self.error(range, "duplicate-export", format!("`{}` is exported twice; a file exports one item per name", b.name));
                    continue;
                }
                let optional = self.res.conditional_export(id).is_some_and(|(c, clauses)| !crate::profile::defined_on_every_path(c, clauses, &self.res, crate::profile::Profile::Full));
                self.res.exports.push((id, ExportInfo { name: b.name, kind: b.kind, params: b.params, decl, optional }));
            }
        }
    }

    // ---- types ----------------------------------------------------------------

    /// Resolves the names in a type expression.
    pub(crate) fn resolve_type(&mut self, ty: &SyntaxNode) {
        match ty.kind() {
            // `(A, B)` lists the values a function returns;
            // a variable or a parameter holds one value.
            TUPLE_TYPE if ty.parent().is_none_or(|p| p.kind() != RETURN_TYPE) => {
                self.error(ty.text_range(), "tuple-type-position", "`(A, B)` names several return values and is only a return type; a variable, a parameter or a field holds one value");
                for c in ty.children().filter(|c| is_type(c.kind())) {
                    self.resolve_type(&c);
                }
            }
            NAMED_TYPE => {
                if has_token(ty, SELF_TYPE_KW) {
                    match self.self_targets.last().cloned().flatten() {
                        Some(t) => {
                            self.res.self_refs.insert(ty.text_range(), t);
                        }
                        None => self.error(ty.text_range(), "self-type-outside", "`Self` is only available in `fn Type.name`, a struct, module or interface body, or an interface implementation"),
                    }
                }
                if let Some(q) = child(ty, QUAL_NAME) {
                    self.resolve_qual_type(&q);
                }
                if let Some(args) = child(ty, TYPE_ARGS) {
                    for t in args.children().filter(|c| is_type(c.kind())) {
                        self.resolve_type(&t);
                    }
                }
            }
            _ => {
                for c in ty.children() {
                    if is_type(c.kind()) {
                        self.resolve_type(&c);
                    } else {
                        for t in c.children().filter(|d| is_type(d.kind())) {
                            self.resolve_type(&t);
                        }
                    }
                }
            }
        }
    }

    /// Resolves the first segment of a type name.
    fn resolve_qual_type(&mut self, q: &SyntaxNode) {
        let segs = qual_segments(q);
        let Some(first) = segs.first() else { return };
        if first.kind() == SELF_TYPE_KW {
            match self.self_targets.last().cloned().flatten() {
                Some(t) => {
                    self.res.self_refs.insert(q.text_range(), t);
                }
                None => self.error(q.text_range(), "self-type-outside", "`Self` is only available in `fn Type.name`, a struct, module or interface body, or an interface implementation"),
            }
            return;
        }
        let name = first.text().to_string();
        if let Some(id) = self.lookup_binding(&name) {
            let ok = segs.len() > 1 || self.binding(id).is_some_and(|b| b.kind == BindingKind::TypeLevel);
            if !ok {
                self.error(q.text_range(), "not-a-type", format!("`{name}` is not a type"));
            }
            self.res.type_refs.insert(q.text_range(), Ref::Binding(id));
            return;
        }
        match prelude::lookup(&name) {
            Some(p) if p.is_type() || segs.len() > 1 => {
                self.res.type_refs.insert(q.text_range(), Ref::Prelude(p));
            }
            _ => {
                // A declared struct or host type, or a member of a declared
                // module.
                if let Some(id) = self.decls.lookup(&name).filter(|id| self.decls.is_type(*id) || segs.len() > 1) {
                    if let Some(root) = self.decls.get(id).and_then(|i| i.root.clone()) {
                        self.res.decl_paths.entry(id).or_insert(root);
                    }
                    if let Some(kind) = self.decls.get(id).map(|i| i.kind) {
                        self.res.decl_kinds.insert(id, kind);
                    }
                    self.res.type_refs.insert(q.text_range(), Ref::Declared(id));
                    return;
                }
                self.error(q.text_range(), "undeclared-type", format!("the type `{}` is not declared", qual_text(q)));
            }
        }
    }

    /// A dotted name in a pattern (`Direction.North`): its first
    /// segment is a value or a type.
    fn resolve_qualified_value(&mut self, q: &SyntaxNode) {
        let segs = qual_segments(q);
        let Some(first) = segs.first() else { return };
        let name = first.text().to_string();
        match self.lookup_value(&name).or_else(|| prelude::lookup(&name).map(Ref::Prelude)) {
            Some(r) => {
                // An enum variant is compared; anything else
                // names a type. A variant is `Enum.Variant`, or
                // `Lib.Enum.Variant` through `import from … as Lib`.
                let variant = match &r {
                    Ref::Binding(id) => match self.res.declared_as(*id) {
                        Some(DeclKind::Enum) => segs.len() == 2,
                        Some(DeclKind::Import) => {
                            let second = segs.get(1).map(|t| t.text().to_string());
                            let exported = self.import_exports.get(id).and_then(|e| e.iter().find(|x| Some(&x.name) == second.as_ref()));
                            segs.len() == 3 && exported.is_some_and(|x| x.decl == DeclKind::Enum)
                        }
                        _ => false,
                    },
                    _ => false,
                };
                if variant {
                    self.res.variants.insert(q.text_range());
                }
                self.res.type_refs.insert(q.text_range(), r);
            }
            None => self.error(q.text_range(), "undeclared-name", format!("`{name}` is not declared")),
        }
    }
}

/// The first-name token of a `LUA_PATH`.
pub fn lua_path_root(node: &SyntaxNode) -> Option<SyntaxToken> {
    let mut after_colon = false;
    for t in tokens(node) {
        if t.kind() == COLON {
            after_colon = true;
            continue;
        }
        if after_colon && (t.kind().is_name() || t.kind().is_keyword()) {
            return Some(t);
        }
        if after_colon {
            return None;
        }
    }
    None
}

/// True when a function node declares a `Function` parameter:
/// `Function`, a `Function(...)` signature, or either made optional with
/// `?` (an optional callback). A union with other members does not count.
pub fn has_function_param(node: &SyntaxNode) -> bool {
    let Some(list) = child(node, PARAM_LIST).or_else(|| child(node, BLOCK_PARAMS)) else { return false };
    children(&list, PARAM).any(|p| {
        pattern_children(&p).next().is_some_and(|pat| pat.kind() == TYPED_PAT && type_child(&pat).is_some_and(|t| takes_block(&t)))
    })
}

fn takes_block(ty: &SyntaxNode) -> bool {
    match ty.kind() {
        OPTIONAL_TYPE | PAREN_TYPE => type_child(ty).is_some_and(|t| takes_block(&t)),
        _ => is_function_type(ty),
    }
}

/// The value of a `STRING` token without interpolation.
pub fn decode_plain_string(tok: &SyntaxToken) -> String {
    let parts = nl_syntax::decode_string(&[tok.text()]);
    String::from_utf8_lossy(parts.first().map_or(&[][..], |v| v.as_slice())).into_owned()
}

#[allow(dead_code)]
fn is_prelude_struct(p: &Prelude) -> bool {
    matches!(p.class, crate::prelude::PreludeClass::Struct)
}

/// A `const` pattern that `Resolver::hoist_consts` declares ahead: a
/// name, or a name with a type. The name node and the type node.
fn simple_const_pattern(pat: &SyntaxNode) -> Option<(SyntaxNode, Option<SyntaxNode>)> {
    match pat.kind() {
        BIND_PAT => Some((child(pat, NAME)?, None)),
        TYPED_PAT => {
            let inner = pattern_children(pat).next().filter(|p| p.kind() == BIND_PAT)?;
            Some((child(&inner, NAME)?, type_child(pat)))
        }
        _ => None,
    }
}

/// The names a statement declares in its block's scope (an
/// approximation for `Resolver::hoist_consts`: every name in the patterns
/// of `let` and `const`, and the name of a declaration).
fn statement_names(s: &SyntaxNode) -> Vec<String> {
    match s.kind() {
        LET_DECL | CONST_DECL => pattern_children(s).flat_map(|p| p.descendants().filter(|n| n.kind() == NAME).collect::<Vec<_>>()).filter_map(|n| name_text(&n)).collect(),
        FN_DECL if child(s, QUAL_NAME).is_some() => Vec::new(),
        FN_DECL | STRUCT_DECL | ENUM_DECL | INTERFACE_DECL | TYPE_DECL | GLOBAL_DECL => child(s, NAME).and_then(|n| name_text(&n)).into_iter().collect(),
        MODULE_DECL => child(s, QUAL_NAME).and_then(|q| qual_segments(&q).first().map(|t| t.text().to_string())).into_iter().collect(),
        IMPORT_DECL => s.descendants().filter(|n| n.kind() == NAME).filter_map(|n| name_text(&n)).collect(),
        _ => Vec::new(),
    }
}

/// The `if` or `unless` of a file-level statement `if … end` (no
/// modifier), whose branches may hold `pub` items.
pub(crate) fn file_branch_construct(s: &SyntaxNode) -> Option<SyntaxNode> {
    if s.kind() != EXPR_STMT || modifier(s).is_some() {
        return None;
    }
    expr_child(s).filter(|e| matches!(e.kind(), IF_EXPR | UNLESS_EXPR))
}

/// The branch blocks of an `if` or `unless` in order: its own, then each
/// `elseif` and the `else`.
pub(crate) fn branch_blocks(construct: &SyntaxNode) -> Vec<SyntaxNode> {
    let mut out: Vec<SyntaxNode> = block(construct).into_iter().collect();
    for c in construct.children().filter(|c| matches!(c.kind(), ELSEIF_CLAUSE | ELSE_CLAUSE)) {
        out.extend(block(&c));
    }
    out
}

/// The names a `pub` item declares, as `hoist` would declare them: the
/// declaring range, the name, the binding and declaration kinds, the
/// annotation and what is known of a function's parameters.
#[allow(clippy::type_complexity)]
/// The member `name` of a declared item (`DECL_MODULE`, `DECL_STRUCT`,
/// `DECL_TYPE`): an `fn`, a field, a `const` or a nested declaration.
fn decl_member_node(item: &SyntaxNode, name: &str) -> Option<SyntaxNode> {
    item.children().find(|m| matches!(m.kind(), DECL_FN | DECL_FIELD | DECL_VALUE | DECL_MODULE | DECL_STRUCT | DECL_TYPE) && child(m, NAME).and_then(|n| name_text(&n)).as_deref() == Some(name))
}

fn pub_item_names(item: &SyntaxNode) -> Vec<(TextRange, String, BindingKind, DeclKind, Option<SyntaxNode>, Option<FnParams>)> {
    let mut out = Vec::new();
    match item.kind() {
        FN_DECL if child(item, QUAL_NAME).is_none() => {
            if let Some(n) = child(item, NAME) {
                let params = Some(FnParams { has_function_param: has_function_param(item), anonymous: false, call_only: 0 });
                out.push((n.text_range(), name_text(&n).unwrap_or_default(), BindingKind::Function, DeclKind::Fn, None, params));
            }
        }
        CONST_DECL => {
            let values: Vec<SyntaxNode> = child(item, EXPR_LIST).map(|l| expr_children(&l).collect()).unwrap_or_default();
            for (i, p) in pattern_children(item).enumerate() {
                let Some((n, ty)) = simple_const_pattern(&p) else { continue };
                let func = match values.get(i) {
                    Some(v) if matches!(v.kind(), FN_EXPR | LAMBDA_EXPR) && ty.is_none() => Some(FnParams { has_function_param: has_function_param(v), anonymous: true, call_only: 0 }),
                    _ => None,
                };
                let kind = if func.is_some() || ty.as_ref().is_some_and(is_function_type) { BindingKind::Function } else { BindingKind::Variable };
                out.push((n.text_range(), name_text(&n).unwrap_or_default(), kind, DeclKind::Const, ty, func));
            }
        }
        STRUCT_DECL | ENUM_DECL | INTERFACE_DECL | TYPE_DECL => {
            let decl = match item.kind() {
                STRUCT_DECL => DeclKind::Struct,
                ENUM_DECL => DeclKind::Enum,
                INTERFACE_DECL => DeclKind::Interface,
                _ => DeclKind::TypeAlias,
            };
            if let Some(n) = child(item, NAME) {
                out.push((n.text_range(), name_text(&n).unwrap_or_default(), BindingKind::TypeLevel, decl, None, None));
            }
        }
        MODULE_DECL => {
            if let Some(q) = child(item, QUAL_NAME) {
                let segs = qual_segments(&q);
                if let (1, Some(first)) = (segs.len(), segs.first()) {
                    out.push((first.text_range(), first.text().to_string(), BindingKind::TypeLevel, DeclKind::Module, None, None));
                }
            }
        }
        _ => {}
    }
    out
}
