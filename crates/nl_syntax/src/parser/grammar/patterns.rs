//! Patterns, shared by `let`, `const`, parameters, block
//! parameters, `for`, `when` and `if let`.

use super::{NameClass, is_lower_word, is_word, oname_message};
use crate::SyntaxKind::*;
use crate::parser::Parser;
use crate::parser::event::CompletedMarker;

/// Where a pattern stands, as far as the grammar depends on it: a bare
/// `CNAME` binds only as a binding of `const`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PatCtx {
    /// A binding of `const`.
    Const,
    /// Anywhere else.
    Other,
}

/// A parsed pattern; `simple` when it is a name with an optional type (what
/// `let` without `=` and `pub const` accept).
#[derive(Clone, Copy, Debug)]
pub(crate) struct PatInfo {
    pub simple: bool,
}

impl Parser<'_> {
    /// `pattern = pattern_primary [ "as" LNAME ] [ ":" type ]`.
    pub(crate) fn pattern(&mut self, ctx: PatCtx) -> Option<PatInfo> {
        match self.guarded(|p| p.pattern_inner(ctx)) {
            Ok(r) => r,
            Err(skipped) => skipped.map(|_| PatInfo { simple: false }),
        }
    }

    fn pattern_inner(&mut self, ctx: PatCtx) -> Option<PatInfo> {
        let (mut cm, mut simple) = self.pattern_primary(ctx)?;
        if self.at(AS_KW) {
            let m = cm.precede(self);
            self.bump();
            self.name(NameClass::Lower, "the name after `as`");
            cm = m.complete(self, AS_PAT);
            simple = false;
        }
        if self.at(COLON) && !self.nl() {
            let m = cm.precede(self);
            self.bump();
            self.type_expected("after `:`");
            cm = m.complete(self, TYPED_PAT);
            if self.at(AS_KW) {
                self.error("write `as` before the type: `pattern as name: Type`");
                let m = cm.precede(self);
                self.bump();
                self.name(NameClass::Lower, "the name after `as`");
                m.complete(self, AS_PAT);
                simple = false;
            }
        }
        Some(PatInfo { simple })
    }

    /// `pattern_primary`. Reports what is not a pattern; returns `None`
    /// when no token was consumed.
    fn pattern_primary(&mut self, ctx: PatCtx) -> Option<(CompletedMarker, bool)> {
        let k = self.cur();
        match k {
            LNAME | ONAME => {
                let m = self.start();
                self.name(NameClass::Lower, "a name");
                Some((m.complete(self, BIND_PAT), true))
            }
            NUMBER | STRING | TRUE_KW | FALSE_KW | NIL_KW => Some((self.literal_or_range_pattern(), false)),
            MINUS if self.nth_at(1, NUMBER) => Some((self.literal_or_range_pattern(), false)),
            STR_BEGIN => {
                self.error("a string in a pattern is a plain string, without interpolation");
                let m = self.start();
                self.interp_string();
                Some((m.complete(self, LITERAL_PAT), false))
            }
            IS_KW => {
                let m = self.start();
                self.bump();
                self.type_expected("after `is`");
                Some((m.complete(self, IS_PAT), false))
            }
            L_CURLY => Some((self.brace_pattern(), false)),
            L_BRACK => Some((self.bracket_pattern(), false)),
            CNAME | SELF_TYPE_KW => {
                if self.at_cap_path_then_curly() {
                    return Some((self.struct_pattern(), false));
                }
                if k == CNAME && self.nth_at(1, DOT) && self.nth_at(2, CNAME) {
                    // A dotted name, resolved later.
                    let m = self.start();
                    self.qual_name("a name");
                    return Some((m.complete(self, QUALIFIED_PAT), false));
                }
                if k == CNAME && ctx == PatCtx::Const {
                    let m = self.start();
                    let n = self.start();
                    self.bump();
                    n.complete(self, NAME);
                    return Some((m.complete(self, BIND_PAT), true));
                }
                let text = self.nth_text(0).to_string();
                self.error(format!(
                    "`{text}` is not a pattern: to test the type write `is {text}`, to bind a \
                     value of that type write `x: {text}`"
                ));
                let m = self.start();
                self.bump();
                Some((m.complete(self, ERROR_NODE), false))
            }
            STAR => {
                self.error("`*rest` is only allowed as the last item of a `[…]` pattern");
                Some((self.rest_pattern(), false))
            }
            _ => {
                let found = self.describe_cur();
                self.error(format!("expected a pattern, found {found}"));
                None
            }
        }
    }

    /// `literal_pattern` or `range_pattern`.
    fn literal_or_range_pattern(&mut self) -> CompletedMarker {
        let is_number = matches!(self.cur(), NUMBER | MINUS);
        let lit = self.literal_pattern();
        if !matches!(self.cur(), DOT2 | DOT3 | DOT2_LT) {
            return lit;
        }
        if self.at(DOT2_LT) {
            self.error("NL has no `..<`: a range that leaves out its end is written `a...b`");
        }
        if !is_number {
            self.error("a range pattern has numbers as its bounds, such as `1..5`");
        }
        let m = lit.precede(self);
        self.bump();
        if matches!(self.cur(), NUMBER | MINUS) {
            self.literal_pattern();
        } else {
            let found = self.describe_cur();
            self.error(format!("expected a number as the end of the range, found {found}"));
        }
        m.complete(self, RANGE_PAT)
    }

    fn literal_pattern(&mut self) -> CompletedMarker {
        let m = self.start();
        if self.eat(MINUS) && !self.at(NUMBER) {
            self.error("expected a number after `-`");
            return m.complete(self, LITERAL_PAT);
        }
        self.bump();
        m.complete(self, LITERAL_PAT)
    }

    fn rest_pattern(&mut self) -> CompletedMarker {
        let m = self.start();
        self.bump();
        if matches!(self.cur(), LNAME | ONAME) {
            self.name(NameClass::Lower, "a name");
        }
        m.complete(self, REST_PAT)
    }

    /// `brace_pattern`: keyed items (`key: value`, `{name}`). A
    /// positional item is an error that points at `[…]`.
    fn brace_pattern(&mut self) -> CompletedMarker {
        let m = self.start();
        self.in_bracket(R_CURLY, |p| {
            p.bump();
            p.comma_list(R_CURLY, |p| {
                let k = p.cur();
                if (is_word(k) || matches!(k, NUMBER | STRING | ONAME)) && p.at_key_colon() {
                    p.keyed_item(true);
                } else if matches!(k, LNAME | ONAME) && p.at_shorthand_end() {
                    p.shorthand_item();
                } else if k.is_keyword() && matches!(p.nth(1), COMMA | R_CURLY) {
                    let text = p.nth_text(0).to_string();
                    p.error(format!(
                        "`{text}` is a keyword and cannot be bound: write `{{{text}: name}}`"
                    ));
                    let e = p.start();
                    p.bump_word();
                    e.complete(p, ERROR_NODE);
                } else if k == STAR {
                    p.error("`*rest` is not allowed in `{…}`; it is only allowed in a `[…]` pattern");
                    p.rest_pattern();
                } else {
                    let start = p.pos();
                    if p.pattern(PatCtx::Other).is_some() {
                        p.pattern_default();
                        let text = p.text_between(start, p.pos()).to_string();
                        p.error_at(
                            p.range_from(start),
                            format!("a `{{…}}` pattern goes by key (`{{name, key: p}}`); a positional pattern is written with brackets: `[{text}]`"),
                        );
                    }
                }
            });
            p.close_bracket(R_CURLY, "{");
        });
        m.complete(self, BRACE_PAT)
    }

    /// After the key of a keyed item: `:`, or `?` and `:` for an optional
    /// field (`{pos?: {x, y}}`).
    fn at_key_colon(&self) -> bool {
        self.nth_at(1, COLON) || (self.nth_at(1, QUESTION) && self.nth_at(2, COLON))
    }

    /// After a shorthand name (`{name}`, `{name?}`): the end of the item.
    fn at_shorthand_end(&self) -> bool {
        let n = if self.nth_at(1, QUESTION) { 2 } else { 1 };
        matches!(self.nth(n), COMMA | R_CURLY | EQ)
    }

    /// `{name}` or `{name?}` as a `PAT_FIELD`. With `?` the field is
    /// optional: a missing or `nil` field matches, and binds `nil`.
    fn shorthand_item(&mut self) {
        let f = self.start();
        self.name(NameClass::Lower, "a name");
        self.eat(QUESTION);
        self.pattern_default();
        f.complete(self, PAT_FIELD);
    }

    /// `= expr` after an item of a pattern: not part of NL; reported and
    /// read into an `ERROR_NODE`.
    fn pattern_default(&mut self) {
        if self.at(EQ) {
            self.error("defaults inside patterns are not part of NL yet");
            let e = self.start();
            self.bump();
            self.expr();
            e.complete(self, ERROR_NODE);
        }
    }

    /// `pattern_key ":" key_value` as a `PAT_FIELD`; `brace` in a keyed brace
    /// pattern, where `true`, `false` and `nil` are not keys, as
    /// opposed to a struct pattern, where every word is a field name.
    fn keyed_item(&mut self, brace: bool) {
        let f = self.start();
        if self.at(ONAME) {
            let text = self.nth_text(0).to_string();
            self.error(format!("a key in this style is written as a string: `{{\"{text}\": …}}`"));
        }
        if brace && matches!(self.cur(), TRUE_KW | FALSE_KW | NIL_KW) {
            let word = self.nth_text(0).to_string();
            let key_range = self.cur_range();
            let key = self.start();
            self.bump();
            key.complete(self, ERROR_NODE);
            self.bump();
            let value_start = self.pos();
            self.key_value();
            let value = self.text_between(value_start, self.pos());
            let value = if value.is_empty() { "x".to_string() } else { value };
            self.error_at(key_range, format!("`{word}` is not a key name here: write the key as a string, `{{\"{word}\": {value}}}`"));
            self.pattern_default();
            f.complete(self, PAT_FIELD);
            return;
        }
        self.bump_word();
        // `key?: p`: an optional field, which matches when missing.
        self.eat(QUESTION);
        self.bump();
        self.key_value();
        self.pattern_default();
        f.complete(self, PAT_FIELD);
    }

    /// `key_value`: a struct pattern; a type when it starts like
    /// one; a pattern otherwise. A bare dotted name is a `QUALIFIED_PAT`.
    fn key_value(&mut self) {
        let k = self.cur();
        let starts_type = match k {
            CNAME => !self.at_cap_path_then_curly() && !self.at_bare_dotted_name(),
            SELF_TYPE_KW => !self.at_cap_path_then_curly(),
            NIL_KW | TILDE | L_PAREN => true,
            LNAME => matches!(self.nth_text(0), "any" | "unknown" | "never"),
            _ => false,
        };
        if starts_type {
            self.type_expected("after `:`");
        } else {
            // A missing pattern is reported by `pattern`.
            self.pattern(PatCtx::Other);
        }
    }

    /// `CNAME "." CNAME { "." CNAME }` with no `<`, `?`, `|` or `{` after it.
    fn at_bare_dotted_name(&self) -> bool {
        if !(self.at(CNAME) && self.nth_at(1, DOT) && self.nth_at(2, CNAME)) {
            return false;
        }
        let mut i = 3;
        while self.peek(i).kind == DOT && self.peek(i + 1).kind == CNAME {
            i += 2;
        }
        !matches!(self.peek(i).kind, LT | QUESTION | QUESTION2 | PIPE | L_CURLY)
    }

    /// `bracket_pattern = "[" [ bracket_items ] "]"`.
    fn bracket_pattern(&mut self) -> CompletedMarker {
        let m = self.start();
        self.in_bracket(R_BRACK, |p| {
            p.bump();
            let mut rest_seen = false;
            p.comma_list(R_BRACK, |p| {
                if rest_seen {
                    p.error("`*rest` must be the last item of the pattern");
                    rest_seen = false;
                }
                if p.at(STAR) {
                    p.rest_pattern();
                    rest_seen = true;
                } else if p.pattern(PatCtx::Other).is_some() {
                    p.pattern_default();
                }
            });
            p.close_bracket(R_BRACK, "[");
        });
        m.complete(self, BRACKET_PAT)
    }

    /// `struct_pattern = cap_path "{" [ struct_item { "," struct_item } ] "}"`.
    fn struct_pattern(&mut self) -> CompletedMarker {
        let m = self.start();
        let q = self.start();
        self.bump();
        while self.at(DOT) && self.nth_at(1, CNAME) {
            self.bump();
            self.bump();
        }
        q.complete(self, QUAL_NAME);
        self.in_bracket(R_CURLY, |p| {
            p.bump();
            p.comma_list(R_CURLY, |p| {
                let k = p.cur();
                if (is_lower_word(k) || matches!(k, CNAME | ONAME)) && p.at_key_colon() {
                    if k == CNAME {
                        let fixed = super::to_snake(p.nth_text(0));
                        p.error(format!("field names are lowercase: write `{fixed}`"));
                    }
                    p.keyed_item(false);
                } else if matches!(k, LNAME | ONAME) && p.at_shorthand_end() {
                    p.shorthand_item();
                } else if k != R_CURLY {
                    if k == ONAME {
                        let msg = oname_message(p.nth_text(0));
                        p.error(msg);
                    } else {
                        p.error(
                            "the items of a struct pattern are keyed by field name: `field: \
                             pattern`, or a field name",
                        );
                    }
                    let e = p.start();
                    if p.pattern(PatCtx::Other).is_none() {
                        p.bump();
                    }
                    e.complete(p, ERROR_NODE);
                }
            });
            p.close_bracket(R_CURLY, "{");
        });
        m.complete(self, STRUCT_PAT)
    }
}
