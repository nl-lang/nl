//! Control flow: `if`, `unless`, `case`, `cond`, `try` (block
//! expressions), and `while`, `until`, `loop`, `for` and scope blocks
//! (statements).

use super::can_start_expr;
use super::patterns::PatCtx;
use crate::SyntaxKind::*;
use crate::parser::event::CompletedMarker;
use crate::parser::{Parser, Shape, closer};

impl Parser<'_> {
    /// `condition = cmd_expr | "let" pattern "=" cmd_expr`, or just a
    /// `cmd_expr` for the headers that take no `let`.
    fn header_condition(&mut self, keyword: &str, allow_let: bool) {
        self.header_same_line(keyword);
        if self.at(LET_KW) {
            if !allow_let {
                if keyword == "while" {
                    self.error(
                        "`while let` is not part of NL yet; use `loop` with `if let … else break \
                         end`",
                    );
                } else {
                    self.error(format!("`{keyword} let` is not allowed; use `if let`"));
                }
            }
            let m = self.start();
            self.bump();
            self.pattern(PatCtx::Other);
            if self.expect(EQ) {
                self.header_expr();
            }
            m.complete(self, LET_CONDITION);
        } else if self.header_expr().is_none() {
            return;
        }
        if self.at(EQ) {
            let msg = if self.at_match_operator() {
                super::MATCH_OPERATOR
            } else {
                "`=` assigns, and assignment is not an expression: to compare, write `==`"
            };
            self.error(msg);
            let e = self.start();
            self.bump();
            self.expr();
            e.complete(self, ERROR_NODE);
        }
    }

    /// A `cmd_expr` in a header: `then` never starts a command argument, and a
    /// `do` at the end of the line is not a trailing block.
    fn header_expr(&mut self) -> Option<crate::parser::Expr> {
        self.with_flags(|c| c.header = true, |p| p.cmd_expr())
    }

    /// `if_expr`.
    pub(crate) fn if_expr(&mut self) -> CompletedMarker {
        let m = self.start();
        self.in_construct(closer::ELSEIF | closer::ELSE | closer::END, |p| {
            p.bump();
            p.header_condition("if", true);
            p.header_then_end("if");
            p.block();
            while p.takes_closer(ELSEIF_KW) {
                let c = p.start();
                p.bump();
                p.header_condition("elseif", true);
                p.header_then_end("elseif");
                p.block();
                c.complete(p, ELSEIF_CLAUSE);
            }
            p.else_clause_and_strays("if");
            p.expect_end("if");
        });
        m.complete(self, IF_EXPR)
    }

    /// `[ "else" block ]`, then any `elseif` or `else` that follows it, which
    /// is reported.
    fn else_clause_and_strays(&mut self, keyword: &str) {
        if !self.takes_closer(ELSE_KW) {
            return;
        }
        let c = self.start();
        self.bump();
        self.block();
        c.complete(self, ELSE_CLAUSE);
        while self.takes_closer(ELSEIF_KW) || self.takes_closer(ELSE_KW) {
            let text = self.nth_text(0).to_string();
            self.error(format!("`{text}` cannot follow the `else` of `{keyword}`: `else` comes last"));
            let e = self.start();
            let is_elseif = self.at(ELSEIF_KW);
            self.bump();
            if is_elseif {
                self.header_condition("elseif", true);
                self.header_then_end("elseif");
            }
            self.block();
            e.complete(self, ERROR_NODE);
        }
    }

    /// `unless_expr`: an `else` but no `elseif` and no `let`.
    pub(crate) fn unless_expr(&mut self) -> CompletedMarker {
        let m = self.start();
        self.in_construct(closer::ELSEIF | closer::ELSE | closer::END, |p| {
            p.bump();
            p.header_condition("unless", false);
            p.header_then_end("unless");
            p.block();
            while p.takes_closer(ELSEIF_KW) {
                p.error("`unless` takes no `elseif`: use `if` with `elseif`, or `else`");
                let e = p.start();
                p.bump();
                p.header_condition("elseif", true);
                p.header_then_end("elseif");
                p.block();
                e.complete(p, ERROR_NODE);
            }
            p.else_clause_and_strays("unless");
            p.expect_end("unless");
        });
        m.complete(self, UNLESS_EXPR)
    }

    /// `case_expr`.
    pub(crate) fn case_expr(&mut self) -> CompletedMarker {
        let m = self.start();
        self.in_construct(closer::WHEN | closer::ELSE | closer::END, |p| {
            p.bump();
            p.header_same_line("case");
            p.header_expr();
            // `case x when 1 then "one" else "many" end` on one line: a
            // `when` may follow the header directly.
            if !(p.at(WHEN_KW) && !p.nl()) {
                p.header_end("case");
            }
            while p.eat(SEMI) {}
            if !p.at(WHEN_KW) {
                if p.at(ELSE_KW) || p.at(END_KW) {
                    p.error("`case` needs at least one `when` clause");
                } else if !p.at_block_end() {
                    let found = p.describe_cur();
                    p.error(format!("expected `when`, found {found}: the body of `case` is a list of `when` clauses"));
                    let e = p.start();
                    p.block();
                    e.complete(p, ERROR_NODE);
                }
            }
            while p.takes_closer(WHEN_KW) {
                p.when_clause();
            }
            p.else_then_stray_whens("case");
            p.expect_end("case");
        });
        m.complete(self, CASE_EXPR)
    }

    /// `else` of `case` or `cond`, then any `when` after it (reported).
    fn else_then_stray_whens(&mut self, keyword: &str) {
        if !self.takes_closer(ELSE_KW) {
            return;
        }
        let c = self.start();
        self.bump();
        self.block();
        c.complete(self, ELSE_CLAUSE);
        while self.takes_closer(WHEN_KW) || self.takes_closer(ELSE_KW) {
            let text = self.nth_text(0).to_string();
            self.error(format!("`{text}` cannot follow the `else` of `{keyword}`: `else` comes last"));
            let e = self.start();
            if self.at(WHEN_KW) {
                if keyword == "case" {
                    self.when_clause();
                } else {
                    self.cond_clause();
                }
            } else {
                self.bump();
                self.block();
            }
            e.complete(self, ERROR_NODE);
        }
    }

    /// `when_clause`.
    fn when_clause(&mut self) {
        let c = self.start();
        self.bump();
        self.header_same_line("when");
        let mut after = "`when`";
        loop {
            if !self.when_item(after) {
                // An empty item (`when 1, , 2`): reported; read on after it.
                if self.at(COMMA) && !self.nl() {
                    self.bump();
                    continue;
                }
                break;
            }
            if self.nl() || !self.eat(COMMA) {
                break;
            }
            after = "`,`";
        }
        if self.at(IF_KW) && !self.nl() {
            let g = self.start();
            self.bump();
            if self.header_expr().is_none() {
                self.error("expected a condition after `if`");
            }
            g.complete(self, WHEN_GUARD);
        }
        self.header_then_end("when");
        self.block();
        c.complete(self, WHEN_CLAUSE);
    }

    /// `when_item`: a pattern when it starts with `{`, `[`, `is`,
    /// or a capitalized name path followed by `{`; an expression otherwise.
    fn when_item(&mut self, after: &str) -> bool {
        if matches!(self.cur(), L_CURLY | L_BRACK | IS_KW) || self.at_cap_path_then_curly() {
            return self.pattern(PatCtx::Other).is_some();
        }
        // After a `,`, the next item may be on the next line.
        let first = after == "`when`";
        if (first && self.nl()) || !can_start_expr(self.cur()) {
            let found = if first { self.describe_cur() } else { self.describe_nth(0) };
            self.error(format!("expected a value or a pattern after {after}, found {found}"));
            return false;
        }
        self.with_flags(|c| c.header = true, |p| p.expr()).is_some()
    }

    /// `cond` is a keyword: a `cond_expr` when a line break (or `;`) and
    /// `when` follow it.
    pub(crate) fn cond_follows(&self) -> bool {
        let mut i = 1;
        let mut sep = self.peek(1).nl;
        while self.peek(i).kind == SEMI {
            sep = true;
            i += 1;
        }
        sep && self.peek(i).kind == WHEN_KW
    }

    /// `cond_expr`.
    pub(crate) fn cond_expr(&mut self) -> CompletedMarker {
        let m = self.start();
        self.in_construct(closer::WHEN | closer::ELSE | closer::END, |p| {
            p.bump();
            while p.eat(SEMI) {}
            while p.takes_closer(WHEN_KW) {
                p.cond_clause();
            }
            p.else_then_stray_whens("cond");
            p.expect_end("cond");
        });
        m.complete(self, COND_EXPR)
    }

    /// `"when" cmd_expr sep block` of a `cond`: one condition, no guard.
    fn cond_clause(&mut self) {
        let c = self.start();
        self.bump();
        self.header_same_line("when");
        self.header_expr();
        if (self.at(COMMA) || self.at(IF_KW)) && !self.nl() {
            let msg = if self.at(COMMA) {
                "a `when` of `cond` takes one condition: combine conditions with `or`"
            } else {
                "a `when` of `cond` takes no guard: combine the conditions with `and`"
            };
            self.error(msg);
            let e = self.start();
            self.bump();
            self.header_expr();
            e.complete(self, ERROR_NODE);
        }
        self.header_then_end("when");
        self.block();
        c.complete(self, COND_CLAUSE);
    }

    /// `try_expr` and `catch_clause`.
    pub(crate) fn try_expr(&mut self) -> CompletedMarker {
        let m = self.start();
        self.in_construct(closer::CATCH | closer::FINALLY | closer::END, |p| {
            let try_range = p.cur_range();
            p.bump();
            p.block();
            let mut handlers = 0;
            let mut finally_seen = false;
            while p.takes_closer(CATCH_KW) || p.takes_closer(FINALLY_KW) {
                handlers += 1;
                if p.at(FINALLY_KW) {
                    if finally_seen {
                        p.error("`try` takes one `finally`");
                    }
                    finally_seen = true;
                    let c = p.start();
                    p.bump();
                    p.block();
                    c.complete(p, FINALLY_CLAUSE);
                    continue;
                }
                if finally_seen {
                    p.error("`catch` cannot follow `finally`: `finally` comes last");
                }
                p.catch_clause();
            }
            if handlers == 0 {
                p.error_at(try_range, "`try` needs at least one `catch` or a `finally`");
            }
            p.expect_end("try");
        });
        m.complete(self, TRY_EXPR)
    }

    /// `catch_clause = "catch" [ LNAME [ ":" type ] ] sep block`.
    fn catch_clause(&mut self) {
        let c = self.start();
        self.bump();
        if !self.nl() {
            match self.cur() {
                LNAME | ONAME => {
                    self.name(super::NameClass::Lower, "the name of the caught error");
                    if self.at(COLON) && !self.nl() {
                        self.bump();
                        self.type_expected("after `:`");
                    }
                }
                CNAME => {
                    let ty = self.nth_text(0).to_string();
                    self.error(format!(
                        "`catch` takes a name first, then the type: write `catch err: {ty}`"
                    ));
                    let e = self.start();
                    self.type_expected("after `catch`");
                    e.complete(self, ERROR_NODE);
                }
                _ => {}
            }
        }
        self.header_end("catch");
        self.block();
        c.complete(self, CATCH_CLAUSE);
    }

    /// `loop_statement`.
    pub(crate) fn loop_statement(&mut self) {
        let m = self.start();
        let kind = self.cur();
        self.in_construct(closer::END, |p| {
            p.bump();
            match kind {
                WHILE_KW => {
                    p.header_condition("while", false);
                    p.header_end("while");
                }
                UNTIL_KW => {
                    p.header_condition("until", false);
                    p.header_end("until");
                }
                FOR_KW => p.for_header(),
                _ => {
                    // Ruby's `loop do … end`: the `do` would open a scope
                    // block and take the `end`.
                    if p.at(DO_KW) && !p.nl() && p.do_ends_line(1) {
                        p.err_bump(
                            "NL's `loop` takes no `do`: write `loop`, the body on the next lines, \
                             then `end`",
                        );
                    }
                }
            }
            p.block();
            let what = match kind {
                WHILE_KW => "while",
                UNTIL_KW => "until",
                FOR_KW => "for",
                _ => "loop",
            };
            p.expect_end(what);
        });
        let node = match kind {
            WHILE_KW => WHILE_STMT,
            UNTIL_KW => UNTIL_STMT,
            FOR_KW => FOR_STMT,
            _ => LOOP_STMT,
        };
        m.complete(self, node);
    }

    /// The header of `for`.
    fn for_header(&mut self) {
        self.header_same_line("for");
        let vars_start = self.pos();
        loop {
            if self.pattern(PatCtx::Other).is_none() {
                break;
            }
            if self.nl() || !self.eat(COMMA) {
                break;
            }
        }
        if self.at(EQ) {
            self.lua_numeric_for(vars_start);
            return;
        }
        if !self.eat(IN_KW) {
            let found = self.describe_cur();
            self.error(format!("expected `in` after the loop variables, found {found}"));
        }
        let iter = self.with_flags(
            |c| {
                c.header = true;
                c.for_iter = true;
            },
            |p| p.cmd_expr(),
        );
        if self.at_word("step") && !self.nl() {
            let s = self.start();
            self.bump_as(STEP_KW);
            if self.with_flags(|c| c.header = true, |p| p.expr()).is_none() {
                self.error("expected the step after `step`");
            }
            s.complete(self, FOR_STEP);
            if let Some(iter) = iter {
                if iter.shape != Shape::Range {
                    let range = self.range_from(iter.start);
                    self.error_at(
                        range,
                        "`step` needs a range written in the header, such as `for i in 1..10 \
                         step 2`",
                    );
                }
            }
        }
        self.header_end("for");
    }

    /// Lua's numeric `for i = a, b[, c] [do]`: one error that shows the NL
    /// form, then the body is read as usual.
    fn lua_numeric_for(&mut self, vars_start: usize) {
        let var = self.text_between(vars_start, self.pos());
        let eq_range = self.cur_range();
        let e = self.start();
        self.bump();
        let mut bounds = Vec::new();
        loop {
            let start = self.pos();
            if self.with_flags(|c| c.header = true, |p| p.expr()).is_none() {
                break;
            }
            bounds.push(self.text_between(start, self.pos()).to_string());
            if self.nl() || !self.eat(COMMA) {
                break;
            }
        }
        // A `do` at the end of the line belongs to the Lua form too.
        let has_do = self.at(DO_KW) && !self.nl() && self.do_ends_line(1);
        if has_do {
            self.bump();
        }
        e.complete(self, ERROR_NODE);
        let fix = match bounds.as_slice() {
            [a, b] => format!("`for {var} in {a}..{b}`"),
            [a, b, c] => format!("`for {var} in {a}..{b} step {c}`"),
            _ => "`for i in 1..10`".to_string(),
        };
        self.error_at(
            eq_range,
            format!(
                "NL's `for` has no `=` form: write {fix} (a range includes its end){}",
                if has_do { ", without `do`" } else { "" }
            ),
        );
        self.header_end("for");
    }

    /// `do_block = "do" block "end"`.
    pub(crate) fn do_block(&mut self) {
        let m = self.start();
        self.in_construct(closer::END, |p| {
            p.bump();
            p.block();
            p.expect_end("do");
        });
        m.complete(self, DO_BLOCK);
    }
}
