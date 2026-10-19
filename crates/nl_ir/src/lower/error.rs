//! `try`/`catch`/`finally` and protected regions.
//!
//! The body, the catch clauses and the `finally` block each become a
//! closure, run under `xpcall` with `rt.handler`. A closure ends with a
//! status code and values: `"n"`
//! (normal, with the value of the construct), `"r"` (a `return`, with its
//! values), `"b"` (`break`), `"c"` (`continue`), `"u"` (no catch clause
//! matched). After the call, the code is carried out where the `try`
//! stands; inside an enclosing region that again means returning a code.
//! The closures are the same NL function: they share its loops and its
//! return type, so no [`FnState`](super::FnState) is pushed for them.
//!
//! When every `return` of the body and catch closures gives a fixed number
//! of values for its code, and so does every `return` in the `finally`
//! block (the usual case), the `try` runs the closures itself and keeps
//! the outcome in locals, with no table and no other local:
//!
//! ```lua
//! local ok, code, v = __rt.xpcall(function() <body>; return "n", value end, __rt.handler)
//! if not ok then
//!   ok, code, v = __rt.xpcall(function(e) if <clause 1 matches> then … return "n", v end … __rt.rethrow(e) end, __rt.handler, code)
//! end
//! ok, code, v = __rt.xpcall(function(ok, code, v) <finally>; if not ok then __rt.rethrow(code) end; return code, v end,
//!                           __rt.handler, ok, code, v)
//! if not ok then __rt.rethrow(code) end
//! if code == "r" then return v end …                -- carry out the code
//! ```
//!
//! The catch closure raises the error again when no clause matches, and
//! the `finally` closure is given the outcome and returns it, raising a
//! pending error again, unless it leaves by `return`, `break` or
//! `continue` (which overrides the outcome) or raises. Raising again
//! keeps the exception and its trace (`rt.handler` fills in only a missing
//! trace), at the cost of a second unwinding on those paths.
//!
//! Otherwise (a closure ends with a call whose values are all returned, as
//! a `try` that is a function's tail does), `rt.try(body, catch,
//! finally)` runs them and packs the outcome in a table. That packed form
//! needs one local where the inline one needs 2 + the number of values,
//! so a file that exceeds LuaJIT's limit on locals is lowered again with
//! every `try` packed ([`super::lower`]).
//!
//! Either way the closures are written where the `try` stands; lift.rs
//! then moves them out, so that they are not made each time the `try`
//! runs.

use nl_sema::util::*;
use nl_syntax::{SyntaxKind, SyntaxNode};

use super::{Lowerer, Tail};
use crate::ir::*;

use SyntaxKind::*;

/// One protected region being lowered.
pub(crate) struct Region {
    /// The number of loops of the function when the region started: a
    /// loop at a lower index is outside the region.
    loop_base: usize,
    has_return: bool,
    has_break: bool,
    has_continue: bool,
}

impl Lowerer<'_> {
    /// True inside a protected region of the current function.
    pub(crate) fn in_protected_region(&self) -> bool {
        self.fns.last().is_some_and(|f| !f.regions.is_empty())
    }

    /// True when the innermost loop is outside the innermost region, so
    /// that `break` and `continue` must leave the region by status code.
    pub(crate) fn loop_outside_region(&self) -> bool {
        self.fns.last().is_some_and(|f| f.regions.last().is_some_and(|r| f.loops.len() <= r.loop_base))
    }

    fn region_mut(&mut self) -> Option<&mut Region> {
        self.fns.last_mut().and_then(|f| f.regions.last_mut())
    }

    pub(crate) fn break_from_region(&mut self, _node: &SyntaxNode) {
        if let Some(r) = self.region_mut() {
            r.has_break = true;
        }
        self.emit(StmtKind::Return(vec![Expr::str("b")]));
    }

    pub(crate) fn continue_from_region(&mut self, _node: &SyntaxNode) {
        if let Some(r) = self.region_mut() {
            r.has_continue = true;
        }
        self.emit(StmtKind::Return(vec![Expr::str("c")]));
    }

    pub(crate) fn return_from_region(&mut self, _node: &SyntaxNode, vals: Vec<Expr>) {
        if let Some(r) = self.region_mut() {
            r.has_return = true;
        }
        let mut all = vec![Expr::str("r")];
        all.extend(vals);
        self.emit(StmtKind::Return(all));
    }

    /// A closure of the region: `params`, and the statements `f` emits.
    fn region_closure(&mut self, params: Vec<LocalId>, f: impl FnOnce(&mut Self)) -> Expr {
        let line = self.line;
        let saved = std::mem::take(&mut self.stmts);
        f(self);
        let stmts = std::mem::replace(&mut self.stmts, saved);
        Expr::Function(Box::new(Function { params, vararg: false, body: Block::new(stmts), line, name: Some("try".to_string()), method: false, transient: false }))
    }

    /// A block of the region that yields the construct's value: `return
    /// "n", value` (or just `"n"` when the value is not used). When the
    /// `try` is a function's tail, the block's tail returns every value of
    /// a final call, as an `if` there does.
    fn region_block(&mut self, block: Option<SyntaxNode>, value_used: bool, all_values: bool) {
        if all_values {
            if let Some(b) = &block {
                let inner = self.lower_block(b, Tail::Yield);
                self.emit(StmtKind::Do(inner));
            }
            if super::func::can_fall_off(&self.stmts) {
                self.emit(StmtKind::Return(vec![Expr::str("n")]));
            }
        } else if value_used {
            let v = self.new_local("value");
            self.emit(StmtKind::Local { locals: vec![v], values: Vec::new() });
            if let Some(b) = &block {
                let inner = self.lower_block(b, Tail::Assign(v));
                self.emit(StmtKind::Do(inner));
            }
            self.emit(StmtKind::Return(vec![Expr::str("n"), Expr::Local(v)]));
        } else {
            if let Some(b) = &block {
                let inner = self.lower_block(b, Tail::Discard);
                self.emit(StmtKind::Do(inner));
            }
            self.emit(StmtKind::Return(vec![Expr::str("n")]));
        }
    }

    pub(crate) fn lower_try(&mut self, node: &SyntaxNode, tail: Tail) {
        let try_line = self.line;
        let value_used = tail != Tail::Discard;
        let all_values = matches!(tail, Tail::Return | Tail::Yield);
        let loop_base = self.fns.last().map_or(0, |f| f.loops.len());
        if let Some(f) = self.fns.last_mut() {
            f.regions.push(Region { loop_base, has_return: false, has_break: false, has_continue: false });
        }

        let body = self.region_closure(Vec::new(), |l| l.region_block(block(node), value_used, all_values));

        let catches: Vec<SyntaxNode> = children(node, CATCH_CLAUSE).collect();
        let catch = if catches.is_empty() {
            None
        } else {
            let e = self.new_local("err");
            // The function starts on the line of the first `catch`.
            self.line = catches.first().map_or(try_line, |c| self.line_of(c));
            Some(self.region_closure(vec![e], |l| {
                for c in &catches {
                    l.line = l.line_of(c);
                    let test = match type_child(c).and_then(|t| l.type_value(&t)) {
                        Some(t) => Expr::intrinsic_call("isa", vec![Expr::Local(e), t]),
                        None => Expr::Bool(true),
                    };
                    let clause = l.sub_block(|l| {
                        if let Some(name) = child(c, NAME) {
                            if let Some(id) = l.res.declaration(name.text_range()) {
                                l.define(id, Some(Expr::Local(e)));
                            }
                        }
                        l.region_block(block(c), value_used, all_values);
                    });
                    l.emit(StmtKind::If { cond: test, then: clause, otherwise: None });
                }
                l.emit(StmtKind::Return(vec![Expr::str("u")]));
            }))
        };

        let finally = match child(node, FINALLY_CLAUSE) {
            Some(f) => {
                self.line = self.line_of(&f);
                Some(self.region_closure(Vec::new(), |l| l.region_block(block(&f), false, false)))
            }
            None => None,
        };

        let region = self.fns.last_mut().and_then(|f| f.regions.pop()).unwrap_or(Region {
            loop_base,
            has_return: false,
            has_break: false,
            has_continue: false,
        });
        // The calls carry the line of the `try` (the catch clauses moved
        // `self.line` on).
        self.line = try_line;
        let arity = if self.packed_try { None } else { Arity::of(&[Some(&body), catch.as_ref()], finally.as_ref()) };
        match arity {
            Some(arity) => self.try_inline(node, tail, &region, arity, [body, catch.unwrap_or(Expr::Nil), finally.unwrap_or(Expr::Nil)]),
            None => self.try_packed(node, tail, &region, [body, catch.unwrap_or(Expr::Nil), finally.unwrap_or(Expr::Nil)]),
        }
    }

    /// The protected call of one closure, `rt.xpcall(f, rt.handler,
    /// args…)`, which the lift pass recognizes (lift.rs).
    fn protected_call(f: Expr, args: Vec<Expr>) -> Expr {
        let mut all = vec![f, Expr::Intrinsic("handler")];
        all.extend(args);
        Expr::intrinsic_call("xpcall", all)
    }

    /// A `try` whose closures return a fixed number of values with each
    /// status code: the outcome stays in locals (see the module
    /// documentation). `closures` are the body, the catch function and
    /// the `finally` function, the last two possibly nil.
    fn try_inline(&mut self, node: &SyntaxNode, tail: Tail, region: &Region, arity: Arity, closures: [Expr; 3]) {
        let [body, catch, finally] = closures;
        let ok = self.new_local("ok");
        let code = self.new_local("code");
        let vals: Vec<LocalId> = (0..arity.n.max(arity.r)).map(|_| self.new_local("v")).collect();
        let outcome = |count: usize| -> Vec<Place> {
            let mut t = vec![Place::Local(ok), Place::Local(code)];
            t.extend(vals.iter().map(|v| Place::Local(*v)));
            t.truncate(count);
            t
        };
        let mut locals = vec![ok, code];
        locals.extend(vals.iter().copied());
        let outcome_values = || -> Vec<Expr> { locals.iter().map(|l| Expr::Local(*l)).collect() };
        self.emit(StmtKind::Local { locals: locals.clone(), values: vec![Self::protected_call(body, Vec::new())] });
        if let Expr::Function(mut c) = catch {
            // No clause matched: the error goes on, raised again.
            if let (Some(&e), Some(last)) = (c.params.first(), c.body.stmts.last_mut()) {
                if matches!(&last.kind, StmtKind::Return(v) if matches!(v.as_slice(), [Expr::Str(s)] if s.as_slice() == b"u")) {
                    last.kind = StmtKind::Expr(Expr::intrinsic_call("rethrow", vec![Expr::Local(e)]));
                }
            }
            let then = self.sub_block(|l| {
                let call = Self::protected_call(Expr::Function(c), vec![Expr::Local(code)]);
                l.emit(StmtKind::Assign { targets: outcome(usize::MAX), values: vec![call] });
            });
            self.emit(StmtKind::If { cond: Expr::lnot(Expr::Local(ok)), then, otherwise: None });
        }
        if let Expr::Function(mut f) = finally {
            // The closure is given the outcome and returns it when it ends
            // normally, raising a pending error again.
            let hints: Vec<String> = locals.iter().map(|l| self.locals.get(l.0 as usize).map_or_else(String::new, |i| i.hint.clone())).collect();
            let params: Vec<LocalId> = hints.iter().map(|h| self.new_local(h)).collect();
            pass_outcome(&mut f.body, &params);
            f.params = params;
            let call = Self::protected_call(Expr::Function(f), outcome_values());
            self.emit(StmtKind::Assign { targets: outcome(usize::MAX), values: vec![call] });
        }
        let raise = self.sub_block(|l| l.emit(StmtKind::Expr(Expr::intrinsic_call("rethrow", vec![Expr::Local(code)]))));
        self.emit(StmtKind::If { cond: Expr::lnot(Expr::Local(ok)), then: raise, otherwise: None });

        let values = |k: usize| -> Vec<Expr> { vals.iter().take(k).map(|v| Expr::Local(*v)).collect() };
        let code_is = |c: &str| Expr::binary(BinOp::Eq, Expr::Local(code), Expr::str(c));
        self.carry_out(node, region, code_is, || values(arity.r));
        match tail {
            Tail::Discard => {}
            Tail::Assign(t) => self.emit(StmtKind::Assign { targets: vec![Place::Local(t)], values: vec![values(1).pop().unwrap_or(Expr::Nil)] }),
            Tail::Return => self.emit_return(values(arity.n)),
            Tail::Yield => {
                let mut all = vec![Expr::str("n")];
                all.extend(values(arity.n));
                self.emit(StmtKind::Return(all));
            }
        }
    }

    /// A `try` whose closures return varying numbers of values: through
    /// `rt.try`, which packs the outcome in a table.
    fn try_packed(&mut self, node: &SyntaxNode, tail: Tail, region: &Region, closures: [Expr; 3]) {
        let r = self.fresh_temp(Expr::intrinsic_call("try", closures.into()), "result");
        let code = Expr::index(Expr::Local(r), Expr::Number(2.0));
        let values = || Expr::intrinsic_call("unpack", vec![Expr::Local(r), Expr::Number(3.0), Expr::index(Expr::Local(r), Expr::str("n"))]);
        let code_is = |c: &str| Expr::binary(BinOp::Eq, code.clone(), Expr::str(c));
        self.carry_out(node, region, code_is, || vec![values()]);
        let value = Expr::index(Expr::Local(r), Expr::Number(3.0));
        match tail {
            Tail::Discard => {}
            Tail::Assign(t) => self.emit(StmtKind::Assign { targets: vec![Place::Local(t)], values: vec![value] }),
            Tail::Return => self.emit_return(vec![values()]),
            Tail::Yield => self.emit(StmtKind::Return(vec![Expr::str("n"), values()])),
        }
    }

    /// Carries out a `return`, `break` or `continue` that left the region,
    /// by its status code.
    fn carry_out(&mut self, node: &SyntaxNode, region: &Region, code_is: impl Fn(&str) -> Expr, returned: impl Fn() -> Vec<Expr>) {
        if region.has_return {
            let then = self.sub_block(|l| l.lower_return_values(node, returned()));
            self.emit(StmtKind::If { cond: code_is("r"), then, otherwise: None });
        }
        if region.has_break {
            let then = self.sub_block(|l| l.lower_break(node));
            self.emit(StmtKind::If { cond: code_is("b"), then, otherwise: None });
        }
        if region.has_continue {
            let then = self.sub_block(|l| l.lower_continue(node));
            self.emit(StmtKind::If { cond: code_is("c"), then, otherwise: None });
        }
    }
}

/// How many values the closures of a `try` return with each status code,
/// when that is fixed: `n` with `"n"` (the body and the catch clauses),
/// `r` with `"r"` (a `return` in any of the three). `"b"`, `"c"` and
/// `"u"` carry none, nor does the `"n"` of the `finally` closure.
#[derive(Clone, Copy, Debug)]
struct Arity {
    n: usize,
    r: usize,
}

impl Arity {
    /// The arity of the body and catch closures (`closures`) and of the
    /// `finally` closure, or `None` when a code is returned with different
    /// numbers of values, or with a final call that can give any number.
    fn of(closures: &[Option<&Expr>], finally: Option<&Expr>) -> Option<Arity> {
        let mut n: Option<usize> = None;
        let mut r: Option<usize> = None;
        let mut fixed = true;
        let all = closures.iter().flatten().map(|c| (*c, false)).chain(finally.map(|f| (f, true)));
        for (c, is_finally) in all {
            let Expr::Function(f) = c else { return None };
            returns(&f.body, &mut |vals| {
                let slot = match vals.first() {
                    Some(Expr::Str(c)) if c.as_slice() == b"n" && !is_finally => &mut n,
                    Some(Expr::Str(c)) if c.as_slice() == b"r" => &mut r,
                    Some(Expr::Str(_)) if vals.len() == 1 => return,
                    _ => {
                        fixed = false;
                        return;
                    }
                };
                match (value_count(vals.get(1..).unwrap_or(&[])), *slot) {
                    (Some(k), None) => *slot = Some(k),
                    (Some(k), Some(j)) if k == j => {}
                    _ => fixed = false,
                }
            });
        }
        fixed.then(|| Arity { n: n.unwrap_or(0), r: r.unwrap_or(0) })
    }
}

/// The number of values a list of expressions gives, or `None` when the
/// last one can give any number.
fn value_count(vals: &[Expr]) -> Option<usize> {
    match vals.last() {
        Some(Expr::Call(f, _)) if matches!(f.as_ref(), Expr::Intrinsic(name) if SINGLE_RESULT_INTRINSICS.contains(name)) => Some(vals.len()),
        Some(e) if e.is_multi() => None,
        _ => Some(vals.len()),
    }
}

/// Makes the `finally` closure of the inline form return the outcome it is
/// given (`params`: ok, code, values) where it ends normally (`return
/// "n"`), raising a pending error again; a `return` of another code
/// overrides the outcome, as the closure's own outcome.
fn pass_outcome(b: &mut Block, params: &[LocalId]) {
    for s in &mut b.stmts {
        let line = s.line;
        match &mut s.kind {
            StmtKind::Return(vals) if matches!(vals.as_slice(), [Expr::Str(c)] if c.as_slice() == b"n") => {
                let mut stmts = Vec::new();
                if let Some((&ok, rest)) = params.split_first() {
                    let raise = Block::new(vec![Stmt { line, kind: StmtKind::Expr(Expr::intrinsic_call("rethrow", rest.first().map(|c| Expr::Local(*c)).into_iter().collect())) }]);
                    stmts.push(Stmt { line, kind: StmtKind::If { cond: Expr::lnot(Expr::Local(ok)), then: raise, otherwise: None } });
                    stmts.push(Stmt { line, kind: StmtKind::Return(rest.iter().map(|p| Expr::Local(*p)).collect()) });
                }
                s.kind = StmtKind::Do(Block::new(stmts));
            }
            StmtKind::Do(b) | StmtKind::While { body: b, .. } | StmtKind::NumericFor { body: b, .. } | StmtKind::GenericFor { body: b, .. } => pass_outcome(b, params),
            StmtKind::If { then, otherwise, .. } => {
                pass_outcome(then, params);
                if let Some(o) = otherwise {
                    pass_outcome(o, params);
                }
            }
            StmtKind::Hoist { defs, rest } => {
                pass_outcome(defs, params);
                pass_outcome(rest, params);
            }
            _ => {}
        }
    }
}

/// Calls `f` with the values of every `return` of a function body (not
/// those of the functions inside it).
fn returns(b: &Block, f: &mut dyn FnMut(&[Expr])) {
    for s in &b.stmts {
        match &s.kind {
            StmtKind::Return(vals) => f(vals),
            StmtKind::Do(b) | StmtKind::While { body: b, .. } | StmtKind::NumericFor { body: b, .. } | StmtKind::GenericFor { body: b, .. } => returns(b, f),
            StmtKind::If { then, otherwise, .. } => {
                returns(then, f);
                if let Some(o) = otherwise {
                    returns(o, f);
                }
            }
            StmtKind::Hoist { defs, rest } => {
                returns(defs, f);
                returns(rest, f);
            }
            StmtKind::Local { .. } | StmtKind::Assign { .. } | StmtKind::Expr(_) | StmtKind::Break | StmtKind::Goto(_) | StmtKind::Label(_) => {}
        }
    }
}
