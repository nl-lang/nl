//! Postfix expressions, command calls and trailing
//! blocks.

use super::decl::ParamKind;
use super::{is_lower_word, is_lua_name, is_word, oname_message};
use crate::SyntaxKind::*;
use crate::parser::{Expr, LuaLast, Parser, Shape, closer};

/// `x:m?.(…)`.
pub(crate) const METHOD_SAFE_CALL: &str = "`x:m?.(…)` is not part of NL: `x:m` is always a call, so \
     there is no method value to test for `nil`. To call `m` only when `x` is not `nil`, write \
     `x?.:m(…)`";

/// Where an argument list is: the positional arguments, after the spread,
/// or among the keyed arguments.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ArgState {
    Positional,
    Spread,
    Keyed,
}

impl Parser<'_> {
    /// `postfix_expr = primary { postfix_op }`.
    pub(crate) fn postfix(&mut self, cmd_ok: bool) -> Option<Expr> {
        // `postfix_expr = primary { postfix_op } | lua_path`: a Lua path
        // takes its own steps.
        if self.at_path_colon() {
            let start = self.pos();
            let (cm, shape) = self.lua_path();
            return Some(Expr { cm, shape, start });
        }
        let e = self.primary(cmd_ok)?;
        Some(self.postfix_steps(e))
    }

    /// The postfix operators after `e`. Each step makes the tree one level
    /// deeper, which counts against the nesting limit.
    pub(crate) fn postfix_steps(&mut self, mut e: Expr) -> Expr {
        let mut links = 0;
        loop {
            let k = self.cur();
            let same_line = !self.nl();
            let step = match k {
                L_PAREN if same_line => true,
                L_BRACK if same_line => true,
                DOT => true,
                COLON => self.at_method_colon(),
                BANG_POSTFIX => true,
                // A `!` touching a keyword used as a name (`:type!`,
                // `x:end!`) was lexed as prefix; it is a postfix `!` here.
                BANG => self.nth_touches(0),
                L_CURLY if same_line => e.shape.is_cap_path() || !e.shape.is_callee(),
                DO_KW | ASYNC_KW => self.at_trailing_do(),
                // `?.` may start a line, like `.`.
                QUESTION_DOT => true,
                QUESTION if same_line => self.nth_at(1, DOT),
                _ => false,
            };
            if !step || !self.chain_link() {
                break;
            }
            links += 1;
            e = match k {
                L_PAREN => self.call(e),
                L_BRACK => self.index(e),
                DOT => self.member(e),
                COLON => self.method_call(e),
                BANG_POSTFIX | BANG => self.bang(e),
                L_CURLY if e.shape.is_cap_path() => self.struct_lit(e),
                L_CURLY => {
                    // `{` after anything but a capitalized path or a
                    // lowercase callee.
                    self.error(
                        "`{` cannot follow this expression on the same line: a struct literal \
                         needs a capitalized name (`Point { x: 1 }`), and a table passed as an \
                         argument needs a function name (`f {a: 1}`) or parentheses (`f(x, {a: \
                         1})`)",
                    );
                    let m = self.start();
                    self.table();
                    m.complete(self, ERROR_NODE);
                    e
                }
                QUESTION_DOT => self.safe_step(e),
                QUESTION => {
                    // `a ? .b`: `?.` is one token. The `.` step
                    // follows as if the `?` were not there.
                    self.err_bump(
                        "`?` alone is only used in types (`T?`); the nil-safe member access is \
                         written `?.`, without a space inside",
                    );
                    e
                }
                _ => self.block_after(e),
            };
        }
        self.chain_done(links);
        e
    }

    /// `call_args` after `e`, and a trailing block on the same line.
    pub(crate) fn call(&mut self, e: Expr) -> Expr {
        self.call_with(e, false)
    }

    /// A nil-safe step after `e`: `?.name`,
    /// `?.[k]`, `?.(args)` or `?.:m(args)`. The `?.` goes into the node of
    /// the step it guards, which has its usual kind. The step is required
    /// after `?.`, wherever it stands.
    fn safe_step(&mut self, e: Expr) -> Expr {
        self.safe_chain_starts.insert(e.start);
        match self.nth(1) {
            L_BRACK => self.index_with(e, true),
            L_PAREN => {
                if matches!(e.shape, Shape::Method { args: false } | Shape::LuaPath { last: LuaLast::Method }) {
                    self.error(METHOD_SAFE_CALL);
                }
                self.call_with(e, true)
            }
            COLON if self.nth_touches(2) && is_lua_name(self.nth(2)) => self.method_call_with(e, true),
            _ => {
                let m = e.cm.precede(self);
                self.bump();
                let k = self.cur();
                let lower = is_lower_word(k) || k == ONAME;
                self.member_name("?.");
                // A nil-safe member never starts a struct literal.
                let shape = Shape::Member { lower, cap_path: false };
                Expr { cm: m.complete(self, FIELD_EXPR), shape, start: e.start }
            }
        }
    }

    fn call_with(&mut self, e: Expr, safe: bool) -> Expr {
        if e.shape == Shape::Bang {
            let msg = self.bang_call_message(e.start);
            self.error(msg);
        }
        let m = e.cm.precede(self);
        if safe {
            self.bump();
        }
        self.arg_list();
        let mut shape = Shape::Call;
        if self.at_trailing_do() {
            self.trailing_block();
            shape = Shape::BlockCall;
        }
        Expr { cm: m.complete(self, CALL_EXPR), shape, start: e.start }
    }

    /// The error for an argument list after `f!`, the `!` expression
    /// starting at token `start`: the ways to call the function value, and
    /// for `Type.name!(…)` the plain call through the type.
    fn bang_call_message(&self, start: usize) -> String {
        let text = self.text_between(start, self.pos());
        let Some(callee) = text.strip_suffix('!') else {
            return "`f!` is the function itself: nothing that calls may follow it. Write \
                    `(f!)(…)` to call the function value"
                .to_string();
        };
        let head = format!("`{text}` is the function itself: nothing that calls may follow it");
        match callee.rsplit_once('.') {
            Some((owner, name)) if owner.starts_with(|c: char| c.is_ascii_uppercase()) => format!(
                "{head}. Write `({text})(…)` to call it with the arguments as written; for a \
                 method defined on `Number`, `String` or `Bool`, `{owner}.{name}(…)` does the \
                 same"
            ),
            Some(_) => format!(
                "{head}. Write `{callee}(…)` to call it as a member, or `({text})(…)` to call \
                 it with the arguments as written"
            ),
            None => format!(
                "{head}. Write `{callee}(…)` to call `{callee}`, or `({text})(…)` to call the \
                 function value"
            ),
        }
    }

    /// `call_args = "(" [ arg { "," arg } [ "," ] ] ")"`, `arg = expr | "*"
    /// expr | LNAME ":" expr | "**" expr`. The positional arguments
    /// come first, then one spread, then the keyed ones.
    pub(crate) fn arg_list(&mut self) {
        let m = self.start();
        self.in_bracket(R_PAREN, |p| {
            p.bump();
            let mut state = ArgState::Positional;
            p.comma_list(R_PAREN, |p| {
                p.arg(&mut state, false);
            });
            p.close_bracket(R_PAREN, "(");
        });
        m.complete(self, ARG_LIST);
    }

    /// `name: value`: a lowercase word, a `:` that touches it, and
    /// whitespace after the `:`; `x:m` with the `:` touching both sides is
    /// a method call.
    fn at_keyed_arg(&self) -> bool {
        self.at(LNAME) && self.nth_at(1, COLON) && self.nth_touches(1) && !self.nth_touches(2)
    }

    /// One argument of a call or a command call, and the order check. Returns
    /// false when there is no argument here. `first` is the first command
    /// argument, where `*` and `**` are operators.
    fn arg(&mut self, state: &mut ArgState, first: bool) -> bool {
        if self.at_keyed_arg() {
            let m = self.start();
            self.bump();
            self.bump();
            if self.expr().is_none() {
                self.error("expected a value after the key");
            }
            m.complete(self, KEYED_ARG);
            *state = ArgState::Keyed;
            return true;
        }
        if self.at_keyed_param() && !first {
            let m = self.start();
            self.bump();
            if self.expr().is_none() {
                self.error("expected the table to pass as keyed arguments after `**`");
            }
            m.complete(self, KEYED_SPREAD_ARG);
            *state = ArgState::Keyed;
            return true;
        }
        if self.at(STAR) && !first {
            if *state != ArgState::Positional {
                self.error(match state {
                    ArgState::Spread => "a spread argument `*list` must be the last positional argument",
                    _ => "positional arguments and the spread come before the keyed arguments `key: value`",
                });
            }
            self.spread_arg();
            *state = ArgState::Spread;
            return true;
        }
        let start = self.pos();
        if self.expr().is_none() {
            return false;
        }
        if *state != ArgState::Positional {
            let range = self.range_from(start);
            self.error_at(
                range,
                match state {
                    ArgState::Spread => "a spread argument `*list` must be the last positional argument",
                    _ => "positional arguments come before the keyed arguments `key: value`",
                },
            );
        }
        true
    }

    fn spread_arg(&mut self) {
        let m = self.start();
        self.bump();
        if self.expr().is_none() {
            self.error("expected the table to spread after `*`");
        }
        m.complete(self, SPREAD_ARG);
    }

    /// `"[" expr "]"`.
    fn index(&mut self, e: Expr) -> Expr {
        self.index_with(e, false)
    }

    fn index_with(&mut self, e: Expr, safe: bool) -> Expr {
        let m = e.cm.precede(self);
        if safe {
            self.bump();
        }
        self.in_bracket(R_BRACK, |p| {
            p.bump();
            if p.at(R_BRACK) {
                p.error("expected an index between `[` and `]`");
            } else {
                p.expr();
                if p.at(COMMA) {
                    // `f [1, 2]` is an index of `f`; a list argument
                    // needs parentheses.
                    p.error("`[` after a value is an index, which takes one expression; to pass a list, write `f([1, 2])`");
                }
            }
            p.close_bracket(R_BRACK, "[");
        });
        Expr { cm: m.complete(self, INDEX_EXPR), shape: Shape::Index, start: e.start }
    }

    /// `"." word_any`: any word, keywords included; `.len` is
    /// an ordinary member here. The `.` may start a line.
    pub(crate) fn member(&mut self, e: Expr) -> Expr {
        let m = e.cm.precede(self);
        self.bump();
        let k = self.cur();
        let (lower, cap) = (is_lower_word(k) || k == ONAME, k == CNAME);
        self.member_name(".");
        let shape = Shape::Member { lower, cap_path: cap && e.shape.is_cap_path() };
        Expr { cm: m.complete(self, FIELD_EXPR), shape, start: e.start }
    }

    /// The word after `.` or `?.` (`op`).
    fn member_name(&mut self, op: &str) {
        let k = self.cur();
        if is_word(k) {
            self.bump_word();
        } else if k == ONAME {
            let msg = oname_message(self.nth_text(0));
            self.error(msg);
            self.bump();
        } else if k == NUMBER && !self.nl() {
            self.err_bump("a member name is a word: `x.1` is not allowed; to index, write `x[1]`");
        } else {
            let found = self.describe_cur();
            self.error(format!("expected a member name after `{op}`, found {found}"));
        }
    }

    /// `":" lua_name` with the `:` touching both sides: a method call, with
    /// arguments, a block, or neither.
    pub(crate) fn method_call(&mut self, e: Expr) -> Expr {
        self.method_call_with(e, false)
    }

    fn method_call_with(&mut self, e: Expr, safe: bool) -> Expr {
        let m = e.cm.precede(self);
        if safe {
            self.bump();
        }
        self.bump();
        self.bump_word();
        let mut args = false;
        if self.at(L_PAREN) && !self.nl() {
            self.arg_list();
            args = true;
        }
        if self.at_trailing_do() {
            self.trailing_block();
            args = true;
        }
        Expr { cm: m.complete(self, METHOD_CALL), shape: Shape::Method { args }, start: e.start }
    }

    /// Postfix `!`.
    fn bang(&mut self, e: Expr) -> Expr {
        if !e.shape.bang_ok() {
            let msg = match e.shape {
                Shape::Call | Shape::BlockCall | Shape::Command => {
                    "`!` cannot follow a call: `!` names a function without calling it, and a \
                     call has already happened"
                }
                Shape::Method { .. } => {
                    "`!` cannot follow a method call `x:m`: a Lua method has no function value to \
                     take"
                }
                Shape::LuaPath { .. } => {
                    "a Lua path is never called automatically, so it takes no `!`"
                }
                Shape::Bang => "`!` is written once",
                _ => "`!` can only follow a name, `@name` or a member `x.name`: it names a function without calling it",
            };
            self.error(msg);
            // Recover as if the `!` were not there, so that what follows is
            // read as it would be without it (one error).
            let m = e.cm.precede(self);
            self.bump();
            return Expr { cm: m.complete(self, BANG_EXPR), shape: e.shape, start: e.start };
        }
        let m = e.cm.precede(self);
        self.bump();
        Expr { cm: m.complete(self, BANG_EXPR), shape: Shape::Bang, start: e.start }
    }

    /// `struct_lit_body` after a capitalized name path.
    fn struct_lit(&mut self, e: Expr) -> Expr {
        let m = e.cm.precede(self);
        self.in_bracket(R_CURLY, |p| {
            p.bump();
            p.comma_list(R_CURLY, |p| p.field_init());
            p.close_bracket(R_CURLY, "{");
        });
        Expr { cm: m.complete(self, STRUCT_LIT), shape: Shape::Other, start: e.start }
    }

    /// `field_init = field_name ":" expr`.
    fn field_init(&mut self) {
        let k = self.cur();
        // Lua's `{id = 1}`: read the `=` as the field's colon.
        if is_lower_word(k) && self.nth_at(1, EQ) {
            let text = self.nth_text(0).to_string();
            let m = self.start();
            self.bump_word();
            self.err_bump(format!("a field in a struct literal is followed by `:`, not `=`: write `{text}: …`"));
            if self.expr().is_none() {
                self.error("expected the field's value");
            }
            m.complete(self, FIELD_INIT);
            return;
        }
        if (is_lower_word(k) || matches!(k, CNAME | ONAME)) && self.nth_at(1, COLON) {
            let m = self.start();
            match k {
                ONAME => {
                    let msg = oname_message(self.nth_text(0));
                    self.error(msg);
                }
                CNAME => {
                    let fixed = super::to_snake(self.nth_text(0));
                    self.error(format!("field names are lowercase: write `{fixed}`"));
                }
                _ => {}
            }
            self.bump_word();
            self.bump();
            if self.expr().is_none() {
                self.error("expected the field's value after `:`");
            }
            m.complete(self, FIELD_INIT);
            return;
        }
        if self.at(R_CURLY) || self.at(EOF) {
            return;
        }
        let msg = if k == LNAME && matches!(self.nth(1), COMMA | R_CURLY) {
            let name = self.nth_text(0);
            format!("a struct literal has no shorthand: write `{name}: {name}`")
        } else {
            "the fields of a struct literal are written `name: value`".to_string()
        };
        self.error(msg);
        let m = self.start();
        if self.expr().is_none() {
            self.bump();
        }
        m.complete(self, ERROR_NODE);
    }

    /// A `do` (or `async do`) here starts a trailing block for the call or
    /// callee before it: on the same line, not in a command call's own
    /// arguments, and not at the end of a header line.
    pub(crate) fn at_trailing_do(&self) -> bool {
        if self.nl() {
            return false;
        }
        let do_at = match self.cur() {
            DO_KW => 0,
            ASYNC_KW if self.nth_at(1, DO_KW) && !self.nth_raw_nl(1) => 1,
            _ => return false,
        };
        let ctx = self.ctx_top();
        !(ctx.cmd_args || (ctx.header && self.do_ends_line(do_at + 1)))
    }

    /// A `do` block after `e`: `e` must be a callee; a call with
    /// parentheses took its block already.
    pub(crate) fn block_after(&mut self, e: Expr) -> Expr {
        if e.shape.is_callee() {
            let m = e.cm.precede(self);
            self.trailing_block();
            return Expr { cm: m.complete(self, CALL_EXPR), shape: Shape::BlockCall, start: e.start };
        }
        let msg = match e.shape {
            Shape::Bang => "`f!` is the function itself: nothing that calls may follow it, not even a block",
            Shape::BlockCall => "this call already has a block",
            _ => {
                "a `do … end` block passed to a call follows the call or the function name on the \
                 same line, as in `items.each do |x| … end`; it cannot follow this expression"
            }
        };
        self.error(msg);
        let m = self.start();
        self.trailing_block();
        m.complete(self, ERROR_NODE);
        e
    }

    /// `trailing_block = [ "async" ] "do" [ block_params ] block "end"`.
    pub(crate) fn trailing_block(&mut self) {
        let m = self.start();
        let in_header = self.ctx_top().header;
        let do_line = self.tok(0).line;
        let do_range = self.cur_range();
        let mut one_line = false;
        self.in_construct(closer::END, |p| {
            p.eat(ASYNC_KW);
            p.bump();
            let params = matches!(p.cur(), PIPE | PIPE2) && !p.nl();
            p.opt_block_params();
            p.block();
            one_line = !params && p.at(END_KW) && p.tok(0).line == do_line;
            p.expect_end("do");
        });
        if in_header && one_line {
            self.mark_header_do(do_range);
        }
        m.complete(self, TRAILING_BLOCK);
    }

    /// `block_params` on the line of `do`, then an optional return type.
    /// A statement cannot begin with `|`, so parameters on the
    /// next line are reported and then read as parameters.
    pub(crate) fn opt_block_params(&mut self) {
        if self.nl() {
            if !matches!(self.cur(), PIPE | PIPE2) {
                return;
            }
            self.error("block parameters go on the line of `do`: write `do |x|`");
        }
        if self.block_params() {
            self.opt_return_type();
        }
    }

    /// `block_params = "||" | "|" [ block_param { "," block_param } [ "," ]
    /// ] "|"` at the current token, as a `BLOCK_PARAMS`; false
    /// when the current token is neither `|` nor `||`. Shared by trailing
    /// blocks, `on` and short functions.
    pub(crate) fn block_params(&mut self) -> bool {
        match self.cur() {
            PIPE2 => {
                let m = self.start();
                self.bump();
                m.complete(self, BLOCK_PARAMS);
            }
            PIPE => {
                let m = self.start();
                self.in_bracket_with(PIPE, true, |p| {
                    p.bump();
                    let mut rest_seen = false;
                    let mut keyed_seen = false;
                    p.comma_list(PIPE, |p| {
                        if keyed_seen {
                            p.error("`**opts` must be the last parameter");
                            keyed_seen = false;
                            rest_seen = false;
                        } else if rest_seen && !p.at_keyed_param() {
                            p.error("`*rest` and `...` must be the last parameter, or followed only by `**opts`");
                            rest_seen = false;
                        }
                        match p.param(true) {
                            ParamKind::Plain => {}
                            ParamKind::Rest => rest_seen = true,
                            ParamKind::Keyed => keyed_seen = true,
                        }
                    });
                    if !p.eat(PIPE) {
                        let found = p.describe_cur();
                        p.error(format!("expected `|` to close the block parameters, found {found}"));
                    }
                });
                m.complete(self, BLOCK_PARAMS);
            }
            _ => return false,
        }
        true
    }

    /// A command call: `callee` is followed by its first argument. When
    /// command calls are not allowed here, the error is reported and the call
    /// parsed all the same.
    pub(crate) fn command_call(&mut self, callee: Expr, cmd_ok: bool) -> Expr {
        let nested = self.ctx_top().cmd_args;
        let args_start = self.pos();
        let m = callee.cm.precede(self);
        self.command_args_into(true);
        if !cmd_ok {
            let name = self.text_between(callee.start, args_start);
            let args = self.text_between(args_start, self.pos()).to_string();
            let fix = if name.chars().count() + args.chars().count() > 40 {
                format!("{name}(…)")
            } else {
                format!("{name}({args})")
            };
            let range = self.range_from(callee.start);
            let msg = if self.lambda_body == Some(callee.start) {
                // `|a| -> print a`.
                format!(
                    "the body of a short function is an expression, not a command call: write \
                     `{fix}`, or use a block body, `-> do … end`"
                )
            } else if nested {
                format!("command calls cannot be nested: write `{fix}`")
            } else {
                format!(
                    "a call without parentheses is only allowed as a whole statement, the value \
                     of an assignment or declaration, or a header; write `{fix}`"
                )
            };
            self.error_at(range, msg);
        }
        if self.at_trailing_do() {
            self.trailing_block();
        }
        Expr { cm: m.complete(self, COMMAND_CALL), shape: Shape::Command, start: callee.start }
    }

    /// `command_args = command_arg { "," command_arg }` as a `COMMAND_ARGS`
    /// node (`node`), or bare into the current node.
    pub(crate) fn command_args_into(&mut self, node: bool) {
        let a = self.start();
        self.with_flags(
            |c| c.cmd_args = true,
            |p| {
                let mut first = true;
                let mut state = ArgState::Positional;
                loop {
                    if !p.arg(&mut state, first) {
                        break;
                    }
                    first = false;
                    // A `,` that starts a line does not continue the
                    // arguments (`,` is not a token that continues a line).
                    if p.nl() || !p.eat(COMMA) {
                        break;
                    }
                }
            },
        );
        if node {
            a.complete(self, COMMAND_ARGS);
        } else {
            a.abandon(self);
        }
    }
}
