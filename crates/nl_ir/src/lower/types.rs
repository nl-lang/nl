//! Runtime types: the runtime value of a type
//! expression, the checks at annotated positions, `is` and `as`.
//!
//! A type value is what the runtime's `rt.check`, `rt.isa` and `rt.as`
//! take: a descriptor from `rt.T` for Lua's types, a struct, enum or
//! interface table, or a combination built by `rt.optional`, `rt.union`,
//! `rt.negate`, `rt.record` and `rt.lazy`. Types that are static only
//! (`any`, type parameters, function signatures beyond "a
//! function", element types) give no value or a coarser one, and no check
//! is emitted for them.
//!
//! # Inline checks
//!
//! A check is a cheap test in the generated code, and `rt.check` is
//! called only when the test fails ([`Inline`]): `type(x) == "number"`
//! for `Number`, `getmetatable(x) == S` for a struct `S`, `x == nil or …`
//! for `T?`. A type without such a test (a record, a negation, an
//! interface, a host type) is left to `rt.check` alone.
//!
//! # Type pools
//!
//! Checks are on by default, so a combination is not built at each check:
//! it is built once, in a *pool*, a table local to the block that declares
//! the types it refers to, and a check reads it from there
//! (`types[3]`). The main block has a pool (for types made of prelude and
//! imported types), and so has every block that declares a struct, enum
//! or interface. A combination goes to the innermost pool whose block can
//! see every type it names; the pool is filled right after the block's
//! type declarations, before its functions and statements (mod.rs,
//! `lower_block_with`).
//!
//! # Aliases
//!
//! An alias is expanded once (memoized), so the work is linear in the size
//! of the aliases. An alias may refer to itself through a record field
//! (`type Node = {value: Number, next: Node?}`): the inner reference is
//! `rt.lazy(name, function() return <the alias's pool slot> end)`, read
//! when a value is first tested. A cycle that does not pass through a
//! record field (`type A = B | Number`, `type B = A | String`) has no
//! value to test and is an error, reported whatever the build mode.

use std::collections::HashMap;

use nl_sema::util::*;
use nl_sema::{Access, BindingId, DeclKind, Prelude, Ref, SelfTarget};
use nl_syntax::{SyntaxKind, SyntaxNode};

use super::Lowerer;
use crate::ir::*;

use SyntaxKind::*;

/// One pool of type values (see the module documentation).
pub(crate) struct TypePool {
    /// The table holding the values, created with the first one.
    pub table: Option<LocalId>,
    /// `table[k] = value`, in the order the values were made.
    pub defs: Vec<(usize, Expr)>,
    /// The index of each value made from an expression, by its text.
    memo: HashMap<String, usize>,
    /// The next free index.
    next: usize,
    /// The line the pool's statements carry: the block's first.
    line: u32,
}

impl TypePool {
    pub(crate) fn new(line: u32) -> Self {
        TypePool { table: None, defs: Vec::new(), memo: HashMap::new(), next: 1, line }
    }
}

/// The expansion state of one alias.
#[derive(Clone)]
pub(crate) enum AliasState {
    /// Being expanded: the pool it goes to (when it can be pooled), the
    /// slot reserved for a reference to itself, and the record nesting at
    /// the start of its expansion.
    InProgress { depth: Option<usize>, slot: Option<usize>, record_level: u32 },
    /// Expanded: its value, and the depth of the pool it refers to.
    Done(Option<Expr>, Option<usize>),
}

impl Lowerer<'_> {
    /// Records the type of every `type` alias of the file, and reports the
    /// aliases that refer to themselves other than through a record field.
    pub(crate) fn collect_aliases(&mut self, root: &SyntaxNode) {
        let mut decls = Vec::new();
        for d in root.descendants().filter(|d| d.kind() == TYPE_DECL) {
            let (Some(name), Some(ty)) = (child(&d, NAME), type_child(&d)) else { continue };
            if let Some(id) = self.res.declaration(name.text_range()) {
                self.aliases.insert(id, ty);
                decls.push((id, name));
            }
        }
        // An edge A -> B when A's type names B outside every record field.
        let edges: HashMap<BindingId, Vec<BindingId>> = decls
            .iter()
            .filter_map(|(id, _)| {
                let ty = self.aliases.get(id)?;
                let mut out = Vec::new();
                self.unguarded_aliases(ty, &mut out);
                Some((*id, out))
            })
            .collect();
        // Depth-first search for cycles: 1 on the path, 2 done.
        let mut state: HashMap<BindingId, u8> = HashMap::new();
        for (start, name) in &decls {
            if state.contains_key(start) {
                continue;
            }
            let mut stack: Vec<(BindingId, usize)> = vec![(*start, 0)];
            state.insert(*start, 1);
            let mut cyclic = false;
            while let Some((node, i)) = stack.pop() {
                let next = edges.get(&node).and_then(|e| e.get(i)).copied();
                match next {
                    Some(n) => {
                        stack.push((node, i + 1));
                        match state.get(&n).copied() {
                            None => {
                                state.insert(n, 1);
                                stack.push((n, 0));
                            }
                            Some(1) => cyclic = true,
                            _ => {}
                        }
                    }
                    None => {
                        state.insert(node, 2);
                    }
                }
            }
            if cyclic {
                self.error(name, "alias-cycle", "this type alias refers to itself without a record in between, so no value can be tested against it");
            }
        }
    }

    /// The aliases a type names outside every record field.
    fn unguarded_aliases(&self, ty: &SyntaxNode, out: &mut Vec<BindingId>) {
        match ty.kind() {
            RECORD_TYPE => {}
            NAMED_TYPE => {
                let Some(q) = child(ty, QUAL_NAME) else { return };
                if qual_segments(&q).len() != 1 {
                    return;
                }
                if let Some(Ref::Binding(id)) = self.res.type_ref(q.text_range()) {
                    if self.aliases.contains_key(id) {
                        out.push(*id);
                    }
                }
            }
            _ => {
                for c in ty.children().filter(|c| is_type(c.kind())) {
                    self.unguarded_aliases(&c, out);
                }
            }
        }
    }

    // ---- pools ---------------------------------------------------------------

    /// Opens the pool of a block; `decls` are the bindings of the types the
    /// block declares (and, for the main block, the imported names).
    /// `line` is the line of the block's first statement.
    pub(crate) fn push_pool(&mut self, decls: &[BindingId], line: u32) {
        let depth = self.pools.len();
        for id in decls {
            self.binding_pool.insert(*id, depth);
        }
        self.pools.push(TypePool::new(line));
    }

    /// Closes the innermost pool: the statements that build its values,
    /// the table's `local` first.
    pub(crate) fn pop_pool(&mut self) -> (Option<Stmt>, Vec<Stmt>) {
        let depth = self.pools.len().saturating_sub(1);
        let Some(pool) = self.pools.pop() else { return (None, Vec::new()) };
        let line = pool.line;
        self.binding_pool.retain(|_, d| *d < depth);
        // Expansions that refer to this pool, or to locals of its block, end
        // with it.
        self.alias_memo.retain(|_, st| matches!(st, AliasState::Done(_, Some(d)) if *d < depth));
        self.alias_depth_memo.clear();
        let Some(table) = pool.table else { return (None, Vec::new()) };
        let decl = Stmt { line, kind: StmtKind::Local { locals: vec![table], values: vec![Expr::Table(Vec::new())] } };
        let defs = pool
            .defs
            .into_iter()
            .map(|(k, v)| Stmt { line, kind: StmtKind::Assign { targets: vec![Place::Index(Expr::Local(table), Expr::Number(k as f64))], values: vec![v] } })
            .collect();
        (Some(decl), defs)
    }

    /// The table of the pool at `depth`.
    fn pool_table(&mut self, depth: usize) -> Option<LocalId> {
        if let Some(t) = self.pools.get(depth).and_then(|p| p.table) {
            return Some(t);
        }
        let t = self.new_local("types");
        self.pools.get_mut(depth)?.table = Some(t);
        Some(t)
    }

    /// Reserves a slot of the pool at `depth`.
    fn reserve_slot(&mut self, depth: usize) -> Option<usize> {
        self.pool_table(depth)?;
        let pool = self.pools.get_mut(depth)?;
        let k = pool.next;
        pool.next += 1;
        Some(k)
    }

    fn fill_slot(&mut self, depth: usize, k: usize, value: Expr) {
        if let Some(pool) = self.pools.get_mut(depth) {
            pool.defs.push((k, value));
        }
    }

    fn slot_ref(&mut self, depth: usize, k: usize) -> Option<Expr> {
        let t = self.pool_table(depth)?;
        Some(Expr::index(Expr::Local(t), Expr::Number(k as f64)))
    }

    /// `value` built once in the pool at `depth`, shared with every equal
    /// value there.
    fn intern(&mut self, depth: usize, value: Expr) -> Option<Expr> {
        let key = format!("{value:?}");
        if let Some(k) = self.pools.get(depth).and_then(|p| p.memo.get(&key)).copied() {
            return self.slot_ref(depth, k);
        }
        let k = self.reserve_slot(depth)?;
        if let Some(pool) = self.pools.get_mut(depth) {
            pool.memo.insert(key, k);
        }
        self.fill_slot(depth, k, value);
        self.slot_ref(depth, k)
    }

    /// The pool a type's value can go to: the innermost pool of the blocks
    /// that declare the types it names. `None` when it names something that
    /// is not declared at the start of a block with a pool (a module, a
    /// variable), or a pool a `spawn` block cannot use.
    fn type_depth(&mut self, ty: &SyntaxNode) -> Option<usize> {
        let d = self.type_depth_at(ty, &mut Vec::new())?;
        if d > 0 && d < self.pool_barrier { None } else { Some(d) }
    }

    fn type_depth_at(&mut self, ty: &SyntaxNode, visiting: &mut Vec<BindingId>) -> Option<usize> {
        match ty.kind() {
            NAMED_TYPE => {
                if has_token(ty, NIL_KW) {
                    return Some(0);
                }
                if has_token(ty, SELF_TYPE_KW) {
                    return self.self_depth(ty);
                }
                let Some(q) = child(ty, QUAL_NAME) else { return Some(0) };
                if qual_segments(&q).first().is_some_and(|s| s.text() == "Self") {
                    return self.self_depth(&q);
                }
                let segs = qual_segments(&q).len();
                match self.res.type_ref(q.text_range()).cloned() {
                    Some(Ref::Prelude(_) | Ref::Declared(_)) => Some(0),
                    Some(Ref::Binding(id)) => {
                        let decl = self.res.binding(id).map(|b| b.decl)?;
                        match decl {
                            DeclKind::TypeParam => Some(0),
                            DeclKind::TypeAlias if segs == 1 => self.alias_depth(id, visiting),
                            DeclKind::Struct | DeclKind::Enum | DeclKind::Interface | DeclKind::Import => self.binding_pool.get(&id).copied(),
                            _ => None,
                        }
                    }
                    _ => None,
                }
            }
            FN_TYPE | ARRAY_TYPE | MAP_TYPE => Some(0),
            _ => {
                let mut depth = 0;
                for c in ty.descendants().filter(|c| c.kind() == NAMED_TYPE) {
                    depth = depth.max(self.type_depth_at(&c, visiting)?);
                }
                Some(depth)
            }
        }
    }

    fn self_depth(&self, node: &SyntaxNode) -> Option<usize> {
        match self.res.self_target(node.text_range())? {
            SelfTarget::Binding(id) => self.binding_pool.get(id).copied(),
            SelfTarget::Prelude(_) => Some(0),
            SelfTarget::Member { .. } => None,
        }
    }

    fn alias_depth(&mut self, id: BindingId, visiting: &mut Vec<BindingId>) -> Option<usize> {
        if let Some(d) = self.alias_depth_memo.get(&id) {
            return *d;
        }
        if visiting.contains(&id) {
            // A cycle adds nothing beyond its other members.
            return Some(0);
        }
        let ty = self.aliases.get(&id)?.clone();
        visiting.push(id);
        let d = self.type_depth_at(&ty, visiting);
        visiting.pop();
        self.alias_depth_memo.insert(id, d);
        d
    }

    // ---- type values -----------------------------------------------------------

    /// The runtime value of a type, or `None` when nothing is checked at
    /// runtime for it. A combination is read from a pool.
    pub(crate) fn type_value(&mut self, ty: &SyntaxNode) -> Option<Expr> {
        let mut record_level = 0;
        let v = self.type_expr(ty, &mut record_level)?;
        if !is_combination(&v) {
            return Some(v);
        }
        match self.type_depth(ty) {
            Some(d) => self.intern(d, v.clone()).or(Some(v)),
            None => Some(v),
        }
    }

    /// The value of a type, built in place. `record_level` counts the
    /// record fields being expanded (for recursive aliases).
    fn type_expr(&mut self, ty: &SyntaxNode, record_level: &mut u32) -> Option<Expr> {
        match ty.kind() {
            NAMED_TYPE => self.named_type_value(ty, record_level),
            FN_TYPE => Some(self.lua_type("Function")),
            ARRAY_TYPE | MAP_TYPE => Some(self.lua_type("Table")),
            PAREN_TYPE => type_child(ty).and_then(|t| self.type_expr(&t, record_level)),
            OPTIONAL_TYPE => {
                let inner = type_child(ty).and_then(|t| self.type_expr(&t, record_level))?;
                Some(Expr::intrinsic_call("optional", vec![inner]))
            }
            NOT_TYPE => {
                let inner = type_child(ty).and_then(|t| self.type_expr(&t, record_level))?;
                Some(Expr::intrinsic_call("negate", vec![inner]))
            }
            UNION_TYPE => {
                let mut members = Vec::new();
                for t in ty.children().filter(|c| is_type(c.kind())) {
                    // A union with a member that is not checked accepts
                    // everything that member accepts: check nothing.
                    members.push(TableEntry::Item(self.type_expr(&t, record_level)?));
                }
                Some(Expr::intrinsic_call("union", vec![Expr::Table(members)]))
            }
            RECORD_TYPE => {
                *record_level += 1;
                let mut fields = Vec::new();
                for f in children(ty, RECORD_FIELD) {
                    let key = match first_token(&f) {
                        Some(t) if t.kind() == STRING => {
                            Expr::Str(nl_syntax::decode_string(&[t.text()]).into_iter().next().unwrap_or_default())
                        }
                        Some(t) => Expr::str(t.text()),
                        None => continue,
                    };
                    let value = match type_child(&f).and_then(|t| self.type_expr(&t, record_level)) {
                        Some(v) => v,
                        None => self.lua_type("Any"),
                    };
                    fields.push(TableEntry::Pair(key, value));
                }
                *record_level -= 1;
                Some(Expr::intrinsic_call("record", vec![Expr::Table(fields)]))
            }
            _ => {
                self.internal(ty, "type");
                None
            }
        }
    }

    /// `rt.T.<name>`: the descriptor of one of Lua's types.
    pub(crate) fn lua_type(&self, name: &str) -> Expr {
        Expr::index(Expr::Intrinsic("T"), Expr::str(name))
    }

    fn named_type_value(&mut self, ty: &SyntaxNode, record_level: &mut u32) -> Option<Expr> {
        if has_token(ty, NIL_KW) {
            return Some(self.lua_type("Nil"));
        }
        if has_token(ty, SELF_TYPE_KW) {
            let t = self.res.self_target(ty.text_range()).cloned();
            return Some(self.self_type_value(ty, t));
        }
        let Some(q) = child(ty, QUAL_NAME) else {
            // `any`, `unknown`, `never`: nothing to check at runtime.
            return None;
        };
        let segs: Vec<String> = qual_segments(&q).iter().map(|t| t.text().to_string()).collect();
        if segs.first().is_some_and(|s| s == "Self") {
            let t = self.res.self_target(q.text_range()).cloned();
            return Some(self.self_type_value(ty, t));
        }
        let r = self.res.type_ref(q.text_range()).cloned();
        let mut base = match r {
            Some(Ref::Prelude(p)) => self.prelude_type(p),
            Some(Ref::Binding(id)) => {
                let b = self.res.binding(id)?.clone();
                match b.decl {
                    DeclKind::TypeParam => return None,
                    DeclKind::TypeAlias if segs.len() == 1 => return self.alias_value(id, &b.name, record_level),
                    _ => Expr::Local(self.local_of(id)),
                }
            }
            Some(Ref::Declared(id)) if segs.len() == 1 => return self.declared_type_value(id),
            // `Module.Type` of a declaration file: nothing is checked.
            Some(Ref::Declared(_)) => return None,
            _ => {
                self.internal(ty, "unresolved type");
                return None;
            }
        };
        for s in segs.iter().skip(1) {
            base = Expr::index(base, Expr::str(s));
        }
        Some(base)
    }

    /// The type value of a name of a declaration file: a
    /// `declare struct` is checked by the identity of its metatable, the
    /// class table its `lua_root` names; a `declare type` by the host type
    /// registered under its name (`rt.gettype`), looked up when a value is
    /// first tested, since the host registers it after the file loads.
    fn declared_type_value(&mut self, id: nl_sema::DeclId) -> Option<Expr> {
        let name = self.res.declared_path(id).map(|r| r.global.clone());
        match self.res.declared_kind(id)? {
            nl_sema::decl::DeclItemKind::Struct => {
                let root = self.res.declared_path(id)?.clone();
                let line = self.line;
                let class = self.lua_root_value(&root);
                let v = self.new_local("v");
                let test = Function {
                    method: false,
                    transient: false,
                    params: vec![v],
                    vararg: false,
                    body: Block::new(vec![Stmt { line, kind: StmtKind::Return(vec![Expr::binary(BinOp::Eq, Expr::call(Expr::Global("getmetatable".to_string()), vec![Expr::Local(v)]), class)]) }]),
                    line,
                    name: None,
                };
                let shown = root.steps.iter().fold(root.global.clone(), |acc, s| match s {
                    nl_sema::LuaStep::Field(f) => format!("{acc}.{f}"),
                    _ => acc,
                });
                let _ = name;
                Some(Expr::Table(vec![TableEntry::Pair(Expr::str("name"), Expr::str(&shown)), TableEntry::Pair(Expr::str("test"), Expr::Function(Box::new(test)))]))
            }
            nl_sema::decl::DeclItemKind::HostType => {
                let type_name = self.declared_name(id);
                let line = self.line;
                let get = Function {
                    method: false,
                    transient: false,
                    params: Vec::new(),
                    vararg: false,
                    body: Block::new(vec![Stmt { line, kind: StmtKind::Return(vec![Expr::intrinsic_call("gettype", vec![Expr::str(&type_name)])]) }]),
                    line,
                    name: None,
                };
                Some(Expr::intrinsic_call("lazy", vec![Expr::str(&type_name), Expr::Function(Box::new(get))]))
            }
            _ => None,
        }
    }

    /// The NL name of a declared name, from the tree (the resolver keeps
    /// no name for it).
    fn declared_name(&self, id: nl_sema::DeclId) -> String {
        self.decl_names.get(&id).cloned().unwrap_or_default()
    }

    /// The value of an alias, expanded once (see the module
    /// documentation).
    fn alias_value(&mut self, id: BindingId, name: &str, record_level: &mut u32) -> Option<Expr> {
        match self.alias_memo.get(&id).cloned() {
            Some(AliasState::Done(v, _)) => return v,
            Some(AliasState::InProgress { depth, slot, record_level: start }) => {
                if *record_level <= start {
                    // Reported by `collect_aliases`.
                    return None;
                }
                // A reference to itself through a record field.
                let Some(depth) = depth else {
                    let node = self.aliases.get(&id).cloned()?;
                    self.error(&node, "alias-recursive", "a type alias that refers to itself can only name structs, enums, interfaces, imported types and other aliases");
                    return None;
                };
                let k = match slot {
                    Some(k) => k,
                    None => {
                        let k = self.reserve_slot(depth)?;
                        self.alias_memo.insert(id, AliasState::InProgress { depth: Some(depth), slot: Some(k), record_level: start });
                        k
                    }
                };
                let target = self.slot_ref(depth, k)?;
                // The getter is part of the pool's statements.
                let line = self.pools.get(depth).map_or(self.line, |p| p.line);
                let get = Function { params: Vec::new(), vararg: false, body: Block::new(vec![Stmt { line, kind: StmtKind::Return(vec![target]) }]), line, name: None, method: false, transient: false };
                return Some(Expr::intrinsic_call("lazy", vec![Expr::str(name), Expr::Function(Box::new(get))]));
            }
            None => {}
        }
        let ty = self.aliases.get(&id).cloned()?;
        let depth = self.type_depth(&ty);
        self.alias_memo.insert(id, AliasState::InProgress { depth, slot: None, record_level: *record_level });
        let value = self.type_expr(&ty, record_level);
        let slot = match self.alias_memo.get(&id) {
            Some(AliasState::InProgress { slot, .. }) => *slot,
            _ => None,
        };
        let result = match (value, depth) {
            (Some(v), Some(d)) => match slot {
                Some(k) => {
                    self.fill_slot(d, k, v);
                    self.slot_ref(d, k)
                }
                None if is_combination(&v) => self.intern(d, v.clone()).or(Some(v)),
                None => Some(v),
            },
            (v, _) => v,
        };
        self.alias_memo.insert(id, AliasState::Done(result.clone(), depth));
        result
    }

    fn self_type_value(&mut self, node: &SyntaxNode, t: Option<SelfTarget>) -> Expr {
        match t {
            Some(SelfTarget::Prelude(p)) => self.prelude_type(p),
            other => self.self_target_value(node, other),
        }
    }

    /// The type value of a prelude type: Lua's types are descriptors in
    /// `rt.T` (also `Number`, `String`, `Table`, whose values as names are
    /// the library tables); the others are runtime values.
    fn prelude_type(&self, p: &Prelude) -> Expr {
        match (p.name, p.access) {
            ("Number" | "String" | "Table", _) => self.lua_type(p.name),
            (_, Access::TypeOnly(n)) => self.lua_type(n),
            (_, Access::Runtime(n)) => Expr::Intrinsic(n),
            (_, Access::LuaGlobal(n)) => Expr::Global(n.to_string()),
        }
    }

    /// Emits a check of `value` against `ty` at an annotated
    /// position. `what` names the position for the message
    /// (`parameter 'amount' of Player.damage`).
    ///
    /// The check is inline where it can be (see [`Inline`]): `if
    /// rt.type(x) ~= "number" then rt.check(x, rt.T.Number, what) end`,
    /// so the runtime is called only when the cheap test fails, and then
    /// applies the whole rule and builds the message.
    pub(crate) fn emit_check(&mut self, value: Expr, ty: &SyntaxNode, what: &str) {
        if !self.opts.checks {
            return;
        }
        if let Some(t) = self.type_value(ty) {
            let test = self.inline_test(ty);
            self.emit_check_value(value, t, test, what);
        }
    }

    /// Emits the check of `value` against the type value `t`, with the
    /// inline test `test` in front of `rt.check` when there is one and
    /// `value` can be read twice (a local or a constant). A constant the
    /// test accepts is not checked.
    pub(crate) fn emit_check_value(&mut self, value: Expr, t: Expr, test: Option<Inline>, what: &str) {
        let pure = value.is_constant() || matches!(value, Expr::Local(_));
        match test {
            Some(test) if pure => {
                if self.accepts_statically(&test, &value) {
                    return;
                }
                let line = self.line;
                let call = Expr::intrinsic_call("check", vec![value.clone(), t, Expr::str(what)]);
                let then = Block::new(vec![Stmt { line, kind: StmtKind::Expr(call) }]);
                let cond = self.inline_fails(&test, &value);
                self.emit(StmtKind::If { cond, then, otherwise: None });
            }
            _ => self.emit(StmtKind::Expr(Expr::intrinsic_call("check", vec![value, t, Expr::str(what)]))),
        }
    }

    /// The inline test of a type (see [`Inline`]), or `None` when the type
    /// has none and is checked by `rt.check` alone.
    pub(crate) fn inline_test(&mut self, ty: &SyntaxNode) -> Option<Inline> {
        self.inline_test_at(ty, 0)
    }

    fn inline_test_at(&mut self, ty: &SyntaxNode, depth: u32) -> Option<Inline> {
        // Aliases are followed; a cycle is an error reported elsewhere.
        if depth > 16 {
            return None;
        }
        match ty.kind() {
            NAMED_TYPE => self.named_inline_test(ty, depth),
            FN_TYPE => Some(Inline::LuaType("function")),
            // Only because `type_expr` gives `[T]` and `{K: V}` the value
            // `rt.T.Table` (element types are static): a
            // checked element type would need a test of its own.
            ARRAY_TYPE | MAP_TYPE => Some(Inline::LuaType("table")),
            PAREN_TYPE => type_child(ty).and_then(|t| self.inline_test_at(&t, depth + 1)),
            OPTIONAL_TYPE => {
                let inner = type_child(ty).and_then(|t| self.inline_test_at(&t, depth + 1))?;
                Some(Inline::Any(vec![Inline::Nil, inner]))
            }
            UNION_TYPE => {
                let mut parts = Vec::new();
                for t in ty.children().filter(|c| is_type(c.kind())) {
                    parts.push(self.inline_test_at(&t, depth + 1)?);
                }
                Some(Inline::Any(parts))
            }
            _ => None,
        }
    }

    fn named_inline_test(&mut self, ty: &SyntaxNode, depth: u32) -> Option<Inline> {
        if has_token(ty, NIL_KW) {
            return Some(Inline::Nil);
        }
        if has_token(ty, SELF_TYPE_KW) {
            let t = self.res.self_target(ty.text_range()).cloned();
            return self.self_inline_test(t);
        }
        let q = child(ty, QUAL_NAME)?;
        let segs = qual_segments(&q);
        if segs.len() != 1 {
            return None;
        }
        if segs.first().is_some_and(|s| s.text() == "Self") {
            let t = self.res.self_target(q.text_range()).cloned();
            return self.self_inline_test(t);
        }
        match self.res.type_ref(q.text_range()).cloned()? {
            Ref::Prelude(p) if p.class == nl_sema::PreludeClass::Struct => match p.access {
                Access::Runtime(n) => Some(Inline::Struct(Expr::Intrinsic(n))),
                _ => None,
            },
            Ref::Prelude(p) => match self.prelude_type(p) {
                Expr::Index(o, k) if matches!(*o, Expr::Intrinsic("T")) => match *k {
                    Expr::Str(name) => lua_type_test(&name),
                    _ => None,
                },
                _ => None,
            },
            Ref::Binding(id) => match self.res.binding(id).map(|b| b.decl)? {
                DeclKind::TypeAlias => {
                    let alias = self.aliases.get(&id).cloned()?;
                    self.inline_test_at(&alias, depth + 1)
                }
                _ => self.binding_inline_test(id),
            },
            _ => None,
        }
    }

    fn self_inline_test(&mut self, t: Option<SelfTarget>) -> Option<Inline> {
        match t? {
            SelfTarget::Binding(id) => self.binding_inline_test(id),
            _ => None,
        }
    }

    /// The test of a struct or enum binding: a struct of this file or one
    /// imported by name, an enum of this file. None in a `spawn do`
    /// block, whose captured structs are copies.
    fn binding_inline_test(&mut self, id: BindingId) -> Option<Inline> {
        if self.spawn_depth > 0 {
            return None;
        }
        let decl = self.res.binding(id).map(|b| b.decl)?;
        match (decl, self.res.declared_as(id)?) {
            (DeclKind::Struct | DeclKind::Import, DeclKind::Struct) => Some(Inline::Struct(Expr::Local(self.local_of(id)))),
            (DeclKind::Enum, DeclKind::Enum) => Some(Inline::Enum(Expr::Local(self.local_of(id)))),
            _ => None,
        }
    }

    /// The struct a struct literal `A { … }` constructs when `A` is a
    /// `type` alias of this file (an alias has no runtime value of its
    /// own): the struct it names, through other aliases.
    /// `None` when the path is not such an alias; an error message when
    /// the alias does not name a struct. A struct reached through an
    /// `import … as` table or a module is not known here; the runtime
    /// checks it, as for a literal written with that path.
    pub(crate) fn alias_literal_target(&mut self, path: &SyntaxNode) -> Option<Result<Expr, String>> {
        if path.kind() != NAME_REF {
            return None;
        }
        let Some(Ref::Binding(first)) = self.res.reference(path.text_range()).cloned() else { return None };
        if self.res.binding(first).map(|b| b.decl) != Some(DeclKind::TypeAlias) {
            return None;
        }
        let alias = self.res.binding(first).map(|b| b.name.clone()).unwrap_or_default();
        let mut id = first;
        for _ in 0..=self.aliases.len() {
            let Some(mut ty) = self.aliases.get(&id).cloned() else { break };
            while ty.kind() == PAREN_TYPE {
                match type_child(&ty) {
                    Some(t) => ty = t,
                    None => break,
                }
            }
            let not_struct = || Err(format!("`{alias}` is an alias of `{}`, which is not a struct, so `{alias} {{ … }}` cannot construct it", ty.text().to_string().trim()));
            let Some(q) = child(&ty, QUAL_NAME).filter(|_| ty.kind() == NAMED_TYPE && !has_token(&ty, SELF_TYPE_KW)) else { return Some(not_struct()) };
            let segs = qual_segments(&q).len();
            match self.res.type_ref(q.text_range()).cloned() {
                Some(Ref::Prelude(p)) if segs == 1 && p.class == nl_sema::PreludeClass::Struct => {
                    return Some(Ok(self.prelude_type(p)));
                }
                Some(Ref::Binding(b)) => {
                    let is_alias = self.res.binding(b).map(|x| x.decl) == Some(DeclKind::TypeAlias);
                    match self.res.declared_as(b) {
                        _ if segs == 1 && is_alias => id = b,
                        Some(DeclKind::Struct) if segs == 1 => return Some(Ok(Expr::Local(self.local_of(b)))),
                        Some(DeclKind::Import | DeclKind::Module) if segs > 1 => {
                            return Some(Ok(self.type_value(&ty).unwrap_or(Expr::Nil)));
                        }
                        _ => return Some(not_struct()),
                    }
                }
                _ => return Some(not_struct()),
            }
        }
        // A cycle of aliases, reported by `collect_aliases`.
        Some(Err(format!("`{alias}` does not name a struct")))
    }

    /// `x is T`.
    pub(crate) fn is_expr(&mut self, node: &SyntaxNode) -> Expr {
        let Some(value) = expr_child(node) else { return self.internal(node, "is") };
        let v = self.expr1(&value);
        match type_child(node).and_then(|t| self.type_value(&t)) {
            Some(t) => Expr::intrinsic_call("isa", vec![v, t]),
            None => Expr::Bool(true),
        }
    }

    /// `x as T`.
    pub(crate) fn as_expr(&mut self, node: &SyntaxNode) -> Expr {
        let Some(value) = expr_child(node) else { return self.internal(node, "as") };
        let v = self.expr1(&value);
        // The value is a `T` already by its declared type: `rt.as` would
        // return it unchanged.
        if self.opts.hints.conversion(node.text_range()) == Some(nl_sema::AsHint::Identity) {
            return v;
        }
        match type_child(node).and_then(|t| self.type_value(&t)) {
            Some(t) => Expr::intrinsic_call("as", vec![v, t]),
            None => v,
        }
    }
}

/// A test generated code makes inline, in front of a call of the
/// runtime. It is sufficient, not necessary: a value that passes
/// is of the type, and one that fails goes to `rt.check` (or
/// `rt.newfield`), which applies the whole rule (a descendant of a struct
/// passes there) and raises the TypeError with its message. A test calls
/// `rt.type` (Lua's `type`) or `rt.getmetatable` (the runtime's raw
/// `getmetatable`, the one its own struct test reads, so the two agree
/// whatever `__metatable` says), both copied into locals of the main
/// function by `Lowerer::rt_cached`, and reads the type's local.
#[derive(Clone, Debug)]
pub(crate) enum Inline {
    /// `rt.type(v) == name`.
    LuaType(&'static str),
    /// `v == nil`.
    Nil,
    /// `rt.getmetatable(v) == S and S ~= nil`: an instance of exactly `S`.
    /// (A struct's local is nil only while the declarations of its block
    /// run.)
    Struct(Expr),
    /// `E ~= nil and E.name(v) ~= nil`: one of the enum's values (`name`
    /// reads the table the runtime's own test reads, `rt.enum` in
    /// runtime/nl_20_struct.lua).
    Enum(Expr),
    /// Any of the parts: a union, `T?`.
    Any(Vec<Inline>),
}

impl Lowerer<'_> {
    /// The condition under which `v` (read more than once) fails `test`.
    pub(crate) fn inline_fails(&mut self, test: &Inline, v: &Expr) -> Expr {
        match test {
            Inline::LuaType(name) => Expr::binary(BinOp::Ne, Expr::call(self.rt_cached("type"), vec![v.clone()]), Expr::str(name)),
            Inline::Nil => Expr::binary(BinOp::Ne, v.clone(), Expr::Nil),
            Inline::Struct(s) => Expr::binary(
                BinOp::Or,
                Expr::binary(BinOp::Ne, Expr::call(self.rt_cached("getmetatable"), vec![v.clone()]), s.clone()),
                Expr::binary(BinOp::Eq, s.clone(), Expr::Nil),
            ),
            Inline::Enum(e) => Expr::binary(
                BinOp::Or,
                Expr::binary(BinOp::Eq, e.clone(), Expr::Nil),
                Expr::binary(BinOp::Eq, Expr::call(Expr::index(e.clone(), Expr::str("name")), vec![v.clone()]), Expr::Nil),
            ),
            Inline::Any(parts) => {
                let mut acc: Option<Expr> = None;
                for p in parts {
                    let f = self.inline_fails(p, v);
                    acc = Some(match acc {
                        Some(a) => Expr::binary(BinOp::And, a, f),
                        None => f,
                    });
                }
                acc.unwrap_or(Expr::Bool(true))
            }
        }
    }

    /// True when `test` accepts `v` whatever happens when it runs: a
    /// constant of the type, or a struct literal built inline of the
    /// struct (`(rt.setmetatable(t, S))`, structs.rs).
    pub(crate) fn accepts_statically(&self, test: &Inline, v: &Expr) -> bool {
        match (test, v) {
            (Inline::LuaType("number"), Expr::Number(_))
            | (Inline::LuaType("string"), Expr::Str(_))
            | (Inline::LuaType("boolean"), Expr::Bool(_))
            | (Inline::Nil, Expr::Nil) => true,
            (Inline::Struct(Expr::Local(s)), Expr::Paren(inner)) => match inner.as_ref() {
                Expr::Call(f, args) => self.is_rt_cached(f, "setmetatable") && matches!(args.as_slice(), [_, Expr::Local(m)] if m == s),
                _ => false,
            },
            (Inline::Any(parts), _) => parts.iter().any(|p| self.accepts_statically(p, v)),
            _ => false,
        }
    }
}

impl Inline {
    /// True when the test rejects a table marked as keyed arguments
    /// (runtime/nl_40_ops.lua, `KW`): a parameter that passes it holds no
    /// keyed arguments.
    pub(crate) fn excludes_keyed(&self) -> bool {
        match self {
            Inline::LuaType(name) => *name != "table",
            Inline::Nil | Inline::Struct(_) | Inline::Enum(_) => true,
            Inline::Any(parts) => parts.iter().all(Inline::excludes_keyed),
        }
    }

}

/// The inline test of a descriptor of `rt.T`, by its key.
fn lua_type_test(name: &[u8]) -> Option<Inline> {
    Some(match name {
        b"Number" => Inline::LuaType("number"),
        b"String" => Inline::LuaType("string"),
        b"Bool" => Inline::LuaType("boolean"),
        b"Table" => Inline::LuaType("table"),
        b"Function" => Inline::LuaType("function"),
        b"Userdata" => Inline::LuaType("userdata"),
        b"Thread" => Inline::LuaType("thread"),
        b"Nil" => Inline::Nil,
        _ => return None,
    })
}

/// True for a type value that is built by a call (and so worth building
/// once): `rt.optional(…)`, `rt.union(…)`, `rt.negate(…)`, `rt.record(…)`,
/// `rt.lazy(…)`.
fn is_combination(e: &Expr) -> bool {
    matches!(e, Expr::Call(f, _) if matches!(**f, Expr::Intrinsic("optional" | "union" | "negate" | "record" | "lazy")))
}
