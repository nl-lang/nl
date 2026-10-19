//! Control flow: `if`/`elseif`/`else`, `unless`, `cond`, `case`
//! on values, `while`, `until`, `loop`, `for … in`, `break`, `continue`.
//!
//! `if let` and pattern items of `when` call into `pattern`; this module
//! only builds the branch structure.

use nl_sema::util::*;
use nl_syntax::{SyntaxKind, SyntaxNode};

use super::expr::single;
use super::func::simple_binding;
use super::{LoopState, Lowerer, Tail};
use crate::ir::*;

use SyntaxKind::*;

/// One branch of an `if` chain: statements that compute its condition
/// (run only when the earlier conditions failed), the condition, the body.
struct Clause {
    /// The line of the clause (`if`, `elseif`, `when`): its test and the
    /// `if` statement carry it, so that a branch's statements follow their
    /// test in line order (the Lua backend cannot move back a line).
    line: u32,
    stmts: Vec<Stmt>,
    cond: Expr,
    body: Block,
}

impl Lowerer<'_> {
    /// Emits `if c1 … elseif c2 … else … end`, keeping each condition's
    /// statements inside the `else` of the previous branch.
    fn emit_chain(&mut self, clauses: Vec<Clause>, otherwise: Option<Block>) {
        let mut rest = otherwise;
        for c in clauses.into_iter().rev() {
            let mut stmts = c.stmts;
            stmts.push(Stmt { line: c.line, kind: StmtKind::If { cond: c.cond, then: c.body, otherwise: rest } });
            rest = Some(Block::new(stmts));
        }
        if let Some(b) = rest {
            self.stmts.extend(b.stmts);
        }
    }

    /// The value of a block expression when no branch is taken, and
    /// of a function whose last statement is such an expression. Used as a
    /// value, the expression is `nil` (its temporary starts out `nil`); as
    /// a function's tail it returns no value. Both compare equal to `nil`,
    /// which is what the suite assumes. This is the one place to change for
    /// another choice.
    fn missing_branch(&mut self, _tail: Tail) -> Option<Block> {
        None
    }

    /// A condition, or an `if let`.
    fn condition(&mut self, owner: &SyntaxNode) -> Expr {
        if let Some(lc) = child(owner, LET_CONDITION) {
            let Some(value) = expr_child(&lc) else { return self.internal(&lc, "let condition") };
            let Some(pat) = pattern_children(&lc).next() else { return self.internal(&lc, "let condition") };
            let v = self.expr1(&value);
            return self.bind_may(&pat, v);
        }
        match expr_child(owner) {
            Some(e) => self.expr1(&e),
            None => self.internal(owner, "condition"),
        }
    }

    pub(crate) fn lower_if(&mut self, node: &SyntaxNode, tail: Tail) {
        let mut clauses = Vec::new();
        let mut otherwise = None;
        let line = self.line;
        let (stmts, cond) = self.capture(|l| l.condition(node));
        let body = block(node).map(|b| self.lower_block(&b, tail)).unwrap_or_default();
        clauses.push(Clause { line, stmts, cond, body });
        for c in node.children() {
            match c.kind() {
                ELSEIF_CLAUSE => {
                    self.line = self.line_of(&c);
                    let line = self.line;
                    let (stmts, cond) = self.capture(|l| l.condition(&c));
                    let body = block(&c).map(|b| self.lower_block(&b, tail)).unwrap_or_default();
                    clauses.push(Clause { line, stmts, cond, body });
                }
                ELSE_CLAUSE => otherwise = block(&c).map(|b| self.lower_block(&b, tail)),
                _ => {}
            }
        }
        if otherwise.is_none() {
            otherwise = self.missing_branch(tail);
        }
        self.emit_chain(clauses, otherwise);
    }

    pub(crate) fn lower_unless(&mut self, node: &SyntaxNode, tail: Tail) {
        let line = self.line;
        let (stmts, cond) = self.capture(|l| l.condition(node));
        let body = block(node).map(|b| self.lower_block(&b, tail)).unwrap_or_default();
        let mut otherwise = child(node, ELSE_CLAUSE).and_then(|c| block(&c)).map(|b| self.lower_block(&b, tail));
        if otherwise.is_none() {
            otherwise = self.missing_branch(tail);
        }
        self.emit_chain(vec![Clause { line, stmts, cond: Expr::lnot(cond), body }], otherwise);
    }

    /// `cond`: the first branch whose condition is truthy.
    pub(crate) fn lower_cond(&mut self, node: &SyntaxNode, tail: Tail) {
        let mut clauses = Vec::new();
        let mut otherwise = None;
        for c in node.children() {
            match c.kind() {
                COND_CLAUSE => {
                    self.line = self.line_of(&c);
                    let line = self.line;
                    let (stmts, cond) = self.capture(|l| l.condition(&c));
                    let body = block(&c).map(|b| self.lower_block(&b, tail)).unwrap_or_default();
                    clauses.push(Clause { line, stmts, cond, body });
                }
                ELSE_CLAUSE => otherwise = block(&c).map(|b| self.lower_block(&b, tail)),
                _ => {}
            }
        }
        if otherwise.is_none() {
            otherwise = self.missing_branch(tail);
        }
        self.emit_chain(clauses, otherwise);
    }

    /// `case`. Expression items compare with `==`, or by
    /// containment for a Range value; pattern items go to `pattern`.
    pub(crate) fn lower_case(&mut self, node: &SyntaxNode, tail: Tail) {
        let Some(subject) = expr_child(node) else {
            self.internal(node, "case without subject");
            return;
        };
        let s = self.expr1(&subject);
        let s = Expr::Local(self.fresh_temp(s, "subject"));
        let mut clauses = Vec::new();
        let mut otherwise = None;
        for c in node.children() {
            match c.kind() {
                WHEN_CLAUSE => {
                    self.line = self.line_of(&c);
                    let line = self.line;
                    let (stmts, cond) = self.capture(|l| l.when_test(&c, &s));
                    let body = block(&c).map(|b| self.lower_block(&b, tail)).unwrap_or_default();
                    clauses.push(Clause { line, stmts, cond, body });
                }
                ELSE_CLAUSE => otherwise = block(&c).map(|b| self.lower_block(&b, tail)),
                _ => {}
            }
        }
        if otherwise.is_none() {
            otherwise = self.missing_branch(tail);
        }
        self.emit_chain(clauses, otherwise);
    }

    /// The test of one `when` clause: its items joined with `or`, then the
    /// guard.
    fn when_test(&mut self, clause: &SyntaxNode, subject: &Expr) -> Expr {
        let items: Vec<SyntaxNode> = clause.children().filter(|c| is_pattern(c.kind()) || is_expr(c.kind())).collect();
        let mut tests = Vec::new();
        for item in &items {
            let (stmts, t) = self.capture(|l| {
                if is_pattern(item.kind()) {
                    l.case_pattern(item, subject.clone())
                } else {
                    l.when_value(item, subject.clone())
                }
            });
            tests.push((stmts, t));
        }
        let simple = tests.iter().all(|(s, _)| s.is_empty());
        let mut cond = if simple {
            let mut acc: Option<Expr> = None;
            for (_, t) in tests {
                acc = Some(match acc {
                    None => t,
                    Some(a) => Expr::binary(BinOp::Or, a, t),
                });
            }
            acc.unwrap_or(Expr::Bool(false))
        } else {
            // Items that need statements: a flag set by the first match.
            let flag = self.fresh_temp(Expr::Bool(false), "matched");
            let line = self.line;
            let mut rest: Vec<Stmt> = Vec::new();
            for (stmts, t) in tests.into_iter().rev() {
                let mut block = stmts;
                block.push(Stmt { line, kind: StmtKind::Assign { targets: vec![Place::Local(flag)], values: vec![t] } });
                if !rest.is_empty() {
                    block.push(Stmt {
                        line,
                        kind: StmtKind::If { cond: Expr::lnot(Expr::Local(flag)), then: Block::new(rest), otherwise: None },
                    });
                }
                rest = block;
            }
            self.stmts.extend(rest);
            Expr::Local(flag)
        };
        if let Some(g) = child(clause, WHEN_GUARD).and_then(|g| expr_child(&g)) {
            let (stmts, gv) = self.capture(|l| l.expr1(&g));
            if stmts.is_empty() {
                cond = Expr::binary(BinOp::And, cond, gv);
            } else {
                let flag = self.fresh_temp(cond, "matched");
                let mut body = stmts;
                body.push(Stmt { line: self.line, kind: StmtKind::Assign { targets: vec![Place::Local(flag)], values: vec![gv] } });
                self.emit(StmtKind::If { cond: Expr::Local(flag), then: Block::new(body), otherwise: None });
                cond = Expr::Local(flag);
            }
        }
        cond
    }

    /// An expression item of `when`: a literal compares with `==`; any
    /// other value goes through the runtime, which matches a Range by
    /// containment (and a Regex by matching) and compares anything else
    /// with `==`.
    fn when_value(&mut self, item: &SyntaxNode, subject: Expr) -> Expr {
        let is_literal = item.kind() == LITERAL
            || (item.kind() == PREFIX_EXPR && expr_child(item).is_some_and(|e| e.kind() == LITERAL));
        let v = self.expr1(item);
        if is_literal {
            Expr::binary(BinOp::Eq, subject, v)
        } else {
            Expr::intrinsic_call("when", vec![subject, v])
        }
    }

    // ---- loops ----------------------------------------------------------------

    fn push_loop(&mut self) {
        if let Some(f) = self.fns.last_mut() {
            f.loops.push(LoopState::default());
        }
    }

    /// `while`, `until` and `loop`.
    pub(crate) fn lower_while(&mut self, node: &SyntaxNode) {
        let (cond_stmts, cond) = match node.kind() {
            LOOP_STMT => (Vec::new(), Expr::Bool(true)),
            k => {
                let (stmts, c) = self.capture(|l| match expr_child(node) {
                    Some(e) => l.expr1(&e),
                    None => l.internal(node, "loop condition"),
                });
                (stmts, if k == UNTIL_STMT { Expr::lnot(c) } else { c })
            }
        };
        self.push_loop();
        let body = block(node).map(|b| self.lower_block(&b, Tail::Discard)).unwrap_or_default();
        let body = self.finish_loop_body(body);
        if cond_stmts.is_empty() {
            self.emit(StmtKind::While { cond, body });
        } else {
            let line = self.line;
            let mut stmts = cond_stmts;
            stmts.push(Stmt {
                line,
                kind: StmtKind::If { cond: Expr::lnot(cond), then: Block::new(vec![Stmt { line, kind: StmtKind::Break }]), otherwise: None },
            });
            stmts.extend(body.stmts);
            self.emit(StmtKind::While { cond: Expr::Bool(true), body: Block::new(stmts) });
        }
    }

    /// `for patterns in expr [step s]`. A literal range is a
    /// numeric loop; anything else asks the runtime for an iterator, which
    /// picks the form from the value.
    pub(crate) fn lower_for(&mut self, node: &SyntaxNode) {
        let mut pats = Vec::new();
        let mut iterable = None;
        let mut seen_in = false;
        for el in node.children_with_tokens() {
            match el {
                rowan::NodeOrToken::Token(t) if t.kind() == IN_KW => seen_in = true,
                rowan::NodeOrToken::Node(n) if !seen_in && is_pattern(n.kind()) => pats.push(n),
                rowan::NodeOrToken::Node(n) if seen_in && iterable.is_none() && is_expr(n.kind()) => iterable = Some(n),
                _ => {}
            }
        }
        let Some(iterable) = iterable else {
            self.internal(node, "for without iterable");
            return;
        };
        let step = child(node, FOR_STEP).and_then(|s| expr_child(&s));
        if let Some(range) = literal_range(&iterable) {
            self.numeric_for(node, &pats, &range, step.as_ref());
            return;
        }
        let it = self.expr(&iterable);
        let mut vars = Vec::new();
        let mut destructure = Vec::new();
        for p in &pats {
            match simple_binding(p) {
                Some((name, ty)) => {
                    let Some(id) = self.res.declaration(name.text_range()) else { continue };
                    let l = self.local_of(id);
                    self.mark_predeclared(id);
                    vars.push(l);
                    if let Some(t) = ty {
                        destructure.push((l, None, Some((t, name_text(&name).unwrap_or_default()))));
                    }
                }
                None => {
                    let t = self.new_local("item");
                    vars.push(t);
                    destructure.push((t, Some(p.clone()), None));
                }
            }
        }
        let n = vars.len();
        self.push_loop();
        let body = self.sub_block(|l| {
            for (local, pat, check) in destructure {
                if let Some((ty, name)) = check {
                    l.emit_check(Expr::Local(local), &ty, &format!("loop variable '{name}'"));
                }
                if let Some(p) = pat {
                    l.bind_must(&p, Expr::Local(local), "the loop variable");
                }
            }
            if let Some(b) = block(node) {
                let inner = l.lower_block(&b, Tail::Discard);
                l.stmts.extend(inner.stmts);
            }
        });
        let body = self.finish_loop_body(body);
        // The loop form fixed by the declared type of the iterable:
        // each form is what `rt.iter` picks for such a value.
        match self.opts.hints.loop_form(node.text_range()) {
            Some(nl_sema::LoopHint::Function) => {
                self.emit(StmtKind::GenericFor { vars, iter: vec![it], body });
            }
            Some(nl_sema::LoopHint::Map) => {
                let t = self.temp_of(single(it), "map");
                self.emit(StmtKind::GenericFor { vars, iter: vec![Expr::Global("next".to_string()), t, Expr::Nil], body });
            }
            Some(nl_sema::LoopHint::Array) if n == 1 => {
                // The array values in order, up to the first nil.
                let t = self.temp_of(single(it), "list");
                let i = self.new_local("i");
                self.emit(StmtKind::Local { locals: vec![i], values: vec![Expr::Number(0.0)] });
                let line = self.line;
                let var = vars.first().copied().unwrap_or(i);
                let mut stmts = vec![
                    Stmt { line, kind: StmtKind::Assign { targets: vec![Place::Local(i)], values: vec![Expr::binary(BinOp::Add, Expr::Local(i), Expr::Number(1.0))] } },
                    Stmt { line, kind: StmtKind::Local { locals: vec![var], values: vec![Expr::index(t, Expr::Local(i))] } },
                    Stmt { line, kind: StmtKind::If { cond: Expr::binary(BinOp::Eq, Expr::Local(var), Expr::Nil), then: Block::new(vec![Stmt { line, kind: StmtKind::Break }]), otherwise: None } },
                ];
                stmts.extend(body.stmts);
                self.emit(StmtKind::While { cond: Expr::Bool(true), body: Block::new(stmts) });
            }
            _ => {
                if let [var] = vars[..] {
                    self.for_one(var, it, body);
                } else {
                    let iter = Expr::intrinsic_call("iter", vec![Expr::Number(n as f64), it]);
                    self.emit(StmtKind::GenericFor { vars, iter: vec![iter], body });
                }
            }
        }
    }

    /// `for var in it` when nothing is known of `it`. The loop keeps the
    /// iterator's control in a hidden variable, so that the runtime can
    /// answer with an iterator that is not a closure for a table or a
    /// string (`rt.iter` with 0 for the number of variables,
    /// runtime/nl_40_ops.lua): LuaJIT compiles no closure creation, and a
    /// closure per loop kept every hot loop or function around such a
    /// `for` out of a trace.
    ///
    /// ```lua
    /// do
    ///   local f, state, control, hidden = rt.iter(0, it)
    ///   hidden = hidden == rt.iter
    ///   for c, v in f, state, control do
    ///     local var = hidden and v or not hidden and c
    ///     …
    ///   end
    /// end
    /// ```
    ///
    /// The iterator of a table or a string returns the control and then
    /// the value, and `rt.iter` says so with a fourth result, itself; a
    /// function, a range and `__iter` return the value first, as Lua's
    /// protocol has it, and the loop is Lua's `for var in f, state,
    /// control`. `var` is declared once with its value (a `false` in a
    /// list stays `false`), as a loop variable is: lift.rs passes a
    /// variable that is never assigned to the closures of a `try`.
    fn for_one(&mut self, var: LocalId, it: Expr, body: Block) {
        let line = self.line;
        // Hints no program is likely to use for a variable of its own: the
        // Lua backend numbers the variables that share a name.
        let [f, state, control, hidden, c, v] =
            ["iter_f", "iter_state", "iter_control", "iter_hidden", "iter_c", "iter_v"].map(|hint| self.new_local(hint));
        let local = |l: LocalId| Expr::Local(l);
        let value = Expr::binary(
            BinOp::Or,
            Expr::binary(BinOp::And, local(hidden), local(v)),
            Expr::binary(BinOp::And, Expr::lnot(local(hidden)), local(c)),
        );
        let mut stmts = vec![Stmt { line, kind: StmtKind::Local { locals: vec![var], values: vec![value] } }];
        stmts.extend(body.stmts);
        let iter = Expr::intrinsic_call("iter", vec![Expr::Number(0.0), it]);
        let marked = Expr::binary(BinOp::Eq, local(hidden), Expr::Intrinsic("iter"));
        self.emit(StmtKind::Do(Block::new(vec![
            Stmt { line, kind: StmtKind::Local { locals: vec![f, state, control, hidden], values: vec![iter] } },
            Stmt { line, kind: StmtKind::Assign { targets: vec![Place::Local(hidden)], values: vec![marked] } },
            Stmt { line, kind: StmtKind::GenericFor { vars: vec![c, v], iter: vec![local(f), local(state), local(control)], body: Block::new(stmts) } },
        ])));
    }

    fn numeric_for(&mut self, node: &SyntaxNode, pats: &[SyntaxNode], range: &SyntaxNode, step: Option<&SyntaxNode>) {
        let exclusive = bin_op(range).is_some_and(|t| t.kind() == DOT3);
        let bounds: Vec<SyntaxNode> = expr_children(range).collect();
        let mut vals = self.operands(&bounds).into_iter().map(single);
        let (Some(start), Some(limit)) = (vals.next(), vals.next()) else {
            self.internal(range, "range bounds");
            return;
        };
        let start = self.temp_of(start, "from");
        let limit = self.temp_of(limit, "to");
        // A negative step counts down, as Lua's numeric `for` does; zero is
        // a runtime error. `down` is known for a literal step.
        let mut down = Some(false);
        let step = match step {
            Some(s) => {
                let v = self.expr1(s);
                Some(match v {
                    Expr::Number(n) if n > 0.0 => v,
                    Expr::Number(n) if n < 0.0 => {
                        down = Some(true);
                        v
                    }
                    v => {
                        down = None;
                        let t = self.new_local("step");
                        self.emit(StmtKind::Local { locals: vec![t], values: vec![Expr::intrinsic_call("step", vec![v])] });
                        Expr::Local(t)
                    }
                })
            }
            None => None,
        };
        let (var, check) = match pats.first().and_then(simple_binding) {
            Some((name, ty)) if pats.len() == 1 => {
                let Some(id) = self.res.declaration(name.text_range()) else {
                    self.internal(node, "loop variable");
                    return;
                };
                let l = self.local_of(id);
                self.mark_predeclared(id);
                (l, ty.map(|t| (t, name_text(&name).unwrap_or_default())))
            }
            _ => {
                self.error(node, "range-loop-pattern", "a loop over a range takes one plain variable");
                return;
            }
        };
        self.push_loop();
        let step_expr = step.clone();
        let body = self.sub_block(|l| {
            if exclusive {
                // `a...b` leaves `b` out: the loop stops at the bound,
                // from whichever side the step approaches it.
                let line = l.line;
                let at_limit = |op: BinOp| Expr::binary(op, Expr::Local(var), limit.clone());
                let cond = match (down, &step_expr) {
                    (Some(false), _) => at_limit(BinOp::Ge),
                    (Some(true), _) => at_limit(BinOp::Le),
                    (None, Some(st)) => Expr::binary(
                        BinOp::Or,
                        Expr::binary(BinOp::And, Expr::binary(BinOp::Gt, st.clone(), Expr::Number(0.0)), at_limit(BinOp::Ge)),
                        Expr::binary(BinOp::And, Expr::binary(BinOp::Lt, st.clone(), Expr::Number(0.0)), at_limit(BinOp::Le)),
                    ),
                    (None, None) => at_limit(BinOp::Ge),
                };
                l.emit(StmtKind::If {
                    cond,
                    then: Block::new(vec![Stmt { line, kind: StmtKind::Break }]),
                    otherwise: None,
                });
            }
            if let Some((ty, name)) = check {
                l.emit_check(Expr::Local(var), &ty, &format!("loop variable '{name}'"));
            }
            if let Some(b) = block(node) {
                let inner = l.lower_block(&b, Tail::Discard);
                l.stmts.extend(inner.stmts);
            }
        });
        let body = self.finish_loop_body(body);
        self.emit(StmtKind::NumericFor { var, start, limit, step, body });
    }

    pub(crate) fn lower_break(&mut self, node: &SyntaxNode) {
        if self.loop_outside_region() {
            self.break_from_region(node);
            return;
        }
        self.emit(StmtKind::Break);
    }

    pub(crate) fn lower_continue(&mut self, node: &SyntaxNode) {
        if self.loop_outside_region() {
            self.continue_from_region(node);
            return;
        }
        let label = match self.fns.last_mut().and_then(|f| f.loops.last_mut()) {
            Some(lp) => lp.continue_label,
            None => {
                self.internal(node, "continue outside a loop");
                return;
            }
        };
        let label = match label {
            Some(l) => l,
            None => {
                let l = self.new_label();
                if let Some(lp) = self.fns.last_mut().and_then(|f| f.loops.last_mut()) {
                    lp.continue_label = Some(l);
                }
                l
            }
        };
        self.emit(StmtKind::Goto(label));
    }
}

/// The range expression of a `for` over a literal range: `a..b` or `a...b`,
/// possibly in parentheses.
fn literal_range(e: &SyntaxNode) -> Option<SyntaxNode> {
    match e.kind() {
        BIN_EXPR if bin_op(e).is_some_and(|t| matches!(t.kind(), DOT2 | DOT3)) => Some(e.clone()),
        PAREN_EXPR => expr_child(e).and_then(|i| literal_range(&i)),
        _ => None,
    }
}
