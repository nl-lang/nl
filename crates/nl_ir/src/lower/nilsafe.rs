//! The nil-safe operators: chains with `?.` steps, `a ?? b`
//! and `x ??= v`.
//!
//! A chain is lowered where it ends (`nl_sema::util::is_safe_chain_top`):
//! each value a `?.` tests is evaluated in order, innermost first, and held
//! in a local (a local that nothing but its own statements assigns is
//! tested in place); the rest of the chain is lowered inside one `if t ~=
//! nil then … end` per test, nested, and writes its value to the chain's
//! result local, which stays `nil` when a test fails:
//!
//! ```lua
//! local v                 -- a?.b.c(d)
//! local t = a
//! if t ~= nil then v = t.b.c(d) end
//! ```
//!
//! The steps after a test are the ordinary lowering of the chain, which
//! reads the tested value through a preset ([`Lowerer::preset`]): when it
//! reaches the node a `?.` tested, it takes the local instead of lowering
//! the node again. So `rt.mget`, `MemberCall`, `AutoGet` and the members the
//! analyzer resolved are lowered inside the guard exactly as
//! for `.`. A statement whose chain is discarded and an assignment to a
//! nil-safe target are guarded the same way, without a result local. No
//! closure, no runtime call: a test and a jump on both backends (`ISNEP`
//! and `JMP` in bytecode).

use nl_sema::util::*;
use nl_sema::MemberHint;
use nl_syntax::{SyntaxKind, SyntaxNode, TextRange};

use super::Lowerer;
use super::expr::single;
use crate::ir::*;

use SyntaxKind::*;

/// The key of a preset: the node's kind and range.
pub(crate) type PresetKey = (SyntaxKind, TextRange);

pub(crate) fn preset_key(node: &SyntaxNode) -> PresetKey {
    (node.kind(), node.text_range())
}

impl Lowerer<'_> {
    /// The value a nil-safe guard holds for `node`, when one does.
    pub(crate) fn preset(&self, node: &SyntaxNode) -> Option<Expr> {
        if self.presets.is_empty() {
            return None;
        }
        self.presets.get(&preset_key(node)).cloned()
    }

    /// Starts the guarded lowering of the chain that ends at `top`, unless
    /// it is already being lowered (the inner lowering of the same node).
    /// Returns false when the caller lowers the node as usual.
    fn enter_chain(&mut self, top: &SyntaxNode) -> bool {
        self.safe_active.insert(preset_key(top))
    }

    fn leave_chain(&mut self, top: &SyntaxNode) {
        self.safe_active.remove(&preset_key(top));
    }

    /// A nil-safe chain whose value is used: `None` when `node` is not the
    /// end of such a chain, or is the one being lowered.
    pub(crate) fn safe_chain_value(&mut self, node: &SyntaxNode) -> Option<Expr> {
        if !is_safe_chain_top(node) || !self.enter_chain(node) {
            return None;
        }
        let r = self.new_local("v");
        self.emit(StmtKind::Local { locals: vec![r], values: Vec::new() });
        self.guard_chain(node, |l| {
            let v = l.expr1(node);
            l.emit(StmtKind::Assign { targets: vec![Place::Local(r)], values: vec![v] });
        });
        self.leave_chain(node);
        Some(Expr::Local(r))
    }

    /// A nil-safe chain whose value is discarded: true when `node` was one
    /// and has been lowered.
    pub(crate) fn safe_chain_discard(&mut self, node: &SyntaxNode) -> bool {
        if !is_safe_chain_top(node) || !self.enter_chain(node) {
            return false;
        }
        self.guard_chain(node, |l| l.discard_expr(node));
        self.leave_chain(node);
        true
    }

    /// An assignment or compound assignment whose target holds a nil-safe
    /// step: the whole statement runs only when every test passes, so
    /// nothing is stored and the value is not evaluated otherwise.
    /// True when it was lowered here.
    pub(crate) fn safe_assignment(&mut self, stmt: &SyntaxNode, target: &SyntaxNode, lower: fn(&mut Self, &SyntaxNode)) -> bool {
        if !has_safe_step(target) || !self.enter_chain(target) {
            return false;
        }
        self.guard_chain(target, |l| lower(l, stmt));
        self.leave_chain(target);
        true
    }

    /// Evaluates the values the `?.` steps of the chain ending at `top`
    /// test, innermost first, each guarding the rest; `inner` lowers the
    /// rest of the chain under every guard.
    fn guard_chain(&mut self, top: &SyntaxNode, inner: impl FnOnce(&mut Self)) {
        let steps = safe_steps(top);
        let mut frames: Vec<(Expr, Vec<Stmt>, u32)> = Vec::new();
        let mut keys = Vec::new();
        for s in &steps {
            let (key, value) = match s {
                SafeStep::Postfix { step, receiver } if step.kind() == CALL_EXPR => (preset_key(receiver), self.safe_callee(receiver, &mut keys)),
                SafeStep::Postfix { receiver, .. } => (preset_key(receiver), self.expr1(receiver)),
                SafeStep::Lua { path, step } => (preset_key(step), self.lua_path_before(path, step)),
            };
            let tested = self.tested(value);
            self.presets.insert(key, tested.clone());
            keys.push(key);
            let cond = Expr::binary(BinOp::Ne, tested, Expr::Nil);
            frames.push((cond, std::mem::take(&mut self.stmts), self.line));
        }
        inner(self);
        while let Some((cond, outer, line)) = frames.pop() {
            let then = Block::new(std::mem::replace(&mut self.stmts, outer));
            self.stmts.push(Stmt { line, kind: StmtKind::If { cond, then, otherwise: None } });
        }
        for k in keys {
            self.presets.remove(&k);
        }
    }

    /// A tested value as the guard holds it: a constant, a local that no
    /// closure assigns (read in place, as Lua reads a register), or a new
    /// local.
    fn tested(&mut self, e: Expr) -> Expr {
        match e {
            e if e.is_constant() => e,
            Expr::Local(l) if !self.is_volatile(l) => Expr::Local(l),
            e => Expr::Local(self.fresh_temp(e, "s")),
        }
    }

    /// The function `f?.(…)` tests: the value itself, never called first,
    /// as `f!` gives it. For a member `x.m?.(…)` the receiver
    /// is held too, and the call that follows (`member_call_of`, call.rs)
    /// calls the held member under the member-call rule: with `x` as
    /// `self` when it is a method, decided statically from a declared type
    /// or at runtime (`Expr::MemberApply`), never reading `m` again.
    fn safe_callee(&mut self, callee: &SyntaxNode, keys: &mut Vec<PresetKey>) -> Expr {
        match callee.kind() {
            NAME_REF => self.name_raw(callee),
            AT_EXPR => self.at_expr(callee),
            FIELD_EXPR if !super::expr::is_len(callee) => {
                let Some(recv) = expr_child(callee) else { return self.internal(callee, "member call") };
                let r = self.expr1(&recv);
                let r = self.tested(r);
                self.presets.insert(preset_key(&recv), r.clone());
                keys.push(preset_key(&recv));
                match self.opts.hints.member(callee.text_range()) {
                    // A member of a declared struct, module or Lua
                    // class: a plain read, as after `.`.
                    Some(MemberHint::Field { key } | MemberHint::Function { key } | MemberHint::Method { key }) => Expr::index(r, Expr::str(key)),
                    // A function of `Table`: called with the receiver.
                    Some(MemberHint::TableFn { key }) => super::expr::table_fn(key),
                    None => {
                        let key = field_name(callee).map(|t| t.text().to_string()).unwrap_or_default();
                        self.index_read(r, Expr::str(&key))
                    }
                }
            }
            _ => self.expr1(callee),
        }
    }

    /// `a ?? b`: `a` once, to one value; `b` only when it is
    /// `nil`.
    pub(crate) fn coalesce(&mut self, a: &SyntaxNode, b: &SyntaxNode) -> Expr {
        let x = self.expr1(a);
        let t = self.fresh_temp(x, "c");
        let body = self.sub_block(|l| {
            let y = l.expr1(b);
            l.emit(StmtKind::Assign { targets: vec![Place::Local(t)], values: vec![single(y)] });
        });
        self.emit(StmtKind::If { cond: Expr::binary(BinOp::Eq, Expr::Local(t), Expr::Nil), then: body, otherwise: None });
        Expr::Local(t)
    }
}
