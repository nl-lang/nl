//! The productions of the grammar, one module per part, plus helpers
//! shared by several of them (names, headers, `end`, bracketed lists).

mod control;
mod decl;
mod declfile;
mod expr;
mod lua_path;
mod patterns;
mod postfix;
mod primary;
mod stmt;
mod types;

use super::{Parser, closer_bit};
use crate::SyntaxKind::{self, *};


/// `file = block EOF`.
pub(crate) fn source_file(p: &mut Parser<'_>) {
    let m = p.start();
    p.block();
    p.finish_file();
    m.complete(p, SOURCE_FILE);
}

/// `decl_file`: the root of a declaration file.
pub(crate) fn decl_file(p: &mut Parser<'_>) {
    p.decl_file();
}

/// `word_any`: a name token or any keyword.
pub(crate) fn is_word(k: SyntaxKind) -> bool {
    matches!(k, LNAME | CNAME) || k.is_keyword()
}

/// `lua_name`: any word, in any style.
pub(crate) fn is_lua_name(k: SyntaxKind) -> bool {
    is_word(k) || k == ONAME
}

/// The targeted error for `x =~ re`.
pub(crate) const MATCH_OPERATOR: &str = "there is no match operator `=~`: use `re.test(x)` or `x.match(re)`";

/// `lower_word`: an `LNAME` or a keyword other than `Self`.
pub(crate) fn is_lower_word(k: SyntaxKind) -> bool {
    k == LNAME || (k.is_keyword() && k != SELF_TYPE_KW)
}

/// A token that can begin an expression.
pub(crate) fn can_start_expr(k: SyntaxKind) -> bool {
    matches!(
        k,
        LNAME
            | CNAME
            | ONAME
            | AT_NAME
            | NUMBER
            | STRING
            | STR_BEGIN
            | NIL_KW
            | TRUE_KW
            | FALSE_KW
            | SELF_KW
            | SELF_TYPE_KW
            | DOT3
            | L_PAREN
            | L_CURLY
            | L_BRACK
            | MINUS
            | BANG
            | NOT_KW
            | TILDE
            | COLON
            | FN_KW
            | ASYNC_KW
            | IF_KW
            | UNLESS_KW
            | CASE_KW
            | COND_KW
            | TRY_KW
            | SPAWN_KW
            | ON_KW
            // A short function `|a| -> …`, `|| -> …`. After a
            // complete expression the two are operators, which may
            // continue it from the next line.
            | PIPE
            | PIPE2
    )
}

/// A token that can begin a statement.
pub(crate) fn can_start_statement(k: SyntaxKind) -> bool {
    can_start_expr(k) || is_statement_keyword(k)
}

/// Keywords that begin a statement and nothing else.
pub(crate) fn is_statement_keyword(k: SyntaxKind) -> bool {
    matches!(
        k,
        LET_KW
            | CONST_KW
            | GLOBAL_KW
            | STRUCT_KW
            | INTERFACE_KW
            | ENUM_KW
            | TYPE_KW
            | MODULE_KW
            | IMPORT_KW
            | PUB_KW
            | WHILE_KW
            | UNTIL_KW
            | LOOP_KW
            | FOR_KW
            | DO_KW
            | RETURN_KW
            | BREAK_KW
            | CONTINUE_KW
    )
}

/// What class a declared name must have.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NameClass {
    /// snake_case: variables, functions, fields.
    Lower,
    /// Capitalized: types, globals, variants, modules.
    Cap,
}

/// `getHealth` → `get_health` (for messages).
pub(crate) fn to_snake(word: &str) -> String {
    let mut out = String::with_capacity(word.len() + 4);
    let mut prev_lower_or_digit = false;
    for c in word.chars() {
        if c.is_ascii_uppercase() {
            if prev_lower_or_digit && !out.ends_with('_') {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
            prev_lower_or_digit = false;
        } else {
            prev_lower_or_digit = c.is_ascii_lowercase() || c.is_ascii_digit();
            out.push(c);
        }
    }
    out
}

/// `get_health` → `GetHealth` (for messages).
pub(crate) fn to_pascal(word: &str) -> String {
    let mut out = String::with_capacity(word.len());
    let mut upper = true;
    for c in word.chars() {
        if c == '_' {
            upper = true;
        } else if upper {
            out.push(c.to_ascii_uppercase());
            upper = false;
        } else {
            out.push(c);
        }
    }
    out
}

/// The message for a name in a style NL does not allow.
pub(crate) fn oname_message(text: &str) -> String {
    format!(
        "`{text}` is not an NL name: NL names are snake_case (`{}`), PascalCase or \
         SCREAMING_SNAKE_CASE. A name in this style is a Lua name, allowed only in a Lua path \
         (`:obj.{text}()`) or as a method name after `:` (`obj:{text}()`); a table key in this \
         style is written as a string (`t[\"{text}\"]`)",
        to_snake(text)
    )
}

impl Parser<'_> {
    /// A declared name of the given class, as a `NAME` node: a name
    /// of the wrong class or style is reported and taken anyway. `what` is
    /// "a struct name", "a variable name", ….
    pub(crate) fn name(&mut self, class: NameClass, what: &str) -> bool {
        let k = self.cur();
        let text = self.nth_text(0);
        match (k, class) {
            (LNAME, NameClass::Lower) | (CNAME, NameClass::Cap) => {}
            (LNAME, NameClass::Cap) => {
                self.error(format!("{what} is capitalized: write `{}`", to_pascal(text)));
            }
            (CNAME, NameClass::Lower) => {
                self.error(format!("{what} is lowercase (snake_case): write `{}`", to_snake(text)));
            }
            (ONAME, _) => {
                let msg = oname_message(text);
                self.error(msg);
            }
            _ if k.is_keyword() && !self.nl() && closer_bit(k) == 0 && !can_start_statement(k) => {
                self.error(format!("`{text}` is a keyword and cannot be {what}"));
            }
            _ => {
                let found = self.describe_cur();
                self.error(format!("expected {what}, found {found}"));
                return false;
            }
        }
        let m = self.start();
        self.bump_word();
        m.complete(self, NAME);
        true
    }

    /// Consumes a word where any word is a name: a keyword gets the
    /// kind of a name in the tree, `LNAME` (`CNAME` for `Self`), so that a
    /// name is never mistaken for a keyword (`x.end`, `{type: 1}`).
    pub(crate) fn bump_word(&mut self) {
        let k = self.cur();
        if k == SELF_TYPE_KW {
            self.bump_as(CNAME);
        } else if k.is_keyword() {
            self.bump_as(LNAME);
        } else {
            self.bump();
        }
    }

    /// `qual_cname` as a `QUAL_NAME`: `Foo`, `Foo.Bar`.
    pub(crate) fn qual_name(&mut self, what: &str) -> bool {
        if !matches!(self.cur(), CNAME | LNAME | ONAME) {
            let found = self.describe_cur();
            self.error(format!("expected {what}, found {found}"));
            return false;
        }
        let m = self.start();
        loop {
            match self.cur() {
                CNAME => {}
                LNAME => {
                    let fixed = to_pascal(self.nth_text(0));
                    self.error(format!("{what} is capitalized: write `{fixed}`"));
                }
                _ => {
                    let msg = oname_message(self.nth_text(0));
                    self.error(msg);
                }
            }
            self.bump();
            if self.at(DOT) && matches!(self.nth(1), CNAME | LNAME | ONAME) {
                self.bump();
                continue;
            }
            break;
        }
        m.complete(self, QUAL_NAME);
        true
    }

    /// The `end` of the innermost construct, which was opened by `what`
    /// (`if`, `fn`, …).
    pub(crate) fn expect_end(&mut self, what: &str) {
        if self.at(END_KW) && !self.hint_end_belongs_outside() {
            self.bump();
            return;
        }
        let opener = self.construct().map_or(self.pos(), |c| c.opener);
        let header_do = self.construct().and_then(|c| c.header_do);
        let line = self.line_of(opener);
        if self.at(EOF) {
            self.unclosed_at_eof.push(opener);
        }
        let msg = if let Some(do_range) = header_do {
            // Lua's `while x do y end` on one line.
            let msg = format!(
                "NL has no `do` after a loop or condition header: in the `{what}` on line {line}, \
                 `do … end` is a block passed to the call before it, so the `{what}` itself has \
                 no body and no `end`. Write the header alone on its line, the body below it, \
                 then `end`"
            );
            let before = self.diag_count();
            self.error_at(do_range, msg);
            if self.diag_count() > before {
                self.missing_end_errors += 1;
            }
            return;
        } else if self.at(EOF) {
            format!("the `{what}` on line {line} has no `end`: the file ends first")
        } else if self.hint_here().is_some() {
            if self.at(END_KW) {
                format!(
                    "the `{what}` on line {line} has no `end`: by its indentation, this `end` \
                     closes an enclosing block"
                )
            } else {
                format!(
                    "the `{what}` on line {line} has no `end`: by its indentation, it ends before \
                     this line"
                )
            }
        } else {
            let found = self.describe_nth(0);
            format!("expected `end` to close the `{what}` on line {line}, found {found}")
        };
        let before = self.diag_count();
        self.error(msg);
        if self.diag_count() > before {
            self.missing_end_errors += 1;
        }
    }

    /// The first token of a header's expression must be on the line of the
    /// keyword, though a line break after the keyword would otherwise be
    /// read past.
    pub(crate) fn header_same_line(&mut self, keyword: &str) {
        // After a character the lexer rejected, that character was the
        // header's expression and has been reported.
        if self.tok(0).nl && !self.at(EOF) && !self.tok(0).after_error {
            self.error(format!(
                "the header of `{keyword}` must be on one line: put what follows `{keyword}` on \
                 the same line as `{keyword}`"
            ));
        }
    }

    /// The end of a header that ends in an expression: a line
    /// break or `;` must follow. The name `then` and a `do` at the end of the
    /// line are what a Lua user writes there: they are unexpected like any
    /// other token, the error carries a hint, and they are dropped.
    pub(crate) fn header_end(&mut self, keyword: &str) {
        const HINT: &str = "Hint: do not write `then` or `do` after a header; put the body on the next line";
        let mut reported = false;
        loop {
            if self.at_word("then") && !self.nl() {
                self.err_bump(format!(
                    "unexpected identifier `then`: the header of `{keyword}` ends at the end of \
                     its line. {HINT}"
                ));
                reported = true;
                continue;
            }
            if self.at(DO_KW) && !self.nl() && self.do_ends_line(1) {
                self.err_bump(format!(
                    "unexpected `do`: the header of `{keyword}` ends at the end of its line. {HINT}"
                ));
                reported = true;
                continue;
            }
            break;
        }
        if self.at(SEMI) {
            while self.eat(SEMI) {}
            return;
        }
        if self.nl() {
            return;
        }
        // A closer directly after the header (`if x end`, `while x end`,
        // `f(if a 1 else 2 end)`) is an error too: the separator
        // is required, as in Ruby, so that a header's command call cannot
        // swallow the body. The closer itself is left for the block.
        if !reported {
            let found = self.describe_cur();
            self.error(format!(
                "unexpected {found}: the header of `{keyword}` ends at the end of its line; put \
                 the body on the next line, or separate it with `;`"
            ));
        }
    }

    /// The current token starts a later line, indented more than the line of
    /// the token at index `opener` (a body under a header, for recovery).
    pub(crate) fn next_line_more_indented(&self, opener: usize) -> bool {
        let t = self.tok(0);
        t.nl && t.kind != EOF && t.col > self.indent_of(opener)
    }

    /// The token `n` ahead ends the line: it is on a later line, `;` or `EOF`.
    pub(crate) fn do_ends_line(&self, n: usize) -> bool {
        self.nth_raw_nl(n) || matches!(self.nth(n), SEMI | EOF)
    }

    /// The current token, at the start of an operand, should not be consumed
    /// by an error node: it belongs to an enclosing construct or begins the
    /// next statement.
    pub(crate) fn at_recovery_point(&self) -> bool {
        let k = self.cur();
        k == EOF
            || k == SEMI
            || k == COMMA
            || self.nl()
            || closer_bit(k) != 0
            || matches!(k, R_PAREN | R_BRACK | R_CURLY | STR_MID | STR_END)
            || is_statement_keyword(k)
    }

    /// Items separated by `,` up to `closer` (not consumed), with a trailing
    /// comma allowed. `item` parses one item. Recovers from a missing comma
    /// and from `;` (an error in a bracket context).
    pub(crate) fn comma_list(&mut self, closer: SyntaxKind, mut item: impl FnMut(&mut Self)) {
        loop {
            if self.at(closer) || self.at(EOF) {
                break;
            }
            let before = self.pos();
            item(self);
            if self.pos() == before {
                if self.at(closer) || self.at_list_stop(closer) {
                    break;
                }
                // Reported once (the item may have reported it already), then
                // skipped up to the next `,` of this list.
                let found = self.describe_cur();
                self.error(format!("unexpected {found}"));
                match self.list_junk_len(closer).filter(|&n| n > 0) {
                    Some(n) => {
                        self.bump_into_error(n);
                        self.eat(COMMA);
                    }
                    None => self.bump_into_error(1),
                }
                continue;
            }
            if self.eat(COMMA) {
                continue;
            }
            if self.at(closer) {
                break;
            }
            if self.at(SEMI) {
                self.err_bump("`;` cannot separate items inside brackets: use `,`");
                continue;
            }
            // A new line in `(…)` or `[…]` that does not continue the list
            // is a missing closer, unless the closer still follows (then, as
            // in a multi-line table, a missing comma).
            let new_line = self.nth_raw_nl(0) && closer != R_CURLY && !self.closer_follows(closer);
            if can_start_expr(self.cur()) && !self.at_list_stop(closer) && !new_line {
                let found = self.describe_cur();
                self.error(format!("expected `,` or `{}`, found {found}", closer.fixed_text().unwrap_or("?")));
                continue;
            }
            // Junk after an item (`f(a, 1 + 2 ? 3, b)`): skip it up to the
            // next `,` of this list, so that the items after it are read.
            if let Some(n) = self.list_junk_len(closer).filter(|&n| n > 0) {
                let found = self.describe_cur();
                self.error(format!("expected `,` or `{}`, found {found}", closer.fixed_text().unwrap_or("?")));
                self.bump_into_error(n);
                self.eat(COMMA);
                continue;
            }
            break;
        }
    }

    /// The number of tokens from the current one up to the next `,` of the
    /// list closed by `closer` (outside nested brackets), or `None` when the
    /// closer, `;`, a keyword or the end of the file comes first.
    fn list_junk_len(&self, closer: SyntaxKind) -> Option<usize> {
        let mut depth = 0u32;
        for i in 0..64 {
            let t = self.toks.get(self.pos + i)?;
            match t.kind {
                COMMA if depth == 0 => return Some(i),
                k if k == closer && depth == 0 => return None,
                L_PAREN | L_BRACK | L_CURLY => depth += 1,
                R_PAREN | R_BRACK | R_CURLY if depth == 0 => return None,
                R_PAREN | R_BRACK | R_CURLY => depth -= 1,
                EOF | SEMI | STR_MID | STR_END => return None,
                k if k.is_keyword() => return None,
                _ => {}
            }
        }
        None
    }

    /// The closer of the list comes later at bracket depth 0, before the end
    /// of the file, a closer keyword, or a line that starts with a statement
    /// keyword (a bounded scan that spends no fuel).
    fn closer_follows(&self, closer: SyntaxKind) -> bool {
        let mut depth = 0u32;
        for i in 0..256 {
            let t = self.peek(i);
            match t.kind {
                k if k == closer && depth == 0 => return true,
                EOF | END_KW | ELSE_KW | ELSEIF_KW | WHEN_KW | CATCH_KW | FINALLY_KW => return false,
                k if i > 0 && t.nl && is_statement_keyword(k) => return false,
                L_PAREN | L_BRACK | L_CURLY => depth += 1,
                R_PAREN | R_BRACK | R_CURLY if depth == 0 => return false,
                R_PAREN | R_BRACK | R_CURLY => depth -= 1,
                _ => {}
            }
        }
        false
    }

    /// Bumps `n` tokens into one `ERROR_NODE`.
    fn bump_into_error(&mut self, n: usize) {
        let m = self.start();
        for _ in 0..n {
            self.bump();
        }
        m.complete(self, ERROR_NODE);
    }

    /// In a list: a token that suggests the list was never closed.
    fn at_list_stop(&self, closer: SyntaxKind) -> bool {
        let k = self.cur();
        k == EOF
            || (closer_bit(k) != 0 && self.closer_accepted(k))
            || (k != closer && self.closes_bracket(k))
            || (self.nth_raw_nl(0) && is_statement_keyword(k))
    }

    /// The closing bracket of a list or group. On a mismatch, reports it and
    /// skips tokens up to the closer unless they look like the rest of the
    /// enclosing construct.
    pub(crate) fn close_bracket(&mut self, closer: SyntaxKind, opener: &str) {
        if self.eat(closer) {
            return;
        }
        let c = closer.fixed_text().unwrap_or("?");
        if self.at_match_operator() {
            self.error(MATCH_OPERATOR);
        } else {
            let found = self.describe_cur();
            self.error(format!("expected `{c}` to close this `{opener}`, found {found}"));
        }
        let m = self.start();
        let mut skipped = false;
        loop {
            if self.at(closer) {
                if skipped {
                    m.complete(self, ERROR_NODE);
                } else {
                    m.abandon(self);
                }
                self.bump();
                return;
            }
            let k = self.cur();
            if k == EOF
                || k == SEMI
                || (closer_bit(k) != 0 && self.closer_accepted(k))
                || self.closes_bracket(k)
                || (self.nth_raw_nl(0) && can_start_statement(k))
            {
                break;
            }
            self.bump();
            skipped = true;
        }
        if skipped {
            m.complete(self, ERROR_NODE);
        } else {
            m.abandon(self);
        }
    }

    /// `=` directly followed by a touching `~`: the Lua/Perl habit `x =~ re`,
    /// which lexes as `=` `~`.
    pub(crate) fn at_match_operator(&self) -> bool {
        self.at(EQ) && self.nth_at(1, TILDE) && self.nth_touches(1)
    }

    /// A capitalized name path (`Foo`, `Self`, `A.B`) followed by `{` on the
    /// same line, starting at the current token.
    pub(crate) fn at_cap_path_then_curly(&self) -> bool {
        if !matches!(self.cur(), CNAME | SELF_TYPE_KW) {
            return false;
        }
        let mut i = 1;
        while self.peek(i).kind == DOT && self.peek(i + 1).kind == CNAME {
            i += 2;
        }
        let t = self.peek(i);
        t.kind == L_CURLY && !(t.nl && self.ctx_top().stmt)
    }

    /// `(` + `)` / `[` + `]` and the like with nothing between.
    pub(crate) fn nth_pair(&self, n: usize, a: SyntaxKind, b: SyntaxKind) -> bool {
        self.nth(n) == a && self.nth(n + 1) == b && self.nth_touches(n + 1)
    }

    /// A `:` after a function header or block parameters starts a return type,
    /// unless it has whitespace before it and touches the word after it: then
    /// it starts a Lua path that begins the body. A warning is
    /// given when such a path is on the header's line.
    pub(crate) fn opt_return_type(&mut self) {
        if !self.at(COLON) {
            return;
        }
        let path_like = !self.nth_touches(0) && self.nth_touches(1) && is_lua_name(self.nth(1));
        if path_like {
            if !self.tok(0).nl {
                let range = self.cur_range().cover(self.tok(1).range);
                let name = self.nth_text(1);
                // The token that ends the header: `)`, the `|` of block
                // parameters, or the function name.
                let before = self
                    .pos
                    .checked_sub(1)
                    .and_then(|i| self.toks.get(i))
                    .and_then(|t| self.src.get(std::ops::Range::<usize>::from(t.range)))
                    .unwrap_or("");
                self.warning_at(
                    range,
                    format!(
                        "this is a Lua path that starts the body; for a return type write the `:` \
                         directly after the header: `…{before}: {name}`"
                    ),
                );
            }
            return;
        }
        let m = self.start();
        self.bump();
        self.type_expected("after `:`");
        m.complete(self, RETURN_TYPE);
    }
}
