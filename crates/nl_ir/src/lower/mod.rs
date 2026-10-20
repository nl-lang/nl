//! Lowering: the syntax tree and its [`Resolution`] to the core IR.
//!
//! See the crate documentation for how the work is split between the
//! modules of this directory. This module holds the context, the statement
//! dispatcher and the block machinery (hoisting, tails, temporaries).

mod assign;
mod call;
mod control;
mod error;
mod event;
mod expr;
mod func;
mod iface;
mod import;
mod lift;
mod limits;
mod localize;
mod lua_path;
mod module;
mod nilsafe;
mod opdef;
mod operator;
mod pattern;
mod spawn;
mod structs;
mod task;
mod types;

use std::collections::{HashMap, HashSet};

use nl_sema::util::*;
use nl_sema::{BindingId, Resolution};
use nl_syntax::{LineIndex, SyntaxKind, SyntaxNode, TextRange};

use crate::ir::*;

use SyntaxKind::*;

pub use localize::{LUA_LIBRARY_FIELDS, Localize, STD_LIBRARIES, declared_globals, directive};

/// Options of one lowering.
#[derive(Clone, Debug)]
pub struct LowerOptions {
    /// Emit the runtime type checks. `false` is the unchecked
    /// build.
    pub checks: bool,
    /// The chunk name (`@name` or `=text`).
    pub chunk_name: String,
    /// The file name as messages and handler names give it (the chunk name
    /// without its `@` or `=`).
    pub file_label: String,
    /// What the analyzer proved and resolved; empty without
    /// it, and then everything is tested at runtime.
    pub hints: nl_sema::Hints,
    /// The feature level of the target. At 0,
    /// stock LuaJIT: `x[k]`, `.len`, `==` and the ordering operators go
    /// through runtime helpers that give them NL's rules. At 1, the fork's
    /// VM applies those rules itself, so they lower to plain
    /// index, length and comparison operations.
    pub level: u8,
    /// Which Lua paths are read once, into hidden locals of the file
    /// (localize.rs).
    pub localize: Localize,
    /// The names of the `global` declarations of the files this file
    /// imports, which `Localize::All` leaves alone.
    pub imported_globals: Vec<String>,
    /// The fields of Lua's libraries that the declarations list
    /// (`nl_sema::Declarations::library_fields`), which `Localize::Std`
    /// reads once besides Lua's own (localize.rs).
    pub library_fields: std::collections::HashSet<(String, String)>,
    /// The profile the file is compiled for. In a realm of Garry's Mod, a
    /// branch whose condition realm constants decide (`if SERVER`) is
    /// lowered only if it runs, and the code that never runs is left out
    /// (nl_sema::profile, "Realm constants"; control.rs).
    pub profile: nl_sema::Profile,
}

/// A diagnostic of the lowering: a construct that is not implemented, or
/// an error that only the lowering finds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LowerDiagnostic {
    pub range: TextRange,
    pub code: &'static str,
    pub message: String,
}

/// What a lowering produced. The chunk is only meant to be used when
/// `diagnostics` is empty.
#[derive(Debug)]
pub struct LowerOutput {
    pub chunk: Chunk,
    pub diagnostics: Vec<LowerDiagnostic>,
}

/// Lowers a resolved file. `source` is the text `root` was parsed from.
///
/// When the chunk exceeds one of LuaJIT's limits (limits.rs), the file is
/// lowered again with every `try` in the packed form, which keeps its
/// outcome in one local instead of two or more (error.rs), and that
/// lowering is the result.
pub fn lower(root: &SyntaxNode, source: &str, res: &Resolution, opts: &LowerOptions) -> LowerOutput {
    let out = lower_once(root, source, res, opts, false);
    if out.diagnostics.iter().any(|d| d.code == "vm-limit") {
        return lower_once(root, source, res, opts, true);
    }
    out
}

fn lower_once(root: &SyntaxNode, source: &str, res: &Resolution, opts: &LowerOptions, packed_try: bool) -> LowerOutput {
    let mut l = Lowerer::new(source, res, opts);
    l.packed_try = packed_try;
    // A file's `#!localize` directive overrides the option.
    let mode = match localize::directive(root) {
        Ok(found) => found.unwrap_or(opts.localize),
        Err(range) => {
            l.diags.push(LowerDiagnostic { range, code: "localize-directive", message: "a `#!localize` directive names `none`, `std` or `all`".to_string() });
            opts.localize
        }
    };
    l.localize = localize::plan(root, res, mode, &opts.imported_globals, &opts.library_fields);
    l.collect_decl_names(root);
    l.collect_aliases(root);
    l.collect_registry_names(root);
    let main = l.lower_main(root);
    let mut chunk = Chunk { locals: std::mem::take(&mut l.locals), main, name: opts.chunk_name.clone(), owner: l.owner };
    for (line, message) in lift::lift(&mut chunk) {
        let range = line_range(source, line);
        l.diags.push(LowerDiagnostic { range, code: "spawn-block", message });
    }
    for o in limits::fit(&mut chunk, &l.rt_cache, &l.localized) {
        let range = line_range(source, o.line);
        l.diags.push(LowerDiagnostic { range, code: "vm-limit", message: o.message });
    }
    LowerOutput { chunk, diagnostics: l.diags }
}

/// The range of a line's text, without its indentation (1-based `line`).
fn line_range(source: &str, line: u32) -> TextRange {
    let mut start = 0usize;
    for (i, text) in source.split_inclusive('\n').enumerate() {
        if i + 1 == line as usize {
            let indent = text.len() - text.trim_start().len();
            let body = text.trim_end().len().max(indent);
            let from = nl_syntax::TextSize::new(u32::try_from(start + indent).unwrap_or(0));
            let to = nl_syntax::TextSize::new(u32::try_from(start + body).unwrap_or(0));
            return TextRange::new(from, to.max(from));
        }
        start += text.len();
    }
    TextRange::empty(nl_syntax::TextSize::new(0))
}

/// At most this many operands are spilled to locals of their own; more go
/// to one table ([`Lowerer::spill`]).
const SPILL_LOCALS: usize = 8;

/// Where the value of a block's last statement goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tail {
    /// Nowhere: the block is run for its effects.
    Discard,
    /// Returned from the enclosing function.
    Return,
    /// Assigned to this local (a block expression used as a value).
    Assign(LocalId),
    /// Every value returned from the closure of a protected region, as
    /// `return "n", values…` (a `try` that is a function's tail; error.rs).
    Yield,
}

/// One function being lowered.
pub(crate) struct FnState {
    /// The declared return type and the description used in messages,
    /// when results are checked.
    pub ret_check: Option<(SyntaxNode, String)>,
    pub loops: Vec<LoopState>,
    /// The protected regions (`try` bodies, catches, finally blocks) being
    /// lowered in this function, innermost last.
    pub regions: Vec<error::Region>,
    /// The function's name for messages (`Player.damage`).
    #[allow(dead_code)]
    pub name: String,
}

/// One loop being lowered.
#[derive(Default)]
pub(crate) struct LoopState {
    /// The label at the end of the body, created by the first `continue`.
    pub continue_label: Option<LabelId>,
}

/// The lowering context.
pub struct Lowerer<'a> {
    pub(crate) res: &'a Resolution,
    pub(crate) opts: &'a LowerOptions,
    source: &'a str,
    lines: LineIndex,
    pub(crate) locals: Vec<LocalInfo>,
    bindings: HashMap<BindingId, LocalId>,
    /// Bindings whose `local` statement was emitted at the start of their
    /// block (hoisting); their declarations become assignments.
    predeclared: HashSet<BindingId>,
    next_label: u32,
    pub(crate) stmts: Vec<Stmt>,
    pub(crate) line: u32,
    pub(crate) fns: Vec<FnState>,
    pub(crate) diags: Vec<LowerDiagnostic>,
    /// The type of each `type` alias of the file.
    pub(crate) aliases: HashMap<BindingId, SyntaxNode>,
    /// Locals of bindings that another function assigns, so that a call can
    /// change them between two reads (see [`Self::pin`]).
    volatile: HashSet<LocalId>,
    /// Temporaries made by [`Self::spill`], which nothing assigns again.
    spilled: HashSet<LocalId>,
    /// The type pools of the blocks being lowered, outermost first
    /// (types.rs).
    pub(crate) pools: Vec<types::TypePool>,
    /// The pool depth of each type binding whose block has a pool.
    pub(crate) binding_pool: HashMap<BindingId, usize>,
    /// The expansion of each alias (types.rs).
    pub(crate) alias_memo: HashMap<BindingId, types::AliasState>,
    pub(crate) alias_depth_memo: HashMap<BindingId, Option<usize>>,
    /// Pools at depths from 1 up to this one are not used: they belong to
    /// the function around a `spawn` block, which a worker does not run.
    pub(crate) pool_barrier: usize,
    /// The local holding the chunk's owner (`rt.owner`), made by the first
    /// definition that needs it ([`Self::owner_value`]).
    owner: Option<LocalId>,
    /// The registry name of each struct and module that has one (spawn.rs).
    pub(crate) registry_names: HashMap<BindingId, String>,
    /// The names registered so far, with the place each stands for
    /// (spawn.rs).
    pub(crate) registered: HashMap<String, spawn::RegistrySlot>,
    /// The NL name of each declared name the file uses as a type
    /// (types.rs, `declared_type_value`).
    pub(crate) decl_names: HashMap<nl_sema::DeclId, String>,
    /// Every `try` takes the packed form (error.rs), which needs fewer
    /// locals: the second lowering of a file whose first one exceeded a
    /// limit ([`lower`]).
    pub(crate) packed_try: bool,
    /// The `spawn do` blocks being lowered: in one, a struct the block
    /// captures is a copy, so its literals and checks go through the
    /// runtime (structs.rs, types.rs).
    pub(crate) spawn_depth: u32,
    /// The fields of each struct of the file whose literals are built
    /// inline (structs.rs), its parents' first; `None` for a struct whose
    /// fields are not all known (an imported parent).
    pub(crate) struct_layouts: HashMap<BindingId, Option<std::rc::Rc<[structs::FieldLayout]>>>,
    /// Runtime values the inline checks read (`rt.type`, `rt.getmetatable`,
    /// `rt.KW`, `rt.setmetatable`), each copied once into a local of the
    /// main function when the chunk starts ([`Self::rt_cached`]).
    rt_cache: Vec<(&'static str, LocalId)>,
    /// What the file localizes (localize.rs).
    pub(crate) localize: localize::Plan,
    /// The locals of the localized paths, in the order they were made, each
    /// with the root and field it holds ([`Self::localized_path`]).
    localized: Vec<(LocalId, String, Option<String>)>,
    /// Where the main function declares the localized paths: the index of
    /// its statement after the imports, and the line ([`Self::lower_main`]).
    prologue_at: Option<(usize, u32)>,
    /// The values the nil-safe guards being lowered hold, by the node each
    /// one tested (nilsafe.rs): [`Self::expr`] takes them instead of
    /// lowering the node again.
    pub(crate) presets: HashMap<nilsafe::PresetKey, Expr>,
    /// The nil-safe chains being lowered under their guards (nilsafe.rs).
    pub(crate) safe_active: HashSet<nilsafe::PresetKey>,
}

impl<'a> Lowerer<'a> {
    fn new(source: &'a str, res: &'a Resolution, opts: &'a LowerOptions) -> Self {
        Lowerer {
            packed_try: false,
            res,
            opts,
            source,
            lines: LineIndex::new(source),
            locals: Vec::new(),
            bindings: HashMap::new(),
            predeclared: HashSet::new(),
            next_label: 0,
            stmts: Vec::new(),
            line: 1,
            fns: Vec::new(),
            diags: Vec::new(),
            aliases: HashMap::new(),
            volatile: HashSet::new(),
            spilled: HashSet::new(),
            pools: Vec::new(),
            binding_pool: HashMap::new(),
            alias_memo: HashMap::new(),
            alias_depth_memo: HashMap::new(),
            pool_barrier: 1,
            owner: None,
            registry_names: HashMap::new(),
            registered: HashMap::new(),
            decl_names: HashMap::new(),
            spawn_depth: 0,
            struct_layouts: HashMap::new(),
            rt_cache: Vec::new(),
            localize: localize::Plan::default(),
            localized: Vec::new(),
            prologue_at: None,
            presets: HashMap::new(),
            safe_active: HashSet::new(),
        }
    }

    /// Records the NL name of every declared name the file refers to as a
    /// type (the first segment of a `QUAL_NAME`).
    fn collect_decl_names(&mut self, root: &SyntaxNode) {
        for q in root.descendants().filter(|n| n.kind() == QUAL_NAME) {
            if let Some(nl_sema::Ref::Declared(id)) = self.res.type_ref(q.text_range()) {
                if let Some(first) = qual_segments(&q).first() {
                    self.decl_names.entry(*id).or_insert_with(|| first.text().to_string());
                }
            }
        }
    }

    /// The owner of this chunk, which a definition of a primitive method
    /// or operator passes to the runtime (one from a different
    /// file cannot replace it). `rt.owner(chunk name)` runs once per load
    /// of the chunk ([`Chunk::owner`]), so each chunk that is not a file
    /// (`load_nl`) has an owner of its own, and a file is owned by its
    /// canonical name (runtime/nl_70_module.lua).
    pub(crate) fn owner_value(&mut self) -> Expr {
        let l = match self.owner {
            Some(l) => l,
            None => {
                let l = self.new_local("owner");
                self.owner = Some(l);
                l
            }
        };
        Expr::Local(l)
    }

    // ---- diagnostics -----------------------------------------------------

    pub(crate) fn error(&mut self, node: &SyntaxNode, code: &'static str, message: impl Into<String>) {
        self.diags.push(LowerDiagnostic { range: trimmed_range(node), code, message: message.into() });
    }

    /// A tree shape the lowering did not expect (after the parser and the
    /// resolver accepted it): an internal error, never a panic.
    pub(crate) fn internal(&mut self, node: &SyntaxNode, what: &str) -> Expr {
        self.diags.push(LowerDiagnostic {
            range: trimmed_range(node),
            code: "internal",
            message: format!("internal compiler error: unexpected {what} ({:?})", node.kind()),
        });
        Expr::Nil
    }

    // ---- lines, locals, labels -------------------------------------------

    /// The 1-based line of a node's first token.
    pub(crate) fn line_of(&self, node: &SyntaxNode) -> u32 {
        self.lines.line_col(self.source, trimmed_range(node).start()).line
    }

    pub(crate) fn new_local(&mut self, hint: &str) -> LocalId {
        crate::ir::new_local(&mut self.locals, hint)
    }

    /// The local that holds a binding.
    pub(crate) fn local_of(&mut self, id: BindingId) -> LocalId {
        if let Some(l) = self.bindings.get(&id) {
            return *l;
        }
        let hint = self.res.binding(id).map_or_else(|| "v".to_string(), |b| b.name.clone());
        let l = self.new_local(&hint);
        self.bindings.insert(id, l);
        if self.res.assigned_in_closure(id) {
            self.volatile.insert(l);
        }
        l
    }

    /// Evaluation order (left to right, each operand once). The
    /// IR evaluates the operands of an operator and the parts of an
    /// assignment target in order, except that a local is read where the
    /// operation happens (as Lua reads a local's register). A local that a
    /// closure assigns can therefore change between its turn and the
    /// operation if a later operand calls code; `pin` copies such a local
    /// to a temporary at its turn. `later` is what is evaluated after it.
    pub(crate) fn pin(&mut self, e: Expr, later: &Expr) -> Expr {
        match e {
            Expr::Local(l) if self.volatile.contains(&l) && may_run_code(later) => Expr::Local(self.fresh_temp(e, "v")),
            e => e,
        }
    }

    /// True for the local of a binding that a closure assigns (see
    /// [`Self::pin`]).
    pub(crate) fn is_volatile(&self, l: LocalId) -> bool {
        self.volatile.contains(&l)
    }

    pub(crate) fn new_label(&mut self) -> LabelId {
        self.next_label += 1;
        LabelId(self.next_label)
    }

    // ---- statements --------------------------------------------------------

    pub(crate) fn emit(&mut self, kind: StmtKind) {
        self.stmts.push(Stmt { line: self.line, kind });
    }

    /// Emits an expression evaluated for its effect. Lua accepts only calls
    /// as statements; anything else is assigned to a throwaway local.
    pub(crate) fn emit_effect(&mut self, e: Expr) {
        match e {
            Expr::Call(..) | Expr::MethodCall(..) | Expr::MemberCall { .. } | Expr::MemberApply { .. } | Expr::AutoGet { .. } => self.emit(StmtKind::Expr(e)),
            e if e.is_constant() => {}
            Expr::Local(_) => {}
            e => {
                let t = self.new_local("_");
                self.emit(StmtKind::Local { locals: vec![t], values: vec![e] });
            }
        }
    }

    /// Runs `f` with an empty statement buffer and returns what it emitted.
    pub(crate) fn capture<T>(&mut self, f: impl FnOnce(&mut Self) -> T) -> (Vec<Stmt>, T) {
        let saved = std::mem::take(&mut self.stmts);
        let saved_line = self.line;
        let value = f(self);
        self.line = saved_line;
        let stmts = std::mem::replace(&mut self.stmts, saved);
        (stmts, value)
    }

    /// The statements `f` emits, as a block.
    pub(crate) fn sub_block(&mut self, f: impl FnOnce(&mut Self)) -> Block {
        let (stmts, ()) = self.capture(f);
        Block::new(stmts)
    }

    /// A fresh temporary holding `e`, or `e` itself when it is a constant.
    pub(crate) fn temp_of(&mut self, e: Expr, hint: &str) -> Expr {
        if e.is_constant() {
            return e;
        }
        let t = self.new_local(hint);
        self.emit(StmtKind::Local { locals: vec![t], values: vec![e] });
        Expr::Local(t)
    }

    /// A fresh temporary holding `e`, also when `e` is a local (whose value
    /// later code might change).
    pub(crate) fn fresh_temp(&mut self, e: Expr, hint: &str) -> LocalId {
        let t = self.new_local(hint);
        self.emit(StmtKind::Local { locals: vec![t], values: vec![e] });
        t
    }

    /// Lowers one more operand of a sequence. When lowering it emits
    /// statements, the operands already in `out` are first moved to
    /// temporaries, so that they are still evaluated before it.
    pub(crate) fn push_operand(&mut self, out: &mut Vec<Expr>, f: impl FnOnce(&mut Self) -> Expr) {
        let (stmts, e) = self.capture(f);
        if !stmts.is_empty() {
            self.spill(out);
            self.stmts.extend(stmts);
        }
        out.push(e);
    }

    /// Moves the values of `out` that statements could change to
    /// temporaries that nothing assigns again (so a later spill leaves them
    /// alone). Many values go to one table rather than to as many locals
    /// (LuaJIT allows 200 locals per function).
    fn spill(&mut self, out: &mut [Expr]) {
        let todo: Vec<usize> = out.iter().enumerate().filter(|(_, v)| !self.is_spilled(v)).map(|(i, _)| i).collect();
        if todo.len() <= SPILL_LOCALS {
            for i in todo {
                if let Some(v) = out.get_mut(i) {
                    let old = std::mem::replace(v, Expr::Nil);
                    let t = self.fresh_temp(old, "t");
                    self.spilled.insert(t);
                    *v = Expr::Local(t);
                }
            }
            return;
        }
        let items = todo.iter().filter_map(|i| out.get(*i)).map(|v| TableEntry::Item(expr::single(v.clone()))).collect();
        let table = self.fresh_temp(Expr::Table(items), "spill");
        self.spilled.insert(table);
        for (j, i) in todo.into_iter().enumerate() {
            if let Some(v) = out.get_mut(i) {
                *v = Expr::index(Expr::Local(table), Expr::Number((j + 1) as f64));
            }
        }
    }

    /// True for a value no statement can change: a constant or a spilled
    /// value.
    fn is_spilled(&self, e: &Expr) -> bool {
        match e {
            Expr::Local(l) => self.spilled.contains(l),
            Expr::Index(o, k) => matches!((&**o, &**k), (Expr::Local(t), Expr::Number(_)) if self.spilled.contains(t)),
            e => e.is_constant(),
        }
    }

    /// Lowers expression nodes in order (see [`Self::push_operand`]).
    pub(crate) fn operands(&mut self, nodes: &[SyntaxNode]) -> Vec<Expr> {
        let mut out = Vec::new();
        for n in nodes {
            self.push_operand(&mut out, |l| l.expr(n));
        }
        out
    }

    /// Declares a binding's local with an initial value, or assigns it when
    /// it was declared ahead (hoisting).
    pub(crate) fn define(&mut self, id: BindingId, value: Option<Expr>) -> LocalId {
        let l = self.local_of(id);
        if self.predeclared.contains(&id) {
            self.emit(StmtKind::Assign { targets: vec![Place::Local(l)], values: vec![value.unwrap_or(Expr::Nil)] });
        } else {
            self.emit(StmtKind::Local { locals: vec![l], values: value.into_iter().collect() });
        }
        l
    }

    /// True when the binding's `local` was emitted ahead.
    pub(crate) fn is_predeclared(&self, id: BindingId) -> bool {
        self.predeclared.contains(&id)
    }

    pub(crate) fn mark_predeclared(&mut self, id: BindingId) {
        self.predeclared.insert(id);
    }

    // ---- blocks --------------------------------------------------------------

    /// `rt.name` (an intrinsic that is a plain value, such as Lua's `type`
    /// or the keyed-arguments mark) as a local of the main function, set
    /// from `rt` before anything else runs: the inline checks of every
    /// function read it as an upvalue, not as a field of `rt` (types.rs,
    /// [`types::Inline`]). Like the other file-level locals, it is read
    /// directly in a `spawn do` block.
    pub(crate) fn rt_cached(&mut self, name: &'static str) -> Expr {
        if let Some((_, l)) = self.rt_cache.iter().find(|(n, _)| *n == name) {
            return Expr::Local(*l);
        }
        let l = self.new_local(&format!("rt_{name}"));
        self.rt_cache.push((name, l));
        Expr::Local(l)
    }

    /// True when `e` is the value [`Self::rt_cached`] gives for `name`.
    pub(crate) fn is_rt_cached(&self, e: &Expr, name: &str) -> bool {
        matches!(e, Expr::Local(l) if self.rt_cache.iter().any(|(n, c)| *n == name && c == l))
    }

    fn lower_main(&mut self, root: &SyntaxNode) -> Function {
        self.fns.push(FnState { ret_check: None, loops: Vec::new(), regions: Vec::new(), name: "main chunk".to_string() });
        if let Some(b) = block(root) {
            self.lower_block_with(&b, Tail::Discard, true);
        }
        self.fns.pop();
        if !self.localized.is_empty() {
            let (at, line) = self.prologue_at.unwrap_or((0, 1));
            let locals = self.localized.iter().map(|(l, _, _)| *l).collect();
            let values = self.localized.iter().map(|(_, r, n)| localize::initial_value(r, n.as_deref())).collect();
            self.stmts.insert(at.min(self.stmts.len()), Stmt { line, kind: StmtKind::Local { locals, values } });
        }
        if !self.rt_cache.is_empty() {
            let (names, locals): (Vec<&'static str>, Vec<LocalId>) = self.rt_cache.iter().copied().unzip();
            let values = names.into_iter().map(Expr::Intrinsic).collect();
            self.stmts.insert(0, Stmt { line: 1, kind: StmtKind::Local { locals, values } });
        }
        let body = Block::new(std::mem::take(&mut self.stmts));
        Function { params: Vec::new(), vararg: true, body, line: 1, name: Some("main chunk".to_string()), method: false, transient: false }
    }

    /// Lowers a `BLOCK` into a new IR block.
    pub(crate) fn lower_block(&mut self, block: &SyntaxNode, tail: Tail) -> Block {
        self.sub_block(|l| l.lower_block_inline(block, tail))
    }

    /// Lowers a `BLOCK` into the current statement buffer. The caller
    /// provides the scope (a function body, a `do` block, a branch).
    pub(crate) fn lower_block_inline(&mut self, block: &SyntaxNode, tail: Tail) {
        self.lower_block_with(block, tail, false);
    }

    /// Lowers a block: hoisting (see [`Self::predeclare`]), its type pool
    /// (types.rs), its statements; `main` for the file's block, which also
    /// ends with the export table.
    ///
    /// The order of what runs: the `local` declarations, the imports and
    /// type declarations, the pool's values, the hoisted functions, then
    /// the other statements in source order. The last four form one
    /// `Hoist` when the block hoists.
    fn lower_block_with(&mut self, block: &SyntaxNode, tail: Tail, main: bool) {
        let stmts: Vec<SyntaxNode> = statements(block).collect();
        let hoisting = needs_hoisting(&stmts);
        let pooled = hoisting || main;
        let pool_line = stmts.first().map_or(self.line, |s| self.line_of(s));
        if pooled {
            let decls = self.pool_bindings(&stmts);
            self.push_pool(&decls, pool_line);
        }
        let (decls, ()) = self.capture(|l| {
            if hoisting {
                l.predeclare(&stmts);
            } else {
                l.predeclare_forward_consts(&stmts);
            }
            if main {
                l.predeclare_conditional_exports();
            }
        });
        // A file that localizes Lua paths declares them after its imports
        // (localize.rs), which are then lowered on their own, before the
        // rest.
        let prologue = main && !self.localize.is_empty();
        let (imports, imports_done) = if prologue { self.capture(|l| l.lower_imports(&stmts)) } else { (Vec::new(), HashSet::new()) };
        let imports_line = imports.last().map_or(1, |s| s.line);
        let (early, mut done) = self.capture(|l| l.lower_early(&stmts, hoisting, !prologue));
        done.extend(imports_done);
        let (fns, fns_done) = self.capture(|l| if hoisting { l.lower_hoisted_fns(&stmts) } else { HashSet::new() });
        done.extend(fns_done);
        let n = stmts.len();
        let (rest, ()) = self.capture(|l| {
            for (i, s) in stmts.iter().enumerate() {
                if done.contains(&s.text_range()) {
                    continue;
                }
                let t = if i + 1 == n { tail } else { Tail::Discard };
                l.stmt(s, t);
            }
            if main {
                l.emit_exports();
            }
        });
        let (pool_decl, pool_defs) = if pooled { self.pop_pool() } else { (None, Vec::new()) };
        if !hoisting {
            self.stmts.extend(decls);
            self.stmts.extend(imports);
            if prologue {
                self.prologue_at = Some((self.stmts.len(), imports_line));
            }
            self.stmts.extend(early);
            self.stmts.extend(pool_decl);
            self.stmts.extend(pool_defs);
            self.stmts.extend(rest);
            return;
        }
        // The `local` declarations come first; the definitions and the
        // other statements form one `Hoist`.
        let (early_locals, early): (Vec<Stmt>, Vec<Stmt>) = early.into_iter().partition(|s| matches!(s.kind, StmtKind::Local { .. }));
        let (fn_locals, fns): (Vec<Stmt>, Vec<Stmt>) = fns.into_iter().partition(|s| matches!(s.kind, StmtKind::Local { .. }));
        self.stmts.extend(decls);
        // The imports assign locals `decls` declared; they come first among
        // the definitions anyway (imports are the first
        // statements of a file).
        self.stmts.extend(imports);
        if prologue {
            self.prologue_at = Some((self.stmts.len(), imports_line));
        }
        self.stmts.extend(early_locals);
        self.stmts.extend(fn_locals);
        self.stmts.extend(pool_decl);
        let mut defs = early;
        defs.extend(pool_defs);
        defs.extend(fns);
        self.emit(StmtKind::Hoist { defs: Block::new(defs), rest: Block::new(rest) });
    }

    /// The bindings whose types a block's pool can hold: the structs,
    /// enums and interfaces it declares, and the imported names.
    fn pool_bindings(&self, stmts: &[SyntaxNode]) -> Vec<BindingId> {
        let mut out = Vec::new();
        for s in stmts {
            match s.kind() {
                STRUCT_DECL | ENUM_DECL | INTERFACE_DECL | IMPORT_DECL => self.declared_bindings(s, &mut out),
                _ => {}
            }
        }
        out
    }

    /// Hoisting. When a block declares functions or types,
    /// every name its statements declare gets its `local` at the start of
    /// the block, and what the resolver hoists (`fn name`, `struct`, `enum`,
    /// `interface`; `type` has no runtime value) is defined right after,
    /// before any other statement runs: the resolver lets the whole block
    /// refer to these names, so they must hold their values wherever the
    /// block can run. The order is: imports (they stand first anyway, and a
    /// struct may inherit from an imported one), enums and interfaces,
    /// structs (each after the struct it inherits from), the type pool,
    /// functions.
    fn predeclare(&mut self, stmts: &[SyntaxNode]) {
        let mut ids = Vec::new();
        for s in stmts {
            self.declared_bindings(s, &mut ids);
        }
        let ids: Vec<BindingId> = ids.into_iter().filter(|id| !self.predeclared.contains(id)).collect();
        // Lua allows 200 locals per function; one statement declares at
        // most a few dozen at a time.
        for chunk in ids.chunks(50) {
            let locals: Vec<LocalId> = chunk.iter().map(|id| self.local_of(*id)).collect();
            self.emit(StmtKind::Local { locals, values: Vec::new() });
        }
        for id in &ids {
            self.predeclared.insert(*id);
        }
    }

    /// In a block that does not hoist, the `local` of each `const` that a
    /// function above its line reads (the resolver's `forward_consts`),
    /// so that the function captures the local the `const` assigns.
    fn predeclare_forward_consts(&mut self, stmts: &[SyntaxNode]) {
        let mut ids = Vec::new();
        for s in stmts.iter().filter(|s| s.kind() == CONST_DECL) {
            self.declared_bindings(s, &mut ids);
        }
        let ids: Vec<BindingId> = ids.into_iter().filter(|id| self.res.forward_const(*id) && !self.predeclared.contains(id)).collect();
        if ids.is_empty() {
            return;
        }
        let locals: Vec<LocalId> = ids.iter().map(|id| self.local_of(*id)).collect();
        self.emit(StmtKind::Local { locals, values: Vec::new() });
        self.predeclared.extend(ids);
    }

    /// The locals of the `pub` items in branches of file-level `if`s, in
    /// the main function: the branch assigns them, and the export table
    /// reads them (`nil` when no branch that declares one ran).
    fn predeclare_conditional_exports(&mut self) {
        let mut ids: Vec<BindingId> = self.res.conditional_export_bindings().filter(|id| !self.predeclared.contains(id)).collect();
        if ids.is_empty() {
            return;
        }
        ids.sort_by_key(|id| id.0);
        for chunk in ids.chunks(50) {
            let locals: Vec<LocalId> = chunk.iter().map(|id| self.local_of(*id)).collect();
            self.emit(StmtKind::Local { locals, values: Vec::new() });
        }
        self.predeclared.extend(ids);
    }

    /// The imports, and when the block hoists, its type declarations (see
    /// [`Self::predeclare`]). Returns the ranges of the statements lowered.
    fn lower_early(&mut self, stmts: &[SyntaxNode], hoisting: bool, with_imports: bool) -> HashSet<TextRange> {
        let mut done = HashSet::new();
        let imports = stmts.iter().filter(|s| with_imports && s.kind() == IMPORT_DECL).cloned();
        let mut todo: Vec<SyntaxNode> = imports.collect();
        if hoisting {
            todo.extend(stmts.iter().filter(|s| matches!(s.kind(), ENUM_DECL | INTERFACE_DECL)).cloned());
            todo.extend(self.struct_order(stmts));
        }
        for s in todo {
            self.line = self.line_of(&s);
            self.simple_stmt(&s, Tail::Discard);
            done.insert(s.text_range());
        }
        done
    }

    /// The imports of a block, alone (see [`Self::lower_block_with`]).
    fn lower_imports(&mut self, stmts: &[SyntaxNode]) -> HashSet<TextRange> {
        let mut done = HashSet::new();
        for s in stmts.iter().filter(|s| s.kind() == IMPORT_DECL) {
            self.line = self.line_of(s);
            self.simple_stmt(s, Tail::Discard);
            done.insert(s.text_range());
        }
        done
    }

    /// The `fn name` declarations of a hoisting block, in source order.
    fn lower_hoisted_fns(&mut self, stmts: &[SyntaxNode]) -> HashSet<TextRange> {
        let mut done = HashSet::new();
        for s in stmts.iter().filter(|s| s.kind() == FN_DECL && child(s, QUAL_NAME).is_none()) {
            self.line = self.line_of(s);
            self.lower_fn_decl(s);
            done.insert(s.text_range());
        }
        done
    }

    /// The `struct` declarations of a block, each after the struct of the
    /// same block it inherits from. A cycle of parents is an error.
    fn struct_order(&mut self, stmts: &[SyntaxNode]) -> Vec<SyntaxNode> {
        let decls: Vec<(BindingId, SyntaxNode)> = stmts
            .iter()
            .filter(|s| s.kind() == STRUCT_DECL)
            .filter_map(|s| child(s, NAME).and_then(|n| self.res.declaration(n.text_range())).map(|id| (id, s.clone())))
            .collect();
        let index: HashMap<BindingId, usize> = decls.iter().enumerate().map(|(i, (id, _))| (*id, i)).collect();
        let parents: Vec<Option<usize>> = decls.iter().map(|(_, s)| self.struct_parent(s).and_then(|p| index.get(&p).copied())).collect();
        // 0: not placed, 1: being placed, 2: placed.
        let mut state = vec![0u8; decls.len()];
        let mut order = Vec::new();
        for start in 0..decls.len() {
            let mut chain = Vec::new();
            let mut at = Some(start);
            while let Some(i) = at {
                match state.get(i).copied() {
                    Some(0) => {
                        if let Some(st) = state.get_mut(i) {
                            *st = 1;
                        }
                        chain.push(i);
                        at = parents.get(i).copied().flatten();
                    }
                    Some(1) => {
                        if let Some((_, s)) = decls.get(i) {
                            let at_node = child(s, STRUCT_PARENT).unwrap_or_else(|| s.clone());
                            let name = child(s, NAME).and_then(|n| name_text(&n)).unwrap_or_default();
                            self.error(&at_node, "inheritance-cycle", format!("struct `{name}` inherits from itself"));
                        }
                        break;
                    }
                    _ => break,
                }
            }
            // The chain runs from a child to its ancestors: place ancestors
            // first.
            for i in chain.into_iter().rev() {
                if let Some(st) = state.get_mut(i) {
                    *st = 2;
                }
                if let Some((_, s)) = decls.get(i) {
                    order.push(s.clone());
                }
            }
        }
        order
    }

    /// The struct binding a `struct` declaration names as its parent,
    /// through aliases.
    fn struct_parent(&self, decl: &SyntaxNode) -> Option<BindingId> {
        let mut ty = child(decl, STRUCT_PARENT).and_then(|p| type_child(&p))?;
        for _ in 0..=self.aliases.len() {
            let q = child(&ty, QUAL_NAME)?;
            if qual_segments(&q).len() != 1 {
                return None;
            }
            let Some(nl_sema::Ref::Binding(id)) = self.res.type_ref(q.text_range()).cloned() else { return None };
            match self.aliases.get(&id) {
                Some(alias) => ty = alias.clone(),
                None => return Some(id),
            }
        }
        None
    }

    /// The bindings a statement declares in its own block.
    fn declared_bindings(&self, stmt: &SyntaxNode, out: &mut Vec<BindingId>) {
        match stmt.kind() {
            LET_DECL | CONST_DECL => {
                for p in pattern_children(stmt) {
                    self.pattern_bindings(&p, out);
                }
            }
            FN_DECL | STRUCT_DECL | ENUM_DECL | INTERFACE_DECL => {
                if child(stmt, QUAL_NAME).is_none() {
                    if let Some(n) = child(stmt, NAME) {
                        if let Some(id) = self.res.declaration(n.text_range()) {
                            out.push(id);
                        }
                    }
                }
            }
            MODULE_DECL => {
                if let Some((id, reopen)) = self.res.module(stmt.text_range()) {
                    if !reopen {
                        out.push(id);
                    }
                }
            }
            IMPORT_DECL => {
                let names = child(stmt, IMPORT_LIST).map_or_else(|| children(stmt, NAME).collect::<Vec<_>>(), |l| children(&l, NAME).collect());
                for n in names {
                    if let Some(id) = self.res.declaration(n.text_range()) {
                        out.push(id);
                    }
                }
            }
            _ => {}
        }
    }

    /// The bindings a pattern declares.
    pub(crate) fn pattern_bindings(&self, pat: &SyntaxNode, out: &mut Vec<BindingId>) {
        for el in pat.descendants_with_tokens() {
            let range = match &el {
                rowan::NodeOrToken::Node(n) if n.kind() == NAME => n.text_range(),
                rowan::NodeOrToken::Token(t) if t.parent().is_some_and(|p| p.kind() == PAT_FIELD) => t.text_range(),
                _ => continue,
            };
            if let Some(id) = self.res.declaration(range) {
                if !out.contains(&id) {
                    out.push(id);
                }
            }
        }
    }

    /// Lowers one statement.
    pub(crate) fn stmt(&mut self, node: &SyntaxNode, tail: Tail) {
        self.line = self.line_of(node);
        if let Some(m) = modifier(node) {
            self.stmt_with_modifier(node, &m, tail);
            return;
        }
        let declares = matches!(
            node.kind(),
            LET_DECL | CONST_DECL | FN_DECL | STRUCT_DECL | ENUM_DECL | INTERFACE_DECL | INTERFACE_IMPL | MODULE_DECL | IMPORT_DECL
        );
        if declares {
            self.simple_stmt(node, tail);
            return;
        }
        // Temporaries of other statements are scoped to the statement.
        let (stmts, ()) = self.capture(|l| l.simple_stmt(node, tail));
        let has_locals = stmts.iter().any(|s| matches!(s.kind, StmtKind::Local { .. }));
        if has_locals {
            self.emit(StmtKind::Do(Block::new(stmts)));
        } else {
            self.stmts.extend(stmts);
        }
    }

    /// A statement followed by `if`/`unless`/`while`/`until`. The tail
    /// statement `x if c` keeps the block's tail for `x`, as the branch of
    /// `if c x end` would: a value is returned or assigned, and a bare
    /// name is called only where `x` alone would be (`nl_sema` checks.rs).
    fn stmt_with_modifier(&mut self, node: &SyntaxNode, m: &SyntaxNode, tail: Tail) {
        let word = first_token(m).map(|t| t.kind());
        let Some(cond_node) = expr_child(m) else {
            self.internal(m, "modifier without condition");
            return;
        };
        let is_loop = matches!(word, Some(WHILE_KW | UNTIL_KW));
        let tail = if is_loop { Tail::Discard } else { tail };
        // `x if SERVER`: left out where realm constants say it never
        // runs, and run without a test where they say it always does.
        if !is_loop {
            if let Some(v) = self.decided(m) {
                if v != matches!(word, Some(UNLESS_KW)) {
                    let body = self.sub_block(|l| {
                        let (stmts, ()) = l.capture(|l| l.simple_stmt(node, tail));
                        l.stmts.extend(stmts);
                    });
                    self.emit(StmtKind::Do(body));
                }
                return;
            }
        }
        if is_loop {
            if let Some(f) = self.fns.last_mut() {
                f.loops.push(LoopState::default());
            }
        }
        let body = self.sub_block(|l| {
            let (stmts, ()) = l.capture(|l| l.simple_stmt(node, tail));
            l.stmts.extend(stmts);
        });
        let body = if is_loop { self.finish_loop_body(body) } else { body };
        let (cond_stmts, cond) = self.capture(|l| l.expr(&cond_node));
        let cond = match word {
            Some(UNLESS_KW | UNTIL_KW) => Expr::lnot(cond),
            _ => cond,
        };
        if is_loop {
            if cond_stmts.is_empty() {
                self.emit(StmtKind::While { cond, body });
            } else {
                let mut stmts = cond_stmts;
                stmts.push(Stmt { line: self.line, kind: StmtKind::If { cond: Expr::lnot(cond), then: Block::new(vec![Stmt { line: self.line, kind: StmtKind::Break }]), otherwise: None } });
                stmts.extend(body.stmts);
                self.emit(StmtKind::While { cond: Expr::Bool(true), body: Block::new(stmts) });
            }
        } else {
            let mut stmts = cond_stmts;
            stmts.push(Stmt { line: self.line, kind: StmtKind::If { cond, then: body, otherwise: None } });
            self.emit(StmtKind::Do(Block::new(stmts)));
        }
    }

    /// Closes the loop opened for a body: adds the `continue` label if a
    /// `continue` used it.
    pub(crate) fn finish_loop_body(&mut self, body: Block) -> Block {
        let state = self.fns.last_mut().and_then(|f| f.loops.pop()).unwrap_or_default();
        match state.continue_label {
            Some(label) => {
                let line = self.line;
                Block::new(vec![Stmt { line, kind: StmtKind::Do(body) }, Stmt { line, kind: StmtKind::Label(label) }])
            }
            None => body,
        }
    }

    fn simple_stmt(&mut self, node: &SyntaxNode, tail: Tail) {
        match node.kind() {
            EXPR_STMT => match expr_child(node) {
                Some(e) => self.expr_stmt(node, &e, tail),
                None => match child(node, EXPR_LIST) {
                    Some(list) => self.value_list_stmt(node, &list, tail),
                    None => {
                        self.internal(node, "empty expression statement");
                    }
                },
            },
            LET_DECL => self.lower_let(node),
            CONST_DECL => self.lower_let(node),
            GLOBAL_DECL => self.lower_global(node),
            FN_DECL => {
                if child(node, QUAL_NAME).is_some() {
                    self.lower_method_decl(node);
                } else {
                    // Normally hoisted; reached only for a declaration the
                    // hoisting did not see.
                    self.lower_fn_decl(node);
                }
            }
            STRUCT_DECL => self.lower_struct(node),
            ENUM_DECL => self.lower_enum(node),
            INTERFACE_DECL => self.lower_interface(node),
            INTERFACE_IMPL => self.lower_interface_impl(node),
            TYPE_DECL => {}
            MODULE_DECL => self.lower_module(node),
            IMPORT_DECL => self.lower_import(node),
            WHILE_STMT | UNTIL_STMT | LOOP_STMT => self.lower_while(node),
            FOR_STMT => self.lower_for(node),
            BREAK_STMT => self.lower_break(node),
            CONTINUE_STMT => self.lower_continue(node),
            RETURN_STMT => self.lower_return(node),
            ASSIGN_STMT => self.lower_assign(node),
            COMPOUND_ASSIGN_STMT => self.lower_compound(node),
            MULTI_ASSIGN_STMT => self.lower_multi_assign(node),
            DO_BLOCK => {
                if let Some(b) = block(node) {
                    let body = self.lower_block(&b, Tail::Discard);
                    self.emit(StmtKind::Do(body));
                }
            }
            _ => {
                self.internal(node, "statement");
            }
        }
    }

    /// `a, b` as the tail of a function body: returns every value.
    /// Where only one value can go (a block expression used as a
    /// value, a `try` tail), the first is taken; the resolver has rejected
    /// every position where the values would be discarded.
    fn value_list_stmt(&mut self, stmt: &SyntaxNode, list: &SyntaxNode, tail: Tail) {
        let nodes: Vec<SyntaxNode> = expr_children(list).collect();
        let vals = self.operands(&nodes);
        let n = vals.len();
        let vals: Vec<Expr> = vals.into_iter().enumerate().map(|(i, v)| if i + 1 < n { expr::single(v) } else { v }).collect();
        match tail {
            Tail::Return => self.lower_return_values(stmt, vals),
            Tail::Discard => {
                for v in vals {
                    self.emit_effect(v);
                }
            }
            Tail::Assign(t) => {
                let first = vals.into_iter().next().map_or(Expr::Nil, expr::single);
                self.emit(StmtKind::Assign { targets: vec![Place::Local(t)], values: vec![first] });
            }
            Tail::Yield => {
                let first = vals.into_iter().next().map_or(Expr::Nil, expr::single);
                self.emit(StmtKind::Return(vec![Expr::str("n"), first]));
            }
        }
    }

    /// An expression statement with the given tail.
    fn expr_stmt(&mut self, stmt: &SyntaxNode, e: &SyntaxNode, tail: Tail) {
        if is_block_construct(e.kind()) {
            self.construct(e, tail);
            return;
        }
        match tail {
            // Whether the value is discarded (a bare name or member is then
            // called) or used and dropped is the resolver's decision
            // (`nl_sema` checks.rs).
            Tail::Discard if self.res.value_discarded(stmt.text_range()) => self.discard_expr(e),
            Tail::Discard => {
                let v = self.expr(e);
                self.emit_effect(v);
            }
            Tail::Return => {
                let v = self.expr(e);
                self.emit_return_at(vec![v], Some(stmt.text_range()));
            }
            Tail::Assign(t) => {
                let v = self.expr(e);
                self.emit(StmtKind::Assign { targets: vec![Place::Local(t)], values: vec![v] });
            }
            Tail::Yield => {
                let v = self.expr(e);
                self.emit(StmtKind::Return(vec![Expr::str("n"), v]));
            }
        }
    }

    /// A block expression (`if`, `unless`, `case`, `cond`, `try`) lowered as
    /// a statement whose branches end with `tail`.
    pub(crate) fn construct(&mut self, node: &SyntaxNode, tail: Tail) {
        match node.kind() {
            IF_EXPR => self.lower_if(node, tail),
            UNLESS_EXPR => self.lower_unless(node, tail),
            COND_EXPR => self.lower_cond(node, tail),
            CASE_EXPR => self.lower_case(node, tail),
            TRY_EXPR => self.lower_try(node, tail),
            _ => {
                self.internal(node, "block expression");
            }
        }
    }

    /// [`Self::emit_return`] from a site (a `return` statement or a tail
    /// expression statement) whose check the analyzer may have proven.
    pub(crate) fn emit_return_at(&mut self, values: Vec<Expr>, site: Option<TextRange>) {
        if site.is_some_and(|s| self.opts.hints.is_proven(s)) {
            self.emit(StmtKind::Return(values));
            return;
        }
        self.emit_return(values);
    }

    /// `return values`, checked against the declared return type. A
    /// `(A, B)` return type checks each value. A `return` that writes a
    /// fixed number of values returns those (at most as many as the tuple
    /// has), as an unchecked build does: `return x` gives one value, and
    /// the missing ones are checked as `nil`, which a type that rejects
    /// `nil` refuses. The tuple says what a caller may destructure, not
    /// how many values a Lua caller's `select('#', …)` sees. When the last
    /// value is a call or a spread, whose count is only known at runtime,
    /// exactly the tuple's count comes back (padded with `nil`, or cut).
    pub(crate) fn emit_return(&mut self, values: Vec<Expr>) {
        let check = self.fns.last().and_then(|f| f.ret_check.clone());
        match check {
            Some((ty, ctx)) if ty.kind() == TUPLE_TYPE && values.len() < ty.children().filter(|c| is_type(c.kind())).count() && !values.last().is_some_and(Expr::is_multi) => {
                // Fewer values than the tuple, all written: each written
                // one in a local, checked, then the missing ones as `nil`.
                let parts: Vec<SyntaxNode> = ty.children().filter(|c| is_type(c.kind())).collect();
                let rs: Vec<LocalId> = (0..values.len()).map(|_| self.new_local("r")).collect();
                if !rs.is_empty() {
                    self.emit(StmtKind::Local { locals: rs.clone(), values });
                }
                for (i, part) in parts.iter().enumerate() {
                    let Some(t) = self.type_value(part) else { continue };
                    let test = self.inline_test(part);
                    let what = ctx.replacen("return value of", &format!("return value {} of", i + 1), 1);
                    let value = rs.get(i).map_or(Expr::Nil, |r| Expr::Local(*r));
                    self.emit_check_value(value, t, test, &what);
                }
                self.emit(StmtKind::Return(rs.into_iter().map(Expr::Local).collect()));
            }
            Some((ty, ctx)) if ty.kind() == TUPLE_TYPE => {
                let parts: Vec<SyntaxNode> = ty.children().filter(|c| is_type(c.kind())).collect();
                let types: Option<Vec<Expr>> = parts.iter().map(|t| self.type_value(t)).collect();
                let tests: Vec<Option<types::Inline>> = parts.iter().map(|t| self.inline_test(t)).collect();
                match types {
                    Some(types) if tests.iter().any(Option::is_some) => {
                        // Checked inline: `local r1, r2 = values` takes
                        // exactly two values as `rt.retn` does, then each is
                        // tested and `rt.check` called when one fails.
                        let rs: Vec<LocalId> = (0..types.len()).map(|_| self.new_local("r")).collect();
                        self.emit(StmtKind::Local { locals: rs.clone(), values });
                        for (i, ((r, t), test)) in rs.iter().zip(types).zip(tests).enumerate() {
                            let what = ctx.replacen("return value of", &format!("return value {} of", i + 1), 1);
                            self.emit_check_value(Expr::Local(*r), t, test, &what);
                        }
                        self.emit(StmtKind::Return(rs.into_iter().map(Expr::Local).collect()));
                    }
                    Some(types) => {
                        let n = types.len();
                        // "return value of f" becomes "return value 2 of f".
                        let what = ctx.replacen("return value of", "return value %d of", 1);
                        let mut args = vec![Expr::Table(types.into_iter().map(TableEntry::Item).collect()), Expr::Number(n as f64), Expr::str(&what)];
                        args.extend(values);
                        self.emit(StmtKind::Return(vec![Expr::intrinsic_call("retn", args)]));
                    }
                    None => self.emit(StmtKind::Return(values)),
                }
            }
            Some((ty, ctx)) => match (self.type_value(&ty), self.inline_test(&ty)) {
                (Some(t), Some(test)) if values.len() == 1 && !values.iter().any(Expr::is_multi) => {
                    // One value: tested inline, `rt.check` when it fails.
                    let v = values.into_iter().next().unwrap_or(Expr::Nil);
                    if self.accepts_statically(&test, &v) {
                        self.emit(StmtKind::Return(vec![v]));
                        return;
                    }
                    let r = match v {
                        v if v.is_constant() || matches!(v, Expr::Local(_)) => v,
                        v => Expr::Local(self.fresh_temp(v, "r")),
                    };
                    self.emit_check_value(r.clone(), t, Some(test), &ctx);
                    self.emit(StmtKind::Return(vec![r]));
                }
                (Some(t), _) => {
                    let mut args = vec![t, Expr::str(&ctx)];
                    args.extend(values);
                    self.emit(StmtKind::Return(vec![Expr::intrinsic_call("ret", args)]));
                }
                (None, _) => self.emit(StmtKind::Return(values)),
            },
            None => self.emit(StmtKind::Return(values)),
        }
    }
}

/// True when a block declares something the resolver hoists and that has a
/// runtime value: `fn name`, `struct`, `enum`, `interface`.
fn needs_hoisting(stmts: &[SyntaxNode]) -> bool {
    stmts.iter().any(|s| matches!(s.kind(), STRUCT_DECL | ENUM_DECL | INTERFACE_DECL) || (s.kind() == FN_DECL && child(s, QUAL_NAME).is_none()))
}

/// True when evaluating `e` may run code (a call, a metamethod) that could
/// assign variables. Constants and locals do not.
pub(crate) fn may_run_code(e: &Expr) -> bool {
    !(e.is_constant() || matches!(e, Expr::Local(_)))
}

/// `if`, `unless`, `case`, `cond` and `try`: expressions that are lowered as
/// statements with a tail.
pub(crate) fn is_block_construct(kind: SyntaxKind) -> bool {
    matches!(kind, IF_EXPR | UNLESS_EXPR | CASE_EXPR | COND_EXPR | TRY_EXPR)
}
