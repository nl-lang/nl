//! Primary expressions and the expression forms that start
//! with a keyword or bracket: tables, anonymous and short functions,
//! `spawn`, `on` and interpolated strings.

use super::{NameClass, can_start_expr, is_statement_keyword, is_word, oname_message};
use crate::SyntaxKind::*;
use crate::parser::event::CompletedMarker;
use crate::parser::{Expr, Parser, Shape, closer};

impl Parser<'_> {
    /// `primary`. Reports a missing operand; returns `None` when no
    /// token was consumed.
    pub(crate) fn primary(&mut self, cmd_ok: bool) -> Option<Expr> {
        let start = self.pos();
        let k = self.cur();
        if self.tok(0).after_error && self.nl() {
            // The operand was a character the lexer rejected and reported
            // (`y = é` at the end of a line): do not take one from the next
            // line, and do not report it again.
            return None;
        }
        let (cm, shape) = match k {
            // `x -> …`: a name with `->` after it on its line. A
            // `CNAME` or `ONAME` there is a parameter with a naming error.
            LNAME | CNAME | ONAME if self.at_name_arrow() => self.lambda(),
            PIPE | PIPE2 => self.lambda(),
            ARROW => {
                self.arrow_without_params();
                return self.junk_then_operand(start, cmd_ok);
            }
            LNAME | CNAME | ONAME => {
                if k == ONAME {
                    let msg = oname_message(self.nth_text(0));
                    self.error(msg);
                }
                let m = self.start();
                self.bump();
                let shape = if k == CNAME { Shape::CapName } else { Shape::Name };
                (m.complete(self, NAME_REF), shape)
            }
            SELF_KW | SELF_TYPE_KW => {
                let m = self.start();
                self.bump();
                let shape = if k == SELF_KW { Shape::SelfValue } else { Shape::SelfType };
                (m.complete(self, SELF_EXPR), shape)
            }
            AT_NAME => {
                let word = self.nth_text(0).trim_start_matches('@');
                if crate::keyword_kind(word).is_none() && crate::name_case(word) == crate::NameCase::Other {
                    let msg = format!(
                        "`@{word}` names a field, and field names are snake_case (`@{}`); a name in \
                         the style of `{word}` is only allowed in a Lua path or after `:` in a \
                         method call",
                        super::to_snake(word)
                    );
                    self.error(msg);
                }
                let m = self.start();
                self.bump();
                (m.complete(self, AT_EXPR), Shape::At)
            }
            NUMBER | STRING | TRUE_KW | FALSE_KW | NIL_KW => {
                let m = self.start();
                self.bump();
                (m.complete(self, LITERAL), Shape::Literal)
            }
            STR_BEGIN => (self.interp_string(), Shape::Literal),
            DOT3 => {
                // Lua's varargs: `...` is only the range operator.
                self.error(
                    "NL has no varargs: `...` is the range operator of `a...b`. Declare the last \
                     parameter as `*args` and read the table `args`; `f(*args)` passes its items \
                     on as arguments",
                );
                let m = self.start();
                self.bump();
                (m.complete(self, ERROR_NODE), Shape::Other)
            }
            L_PAREN => self.paren_expr(),
            L_CURLY => (self.table(), Shape::Table),
            L_BRACK => (self.list(), Shape::Table),
            FN_KW => (self.fn_expr(), Shape::Construct),
            ASYNC_KW if self.nth_at(1, FN_KW) => (self.fn_expr(), Shape::Construct),
            IF_KW => (self.if_expr(), Shape::Construct),
            UNLESS_KW => (self.unless_expr(), Shape::Construct),
            CASE_KW => (self.case_expr(), Shape::Construct),
            COND_KW if self.cond_follows() => (self.cond_expr(), Shape::Construct),
            COND_KW => {
                self.error("`cond` is a keyword: it starts a `cond … when … end` block. Use another name");
                // Recover as if it were a name, so that `while cond` keeps
                // its block.
                let m = self.start();
                self.bump();
                (m.complete(self, ERROR_NODE), Shape::Name)
            }
            TRY_KW => (self.try_expr(), Shape::Construct),
            SPAWN_KW => self.spawn_expr(cmd_ok),
            ON_KW => (self.on_expr(), Shape::Construct),
            COLON if self.at_path_colon() => self.lua_path(),
            COLON => {
                self.err_bump(
                    "a `:` here must be directly followed by a name, as in the Lua path `:print` \
                     or the method call `x:m()`",
                );
                return None;
            }
            WHILE_KW | UNTIL_KW | LOOP_KW | FOR_KW | DO_KW | STRUCT_KW | INTERFACE_KW | ENUM_KW
            | MODULE_KW => {
                let text = self.nth_text(0).to_string();
                let msg = if k == DO_KW {
                    "`do … end` here is a scope block, which is a statement, not a value; a block \
                     passed to a call follows the call on the same line"
                        .to_string()
                } else {
                    format!("`{text}` begins a statement, not an expression")
                };
                self.error(msg);
                let m = self.start();
                self.statement();
                (m.complete(self, ERROR_NODE), Shape::Other)
            }
            ASYNC_KW if matches!(self.nth(1), PIPE | PIPE2) || (self.nth(1) == LNAME && self.nth_at(2, ARROW)) => {
                self.error("a short function cannot be async: write `async fn(a) … end`");
                return self.junk_then_operand(start, cmd_ok);
            }
            ASYNC_KW => {
                self.error("`async` must be followed by `fn` or `do`");
                return self.junk_then_operand(start, cmd_ok);
            }
            QUESTION => {
                self.err_bump(
                    "`?` alone is only used in types (`T?`); the nil-safe operators `?.` and `??` \
                     follow a value",
                );
                return None;
            }
            _ => {
                // After a character the lexer rejected (`x = @ + 1`), that
                // character was the operand, and it has been reported.
                if k == DOT && self.nth_at(1, NUMBER) && self.nth_touches(1) {
                    // `.5` is not a number.
                    let digits = self.nth_text(1).to_string();
                    self.error(format!("a number starts with a digit: write `0.{digits}`"));
                } else if !self.tok(0).after_error {
                    let found = self.describe_cur();
                    self.error(format!("expected an expression, found {found}"));
                }
                if self.at_recovery_point() || is_statement_keyword(k) || self.at(EOF) {
                    return None;
                }
                return self.junk_then_operand(start, cmd_ok);
            }
        };
        Some(Expr { cm, shape, start })
    }

    /// After an error at a token that cannot start an operand (already
    /// reported): skips it and what follows on the line that cannot start
    /// one either, prefix operators and stray `async` included, into one
    /// `ERROR_NODE`; then reads the operand after it, if any. A loop, not a
    /// recursion: the operand starts with a token `primary` accepts, so it
    /// cannot come back here (`x = - * - * … y`).
    fn junk_then_operand(&mut self, start: usize, cmd_ok: bool) -> Option<Expr> {
        let m = self.start();
        loop {
            let k = self.cur();
            let operand_start = can_start_expr(k)
                && !matches!(k, MINUS | BANG | NOT_KW | TILDE)
                && !(k == ASYNC_KW && !self.nth_at(1, FN_KW));
            if self.pos() > start && (operand_start || self.at_recovery_point()) {
                break;
            }
            if self.at(EOF) {
                break;
            }
            self.bump();
        }
        let junk = m.complete(self, ERROR_NODE);
        if can_start_expr(self.cur()) && !self.nl() && !self.at_recovery_point() {
            // The operand: its own errors, if any, are reported as usual.
            if let Some(e) = self.postfix(cmd_ok) {
                return Some(e);
            }
        }
        Some(Expr { cm: junk, shape: Shape::Other, start })
    }

    /// `"(" expr ")"`: an `expr`, in a bracket context.
    fn paren_expr(&mut self) -> (CompletedMarker, Shape) {
        let m = self.start();
        let shape = self.in_bracket(R_PAREN, |p| {
            p.bump();
            let inner = if p.at(R_PAREN) {
                if !p.tok(0).after_error {
                    p.error("expected an expression between `(` and `)`");
                }
                None
            } else {
                p.expr()
            };
            p.close_bracket(R_PAREN, "(");
            match inner {
                Some(e) if e.shape == Shape::Range => Shape::Range,
                _ => Shape::Other,
            }
        });
        (m.complete(self, PAREN_EXPR), shape)
    }

    /// `table`.
    pub(crate) fn table(&mut self) -> CompletedMarker {
        let m = self.start();
        self.in_bracket(R_CURLY, |p| {
            p.bump();
            p.comma_list(R_CURLY, |p| p.table_item());
            p.close_bracket(R_CURLY, "{");
        });
        m.complete(self, TABLE_EXPR)
    }

    /// `table_item = table_key ":" expr | expr`.
    fn table_item(&mut self) {
        let m = self.start();
        let k = self.cur();
        let keyed = self.nth_at(1, COLON);
        if matches!(k, TRUE_KW | FALSE_KW | NIL_KW) && keyed {
            // Not a key name in a table. One error; the value is read.
            let word = self.nth_text(0).to_string();
            let key_range = self.cur_range();
            let key = self.start();
            self.bump();
            key.complete(self, ERROR_NODE);
            self.bump();
            let value_start = self.pos();
            if self.expr().is_none() {
                self.error("expected a value after the key's `:`");
            }
            let value = self.text_between(value_start, self.pos());
            let value = if value.is_empty() { "…".to_string() } else { value };
            let msg = if k == NIL_KW {
                format!(
                    "`nil` is not a key name here, and `nil` cannot be a key. For the string key \
                     write `{{\"nil\": {value}}}`"
                )
            } else {
                format!(
                    "`{word}` is not a key name here. For the string key write `{{\"{word}\": \
                     {value}}}`; a boolean key is set with an assignment, `t[{word}] = {value}`"
                )
            };
            self.error_at(key_range, msg);
            m.complete(self, TABLE_PAIR);
            return;
        }
        if (is_word(k) || matches!(k, NUMBER | STRING | ONAME)) && keyed {
            if k == ONAME {
                let text = self.nth_text(0).to_string();
                self.error(format!(
                    "a table key in this style is written as a string: `{{\"{text}\": …}}`"
                ));
            }
            let key = self.start();
            self.bump_word();
            key.complete(self, TABLE_KEY);
            self.pair_value(m);
            return;
        }
        // Lua's `{a = 1}`: at the start of an item (after `{` or `,`), a word
        // followed by `=` can only be a key written the Lua way. Read the `=`
        // as the key's colon so the rest of the table parses.
        let at_item_start =
            self.pos.checked_sub(1).and_then(|i| self.toks.get(i)).is_some_and(|t| matches!(t.kind, L_CURLY | COMMA));
        if is_word(k) && self.nth_at(1, EQ) && at_item_start {
            let text = self.nth_text(0).to_string();
            let key = self.start();
            self.bump_word();
            key.complete(self, TABLE_KEY);
            self.err_bump(format!("a table key is followed by `:`, not `=`: write `{text}: …`"));
            if self.expr().is_none() {
                self.error("expected a value after the key");
            }
            m.complete(self, TABLE_PAIR);
            return;
        }
        if k == MINUS && self.nth_at(1, NUMBER) && self.nth_at(2, COLON) {
            let num = self.nth_text(1).to_string();
            self.error(format!("a table key cannot be negative: set it with an assignment, `t[-{num}] = …`"));
            let key = self.start();
            self.bump();
            self.bump();
            key.complete(self, TABLE_KEY);
            self.pair_value(m);
            return;
        }
        if k == L_BRACK && self.list_is_computed_key() {
            // `[expr]: v`, Lua's computed key: not in NL. Read and
            // reported; the pair stays in the tree.
            let start = self.pos();
            let key = self.start();
            self.in_bracket(R_BRACK, |p| {
                p.bump();
                if p.expr().is_none() {
                    p.error("expected the key between `[` and `]`");
                }
                p.close_bracket(R_BRACK, "[");
            });
            key.complete(self, TABLE_KEY);
            let text = self.text_between(start + 1, self.pos() - 1).to_string();
            self.error_at(
                self.range_from(start),
                format!("a key is a word, a number or a plain string; a computed key is set with an assignment, `t[{text}] = …`"),
            );
            self.pair_value(m);
            return;
        }
        let Some(e) = self.expr() else {
            m.abandon(self);
            return;
        };
        if self.at(COLON) {
            let msg = if e.cm.kind(self) == Some(INTERP_STRING) {
                "a string key cannot be interpolated; set it with an assignment, `t[\"…#{x}\"] = …`"
            } else {
                "this cannot be a table key: a key is a word, a number or a plain string"
            };
            let range = self.range_from(e.start);
            self.error_at(range, msg);
            let key = e.cm.precede(self);
            key.complete(self, TABLE_KEY);
            self.pair_value(m);
            return;
        }
        // A positional item: braces hold `key: value` pairs; lists are
        // written with brackets.
        let range = self.range_from(e.start);
        let text = self.text_between(e.start, self.pos()).to_string();
        self.error_at(range, format!("a brace table holds `key: value` pairs; a list is written with brackets: `[{text}]`"));
        m.complete(self, TABLE_ITEM);
    }

    /// At `[` inside a table: `[expr]:` is Lua's computed key, `[1, 2]` a
    /// list item. Both are errors; the distinction picks the message.
    fn list_is_computed_key(&self) -> bool {
        let mut depth = 0usize;
        let mut i = 0;
        loop {
            let k = self.nth(i);
            match k {
                L_BRACK | L_PAREN | L_CURLY => depth += 1,
                R_BRACK | R_PAREN | R_CURLY => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return self.nth_at(i + 1, COLON);
                    }
                }
                EOF => return false,
                _ => {}
            }
            i += 1;
            if i > 4096 {
                return false;
            }
        }
    }

    /// `":" expr` of a table pair.
    fn pair_value(&mut self, m: crate::parser::event::Marker) {
        self.bump();
        if self.expr().is_none() {
            self.error("expected a value after the key's `:`");
        }
        m.complete(self, TABLE_PAIR);
    }

    /// `list = "[" [ list_item { "," list_item } [ "," ] ] "]"`, `list_item =
    /// expr | "*" expr`.
    fn list(&mut self) -> CompletedMarker {
        let m = self.start();
        self.in_bracket(R_BRACK, |p| {
            p.bump();
            p.comma_list(R_BRACK, |p| {
                if p.at(STAR) {
                    let s = p.start();
                    p.bump();
                    if p.expr().is_none() {
                        p.error("expected the list to spread after `*`");
                    }
                    s.complete(p, SPREAD_ARG);
                } else if p.expr().is_none() && !p.at(R_BRACK) {
                    let found = p.describe_cur();
                    p.error(format!("expected a list item, found {found}"));
                }
            });
            p.close_bracket(R_BRACK, "[");
        });
        m.complete(self, LIST_EXPR)
    }

    /// `string_lit` with interpolation: `STR_BEGIN expr { STR_MID
    /// expr } STR_END`. An empty `#{}` was reported by the lexer.
    pub(crate) fn interp_string(&mut self) -> CompletedMarker {
        let m = self.start();
        self.bump();
        loop {
            if !matches!(self.cur(), STR_MID | STR_END) {
                self.in_bracket(STR_END, |p| {
                    if p.at(EOF) {
                        return;
                    }
                    p.expr();
                    if !matches!(p.cur(), STR_MID | STR_END | EOF) {
                        let found = p.describe_cur();
                        p.error(format!(
                            "expected `}}` to end the interpolation, found {found}: an \
                             interpolation holds one expression"
                        ));
                        p.skip_to_string_part();
                    }
                });
            }
            match self.cur() {
                STR_MID => self.bump(),
                STR_END => {
                    self.bump();
                    break;
                }
                _ => break,
            }
        }
        m.complete(self, INTERP_STRING)
    }

    /// Skips to the `STR_MID` or `STR_END` of the current string, past
    /// nested strings, into an `ERROR_NODE`.
    fn skip_to_string_part(&mut self) {
        let m = self.start();
        let mut depth = 0u32;
        loop {
            match self.cur() {
                EOF => break,
                STR_BEGIN => depth += 1,
                STR_MID if depth == 0 => break,
                STR_END if depth == 0 => break,
                STR_END => depth -= 1,
                _ => {}
            }
            self.bump();
        }
        m.complete(self, ERROR_NODE);
    }

    /// `fn_expr = [ "async" ] "fn" [ params ] [ return_type ] block "end"`.
    pub(crate) fn fn_expr(&mut self) -> CompletedMarker {
        let m = self.start();
        self.in_construct(closer::END, |p| {
            p.eat(ASYNC_KW);
            p.bump();
            if p.at(LT) && !p.nl() {
                p.error("an anonymous function takes no type parameters");
                let e = p.start();
                p.type_params();
                e.complete(p, ERROR_NODE);
            }
            // `(` on the line of `fn` opens the parameters.
            if p.at(L_PAREN) && !p.nl() {
                p.param_list();
            }
            p.opt_return_type();
            p.block();
            p.expect_end("fn");
        });
        m.complete(self, FN_EXPR)
    }

    /// A name followed by `->` on its line: the parameter of a short
    /// function `x -> …`. In a statement context, `->` at the start of the
    /// next line does not belong to the name.
    fn at_name_arrow(&self) -> bool {
        self.nth_at(1, ARROW) && !(self.nth_raw_nl(1) && self.ctx_top().stmt)
    }

    /// `lambda = lambda_params "->" ( expr | "do" block "end" )` at
    /// `x`, `|` or `||`. The parameters are block parameters,
    /// and `x -> …` makes a `BLOCK_PARAMS` of its one name. The expression
    /// body is a `BLOCK` holding one `EXPR_STMT` (see
    /// [`crate::SyntaxKind::LAMBDA_EXPR`]); it is an `expr`, never a command
    /// call, and extends as far as an `expr` does. The `->` must be on the
    /// line of the parameters; the body may start on the next line.
    pub(crate) fn lambda(&mut self) -> (CompletedMarker, Shape) {
        let m = self.start();
        if self.block_params() {
            // After `|…|` the body follows `->`, so a `:` is always the
            // return type: there is no Lua path to tell it from.
            if self.at(COLON) && !self.nl() {
                let r = self.start();
                self.bump();
                self.type_expected("after `:`");
                r.complete(self, RETURN_TYPE);
            }
        } else {
            let list = self.start();
            let param = self.start();
            let pat = self.start();
            self.name(NameClass::Lower, "the parameter of a short function");
            pat.complete(self, BIND_PAT);
            param.complete(self, PARAM);
            list.complete(self, BLOCK_PARAMS);
        }
        if self.at(ARROW) {
            if self.nl() {
                self.error(
                    "the `->` of a short function goes on the line of its parameters: `|a| -> …`",
                );
            }
            self.bump();
        } else {
            let found = self.describe_cur();
            self.error(format!(
                "expected `->` after the parameters of a short function, as in `|a, b| -> a + b`, \
                 found {found}"
            ));
            // Read a body that follows on the line (`|a| a + 1`), so that
            // its errors are reported and the rest of the line parses.
            if self.nl() || !(can_start_expr(self.cur()) || self.at(DO_KW)) {
                return (m.complete(self, LAMBDA_EXPR), Shape::Other);
            }
        }
        let shape = if self.at(DO_KW) {
            self.in_construct(closer::END, |p| {
                p.bump();
                p.block();
                p.expect_end("-> do");
            });
            Shape::Construct
        } else if can_start_expr(self.cur()) {
            let block = self.start();
            let stmt = self.start();
            // In block parameters a top-level `|` closes the list, so
            // the body of a short function written as a default stops there.
            let min = if self.ctx_top().no_union { 7 } else { 0 };
            let saved = self.lambda_body.replace(self.pos());
            let body = self.expr_min(min);
            self.lambda_body = saved;
            if body.is_some() {
                stmt.complete(self, EXPR_STMT);
                block.complete(self, BLOCK);
            } else {
                stmt.abandon(self);
                block.abandon(self);
            }
            Shape::Other
        } else {
            if !self.tok(0).after_error {
                let found = self.describe_cur();
                self.error(format!(
                    "expected the body of the short function after `->`: an expression, or `do … \
                     end` for statements; found {found}"
                ));
            }
            Shape::Other
        };
        (m.complete(self, LAMBDA_EXPR), shape)
    }

    /// `->` where an operand should start: no parameters before it.
    fn arrow_without_params(&mut self) {
        let prev = self.pos.checked_sub(1).and_then(|i| self.toks.get(i)).map(|t| t.kind);
        let msg = if matches!(prev, Some(PIPE | PIPE2)) {
            // `f |a| -> a`: after a value, `|` and `||` are operators.
            "after a value, `|` is bitwise or and `||` logical or, so a short function written \
             with pipes cannot start a command argument: pass it in parentheses, `f(|a| -> …)`"
        } else if self.tok(0).nl {
            "`->` cannot start a line: write it on the line of the short function's parameters, \
             `x -> …` or `|a, b| -> …`"
        } else {
            "`->` needs the parameters of a short function before it: `x -> …`, `|a, b| -> …` or \
             `|| -> …`"
        };
        self.error(msg);
    }

    /// `spawn_expr` and `spawn_command`.
    fn spawn_expr(&mut self, cmd_ok: bool) -> (CompletedMarker, Shape) {
        let m = self.start();
        let spawn_range = self.cur_range();
        self.bump();
        if self.at(ASYNC_KW) && !self.nl() {
            self.err_bump("`spawn` takes no `async`: write `spawn do … end`");
        }
        if self.at(DO_KW) && !self.nl() {
            self.in_construct(closer::END, |p| {
                p.bump();
                if matches!(p.cur(), PIPE | PIPE2) && !p.nl() {
                    p.error(
                        "`spawn do` takes no parameters: the block runs on a worker with no \
                         arguments",
                    );
                    let e = p.start();
                    p.opt_block_params();
                    e.complete(p, ERROR_NODE);
                }
                p.block();
                p.expect_end("spawn do");
            });
            return (m.complete(self, SPAWN_EXPR), Shape::Construct);
        }
        if let Some(callee) = self.spawn_callee() {
            if self.at(L_PAREN) && !self.nl() {
                let c = callee.cm.precede(self);
                self.arg_list();
                c.complete(self, CALL_EXPR);
            } else if self.at_command_arg_start() {
                if !cmd_ok {
                    self.error(
                        "`spawn f a` without parentheses is only allowed as a whole statement or \
                         value; write `spawn f(a)`",
                    );
                }
                let c = callee.cm.precede(self);
                self.command_args_into(true);
                c.complete(self, COMMAND_CALL);
            } else if self.at_method_colon() {
                self.error(
                    "`spawn` calls a function by its name; a method call `x:m` cannot be sent to a \
                     worker",
                );
                self.method_call(callee);
            } else if self.at(L_BRACK) && !self.nl() {
                self.error(
                    "`spawn` takes a function name with `.member` steps, not an index; for a \
                     computed function use `Worker.run`",
                );
            } else if self.at(QUESTION_DOT) {
                // `spawn x?.m(…)`: the chain is read as the callee's,
                // and reported once.
                self.error(
                    "a `spawn` callee cannot be nil-safe: `spawn` sends a call of a named \
                     function to a worker. Test the value first, then write `spawn x.m(…)`",
                );
                self.postfix_steps(callee);
            } else {
                let range = spawn_range.cover(self.range_from(callee.start));
                let name = self.text_between(callee.start, self.pos()).to_string();
                self.error_at(range, format!("`spawn` needs an argument list: `spawn {name}()`"));
            }
        }
        if self.at_trailing_do() {
            self.error(
                "a block cannot be sent to a worker: pass a named function, or use `spawn do … \
                 end`",
            );
            let e = self.start();
            self.trailing_block();
            e.complete(self, ERROR_NODE);
        }
        (m.complete(self, SPAWN_EXPR), Shape::Other)
    }

    /// The callee of `spawn`: a `call_path`, an `LNAME`, `@name` or a
    /// name chain ending in a lowercase word.
    fn spawn_callee(&mut self) -> Option<Expr> {
        match self.cur() {
            LNAME | CNAME | ONAME | AT_NAME | SELF_KW | SELF_TYPE_KW => {}
            COLON if self.at_path_colon() => {
                // One error for the whole path, its calls included.
                self.error(
                    "`spawn` takes a function name with `.member` steps; for a Lua function use \
                     `Worker.run`",
                );
                let e = self.start();
                self.lua_path();
                e.complete(self, ERROR_NODE);
                return None;
            }
            _ => {
                let found = self.describe_cur();
                self.error(format!(
                    "expected a function to call after `spawn` (`spawn f(x)`) or `do`, found {found}"
                ));
                return None;
            }
        }
        let e = self.primary(false)?;
        let e = self.member_steps(e);
        if !matches!(e.shape, Shape::Name | Shape::At | Shape::Member { lower: true, .. }) {
            let range = self.range_from(e.start);
            self.error_at(
                range,
                "the callee of `spawn` must end in a lowercase function name, such as `spawn \
                 f(x)` or `spawn Nav.find(a)`",
            );
        }
        Some(e)
    }

    /// `on_expr`.
    fn on_expr(&mut self) -> CompletedMarker {
        let m = self.start();
        let opener = self.pos();
        self.bump();
        self.qual_name("an event name");
        if self.at(AS_KW) && !self.nl() {
            self.bump();
            match self.cur() {
                STRING => self.bump(),
                STR_BEGIN => {
                    self.error("a handler name is a plain string, without interpolation");
                    self.interp_string();
                }
                _ => {
                    let found = self.describe_cur();
                    self.error(format!("expected the handler's name as a string after `as`, found {found}"));
                }
            }
        }
        if self.at(ASYNC_KW) && !self.nl() {
            self.bump();
        }
        if self.at(DO_KW) && !self.nl() {
            self.in_construct(closer::END, |p| {
                p.bump();
                p.opt_block_params();
                p.block();
                p.expect_end("on");
            });
        } else {
            let found = self.describe_cur();
            self.error(format!(
                "expected `do` on the line of `on`: `on Event do |args| … end`, found {found}"
            ));
            if self.next_line_more_indented(opener) {
                // The body is there; keep its `end` matched.
                let e = self.start();
                self.in_construct(closer::END, |p| {
                    p.block();
                    p.expect_end("on");
                });
                e.complete(self, ERROR_NODE);
            }
        }
        m.complete(self, ON_EXPR)
    }
}
