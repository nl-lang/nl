//! Declarations of variables and assignment: `let`,
//! `const`, `global`, `=`, multiple assignment and compound assignment.
//!
//! An annotated variable is checked on every assignment; a failed check
//! leaves the old value (the value is checked before it is stored).

use nl_sema::util::*;
use nl_sema::{DeclKind, Ref};
use nl_syntax::{SyntaxKind, SyntaxNode};

use super::Lowerer;
use super::expr::single;
use super::func::simple_binding;
use crate::ir::*;

use SyntaxKind::*;

/// A lowered assignment target, with what is needed to check it.
pub(crate) struct Target {
    pub place: Place,
    /// The annotation to check values against, and the variable's name.
    pub check: Option<(SyntaxNode, String)>,
    /// The binding is `let f = fn … end`: it counts as annotated
    /// `Function`. Its name, for messages.
    pub function_kind: Option<String>,
    /// The target node, which the analyzer's proven checks are keyed by.
    pub site: Option<nl_syntax::TextRange>,
}

/// How the parts of an index target are fixed before the values
/// ([`Lowerer::stable_place`]).
enum Fix<'a> {
    /// Statements run before the store and may assign any variable: every
    /// part goes to a temporary.
    All,
    /// The target is read and then written (compound assignment): each
    /// part is evaluated once, and a call may run in between.
    Reused,
    /// These expressions are evaluated before the store: a local that a
    /// call among them could change is pinned.
    Before(&'a [Expr]),
}

impl Lowerer<'_> {
    /// `let` and `const`.
    pub(crate) fn lower_let(&mut self, node: &SyntaxNode) {
        let pats: Vec<SyntaxNode> = pattern_children(node).collect();
        let value_nodes: Vec<SyntaxNode> = child(node, EXPR_LIST).map(|l| expr_children(&l).collect()).unwrap_or_default();
        let simple: Option<Vec<(SyntaxNode, Option<SyntaxNode>)>> = pats.iter().map(simple_binding).collect();
        match simple {
            Some(names) => self.let_simple(&names, &value_nodes),
            None => self.let_patterns(&pats, &value_nodes),
        }
        // A `const` directly in a module body is a member of the
        // module: the local is also stored under its name in the
        // module table, for `Self.limit` and `Inventory.limit`.
        if let Some(m) = self.res.module_const(node.text_range()) {
            let module = Expr::Local(self.local_of(m));
            let mut ids = Vec::new();
            for p in &pats {
                self.pattern_bindings(p, &mut ids);
            }
            for id in ids {
                let name = self.res.binding(id).map(|b| b.name.clone()).unwrap_or_default();
                let l = self.local_of(id);
                self.emit(StmtKind::Assign { targets: vec![Place::Index(module.clone(), Expr::str(&name))], values: vec![Expr::Local(l)] });
            }
        }
    }

    /// `let a, b: T = values` with plain names.
    fn let_simple(&mut self, names: &[(SyntaxNode, Option<SyntaxNode>)], value_nodes: &[SyntaxNode]) {
        let ids: Vec<_> = names.iter().filter_map(|(n, _)| self.res.declaration(n.text_range())).collect();
        if ids.len() != names.len() {
            if let Some((n, _)) = names.first() {
                self.internal(n, "undeclared let");
            }
            return;
        }
        let locals: Vec<LocalId> = ids.iter().map(|id| self.local_of(*id)).collect();
        let (stmts, values) = self.capture(|l| l.values_for(value_nodes, names.len()));
        let predeclared = ids.iter().all(|id| self.is_predeclared(*id));
        if predeclared {
            // The temporaries of the values end with the statement.
            let assign = (!values.is_empty() || value_nodes.is_empty()).then(|| {
                let values = if values.is_empty() { vec![Expr::Nil] } else { values };
                Stmt { line: self.line, kind: StmtKind::Assign { targets: locals.iter().map(|l| Place::Local(*l)).collect(), values } }
            });
            if stmts.is_empty() {
                self.stmts.extend(assign);
            } else {
                let mut body = stmts;
                body.extend(assign);
                self.emit(StmtKind::Do(Block::new(body)));
            }
        } else if stmts.is_empty() {
            self.emit(StmtKind::Local { locals: locals.clone(), values });
        } else {
            self.emit(StmtKind::Local { locals: locals.clone(), values: Vec::new() });
            let mut body = stmts;
            body.push(Stmt { line: self.line, kind: StmtKind::Assign { targets: locals.iter().map(|l| Place::Local(*l)).collect(), values } });
            self.emit(StmtKind::Do(Block::new(body)));
        }
        for id in &ids {
            self.mark_predeclared(*id);
        }
        // `let d: Number` without a value declares `d` nil; the type is
        // checked on later assignments (as for `global`).
        if value_nodes.is_empty() {
            return;
        }
        for ((name, ty), l) in names.iter().zip(&locals) {
            if let Some(t) = ty {
                // Dropped when the analyzer proved it.
                if self.opts.hints.is_proven(t.text_range()) {
                    continue;
                }
                let what = format!("variable '{}'", name_text(name).unwrap_or_default());
                self.emit_check(Expr::Local(*l), t, &what);
            }
        }
    }

    /// Lowers the values of a declaration or assignment: every value but
    /// the last gives one value; the last may give several.
    pub(crate) fn values_for(&mut self, nodes: &[SyntaxNode], _wanted: usize) -> Vec<Expr> {
        let vals = self.operands(nodes);
        let n = vals.len();
        vals.into_iter().enumerate().map(|(i, v)| if i + 1 < n { single(v) } else { v }).collect()
    }

    /// `let` with destructuring patterns: the values go to
    /// temporaries, then each pattern binds from its value. The bound names
    /// are declared first, and the temporaries end with the statement.
    fn let_patterns(&mut self, pats: &[SyntaxNode], value_nodes: &[SyntaxNode]) {
        let mut ids = Vec::new();
        for p in pats {
            self.pattern_bindings(p, &mut ids);
        }
        let fresh: Vec<_> = ids.into_iter().filter(|id| !self.is_predeclared(*id)).collect();
        if !fresh.is_empty() {
            let locals = fresh.iter().map(|id| self.local_of(*id)).collect();
            self.emit(StmtKind::Local { locals, values: Vec::new() });
            for id in fresh {
                self.mark_predeclared(id);
            }
        }
        let body = self.sub_block(|l| {
            let values = l.values_for(value_nodes, pats.len());
            let temps: Vec<LocalId> = pats.iter().map(|_| l.new_local("value")).collect();
            l.emit(StmtKind::Local { locals: temps.clone(), values });
            for (p, t) in pats.iter().zip(temps) {
                l.bind_must(p, Expr::Local(t), "");
            }
        });
        self.emit(StmtKind::Do(body));
    }

    /// `global Name [: T] [= value]`: a Lua global.
    pub(crate) fn lower_global(&mut self, node: &SyntaxNode) {
        let Some(value) = expr_child(node) else { return };
        let name = child(node, NAME).and_then(|n| name_text(&n)).unwrap_or_default();
        let v = self.expr1(&value);
        match type_child(node) {
            Some(t) if self.opts.checks => {
                let tmp = self.temp_of(v, "value");
                self.emit_check(tmp.clone(), &t, &format!("global '{name}'"));
                self.emit(StmtKind::Assign { targets: vec![Place::Global(name)], values: vec![tmp] });
            }
            _ => self.emit(StmtKind::Assign { targets: vec![Place::Global(name)], values: vec![v] }),
        }
    }

    /// Lowers an assignment target. Subexpressions are evaluated now and
    /// pushed on `vals` so later operands can be ordered after them.
    pub(crate) fn target(&mut self, node: &SyntaxNode) -> Target {
        let none = |place| Target { place, check: None, function_kind: None, site: None };
        match node.kind() {
            NAME_REF => {
                let r = self.res.reference(node.text_range()).cloned();
                match r {
                    Some(Ref::Binding(id)) => {
                        let Some(b) = self.res.binding(id).cloned() else { return none(Place::Global(String::new())) };
                        let place = if b.decl == DeclKind::Global { Place::Global(b.name.clone()) } else { Place::Local(self.local_of(id)) };
                        let function_kind = b.params.is_some_and(|p| p.anonymous).then(|| b.name.clone());
                        Target { place, check: b.annotation.map(|a| (a, b.name.clone())), function_kind, site: Some(node.text_range()) }
                    }
                    Some(Ref::Declared(_)) => {
                        // A declared `let`: its Lua path.
                        match self.name_raw(node) {
                            Expr::Global(g) => none(Place::Global(g)),
                            Expr::Index(o, k) => none(Place::Index(*o, *k)),
                            _ => {
                                self.internal(node, "assignment to a declared value that is not a place");
                                none(Place::Global(String::new()))
                            }
                        }
                    }
                    _ => {
                        self.internal(node, "assignment to an unresolved name");
                        none(Place::Global(String::new()))
                    }
                }
            }
            FIELD_EXPR => {
                // A declared member's Lua key.
                let name = match self.opts.hints.member(node.text_range()) {
                    Some(h) => h.key().to_string(),
                    None => field_name(node).map(|t| t.text().to_string()).unwrap_or_default(),
                };
                let o = match expr_child(node) {
                    Some(r) => self.expr1(&r),
                    None => self.internal(node, "member target"),
                };
                none(Place::Index(o, Expr::str(&name)))
            }
            INDEX_EXPR => {
                let parts: Vec<SyntaxNode> = expr_children(node).collect();
                let mut it = self.operands(&parts).into_iter().map(single);
                let (o, k) = (it.next().unwrap_or(Expr::Nil), it.next().unwrap_or(Expr::Nil));
                none(Place::Index(o, k))
            }
            AT_EXPR => match self.at_expr(node) {
                Expr::Index(o, k) => none(Place::Index(*o, *k)),
                _ => none(Place::Global(String::new())),
            },
            LUA_PATH => {
                let place = self.lua_path_place(node);
                none(place)
            }
            _ => {
                self.internal(node, "assignment target");
                none(Place::Global(String::new()))
            }
        }
    }

    /// Emits the checks a target needs for `value`, returning the value to
    /// store (moved to a temporary when checked).
    fn checked(&mut self, t: &Target, value: Expr) -> Expr {
        if !self.opts.checks || t.site.is_some_and(|s| self.opts.hints.is_proven(s)) {
            return value;
        }
        if let Some((ty, name)) = &t.check {
            let v = self.temp_of(value, "value");
            self.emit_check(v.clone(), ty, &format!("variable '{name}'"));
            return v;
        }
        if let Some(name) = &t.function_kind {
            let v = self.temp_of(value, "value");
            let what = format!("variable '{name}' (initialized with `fn`)");
            let t = Expr::index(Expr::Intrinsic("T"), Expr::str("Function"));
            self.emit_check_value(v.clone(), t, Some(super::types::Inline::LuaType("function")), &what);
            return v;
        }
        value
    }

    /// Fixes the object and key of an index target before the values are
    /// evaluated, so that the store goes where the target pointed when it
    /// was evaluated (left to right, the target first and
    /// once).
    fn stable_place(&mut self, place: Place, fix: Fix<'_>) -> Place {
        let Place::Index(o, k) = place else { return place };
        let part = |l: &mut Self, e: Expr, hint: &str| -> Expr {
            match (&fix, e) {
                (_, e) if e.is_constant() => e,
                (Fix::All, e) => Expr::Local(l.fresh_temp(e, hint)),
                (Fix::Reused, e @ Expr::Local(_)) => l.pin(e, &Expr::Vararg),
                (Fix::Reused, e) => Expr::Local(l.fresh_temp(e, hint)),
                (Fix::Before(later), e @ Expr::Local(_)) if later.iter().any(super::may_run_code) => l.pin(e, &Expr::Vararg),
                (Fix::Before(_), e) => e,
            }
        };
        let o = part(self, o, "obj");
        let k = part(self, k, "key");
        Place::Index(o, k)
    }

    /// `target = value`.
    pub(crate) fn lower_assign(&mut self, node: &SyntaxNode) {
        let parts: Vec<SyntaxNode> = expr_children(node).collect();
        let (Some(tnode), Some(vnode)) = (parts.first(), parts.get(1)) else {
            self.internal(node, "assignment");
            return;
        };
        // `a?.b = v`: nothing is stored, and `v` is not evaluated, when
        // `a` is `nil`.
        if self.safe_assignment(node, tnode, Self::lower_assign) {
            return;
        }
        let t = self.target(tnode);
        let (stmts, v) = self.capture(|l| l.expr1(vnode));
        let fix = if stmts.is_empty() { Fix::Before(std::slice::from_ref(&v)) } else { Fix::All };
        let place = self.stable_place(t.place, fix);
        self.stmts.extend(stmts);
        let t = Target { place, check: t.check, function_kind: t.function_kind, site: t.site };
        let v = self.checked(&t, v);
        self.emit(StmtKind::Assign { targets: vec![t.place], values: vec![v] });
    }

    /// `a, b = values`.
    pub(crate) fn lower_multi_assign(&mut self, node: &SyntaxNode) {
        let Some(list) = child(node, TARGET_LIST) else {
            self.internal(node, "multiple assignment");
            return;
        };
        let tnodes: Vec<SyntaxNode> = expr_children(&list).collect();
        let vnodes: Vec<SyntaxNode> = child(node, EXPR_LIST).map(|l| expr_children(&l).collect()).unwrap_or_default();
        // Targets left to right, then the values; what is evaluated after a
        // target decides how its parts are fixed (`stable_place`).
        let mut lowered = Vec::new();
        for t in &tnodes {
            lowered.push(self.capture(|l| l.target(t)));
        }
        let (vstmts, values) = self.capture(|l| l.values_for(&vnodes, tnodes.len()));
        // For each target: whether statements run after it, and what is
        // evaluated after it (the parts of later targets, the values).
        let mut after: Vec<(bool, Vec<Expr>)> = Vec::new();
        let mut stmts_after = !vstmts.is_empty();
        let mut exprs_after: Vec<Expr> = values.clone();
        for (tstmts, tg) in lowered.iter().rev() {
            after.push((stmts_after, exprs_after.clone()));
            stmts_after |= !tstmts.is_empty();
            if let Place::Index(o, k) = &tg.place {
                exprs_after.push(o.clone());
                exprs_after.push(k.clone());
            }
        }
        after.reverse();
        let mut targets = Vec::new();
        for ((tstmts, tg), (stmts_after, exprs_after)) in lowered.into_iter().zip(after) {
            self.stmts.extend(tstmts);
            let fix = if stmts_after { Fix::All } else { Fix::Before(&exprs_after) };
            let place = self.stable_place(tg.place, fix);
            targets.push(Target { place, check: tg.check, function_kind: tg.function_kind, site: tg.site });
        }
        self.stmts.extend(vstmts);
        let any_check = self.opts.checks && targets.iter().any(|t| t.check.is_some() || t.function_kind.is_some());
        if !any_check {
            self.emit(StmtKind::Assign { targets: targets.into_iter().map(|t| t.place).collect(), values });
            return;
        }
        let temps: Vec<LocalId> = targets.iter().map(|_| self.new_local("value")).collect();
        self.emit(StmtKind::Local { locals: temps.clone(), values });
        let mut vals = Vec::new();
        for (t, tmp) in targets.iter().zip(&temps) {
            vals.push(self.checked(t, Expr::Local(*tmp)));
        }
        self.emit(StmtKind::Assign { targets: targets.into_iter().map(|t| t.place).collect(), values: vals });
    }

    /// `target op= value`: the target is evaluated once.
    pub(crate) fn lower_compound(&mut self, node: &SyntaxNode) {
        let parts: Vec<SyntaxNode> = expr_children(node).collect();
        let op = tokens(node).find(|t| t.kind().is_compound_assign()).map(|t| t.kind());
        let (Some(tnode), Some(vnode), Some(op)) = (parts.first(), parts.get(1), op) else {
            self.internal(node, "compound assignment");
            return;
        };
        if self.safe_assignment(node, tnode, Self::lower_compound) {
            return;
        }
        let t = self.target(tnode);
        // The target is read now and written after the value: its parts are
        // fixed as if code ran in between (`t[i] += f()` where `f` assigns
        // `i` still writes `t[old i]`).
        let place = self.stable_place(t.place, Fix::Reused);
        let current = match &place {
            Place::Local(l) => Expr::Local(*l),
            Place::Global(g) => Expr::Global(g.clone()),
            Place::Index(o, k) => Expr::index(o.clone(), k.clone()),
        };
        let t = Target { place, check: t.check, function_kind: t.function_kind, site: t.site };
        if matches!(op, PIPE2_EQ | AMP2_EQ | QUESTION2_EQ) {
            // `a ||= b` assigns when `a` is falsy, `a &&= b` when truthy,
            // `a ??= b` when `nil`.
            let cond = match op {
                PIPE2_EQ => Expr::lnot(current),
                QUESTION2_EQ => Expr::binary(BinOp::Eq, current, Expr::Nil),
                _ => current,
            };
            let body = self.sub_block(|l| {
                let v = l.expr1(vnode);
                let v = l.checked(&t, v);
                l.emit(StmtKind::Assign { targets: vec![t.place.clone()], values: vec![v] });
            });
            self.emit(StmtKind::If { cond, then: body, otherwise: None });
            return;
        }
        let (stmts, v) = self.capture(|l| l.expr1(vnode));
        let current = if stmts.is_empty() { self.pin(current, &v) } else { self.temp_of(current, "old") };
        self.stmts.extend(stmts);
        let new = self.binary_op(node, op, current, v);
        let new = self.checked(&t, new);
        self.emit(StmtKind::Assign { targets: vec![t.place], values: vec![new] });
    }
}
