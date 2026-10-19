//! Types.

use super::{is_word, oname_message};
use crate::SyntaxKind::*;
use crate::parser::Parser;
use crate::parser::event::CompletedMarker;

impl Parser<'_> {
    /// `type`, reporting "expected a type {after}" when there is none.
    pub(crate) fn type_expected(&mut self, after: &str) -> Option<CompletedMarker> {
        let t = self.type_();
        if t.is_none() {
            let found = self.describe_cur();
            self.error(format!("expected a type {after}, found {found}"));
        }
        t
    }

    /// `type_ref = qual_cname [ type_args ]`, where a struct parent and an
    /// interface need one: any other type is parsed, then reported. `what` names the
    /// position for the message.
    pub(crate) fn type_ref(&mut self, after: &str, what: &str) {
        let start = self.pos();
        let first = self.cur();
        let Some(t) = self.type_expected(after) else { return };
        if first != CNAME || t.kind(self) != Some(NAMED_TYPE) {
            let range = self.range_from(start);
            self.error_at(
                range,
                format!(
                    "{what} is a type name such as `Foo` or `Box<T>`, not a union, an optional or \
                     another type expression"
                ),
            );
        }
    }

    /// `type = union_type`. Returns `None` (consuming nothing) when no
    /// type starts here.
    pub(crate) fn type_(&mut self) -> Option<CompletedMarker> {
        self.guarded(|p| p.union_type()).unwrap_or_else(|skipped| skipped)
    }

    /// `union_type = prefix_type { "|" prefix_type }`. A `|` may end or start
    /// a line. In block parameters a top-level `|` closes the list
    /// instead.
    fn union_type(&mut self) -> Option<CompletedMarker> {
        let first = self.prefix_type()?;
        if !self.at(PIPE) || self.ctx_top().no_union {
            return Some(first);
        }
        let m = first.precede(self);
        while self.at(PIPE) {
            self.bump();
            if self.prefix_type().is_none() {
                let found = self.describe_cur();
                self.error(format!("expected a type after `|`, found {found}"));
                break;
            }
        }
        Some(m.complete(self, UNION_TYPE))
    }

    /// `prefix_type = "~" prefix_type | postfix_type` (a loop).
    fn prefix_type(&mut self) -> Option<CompletedMarker> {
        let mut nots = Vec::new();
        while self.at(TILDE) && self.chain_link() {
            nots.push(self.start());
            self.bump();
        }
        let inner = self.postfix_type();
        if inner.is_none() && !nots.is_empty() {
            let found = self.describe_cur();
            self.error(format!("expected a type after `~`, found {found}"));
        }
        let links = u32::try_from(nots.len()).unwrap_or(u32::MAX);
        let mut last = inner;
        while let Some(m) = nots.pop() {
            last = Some(m.complete(self, NOT_TYPE));
        }
        self.chain_done(links);
        last
    }

    /// `postfix_type = primary_type [ "?" ]`.
    fn postfix_type(&mut self) -> Option<CompletedMarker> {
        let t = self.primary_type()?;
        if self.at(QUESTION2) && !self.nl() {
            // `T??` lexes as `??`.
            let m = t.precede(self);
            self.err_bump("a type takes one `?`");
            return Some(m.complete(self, OPTIONAL_TYPE));
        }
        if self.at(QUESTION) && !self.nl() {
            let m = t.precede(self);
            self.bump();
            let t = m.complete(self, OPTIONAL_TYPE);
            if matches!(self.cur(), QUESTION | QUESTION2) && !self.nl() {
                self.err_bump("a type takes one `?`");
            }
            return Some(t);
        }
        Some(t)
    }

    /// `primary_type`.
    fn primary_type(&mut self) -> Option<CompletedMarker> {
        let k = self.cur();
        match k {
            CNAME if self.nth_text(0) == "Function" && self.nth_at(1, L_PAREN) && self.nth_touches(1) => {
                Some(self.fn_type())
            }
            CNAME => {
                let m = self.start();
                let q = self.start();
                self.bump();
                while self.at(DOT) && self.nth_at(1, CNAME) {
                    self.bump();
                    self.bump();
                }
                q.complete(self, QUAL_NAME);
                // `<` after a type name always opens type arguments.
                if self.at(LT) && !self.nl() {
                    self.type_args();
                }
                Some(m.complete(self, NAMED_TYPE))
            }
            SELF_TYPE_KW | NIL_KW => {
                let m = self.start();
                self.bump();
                Some(m.complete(self, NAMED_TYPE))
            }
            LNAME | ONAME if !self.nl() => {
                let text = self.nth_text(0).to_string();
                if k == ONAME {
                    self.error(oname_message(&text));
                } else if !matches!(text.as_str(), "any" | "unknown" | "never") {
                    self.error(format!(
                        "`{text}` is not a type: type names are capitalized, and the lowercase \
                         types are only `nil`, `any`, `unknown` and `never`"
                    ));
                }
                let m = self.start();
                self.bump();
                Some(m.complete(self, NAMED_TYPE))
            }
            L_CURLY => Some(if self.nth_at(1, L_BRACK) { self.map_type() } else { self.record_type() }),
            L_BRACK => {
                let m = self.start();
                self.in_bracket(R_BRACK, |p| {
                    p.bump();
                    p.type_expected("inside `[…]`");
                    p.close_bracket(R_BRACK, "[");
                });
                Some(m.complete(self, ARRAY_TYPE))
            }
            L_PAREN => {
                // `(T)` groups; `(A, B, …)` lists several return values.
                // Which one it is, the resolver checks by position.
                let m = self.start();
                let mut count = 0;
                self.in_bracket(R_PAREN, |p| {
                    p.bump();
                    if p.type_expected("inside `(…)`").is_some() {
                        count += 1;
                    }
                    while p.at(COMMA) {
                        p.bump();
                        if p.at(R_PAREN) {
                            break;
                        }
                        if p.type_expected("after `,`").is_some() {
                            count += 1;
                        }
                    }
                    p.close_bracket(R_PAREN, "(");
                });
                Some(m.complete(self, if count > 1 { TUPLE_TYPE } else { PAREN_TYPE }))
            }
            _ => None,
        }
    }

    /// `type_args = "<" type { "," type } [ "," ] ">"`; the closing `>` may
    /// be split off `>>` and the like.
    pub(crate) fn type_args(&mut self) {
        let m = self.start();
        self.in_bracket(GT, |p| {
            p.bump();
            loop {
                if p.type_expected("in the type arguments").is_none() {
                    break;
                }
                if !p.eat(COMMA) {
                    break;
                }
                if matches!(p.cur(), GT | SHR | USHR | GT_EQ | SHR_EQ | USHR_EQ) {
                    break;
                }
            }
            if !p.bump_split_gt() {
                let found = p.describe_cur();
                p.error(format!("expected `>` to close the type arguments, found {found}"));
            }
        });
        m.complete(self, TYPE_ARGS);
    }

    /// `function_type = "Function" fn_signature`.
    fn fn_type(&mut self) -> CompletedMarker {
        let m = self.start();
        self.bump();
        let ps = self.start();
        self.in_bracket(R_PAREN, |p| {
            p.bump();
            p.comma_list(R_PAREN, |p| p.fn_type_param());
            p.close_bracket(R_PAREN, "(");
        });
        ps.complete(self, FN_TYPE_PARAMS);
        // Unlike after an `fn` header, any `:` here is the return type: no
        // body can follow a type.
        if self.at(COLON) && !self.nl() {
            let r = self.start();
            self.bump();
            self.type_expected("after `:`");
            r.complete(self, RETURN_TYPE);
        }
        m.complete(self, FN_TYPE)
    }

    /// `fn_type_param = [ LNAME ":" ] type | "*" LNAME ":" type | "**" LNAME
    /// ":" type`. Lua's `...` is reported.
    fn fn_type_param(&mut self) {
        let m = self.start();
        match self.cur() {
            STAR2 => {
                self.bump();
                self.name(super::NameClass::Lower, "the name of the keyed parameter");
                if self.expect(COLON) {
                    self.type_expected("after `:`");
                }
            }
            DOT3 => {
                self.error(
                    "NL has no varargs parameter `...`: in a `Function` type the remaining \
                     arguments are written `*args: Type`",
                );
                self.bump();
                m.complete(self, ERROR_NODE);
                return;
            }
            STAR => {
                self.bump();
                self.name(super::NameClass::Lower, "the name of the rest parameter");
                if self.expect(COLON) {
                    self.type_expected("after `:`");
                }
            }
            LNAME if self.nth_at(1, COLON) => {
                let n = self.start();
                self.bump();
                n.complete(self, NAME);
                self.bump();
                self.type_expected("after `:`");
            }
            _ => {
                if self.type_().is_none() {
                    m.abandon(self);
                    return;
                }
            }
        }
        m.complete(self, FN_TYPE_PARAM);
    }

    /// `record_type = "{" record_field { "," record_field } [ "," ] "}"`.
    fn record_type(&mut self) -> CompletedMarker {
        let m = self.start();
        self.in_bracket(R_CURLY, |p| {
            p.bump();
            if p.at(R_CURLY) {
                p.error("a record type needs at least one field: `{name: Type}`");
            }
            p.comma_list(R_CURLY, |p| p.record_field());
            p.close_bracket(R_CURLY, "{");
        });
        m.complete(self, RECORD_TYPE)
    }

    /// `record_field = ( word_any | plain_string ) ":" type`.
    fn record_field(&mut self) {
        let k = self.cur();
        if !(is_word(k) || matches!(k, STRING | ONAME | STR_BEGIN)) {
            return;
        }
        let m = self.start();
        match k {
            ONAME => {
                let text = self.nth_text(0).to_string();
                self.error(format!("a field name in this style is written as a string: `\"{text}\": Type`"));
                self.bump();
            }
            STR_BEGIN => {
                self.error("a record field name is a plain string, without interpolation");
                self.interp_string();
            }
            _ => self.bump_word(),
        }
        if self.expect(COLON) {
            self.type_expected("after `:`");
        }
        m.complete(self, RECORD_FIELD);
    }

    /// `map_type = "{" "[" type "]" ":" type "}"`.
    fn map_type(&mut self) -> CompletedMarker {
        let m = self.start();
        self.in_bracket(R_CURLY, |p| {
            p.bump();
            p.in_bracket(R_BRACK, |p| {
                p.bump();
                p.type_expected("for the keys");
                p.close_bracket(R_BRACK, "[");
            });
            if p.expect(COLON) {
                p.type_expected("for the values");
            }
            p.close_bracket(R_CURLY, "{");
        });
        m.complete(self, MAP_TYPE)
    }
}
