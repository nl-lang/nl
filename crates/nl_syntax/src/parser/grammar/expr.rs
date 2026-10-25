//! Expressions by precedence: `cmd_expr`, the binary
//! levels, unary operators, `**`, and the pipe.
//!
//! Binary operators use precedence climbing: [`Parser::binary_loop`] takes
//! operators of a level at least `min` and parses each right operand one
//! level higher, so left-associative chains are a loop. The right-associative
//! `<>` and `**` chains and runs of prefix operators are loops too, keeping a
//! stack of open markers; only nesting (parentheses, blocks, …) recurses.

use super::{can_start_expr, is_lua_name, is_word};
use crate::SyntaxKind::*;
use crate::parser::{Expr, LuaLast, Parser, Shape};

/// How a binary operator is parsed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BinOp {
    /// A plain operator token; the right operand is one level higher.
    Plain,
    /// `is type`.
    Is,
    /// `as as_type`.
    As,
    /// `|> pipe_target`.
    Pipe,
    /// `<>`, right associative.
    Concat,
    /// `~=` (Lua's "not equal"): reported, parsed as a comparison.
    TildeEq,
    /// `a..<b`: reported, parsed as a range.
    Dot2Lt,
}

/// The operators of level 3 and 5 are non-associative.
fn non_associative(level: u8) -> bool {
    level == 3 || level == 5
}

impl Parser<'_> {
    /// `cmd_expr = command_call | spawn_command | expr`: only in the
    /// positions where a command call may appear.
    pub(crate) fn cmd_expr(&mut self) -> Option<Expr> {
        self.expr_bp(0, true)
    }

    /// `expr`: no command call.
    pub(crate) fn expr(&mut self) -> Option<Expr> {
        self.expr_bp(0, false)
    }

    /// An expression with operators of level `min` or higher only (the
    /// default of a block parameter is a `bxor_expr`, level 7).
    pub(crate) fn expr_min(&mut self, min: u8) -> Option<Expr> {
        self.expr_bp(min, false)
    }

    fn expr_bp(&mut self, min: u8, cmd_ok: bool) -> Option<Expr> {
        let start = self.pos();
        self.guarded_expr(start, |p| p.expr_bp_inner(min, cmd_ok))
    }

    /// [`Parser::guarded`] for a function that parses an expression: when a
    /// limit skipped the expression, its `ERROR_NODE` stands for it.
    pub(crate) fn guarded_expr(&mut self, start: usize, f: impl FnOnce(&mut Self) -> Option<Expr>) -> Option<Expr> {
        match self.guarded(f) {
            Ok(r) => r,
            Err(skipped) => skipped.map(|cm| Expr { cm, shape: Shape::Other, start }),
        }
    }

    fn expr_bp_inner(&mut self, min: u8, cmd_ok: bool) -> Option<Expr> {
        let lhs = self.unary(cmd_ok)?;
        if self.at_command_arg_start() {
            if lhs.shape.is_callee() {
                // A command call is not an operand: nothing but the end of
                // the statement, a modifier or a block may follow it.
                return Some(self.command_call(lhs, cmd_ok));
            }
            match lhs.shape {
                Shape::Bang => self.error(
                    "`f!` is the function itself and cannot be called: to call it, write `f(…)` \
                     or `f x` without the `!`",
                ),
                Shape::CapName | Shape::Member { cap_path: true, .. } => {
                    let callee = self.text_between(lhs.start, self.pos()).to_string();
                    self.error(format!(
                        "a capitalized name is not called without parentheses: write \
                         `{callee}(…)`"
                    ));
                }
                Shape::Call
                | Shape::BlockCall
                | Shape::Method { args: true }
                | Shape::LuaPath { last: LuaLast::Call } => {
                    self.error(
                        "this call already has its arguments in parentheses; to pass more, put \
                         them inside the parentheses",
                    );
                }
                _ => {}
            }
        }
        Some(self.binary_loop(lhs, min))
    }

    /// The binary operator at the current token, with its level,
    /// when it may continue the expression here (at the start of a line,
    /// only operators that cannot begin an expression continue).
    fn binary_op(&self) -> Option<(u8, BinOp)> {
        let k = self.cur();
        let op = match k {
            // `??` shares the level of `or`.
            OR_KW | PIPE2 | QUESTION2 => (1, BinOp::Plain),
            AND_KW | AMP2 => (2, BinOp::Plain),
            EQ2 | NEQ | LT | LT_EQ | GT | GT_EQ => (3, BinOp::Plain),
            IS_KW => (3, BinOp::Is),
            PIPE_GT => (4, BinOp::Pipe),
            DOT2 | DOT3 => (5, BinOp::Plain),
            DOT2_LT => (5, BinOp::Dot2Lt),
            PIPE => (6, BinOp::Plain),
            CARET => (7, BinOp::Plain),
            AMP => (8, BinOp::Plain),
            SHL | SHR | USHR => (9, BinOp::Plain),
            CONCAT => (10, BinOp::Concat),
            PLUS | MINUS => (11, BinOp::Plain),
            STAR | SLASH | SLASH2 | PERCENT => (12, BinOp::Plain),
            AS_KW => (13, BinOp::As),
            TILDE if self.nth_at(1, EQ) && self.nth_touches(1) => (3, BinOp::TildeEq),
            _ => return None,
        };
        if self.nl() && matches!(k, MINUS | TILDE) {
            return None;
        }
        Some(op)
    }

    /// Precedence climbing over levels `min` to 13.
    fn binary_loop(&mut self, mut lhs: Expr, min: u8) -> Expr {
        let mut links = 0;
        while let Some((level, op)) = self.binary_op() {
            if level < min || !self.chain_link() {
                break;
            }
            links += 1;
            let op_kind = self.cur();
            let start = lhs.start;
            let m = lhs.cm.precede(self);
            let (kind, shape) = match op {
                BinOp::Is => {
                    self.bump();
                    self.type_expected("after `is`");
                    (IS_EXPR, Shape::Other)
                }
                BinOp::As => {
                    self.bump();
                    self.as_type();
                    (AS_EXPR, Shape::Other)
                }
                BinOp::Pipe => {
                    self.bump();
                    self.pipe_target(start);
                    (PIPE_EXPR, Shape::Other)
                }
                BinOp::Concat => {
                    let outer = self.concat_chain(m, &mut links);
                    lhs = Expr { cm: outer, shape: Shape::Other, start };
                    continue;
                }
                BinOp::TildeEq => {
                    self.error("use `!=` for \"not equal\"; `~=` is Lua's spelling");
                    self.bump();
                    self.bump();
                    self.operand(level + 1, "`!=`");
                    (BIN_EXPR, Shape::Other)
                }
                BinOp::Dot2Lt => {
                    self.error("NL has no `..<`: a range that leaves out its end is written `a...b`");
                    self.bump();
                    self.operand(level + 1, "`..<`");
                    (BIN_EXPR, Shape::Range)
                }
                BinOp::Plain if op_kind == DOT3 && !can_start_expr(self.nth(1)) => {
                    // `print ...`, `x = ...`: Lua's varargs.
                    self.error(
                        "expected the end of the range after `...`: `a...b` is the range from `a` \
                         up to, but not including, `b`. NL has no varargs; use a `*args` parameter",
                    );
                    self.bump();
                    (BIN_EXPR, Shape::Range)
                }
                BinOp::Plain => {
                    let text = self.nth_text(0).to_string();
                    self.bump();
                    self.operand(level + 1, &format!("`{text}`"));
                    let shape = if matches!(op_kind, DOT2 | DOT3) { Shape::Range } else { Shape::Other };
                    (BIN_EXPR, shape)
                }
            };
            lhs = Expr { cm: m.complete(self, kind), shape, start };
            self.after_binary(level, op);
        }
        self.chain_done(links);
        lhs
    }

    /// The right operand of a binary operator.
    fn operand(&mut self, min: u8, after: &str) {
        if self.expr_bp(min, false).is_none() {
            self.missing_operand(after);
        }
    }

    fn missing_operand(&mut self, after: &str) {
        if self.tok(0).after_error {
            // The lexer reported the character that stood here.
            return;
        }
        let found = self.describe_cur();
        self.error(format!("expected an expression after {after}, found {found}"));
    }

    /// Checks what follows a completed binary expression: a second operator
    /// of a non-associative level, or a higher-level operator after a
    /// pipe target.
    fn after_binary(&mut self, level: u8, op: BinOp) {
        let Some((next, next_op)) = self.binary_op() else { return };
        if non_associative(level) && next == level {
            let msg = if level == 3 {
                "comparisons cannot be chained (`a < b < c`): write `a < b and b < c`"
            } else {
                "ranges cannot be chained (`1..2..3`); a range's bounds are single values"
            };
            self.error(msg);
        } else if op == BinOp::Pipe && next > 4 && next_op != BinOp::Pipe {
            let text = self.nth_text(0).to_string();
            self.error(format!(
                "only `|>`, a comparison, `and`, `or` or the end of the expression may follow a \
                 pipe target; to use the result with `{text}`, put the pipe in parentheses: `(a \
                 |> f) {text} …`"
            ));
        }
    }

    /// `concat_expr = add_expr [ "<>" concat_expr ]` (right associative),
    /// as a loop: `m` already precedes the left operand.
    fn concat_chain(&mut self, m: crate::parser::event::Marker, links: &mut u32) -> crate::parser::event::CompletedMarker {
        let mut stack = vec![m];
        self.bump();
        loop {
            let Some(rhs) = self.expr_bp(11, false) else {
                self.missing_operand("`<>`");
                break;
            };
            if self.at(CONCAT) && self.chain_link() {
                *links += 1;
                stack.push(rhs.cm.precede(self));
                self.bump();
                continue;
            }
            break;
        }
        let mut last = None;
        while let Some(m) = stack.pop() {
            last = Some(m.complete(self, BIN_EXPR));
        }
        // `stack` started non-empty, so `last` is set.
        match last {
            Some(cm) => cm,
            None => self.start().complete(self, ERROR_NODE),
        }
    }

    /// `unary_expr = ( "-" | "!" | "not" | "~" ) unary_expr | power_expr`.
    /// The operand of a unary operator is a power expression, so
    /// `-5.abs` is `-(5.abs)` and `-x ** 2` is `-(x ** 2)`.
    pub(crate) fn unary(&mut self, cmd_ok: bool) -> Option<Expr> {
        let start = self.pos();
        let mut ops = Vec::new();
        while matches!(self.cur(), MINUS | BANG | NOT_KW | TILDE) {
            if !self.chain_link() {
                break;
            }
            ops.push((self.start(), self.cur()));
            self.bump();
        }
        if ops.is_empty() {
            return self.power(cmd_ok);
        }
        let operand = self.power(false);
        if operand.is_none() {
            let after = ops.last().and_then(|o| o.1.fixed_text()).unwrap_or("");
            self.missing_operand(&format!("`{after}`"));
        }
        let links = u32::try_from(ops.len()).unwrap_or(u32::MAX);
        let mut last = None;
        while let Some((m, _)) = ops.pop() {
            last = Some(m.complete(self, PREFIX_EXPR));
        }
        self.chain_done(links);
        last.map(|cm| Expr { cm, shape: Shape::Other, start })
    }

    /// `power_expr = postfix_expr [ "**" unary_expr ]`, right
    /// associative, as a loop.
    fn power(&mut self, cmd_ok: bool) -> Option<Expr> {
        let base = self.postfix(cmd_ok)?;
        if !self.at(STAR2) {
            return Some(base);
        }
        let start = base.start;
        let mut stack = Vec::new();
        let mut left = base;
        while self.at(STAR2) && self.chain_link() {
            stack.push(left.cm.precede(self));
            self.bump();
            if matches!(self.cur(), MINUS | BANG | NOT_KW | TILDE) {
                // `unary` comes back here for `a ** -b ** -c`: guarded.
                let start = self.pos();
                if self.guarded_expr(start, |p| p.unary(false)).is_none() {
                    self.missing_operand("`**`");
                }
                break;
            }
            match self.postfix(false) {
                Some(e) => left = e,
                None => {
                    self.missing_operand("`**`");
                    break;
                }
            }
        }
        let links = u32::try_from(stack.len()).unwrap_or(u32::MAX);
        let mut last = None;
        while let Some(m) = stack.pop() {
            last = Some(m.complete(self, BIN_EXPR));
        }
        self.chain_done(links);
        Some(last.map_or(left, |cm| Expr { cm, shape: Shape::Other, start }))
    }

    /// `pipe_target`: a name chain without calls, `x:m` or a
    /// Lua path; then at most one argument list and a trailing block. No
    /// command arguments, and no postfix operator after it.
    fn pipe_target(&mut self, pipe_start: usize) {
        let k = self.cur();
        let mut lua = false;
        let target = match k {
            COLON if self.at_path_colon() => {
                // Names only, then one call, all in the `LUA_PATH`.
                let start = self.pos();
                let (cm, shape) = self.pipe_lua_path();
                lua = true;
                Expr { cm, shape, start }
            }
            LNAME | CNAME | ONAME | SELF_KW | SELF_TYPE_KW | AT_NAME => {
                let Some(e) = self.primary(false) else { return };
                self.member_steps(e)
            }
            _ => {
                let found = self.describe_cur();
                self.error(format!(
                    "expected the function to pipe into after `|>`, such as `|> f` or `|> f(b)`, \
                     found {found}"
                ));
                return;
            }
        };
        let mut e = target;
        if lua {
            // `pipe_lua_path` took the method step and the call.
        } else if self.at_method_colon() {
            e = self.method_call(e);
        } else {
            if !matches!(e.shape, Shape::Name | Shape::At | Shape::Member { lower: true, .. } | Shape::LuaPath { .. }) {
                let range = self.range_from(e.start);
                self.error_at(
                    range,
                    "the target of `|>` must name a function: a lowercase name, `@name`, a chain \
                     ending in `.name`, `x:m` or a Lua path",
                );
            }
            if self.at(L_PAREN) && !self.nl() {
                e = self.call(e);
            } else if self.at_trailing_do() {
                e = self.block_after(e);
            }
        }
        if self.at_command_arg_start() {
            let callee = self.text_between(e.start, self.pos()).to_string();
            self.error(format!(
                "a pipe target takes no arguments without parentheses: write `{callee}(…)` (the \
                 piped value comes first)"
            ));
            let m = self.start();
            self.command_args_into(false);
            m.complete(self, ERROR_NODE);
        }
        let follows = self.at(DOT)
            || self.at(QUESTION_DOT)
            || (!self.nl() && matches!(self.cur(), L_PAREN | L_BRACK | BANG_POSTFIX))
            || self.at_method_colon();
        if follows {
            let pipe = self.text_between(pipe_start, self.pos()).to_string();
            let step = if self.at(DOT) && is_word(self.nth(1)) {
                format!(".{}", self.nth_text(1))
            } else if self.at(L_BRACK) {
                "[…]".to_string()
            } else if self.at(L_PAREN) {
                "(…)".to_string()
            } else {
                self.nth_text(0).to_string()
            };
            let pipe = if pipe.chars().count() > 40 { "…".to_string() } else { pipe };
            self.error(format!(
                "nothing may follow a pipe target: to apply `{step}` to the result, write \
                 `({pipe}){step}`"
            ));
            self.postfix_steps(e);
        }
    }

    /// `.name` steps after `e`, without calls (the callee of a pipe or of
    /// `spawn`). Each step is a link of a chain (nesting limit).
    pub(crate) fn member_steps(&mut self, mut e: Expr) -> Expr {
        let mut links = 0;
        while self.at(DOT) && is_lua_name(self.nth(1)) && self.chain_link() {
            links += 1;
            e = self.member(e);
        }
        self.chain_done(links);
        e
    }

    /// `as_type = qual_cname | "Self"`: no type arguments, so a `<`
    /// after it is less-than.
    pub(crate) fn as_type(&mut self) {
        let m = self.start();
        match self.cur() {
            CNAME => {
                let q = self.start();
                self.bump();
                while self.at(DOT) && self.nth_at(1, CNAME) {
                    self.bump();
                    self.bump();
                }
                q.complete(self, QUAL_NAME);
            }
            SELF_TYPE_KW => self.bump(),
            LNAME if !self.nl() => {
                let text = self.nth_text(0).to_string();
                self.error(format!(
                    "`as` converts to a named type, such as `x as String`; `{text}` is not one"
                ));
                self.bump();
            }
            _ => {
                let found = self.describe_cur();
                self.error(format!("expected a type name after `as`, such as `x as String`, found {found}"));
                m.abandon(self);
                return;
            }
        }
        m.complete(self, NAMED_TYPE);
    }

    /// The current token can start a command argument, on the
    /// callee's line.
    pub(crate) fn at_command_arg_start(&self) -> bool {
        let ctx = self.ctx_top();
        // In brackets a command call is an error anyway; a word on the next
        // line there is the next item, with a `,` missing (`f(a⏎ b)`).
        if self.nl() || (!ctx.stmt && self.tok(0).nl) {
            return false;
        }
        match self.cur() {
            LNAME => {
                let text = self.nth_text(0);
                !(ctx.header && text == "then") && !(ctx.for_iter && text == "step")
            }
            CNAME | ONAME | AT_NAME | NUMBER | STRING | STR_BEGIN | NIL_KW | TRUE_KW | FALSE_KW
            | SELF_KW | SELF_TYPE_KW | NOT_KW | L_CURLY | FN_KW | SPAWN_KW | ON_KW | CASE_KW
            | COND_KW | TRY_KW => true,
            // A prefix `!` starts an argument only with whitespace before it:
            // `:type!x` is not `:type(!x)`.
            BANG => !self.nth_touches(0),
            TILDE => !(self.nth_at(1, EQ) && self.nth_touches(1)),
            COLON => !self.nth_touches(0) && self.at_path_colon(),
            _ => false,
        }
    }

    /// A `:` directly followed by a word: the start of a Lua path where an
    /// operand may start.
    pub(crate) fn at_path_colon(&self) -> bool {
        self.at(COLON) && self.nth_touches(1) && is_lua_name(self.nth(1))
    }

    /// A `:` touching the expression before it and the word after it: a
    /// method call.
    pub(crate) fn at_method_colon(&self) -> bool {
        self.at(COLON) && self.nth_touches(0) && self.nth_touches(1) && is_lua_name(self.nth(1))
    }
}
