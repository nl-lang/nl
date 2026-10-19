//! Unary and binary operators, ranges and the logical
//! operators with short-circuit evaluation (`??` is in nilsafe.rs).
//!
//! Arithmetic, concatenation and unary minus use Lua's operators; for
//! operands that are not numbers (or strings) their events reach the
//! generic metamethods that operator definitions install, which apply
//! NL's rules (the left operand decides, `coerce`). Floor
//! division and the bitwise operators, which stock LuaJIT has no events
//! for, and the ordering operators, go through runtime
//! helpers: LuaJIT compares values of different types
//! without asking a metamethod, so `1 < money` could not reach `coerce`,
//! and it has no `>` or `>=` events. `==` and `!=` go through `rt.eq`
//! (LuaJIT asks `__eq` only when both operands have the same handler, so
//! the left operand would not decide), except when an operand is a
//! literal number, string, boolean or `nil`: no definition can run then,
//! and Lua's `==` is the answer. See `runtime/nl_40_ops.lua`.
//!
//! When the analyzer proves the operands of a comparison two numbers or
//! two strings, those of `//` or a bitwise operator two numbers, or the
//! operand of `~` a number (`direct_ops` in nl_sema's hints),
//! the operation is direct code: Lua's comparison, `rt.nfloor(a / b)` and
//! a test for zero, or a call of `rt.nband`, `rt.nbnot` and their
//! siblings: the functions of Lua's `math` and `bit` libraries that the
//! helpers call, as the runtime found them when it opened (generated
//! code does not depend on globals that scripts or sandboxes
//! can replace). At level 1 the bytecode backend emits the bitwise ones
//! as the VM's bit opcodes (`BAND`…, nl_codegen), which compute the same.

use nl_sema::util::*;
use nl_syntax::{SyntaxKind, SyntaxNode};

use super::Lowerer;
use super::expr::single;
use crate::ir::*;

use SyntaxKind::*;

impl Lowerer<'_> {
    pub(crate) fn prefix(&mut self, node: &SyntaxNode) -> Expr {
        let op = first_token(node).map(|t| t.kind());
        let Some(operand) = expr_child(node) else { return self.internal(node, "prefix operator") };
        // `-5` is a constant.
        if op == Some(MINUS) && operand.kind() == LITERAL {
            if let Some(t) = first_token(&operand).filter(|t| t.kind() == NUMBER) {
                return Expr::Number(-nl_syntax::number_value(t.text()).unwrap_or(0.0));
            }
        }
        let v = self.expr1(&operand);
        match op {
            Some(MINUS) => Expr::Unary(UnOp::Neg, Box::new(v)),
            Some(BANG | NOT_KW) => Expr::lnot(v),
            // A proven number: `bit.bnot` as the runtime holds
            // it, `BNOT` at level 1 (nl_codegen).
            Some(TILDE) if self.opts.hints.is_direct_op(node.text_range()) => Expr::intrinsic_call("nbnot", vec![v]),
            Some(TILDE) => Expr::intrinsic_call("bnot", vec![v]),
            _ => self.internal(node, "prefix operator"),
        }
    }

    pub(crate) fn binary(&mut self, node: &SyntaxNode) -> Expr {
        let Some(op) = bin_op(node).map(|t| t.kind()) else { return self.internal(node, "binary operator") };
        let parts: Vec<SyntaxNode> = expr_children(node).collect();
        let (Some(a), Some(b)) = (parts.first(), parts.get(1)) else { return self.internal(node, "binary operands") };
        if matches!(op, AND_KW | AMP2 | OR_KW | PIPE2) {
            return self.logical(op, a, b);
        }
        if op == QUESTION2 {
            return self.coalesce(a, b);
        }
        let ops = self.operands(&parts);
        let mut it = ops.into_iter().map(single);
        let (Some(x), Some(y)) = (it.next(), it.next()) else { return self.internal(node, "binary operands") };
        let x = self.pin(x, &y);
        self.binary_op(node, op, x, y)
    }

    /// The operation of a binary operator token on two lowered operands.
    /// Shared with compound assignment.
    pub(crate) fn binary_op(&mut self, node: &SyntaxNode, op: SyntaxKind, x: Expr, y: Expr) -> Expr {
        let lua = |op| Expr::binary(op, x.clone(), y.clone());
        // Two numbers, or two strings, by the analyzer: Lua's
        // own comparison, which is what the runtime helper does for them;
        // for two numbers, floor division and the bitwise operators as the
        // helper computes them, without the call of the helper.
        if self.opts.hints.is_direct_op(node.text_range()) {
            match op {
                EQ2 => return lua(BinOp::Eq),
                NEQ => return lua(BinOp::Ne),
                LT => return lua(BinOp::Lt),
                LT_EQ => return lua(BinOp::Le),
                GT => return lua(BinOp::Gt),
                GT_EQ => return lua(BinOp::Ge),
                SLASH2 | SLASH2_EQ => return self.floor_div(x, y),
                PIPE | PIPE_EQ => return Expr::intrinsic_call("nbor", vec![x, y]),
                AMP | AMP_EQ => return Expr::intrinsic_call("nband", vec![x, y]),
                CARET | CARET_EQ => return Expr::intrinsic_call("nbxor", vec![x, y]),
                SHL | SHL_EQ => return Expr::intrinsic_call("nlshift", vec![x, y]),
                SHR | SHR_EQ => return Expr::intrinsic_call("narshift", vec![x, y]),
                USHR | USHR_EQ => return Expr::intrinsic_call("nrshift", vec![x, y]),
                _ => {}
            }
        }
        // Level 1: the fork's VM applies NL's rules
        // to `==` (the left operand's `__eq` alone) and to `<`, `<=`, `>`,
        // `>=` (mixed types, `__gt`/`__ge`, coerce), so they are plain
        // comparisons.
        if self.opts.level >= 1 {
            match op {
                EQ2 => return lua(BinOp::Eq),
                NEQ => return lua(BinOp::Ne),
                LT => return lua(BinOp::Lt),
                LT_EQ => return lua(BinOp::Le),
                GT => return lua(BinOp::Gt),
                GT_EQ => return lua(BinOp::Ge),
                _ => {}
            }
        }
        match op {
            PLUS | PLUS_EQ => lua(BinOp::Add),
            MINUS | MINUS_EQ => lua(BinOp::Sub),
            STAR | STAR_EQ => lua(BinOp::Mul),
            SLASH | SLASH_EQ => lua(BinOp::Div),
            PERCENT | PERCENT_EQ => lua(BinOp::Mod),
            STAR2 | STAR2_EQ => lua(BinOp::Pow),
            CONCAT | CONCAT_EQ => lua(BinOp::Concat),
            EQ2 | NEQ if is_primitive_literal(&x) || is_primitive_literal(&y) => lua(if op == EQ2 { BinOp::Eq } else { BinOp::Ne }),
            EQ2 => Expr::intrinsic_call("eq", vec![x, y]),
            NEQ => Expr::lnot(Expr::intrinsic_call("eq", vec![x, y])),
            // Two numbers or two strings compare as Lua compares them;
            // other constants may reach a primitive operator definition
            // (`fn Number.<`), so they go through the runtime.
            LT | LT_EQ | GT | GT_EQ if same_primitive_constants(&x, &y) => {
                lua(match op {
                    LT => BinOp::Lt,
                    LT_EQ => BinOp::Le,
                    GT => BinOp::Gt,
                    _ => BinOp::Ge,
                })
            }
            LT => Expr::intrinsic_call("lt", vec![x, y]),
            LT_EQ => Expr::intrinsic_call("le", vec![x, y]),
            GT => Expr::intrinsic_call("gt", vec![x, y]),
            GT_EQ => Expr::intrinsic_call("ge", vec![x, y]),
            SLASH2 | SLASH2_EQ => Expr::intrinsic_call("idiv", vec![x, y]),
            PIPE | PIPE_EQ => Expr::intrinsic_call("bor", vec![x, y]),
            AMP | AMP_EQ => Expr::intrinsic_call("band", vec![x, y]),
            CARET | CARET_EQ => Expr::intrinsic_call("bxor", vec![x, y]),
            SHL | SHL_EQ => Expr::intrinsic_call("shl", vec![x, y]),
            SHR | SHR_EQ => Expr::intrinsic_call("shr", vec![x, y]),
            USHR | USHR_EQ => Expr::intrinsic_call("ushr", vec![x, y]),
            DOT2 => Expr::intrinsic_call("range", vec![x, y, Expr::Bool(false)]),
            DOT3 => Expr::intrinsic_call("range", vec![x, y, Expr::Bool(true)]),
            _ => self.internal(node, "binary operator"),
        }
    }

    /// `rt.idiv` of two numbers without the helper: `local q =
    /// rt.nfloor(x / y)`, then `if q == 0 then q = 0 end`, which turns
    /// the `-0` that `floor` gives for a zero or small negative quotient
    /// into `0`, as `rt.idiv` does (runtime/nl_40_ops.lua). `rt.nfloor` is
    /// `math.floor` as the runtime found it when it opened, so a script
    /// that replaces `math.floor` changes neither. The test is cheaper in the
    /// interpreter than `math.floor(x / y) + 0` (about 15% on
    /// bench/floordiv.nl with declared types) and costs about 3% in the
    /// JIT.
    fn floor_div(&mut self, x: Expr, y: Expr) -> Expr {
        let q = self.fresh_temp(Expr::intrinsic_call("nfloor", vec![Expr::binary(BinOp::Div, x, y)]), "q");
        let zero = Stmt { line: self.line, kind: StmtKind::Assign { targets: vec![Place::Local(q)], values: vec![Expr::Number(0.0)] } };
        self.emit(StmtKind::If { cond: Expr::binary(BinOp::Eq, Expr::Local(q), Expr::Number(0.0)), then: Block::new(vec![zero]), otherwise: None });
        Expr::Local(q)
    }

    /// `a and b`, `a or b`. When `b` needs statements, they run only when
    /// `b` is evaluated.
    fn logical(&mut self, op: SyntaxKind, a: &SyntaxNode, b: &SyntaxNode) -> Expr {
        let is_and = matches!(op, AND_KW | AMP2);
        let x = self.expr1(a);
        let (stmts, y) = self.capture(|l| l.expr1(b));
        if stmts.is_empty() {
            return Expr::binary(if is_and { BinOp::And } else { BinOp::Or }, x, y);
        }
        let t = self.fresh_temp(x, "c");
        let mut body = stmts;
        body.push(Stmt { line: self.line, kind: StmtKind::Assign { targets: vec![Place::Local(t)], values: vec![y] } });
        let cond = if is_and { Expr::Local(t) } else { Expr::lnot(Expr::Local(t)) };
        self.emit(StmtKind::If { cond, then: Block::new(body), otherwise: None });
        Expr::Local(t)
    }
}

/// A literal number, string, boolean or `nil`.
fn is_primitive_literal(e: &Expr) -> bool {
    matches!(e, Expr::Nil | Expr::Bool(_) | Expr::Number(_) | Expr::Str(_))
}

/// Two number constants or two string constants.
fn same_primitive_constants(x: &Expr, y: &Expr) -> bool {
    matches!((x, y), (Expr::Number(_), Expr::Number(_)) | (Expr::Str(_), Expr::Str(_)))
}
