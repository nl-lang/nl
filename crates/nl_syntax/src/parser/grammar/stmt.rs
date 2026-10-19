//! Blocks and statements, statement modifiers,
//! assignment and `return`/`break`/`continue`.

use super::{can_start_expr, can_start_statement};
use crate::SyntaxKind::{self, *};
use crate::parser::{Expr, LuaLast, Parser, Shape, closer_bit};

impl Parser<'_> {
    /// `block`: statements separated by line breaks or `;`, up to a
    /// token that ends the block (a closer keyword some construct accepts,
    /// the closer of an enclosing bracket, `EOF`). Stray closers are reported
    /// and skipped.
    pub(crate) fn block(&mut self) {
        let m = self.start();
        let _ = self.guarded(|p| p.block_items());
        m.complete(self, BLOCK);
    }

    fn block_items(&mut self) {
        loop {
            while self.eat(SEMI) {
                self.sep_error_reported = false;
            }
            let k = self.cur();
            if k == EOF || self.closer_accepted(k) || self.closes_bracket(k) {
                break;
            }
            if closer_bit(k) != 0 {
                let text = self.nth_text(0).to_string();
                let msg = match k {
                    END_KW => "unexpected `end`: there is no open block for it to close".to_string(),
                    ELSE_KW | ELSEIF_KW => {
                        format!("unexpected `{text}`: there is no `if` here for it to continue")
                    }
                    WHEN_KW => "unexpected `when`: it belongs inside `case … end` or `cond … end`"
                        .to_string(),
                    _ => format!("unexpected `{text}`: it belongs inside `try … end`"),
                };
                self.err_bump(msg);
                continue;
            }
            if matches!(k, R_PAREN | R_BRACK | R_CURLY | STR_MID | STR_END) {
                let found = self.describe_nth(0);
                self.err_bump(format!("unexpected {found}: there is no open bracket for it to close"));
                continue;
            }
            if self.hint_closes_construct() {
                break;
            }
            if matches!(k, DOT | DOT2 | DOT3 | DOT2_LT) && self.tok(0).nl {
                // A line starting with `.` would continue the line before, but its statement was
                // complete and cannot go on (`return` then `.x`).
                let text = self.nth_text(0).to_string();
                self.err_bump(format!(
                    "a line that starts with `{text}` continues the expression of the line before, \
                     and there is none to continue there"
                ));
                self.sep_error_reported = true;
                continue;
            }
            if k == COMMA {
                self.err_bump(
                    "a line cannot start with `,`: a list continues on the next line only when \
                     the `,` ends the line before",
                );
                // The rest of the line continues the list: no second error.
                self.sep_error_reported = true;
                continue;
            }
            if self.tok(0).nl {
                self.sep_error_reported = false;
            }
            if self.at_file_level() {
                self.top_level_statement();
            }
            let before = self.pos();
            self.statement();
            if self.pos() == before {
                let found = self.describe_cur();
                self.err_bump(format!("expected a statement, found {found}"));
                continue;
            }
            self.statement_end();
        }
    }

    /// After a statement: a line break, `;`, or a token that ends the
    /// block. Anything else on the line is reported once; the parser then
    /// reads it as the next statement, or skips it if it cannot begin one.
    fn statement_end(&mut self) {
        if self.at(SEMI) {
            while self.eat(SEMI) {}
            self.sep_error_reported = false;
            return;
        }
        let k = self.cur();
        if self.nl() || self.at_block_end() || closer_bit(k) != 0 {
            return;
        }
        if k == EQ {
            // After a complete statement that is not an assignment (`let a = b
            // = 1`, `return a = 1`, `x() if a = 1`): one error for the `=` and
            // the value after it.
            let msg = if self.at_match_operator() {
                super::MATCH_OPERATOR
            } else {
                "unexpected `=`: assignment is a statement, not an expression (`a = b = 1` is not \
                 allowed); to compare, write `==`"
            };
            if !self.sep_error_reported {
                self.error(msg);
                self.sep_error_reported = true;
            }
            let m = self.start();
            self.bump();
            self.cmd_expr();
            m.complete(self, ERROR_NODE);
            return;
        }
        if !self.sep_error_reported {
            let found = self.describe_cur();
            self.error(format!(
                "unexpected {found} after the end of a statement: statements are separated by line \
                 breaks or `;`"
            ));
            self.sep_error_reported = true;
        }
        if !can_start_statement(k) {
            let m = self.start();
            loop {
                let k = self.cur();
                if k == EOF
                    || k == SEMI
                    || self.nl()
                    || self.at_block_end()
                    || closer_bit(k) != 0
                    || can_start_statement(k)
                {
                    break;
                }
                self.bump();
            }
            m.complete(self, ERROR_NODE);
        }
    }

    /// `statement`. Guarded: recovery parses statements inside struct,
    /// interface and enum bodies and inside operands, which nest.
    pub(crate) fn statement(&mut self) {
        let _ = self.guarded(|p| p.statement_inner());
    }

    fn statement_inner(&mut self) {
        // A statement on a new line is never the operand of a character the
        // lexer rejected at the end of the line before (`Tok::after_error`).
        if self.nl() {
            if let Some(t) = self.toks.get_mut(self.pos) {
                t.after_error = false;
            }
        }
        match self.cur() {
            LET_KW | CONST_KW | GLOBAL_KW | STRUCT_KW | INTERFACE_KW | ENUM_KW | TYPE_KW | MODULE_KW
            | IMPORT_KW | PUB_KW => {
                self.declaration();
                self.no_modifier_after("a declaration");
            }
            FN_KW if self.fn_decl_follows(1) => {
                self.declaration();
                self.no_modifier_after("a declaration");
            }
            ASYNC_KW if self.nth_at(1, FN_KW) && self.fn_decl_follows(2) => {
                self.declaration();
                self.no_modifier_after("a declaration");
            }
            WHILE_KW | UNTIL_KW | LOOP_KW | FOR_KW => {
                self.loop_statement();
                self.no_modifier_after("a loop");
            }
            DO_KW => {
                self.do_block();
                self.no_modifier_after("a `do … end` block");
            }
            LNAME if self.at_lua_habit() => self.lua_habit(),
            _ => self.simple_statement(),
        }
    }

    /// `fn` at the start of a statement declares a function when followed by
    /// an `LNAME` or by a `CNAME` that starts `Type.name`. An `ONAME`,
    /// or a `CNAME` followed by `(` on its line (`fn Foo(x)`), is taken as a
    /// declaration too, and reported as a naming error.
    pub(crate) fn fn_decl_follows(&self, n: usize) -> bool {
        match self.nth(n) {
            LNAME | ONAME => true,
            CNAME => self.nth_at(n + 1, DOT) || (self.nth_at(n + 1, L_PAREN) && !self.nth_raw_nl(n + 1)),
            _ => false,
        }
    }

    /// `local x = …`, `local function`, `function f(`: Lua habits.
    fn at_lua_habit(&self) -> bool {
        let word = self.nth_text(0);
        let same_line = !self.nth_raw_nl(1);
        match word {
            "local" => {
                same_line
                    && (self.nth_at(1, LNAME) && self.nth_text(1) == "function"
                        || (matches!(self.nth(1), LNAME | CNAME | ONAME) && self.local_assign_follows()))
            }
            "function" => same_line && matches!(self.nth(1), LNAME | CNAME | ONAME),
            _ => false,
        }
    }

    /// `local a, b =` on one line.
    fn local_assign_follows(&self) -> bool {
        let mut i = 1;
        loop {
            let t = self.peek(i);
            if !matches!(t.kind, LNAME | CNAME | ONAME) || t.nl {
                return false;
            }
            i += 1;
            let t = self.peek(i);
            match t.kind {
                COMMA if !t.nl => i += 1,
                EQ => return !t.nl,
                _ => return false,
            }
        }
    }

    fn lua_habit(&mut self) {
        let m = self.start();
        if self.nth_text(0) == "local" {
            let e = self.start();
            self.bump();
            if self.at_word("function") {
                self.bump();
                e.complete(self, ERROR_NODE);
                self.error_at(
                    self.range_from(self.pos().saturating_sub(2)),
                    "NL declares functions with `fn`: write `fn name(…) … end`",
                );
                self.fn_decl_after_keyword(m);
            } else {
                e.complete(self, ERROR_NODE);
                let range = self.range_from(self.pos().saturating_sub(1));
                if self.lua_function_value_follows() {
                    // `local f = function(x) … end`: one error for both.
                    self.error_at(
                        range,
                        "NL declares variables with `let` and writes functions with `fn`: write \
                         `let f = fn(x) … end`",
                    );
                    self.let_lua_function(m);
                } else {
                    self.error_at(range, "NL declares variables with `let`: write `let x = …`");
                    self.let_rest(m);
                }
            }
        } else {
            let e = self.start();
            self.bump();
            e.complete(self, ERROR_NODE);
            let range = self.range_from(self.pos().saturating_sub(1));
            self.error_at(range, "NL declares functions with `fn`: write `fn name(…) … end`");
            self.fn_decl_after_keyword(m);
        }
    }

    /// `name = function(` on one line, after `local` (Lua).
    fn lua_function_value_follows(&self) -> bool {
        let (a, b, c, d) = (self.peek(0), self.peek(1), self.peek(2), self.peek(3));
        matches!(a.kind, LNAME | CNAME | ONAME)
            && b.kind == EQ
            && c.kind == LNAME
            && self.src.get(std::ops::Range::<usize>::from(c.range)) == Some("function")
            && d.kind == L_PAREN
            && !(b.nl || c.nl || d.nl)
    }

    /// `let name = function(…) … end` after a reported `local`: the value is
    /// an anonymous function whose keyword is `function`.
    fn let_lua_function(&mut self, m: crate::parser::event::Marker) {
        self.pattern(super::patterns::PatCtx::Other);
        self.bump();
        let list = self.start();
        let start = self.pos();
        let f = self.start();
        self.in_construct(crate::parser::closer::END, |p| {
            let kw = p.start();
            p.bump();
            kw.complete(p, ERROR_NODE);
            p.param_list();
            p.opt_return_type();
            p.block();
            p.expect_end("function");
        });
        let cm = f.complete(self, FN_EXPR);
        self.postfix_steps(Expr { cm, shape: Shape::Construct, start });
        list.complete(self, EXPR_LIST);
        m.complete(self, LET_DECL);
    }

    /// After a declaration, a loop or a scope block, a modifier is an error.
    /// It is parsed into an `ERROR_NODE`.
    fn no_modifier_after(&mut self, what: &str) {
        if self.at_modifier() {
            let kw = self.nth_text(0).to_string();
            self.error(format!(
                "a statement modifier (`{kw}`) cannot follow {what}; it only follows a call, an \
                 assignment, `return`, `break` or `continue`. Use a block: `{kw} cond … end`"
            ));
            let m = self.start();
            self.modifier_clause();
            m.complete(self, ERROR_NODE);
        }
    }

    /// The current token is a statement-modifier keyword on the statement's
    /// line.
    pub(crate) fn at_modifier(&self) -> bool {
        matches!(self.cur(), IF_KW | UNLESS_KW | WHILE_KW | UNTIL_KW) && !self.nl()
    }

    /// `modifier = ( "if" | "unless" | "while" | "until" ) cmd_expr`.
    fn modifier_clause(&mut self) {
        let m = self.start();
        let kw = self.nth_text(0).to_string();
        self.bump();
        if self.nl() || !can_start_expr(self.cur()) {
            let found = self.describe_cur();
            self.error(format!("expected a condition after `{kw}`, found {found}"));
        } else {
            self.cmd_expr();
        }
        m.complete(self, MODIFIER);
    }

    /// A simple statement and its optional modifier.
    fn simple_statement(&mut self) {
        let before = self.pos();
        let m = self.start();
        let (kind, shape) = match self.cur() {
            RETURN_KW => {
                self.bump();
                if self.return_values_follow() {
                    self.expr_list();
                }
                (RETURN_STMT, None)
            }
            BREAK_KW => {
                self.bump();
                (BREAK_STMT, None)
            }
            CONTINUE_KW => {
                self.bump();
                (CONTINUE_STMT, None)
            }
            _ => self.expr_or_assignment(),
        };
        if self.pos() == before {
            // Nothing that could start a statement: no node (the caller
            // reports and skips the token).
            m.abandon(self);
            return;
        }
        if self.at_modifier() {
            if shape == Some(Shape::Construct) {
                let kw = self.nth_text(0).to_string();
                self.error(format!(
                    "a statement modifier (`{kw}`) cannot follow a block that ends in `end`; put \
                     the block inside `{kw} cond … end`"
                ));
            }
            self.modifier_clause();
            while self.at_modifier() {
                self.error(
                    "a statement takes one modifier only: combine the conditions with `and` or \
                     `or`, or use a block",
                );
                let e = self.start();
                self.modifier_clause();
                e.complete(self, ERROR_NODE);
            }
        }
        m.complete(self, kind);
    }

    /// The values of `return` are present when the next token is on the same
    /// line, can begin an expression, and is not a modifier keyword.
    fn return_values_follow(&self) -> bool {
        !self.nl() && can_start_expr(self.cur()) && !matches!(self.cur(), IF_KW | UNLESS_KW)
    }

    /// `expr_list = { expr "," } cmd_expr`: every value is read as a
    /// `cmd_expr`, since a command call consumes the commas after it.
    pub(crate) fn expr_list(&mut self) {
        let m = self.start();
        loop {
            if self.cmd_expr().is_none() {
                break;
            }
            // `,` is not a continuation token: a `,` that starts a line
            // is not part of this list.
            if self.nl() || !self.eat(COMMA) {
                break;
            }
        }
        m.complete(self, EXPR_LIST);
    }

    /// Reads a `cmd_expr`, then reinterprets it as an assignment target if
    /// `=` or a compound operator follows, and as the start of a target list
    /// or a value list if `,` follows.
    fn expr_or_assignment(&mut self) -> (SyntaxKind, Option<Shape>) {
        let Some(lhs) = self.cmd_expr() else {
            return (EXPR_STMT, None);
        };
        let k = self.cur();
        if self.at_match_operator() && !self.nth_touches(2) {
            // `x =~ re` (spaces after `~`): Lua or Perl's match, not `x =
            // ~re`.
            self.error(super::MATCH_OPERATOR);
            let m = self.start();
            self.bump();
            self.bump();
            self.cmd_expr();
            m.complete(self, ERROR_NODE);
            return (EXPR_STMT, None);
        }
        if k == EQ {
            self.check_target(lhs);
            self.bump();
            self.assigned_value();
            (ASSIGN_STMT, None)
        } else if k.is_compound_assign() {
            self.check_target(lhs);
            self.bump();
            self.assigned_value();
            (COMPOUND_ASSIGN_STMT, None)
        } else if k == COMMA && lhs.shape != Shape::Command && !self.nl() {
            // `a, b = …` is a multiple assignment; `a, b` alone is a value
            // list, which is a statement only as the tail of a function
            // body: the resolver rejects it elsewhere. The
            // targets are checked only if `=` follows.
            // `f (a), b` is `f(a), b`, but the whitespace before `(`
            // shows that command arguments were meant.
            let spaced_call = lhs.shape == Shape::Call && self.spaced_call_paren(lhs.start, self.pos());
            let tl = lhs.cm.precede(self);
            let mut target_diags = Vec::new();
            let before = self.diag_count();
            self.check_multi_target(lhs);
            target_diags.extend(self.take_diags(before));
            while !self.nl() && self.eat(COMMA) {
                match self.expr() {
                    Some(t) => {
                        let before = self.diag_count();
                        self.check_multi_target(t);
                        target_diags.extend(self.take_diags(before));
                    }
                    None => break,
                }
            }
            if self.at(EQ) {
                tl.complete(self, TARGET_LIST);
                self.restore_diags(target_diags);
                self.bump();
                self.expr_list();
                (MULTI_ASSIGN_STMT, None)
            } else if self.cur().is_compound_assign() {
                tl.complete(self, TARGET_LIST);
                self.restore_diags(target_diags);
                self.error("a compound assignment has exactly one target");
                self.bump();
                self.expr_list();
                (MULTI_ASSIGN_STMT, None)
            } else {
                tl.complete(self, EXPR_LIST);
                if spaced_call {
                    let range = self.range_from(lhs.start);
                    self.error_at(
                        range,
                        "`f (a), b` is the call `f(a)` followed by `, b`, not a call with two \
                         arguments; to pass several arguments, put them all inside the \
                         parentheses: `f((a), b)`",
                    );
                }
                (EXPR_STMT, None)
            }
        } else {
            (EXPR_STMT, Some(lhs.shape))
        }
    }

    /// True when the tokens from `from` to `to` (exclusive) hold a `(` at
    /// bracket depth zero with whitespace before it: the `(` of `f (a)`.
    fn spaced_call_paren(&self, from: usize, to: usize) -> bool {
        let mut depth = 0usize;
        for i in from..to {
            let Some(t) = self.toks.get(i) else { break };
            match t.kind {
                L_PAREN | L_BRACK | L_CURLY => {
                    if depth == 0 && t.kind == L_PAREN && t.sp && i > from {
                        return true;
                    }
                    depth += 1;
                }
                R_PAREN | R_BRACK | R_CURLY => depth = depth.saturating_sub(1),
                _ => {}
            }
        }
        false
    }

    /// The right side of `=` or of a compound assignment: a `cmd_expr`.
    /// `a = b = 1` is reported.
    fn assigned_value(&mut self) {
        self.cmd_expr();
        if self.at_match_operator() {
            self.error(super::MATCH_OPERATOR);
            let m = self.start();
            self.bump();
            self.cmd_expr();
            m.complete(self, ERROR_NODE);
        } else if self.at(EQ) || self.cur().is_compound_assign() {
            self.error(
                "assignment is a statement, not an expression: `a = b = 1` is not allowed; assign \
                 each target on its own line",
            );
            let m = self.start();
            self.bump();
            self.cmd_expr();
            m.complete(self, ERROR_NODE);
        }
    }

    /// [`Self::check_target`] for one target of a multiple assignment, where
    /// a nil-safe target is not allowed: when its receiver is
    /// `nil` it would take no value, and the other targets would.
    fn check_multi_target(&mut self, e: Expr) {
        let target_shape = matches!(e.shape, Shape::Member { .. } | Shape::Index | Shape::LuaPath { last: LuaLast::Field | LuaLast::Index });
        if target_shape && self.safe_chain_starts.contains(&e.start) {
            let range = self.range_from(e.start);
            self.error_at(
                range,
                "a nil-safe target needs an assignment of its own: write `a?.b = x` on its own \
                 line, not in a multiple assignment",
            );
            return;
        }
        self.check_target(e);
    }

    /// Reports an expression that cannot be assigned to.
    fn check_target(&mut self, e: Expr) {
        let msg = match e.shape {
            Shape::Name
            | Shape::CapName
            | Shape::At
            | Shape::Member { .. }
            | Shape::Index
            | Shape::LuaPath { last: LuaLast::Field | LuaLast::Index } => return,
            Shape::LuaPath { last: LuaLast::Name } => {
                "a Lua path without a step cannot be assigned to: to set a Lua global, write \
                 `:_G.name = value`"
            }
            Shape::LuaPath { .. } => {
                "a Lua path that ends in a call cannot be assigned to: a path is a target when it \
                 ends in `.name` or `[expr]`"
            }
            Shape::Bang => {
                let text = self.text_between(e.start, self.pos());
                let name = text.trim_end_matches('!').to_string();
                let msg = format!(
                    "`{text}` is the function value itself and cannot be assigned to; assign to \
                     `{name}`"
                );
                let range = self.range_from(e.start);
                self.error_at(range, msg);
                return;
            }
            Shape::Call | Shape::BlockCall | Shape::Method { .. } | Shape::Command => {
                "a call cannot be assigned to"
            }
            Shape::Literal => "a literal cannot be assigned to",
            Shape::SelfValue | Shape::SelfType => {
                "`self` and `Self` cannot be assigned to; to set a field, write `@name = value`"
            }
            Shape::Table => {
                "there is no destructuring assignment: declare new names with `let {a, b} = t`, \
                 or assign each target"
            }
            Shape::Range | Shape::Construct | Shape::Other => {
                "this cannot be assigned to: an assignment target is a name, `@field`, a member \
                 `x.name`, an index `x[i]` or a Lua path `:a.b`"
            }
        };
        let range = self.range_from(e.start);
        self.error_at(range, msg);
    }
}
