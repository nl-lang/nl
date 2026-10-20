//! Declarations.

use super::patterns::PatCtx;
use super::{NameClass, can_start_statement, is_lower_word, is_word, oname_message};
use crate::SyntaxKind;
use crate::SyntaxKind::*;
use crate::parser::event::Marker;
use crate::parser::{Parser, closer, closer_bit};

/// What `param` parsed: an ordinary parameter, `*rest` or `**opts`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParamKind {
    Plain,
    Rest,
    Keyed,
}

impl Parser<'_> {
    /// `declaration`, including `pub_decl`.
    pub(crate) fn declaration(&mut self) {
        let m = self.start();
        match self.cur() {
            PUB_KW => self.pub_decl(m),
            LET_KW => {
                self.bump();
                self.let_rest(m);
            }
            CONST_KW => self.const_decl(m, false),
            GLOBAL_KW => self.global_decl(m),
            STRUCT_KW => self.struct_decl(m),
            INTERFACE_KW => self.interface(m),
            ENUM_KW => self.enum_decl(m),
            TYPE_KW => self.type_decl(m),
            MODULE_KW => self.module_decl(m),
            IMPORT_KW => self.import_decl(m),
            _ => self.fn_decl(m),
        }
    }

    /// `pub_decl`: only at file level or directly in a branch of a
    /// file-level `if` or `unless` (an export that exists only where the
    /// branch runs), and not on `let`, `global`, `on`, an implementation
    /// or a method.
    fn pub_decl(&mut self, m: Marker) {
        let pub_range = self.cur_range();
        if !self.at_file_level() && !self.in_file_level_branch() {
            self.error("`pub` is only allowed at the top level of a file, or directly in a branch of an `if` or `unless` there");
        }
        self.bump();
        match self.cur() {
            FN_KW | ASYNC_KW => {
                let n = if self.at(ASYNC_KW) { 2 } else { 1 };
                if self.nth_at(n, CNAME) && self.nth_at(n + 1, DOT) {
                    self.error_at(
                        pub_range,
                        "`pub` cannot be used on a method: a method is exported with its type",
                    );
                }
                self.fn_decl(m);
            }
            CONST_KW => self.const_decl(m, true),
            STRUCT_KW => self.struct_decl(m),
            INTERFACE_KW => {
                if self.interface_is_impl() {
                    self.error_at(
                        pub_range,
                        "`pub` cannot be used on an implementation `interface … for …`: it is \
                         exported with the types",
                    );
                }
                self.interface(m);
            }
            ENUM_KW => self.enum_decl(m),
            TYPE_KW => self.type_decl(m),
            MODULE_KW => self.module_decl(m),
            _ => {
                let found = self.describe_cur();
                self.error_at(
                    pub_range,
                    format!(
                        "`pub` cannot be used on {found}: it applies to `fn`, `const`, `struct`, \
                         `interface`, `enum`, `type` and `module` declarations"
                    ),
                );
                m.complete(self, ERROR_NODE);
                if can_start_statement(self.cur()) && !self.nl() {
                    self.statement();
                }
            }
        }
    }

    // ---- Functions ----------------------------------------------------------

    /// `fn_decl`; `m` was started before `pub`, if any.
    pub(crate) fn fn_decl(&mut self, m: Marker) {
        self.in_construct(closer::END, |p| {
            p.eat(ASYNC_KW);
            p.expect(FN_KW);
            p.fn_decl_header_and_body();
        });
        m.complete(self, FN_DECL);
    }

    /// For `function f(…)`: the declaration after the (wrong) keyword.
    pub(crate) fn fn_decl_after_keyword(&mut self, m: Marker) {
        self.in_construct(closer::END, |p| p.fn_decl_header_and_body());
        m.complete(self, FN_DECL);
    }

    fn fn_decl_header_and_body(&mut self) {
        let mut operator = false;
        match self.cur() {
            CNAME if self.nth_at(1, DOT) => {
                let q = self.start();
                self.bump();
                while self.at(DOT) && self.nth_at(1, CNAME) && self.nth_at(2, DOT) {
                    self.bump();
                    self.bump();
                }
                q.complete(self, QUAL_NAME);
                self.expect(DOT);
                operator = self.method_name();
            }
            _ => {
                self.name(NameClass::Lower, "a function name");
            }
        }
        if self.at(LT) && !self.nl() {
            if operator {
                self.error("an operator method takes no type parameters");
            }
            self.type_params();
        }
        if self.at(L_PAREN) && !self.nl() {
            self.param_list();
        }
        self.opt_return_type();
        self.block();
        self.expect_end("fn");
    }

    /// `method_name = word_any | op_name`. Returns true for an
    /// operator name.
    pub(crate) fn method_name(&mut self) -> bool {
        let k = self.cur();
        if is_word(k) || k == ONAME {
            if k == ONAME {
                let msg = oname_message(self.nth_text(0));
                self.error(msg);
            }
            let m = self.start();
            self.bump_word();
            m.complete(self, NAME);
            return false;
        }
        let m = self.start();
        match k {
            PLUS | MINUS | STAR | SLASH | PERCENT | STAR2 | SLASH2 | CONCAT | PIPE | AMP | CARET
            | SHL | SHR | USHR | EQ2 | LT | LT_EQ | GT | GT_EQ | TILDE => self.bump(),
            L_BRACK if self.nth_pair(0, L_BRACK, R_BRACK) => {
                self.bump();
                self.bump();
                if self.at(EQ) && self.nth_touches(0) {
                    self.bump();
                }
            }
            L_PAREN if self.nth_pair(0, L_PAREN, R_PAREN) => {
                self.bump();
                self.bump();
            }
            NEQ => {
                self.error("`!=` cannot be defined: it is always the negation of `==`");
                self.bump();
            }
            BANG | BANG_POSTFIX => {
                self.error("`!` cannot be defined: it is the logical not of every value");
                self.bump();
            }
            _ => {
                let found = self.describe_cur();
                self.error(format!(
                    "expected a method name or an operator after `.`, found {found}"
                ));
                m.abandon(self);
                return false;
            }
        }
        m.complete(self, OP_NAME);
        true
    }

    /// `type_params = "<" CNAME { "," CNAME } [ "," ] ">"`.
    pub(crate) fn type_params(&mut self) {
        let m = self.start();
        self.in_bracket(GT, |p| {
            p.bump();
            loop {
                if !matches!(p.cur(), CNAME | LNAME | ONAME) {
                    break;
                }
                let tp = p.start();
                p.name(NameClass::Cap, "a type parameter");
                if p.at(COLON) {
                    p.error("generic constraints such as `<T: Shape>` are not part of NL yet");
                    let e = p.start();
                    p.bump();
                    p.type_expected("after `:`");
                    e.complete(p, ERROR_NODE);
                }
                tp.complete(p, TYPE_PARAM);
                if p.at(LT) {
                    // `interface From<Box<T>>` without `for`.
                    p.error(
                        "a type parameter is a single capitalized name such as `T`; type arguments \
                         such as `Box<T>` belong to an implementation `interface … for Type`",
                    );
                    let e = p.start();
                    p.type_args();
                    e.complete(p, ERROR_NODE);
                }
                if !p.eat(COMMA) {
                    break;
                }
            }
            if !p.bump_split_gt() {
                let found = p.describe_cur();
                p.error(format!("expected `>` to close the type parameters, found {found}"));
            }
        });
        m.complete(self, TYPE_PARAMS);
    }

    // ---- Parameters ---------------------------------------------------------

    /// `params = "(" [ param { "," param } [ "," ] ] ")"`.
    pub(crate) fn param_list(&mut self) {
        let m = self.start();
        self.in_bracket(R_PAREN, |p| {
            p.bump();
            let mut rest_seen = false;
            let mut keyed_seen = false;
            p.comma_list(R_PAREN, |p| {
                if keyed_seen {
                    p.error("`**opts` must be the last parameter");
                    keyed_seen = false;
                    rest_seen = false;
                } else if rest_seen && !p.at_keyed_param() {
                    p.error("`*rest` must be the last parameter, or followed only by `**opts`");
                    rest_seen = false;
                }
                match p.param(false) {
                    ParamKind::Plain => {}
                    ParamKind::Rest => rest_seen = true,
                    ParamKind::Keyed => keyed_seen = true,
                }
            });
            p.close_bracket(R_PAREN, "(");
        });
        m.complete(self, PARAM_LIST);
    }

    /// `**`: the keyed parameter or the keyed spread.
    pub(crate) fn at_keyed_param(&self) -> bool {
        self.at(STAR2)
    }

    /// `param`; in block parameters (`block`), a default is a
    /// `bxor_expr`. Lua's `...` is reported and counts as a rest
    /// parameter.
    pub(crate) fn param(&mut self, block: bool) -> ParamKind {
        let m = self.start();
        match self.cur() {
            STAR | STAR2 => {
                let keyed = self.at_keyed_param();
                self.bump();
                if keyed {
                    self.name(NameClass::Lower, "the name of the keyed parameter");
                } else {
                    self.name(NameClass::Lower, "the name of the rest parameter");
                }
                if self.at(COLON) {
                    self.bump();
                    self.type_expected("after `:`");
                }
                if keyed {
                    m.complete(self, KEYED_PARAM);
                    ParamKind::Keyed
                } else {
                    m.complete(self, REST_PARAM);
                    ParamKind::Rest
                }
            }
            DOT3 => {
                self.error(
                    "NL has no varargs parameter `...`: write `*args` as the last parameter; it \
                     collects the remaining arguments into the table `args`",
                );
                self.bump();
                m.complete(self, ERROR_NODE);
                ParamKind::Rest
            }
            _ => {
                if self.pattern(PatCtx::Other).is_none() {
                    m.abandon(self);
                    return ParamKind::Plain;
                }
                if self.eat(EQ) {
                    let value = if block { self.expr_min(7) } else { self.expr() };
                    if value.is_none() {
                        self.error("expected a default value after `=`");
                    }
                }
                m.complete(self, PARAM);
                ParamKind::Plain
            }
        }
    }

    // ---- Variables ----------------------------------------------------------

    /// `let_decl` after `let`.
    pub(crate) fn let_rest(&mut self, m: Marker) {
        let mut all_simple = true;
        let start = self.pos();
        while let Some(info) = self.pattern(PatCtx::Other) {
            all_simple &= info.simple;
            if self.nl() || !self.eat(COMMA) {
                break;
            }
        }
        if self.eat(EQ) {
            self.expr_list();
        } else if !all_simple {
            // At the patterns, not at whatever follows on the next line.
            let range = self.range_from(start);
            self.error_at(
                range,
                "a destructuring pattern needs a value: write `let {a, b} = value`; without `=`, \
                 `let` declares plain names",
            );
        }
        m.complete(self, LET_DECL);
    }

    /// `const_decl`.
    fn const_decl(&mut self, m: Marker, public: bool) {
        self.bump();
        let first = self.pos();
        loop {
            let start = self.pos();
            match self.pattern(PatCtx::Const) {
                Some(info) => {
                    if public && !info.simple {
                        let range = self.range_from(start);
                        self.error_at(range, "`pub const` takes names only, not patterns");
                    }
                }
                None => break,
            }
            if self.nl() || !self.eat(COMMA) {
                break;
            }
        }
        if self.eat(EQ) {
            self.expr_list();
        } else if self.nl() || self.at(EOF) {
            // At the bindings, not at whatever follows on the next line.
            let range = self.range_from(first);
            self.error_at(range, "a `const` needs a value: write `= value` after the name");
        } else {
            let found = self.describe_cur();
            self.error(format!("expected `=` and a value: a `const` needs a value, found {found}"));
        }
        m.complete(self, CONST_DECL);
    }

    /// `global_decl = "global" CNAME [ ":" type ] [ "=" cmd_expr ]`, or
    /// `"global"` before an `fn`, `module` or `struct` declaration: an item
    /// that is exported, as with `pub`, and also stored in the Lua global
    /// of its name when the file runs. It stands where `pub` may.
    fn global_decl(&mut self, m: Marker) {
        if self.nth_at(1, FN_KW) || self.nth_at(1, ASYNC_KW) || self.nth_at(1, MODULE_KW) || self.nth_at(1, STRUCT_KW) {
            let global_range = self.cur_range();
            if !self.at_file_level() && !self.in_file_level_branch() {
                self.error("`global fn`, `global module` and `global struct` are only allowed at the top level of a file, or directly in a branch of an `if` or `unless` there");
            }
            self.bump();
            match self.cur() {
                FN_KW | ASYNC_KW => {
                    let n = if self.at(ASYNC_KW) { 2 } else { 1 };
                    if self.nth_at(n, CNAME) && self.nth_at(n + 1, DOT) {
                        self.error_at(global_range, "`global` cannot be used on a method: a method is reached through its type, which can be `global`");
                    }
                    self.fn_decl(m);
                }
                MODULE_KW => self.module_decl(m),
                _ => self.struct_decl(m),
            }
            return;
        }
        self.bump();
        self.name(NameClass::Cap, "a global's name");
        if self.eat(COLON) {
            self.type_expected("after `:`");
        }
        if self.eat(EQ) && self.cmd_expr().is_none() {
            self.error("expected a value after `=`");
        }
        m.complete(self, GLOBAL_DECL);
    }

    // ---- Structs, interfaces, enums -----------------------------------------

    /// `struct_decl`.
    fn struct_decl(&mut self, m: Marker) {
        self.in_construct(closer::END, |p| {
            p.bump();
            p.name(NameClass::Cap, "a struct name");
            if p.at(LT) && !p.nl() {
                p.type_params();
            }
            if p.at(COLON) {
                let s = p.start();
                p.bump();
                p.type_ref("after `:`", "the parent of a struct");
                s.complete(p, STRUCT_PARENT);
            }
            p.member_block("struct", &[COLON, EQ], |p| p.field_decl());
            p.expect_end("struct");
        });
        m.complete(self, STRUCT_DECL);
    }

    /// The body of `struct`, `interface` or `enum`: one member per line,
    /// after a header that ends at the line break.
    ///
    /// A keyword that starts a member line and is followed on that line by one
    /// of `field_after` is a field name, even a closer: `end = 0` on its
    /// own line in a struct declares the field `end`. On the header's line
    /// `end` always closes (`struct Foo end`).
    pub(crate) fn member_block(&mut self, keyword: &str, field_after: &[SyntaxKind], mut member: impl FnMut(&mut Self)) {
        if !self.at(END_KW) {
            self.header_end(keyword);
        }
        loop {
            while self.eat(SEMI) {}
            let after_sep = self.nl() || self.pos.checked_sub(1).and_then(|i| self.toks.get(i)).is_some_and(|t| t.kind == SEMI);
            let keyword_field = after_sep
                && self.cur().is_keyword()
                && field_after.contains(&self.nth(1))
                && !self.nth_raw_nl(1);
            if self.at_block_end() && !keyword_field {
                break;
            }
            let k = self.cur();
            if closer_bit(k) != 0 && !keyword_field {
                let text = self.nth_text(0).to_string();
                self.err_bump(format!("unexpected `{text}` in the body of `{keyword}`"));
                continue;
            }
            if self.hint_closes_construct() {
                break;
            }
            let before = self.pos();
            member(self);
            if self.pos() == before {
                // One error for the rest of the line, not one per token.
                let found = self.describe_nth(0);
                self.error(format!("unexpected {found} in the body of `{keyword}`"));
                let e = self.start();
                self.bump();
                while !(self.at(EOF) || self.at(SEMI) || self.nl() || self.at_block_end()) {
                    self.bump();
                }
                e.complete(self, ERROR_NODE);
                continue;
            }
            if self.at(SEMI) || self.nl() || self.at_block_end() || closer_bit(self.cur()) != 0 {
                continue;
            }
            if self.at(COMMA) {
                self.err_bump(format!(
                    "the members of `{keyword}` are written one per line, not separated by `,`"
                ));
                continue;
            }
            let found = self.describe_cur();
            self.error(format!(
                "unexpected {found}: the members of `{keyword}` are written one per line"
            ));
            let e = self.start();
            while !(self.at(EOF) || self.at(SEMI) || self.nl() || self.at_block_end()) {
                self.bump();
            }
            e.complete(self, ERROR_NODE);
        }
    }

    /// `field_decl`. A keyword is a field name only when `:` or `=`
    /// follows it on the same line.
    fn field_decl(&mut self) {
        let k = self.cur();
        let typed_or_default = matches!(self.nth(1), COLON | EQ) && !self.nth_raw_nl(1);
        if (is_lower_word(k) || matches!(k, CNAME | ONAME)) && typed_or_default || matches!(k, LNAME | ONAME) {
            let m = self.start();
            if k == CNAME {
                self.name(NameClass::Lower, "a field name");
            } else {
                if k == ONAME {
                    let msg = oname_message(self.nth_text(0));
                    self.error(msg);
                }
                let n = self.start();
                self.bump_word();
                n.complete(self, NAME);
            }
            if self.eat(COLON) {
                self.type_expected("after `:`");
            }
            if self.eat(EQ) && self.expr().is_none() {
                self.error("expected a default value after `=`");
            }
            m.complete(self, FIELD_DECL);
            return;
        }
        if k.is_keyword() && (self.nth_raw_nl(1) || matches!(self.nth(1), SEMI | EOF)) {
            // A keyword is a field name only with `:` or `=`.
            let text = self.nth_text(0).to_string();
            self.error(format!(
                "`{text}` is a keyword: a field named by a keyword needs a type or a default, as in \
                 `{text}: String`"
            ));
            let e = self.start();
            self.bump_word();
            e.complete(self, ERROR_NODE);
            return;
        }
        if matches!(k, FN_KW | ASYNC_KW) {
            self.error(
                "a struct body holds fields only: declare methods after the struct, as `fn \
                 Name.method … end`",
            );
        } else if can_start_statement(k) {
            self.error("a struct body holds fields only, one `name: Type` per line");
        } else {
            return;
        }
        let e = self.start();
        self.statement();
        e.complete(self, ERROR_NODE);
    }

    /// `interface_decl` or `interface_impl`.
    fn interface(&mut self, m: Marker) {
        if self.interface_is_impl() {
            self.in_construct(closer::END, |p| {
                p.bump();
                p.type_ref("after `interface`", "the interface of an implementation");
                p.expect(FOR_KW);
                p.type_ref("after `for`", "the type of an implementation");
                p.member_block("interface", &[], |p| p.impl_member());
                p.expect_end("interface");
            });
            m.complete(self, INTERFACE_IMPL);
        } else {
            self.in_construct(closer::END, |p| {
                p.bump();
                p.name(NameClass::Cap, "an interface name");
                if p.at(LT) && !p.nl() {
                    p.type_params();
                }
                p.member_block("interface", &[COLON], |p| p.iface_member());
                p.expect_end("interface");
            });
            m.complete(self, INTERFACE_DECL);
        }
    }

    /// After `interface` and a type reference with its `<…>`, the word `for`
    /// on the same line makes an implementation.
    pub(crate) fn interface_is_impl(&self) -> bool {
        let mut depth: i32 = 0;
        for i in 1..400 {
            let Some(t) = self.toks.get(self.pos + i) else { return false };
            if t.kind == EOF || (depth <= 0 && i > 1 && (t.nl || t.kind == SEMI)) {
                return false;
            }
            match t.kind {
                LT | L_PAREN | L_BRACK | L_CURLY => depth += 1,
                GT | R_PAREN | R_BRACK | R_CURLY | GT_EQ => depth -= 1,
                SHR | SHR_EQ => depth -= 2,
                USHR | USHR_EQ => depth -= 3,
                FOR_KW if depth <= 0 => return true,
                _ => {}
            }
        }
        false
    }

    /// `iface_member`: a signature or a field requirement.
    fn iface_member(&mut self) {
        let k = self.cur();
        let is_method = (k == FN_KW && !self.nth_at(1, COLON)) || (k == ASYNC_KW && self.nth_at(1, FN_KW));
        if is_method {
            let opener = self.pos();
            let m = self.start();
            self.eat(ASYNC_KW);
            self.bump();
            self.name(NameClass::Lower, "a method name");
            if self.at(LT) && !self.nl() {
                self.type_params();
            }
            if self.at(L_PAREN) && !self.nl() {
                self.param_list();
            }
            // No body follows, so every `:` is the return type.
            if self.at(COLON) {
                let r = self.start();
                self.bump();
                self.type_expected("after `:`");
                r.complete(self, RETURN_TYPE);
            }
            m.complete(self, IFACE_METHOD);
            let body_follows = if self.nl() {
                self.next_line_more_indented(opener)
            } else {
                !(self.at(SEMI) || self.at_block_end())
            };
            if body_follows && can_start_statement(self.cur()) {
                self.error(
                    "a method in an interface declaration has no body: implement it in \
                     `interface Name for Type … end`",
                );
                let e = self.start();
                self.block();
                self.eat(END_KW);
                e.complete(self, ERROR_NODE);
            }
            return;
        }
        if (is_lower_word(k) || matches!(k, CNAME | ONAME)) && self.nth_at(1, COLON) {
            let m = self.start();
            match k {
                CNAME => {
                    self.name(NameClass::Lower, "a field name");
                }
                _ => {
                    if k == ONAME {
                        let msg = oname_message(self.nth_text(0));
                        self.error(msg);
                    }
                    let n = self.start();
                    self.bump_word();
                    n.complete(self, NAME);
                }
            }
            self.bump();
            self.type_expected("after `:`");
            m.complete(self, IFACE_FIELD);
            return;
        }
        if k == LNAME {
            self.error("a field requirement needs a type: `name: Type`");
            let m = self.start();
            let n = self.start();
            self.bump();
            n.complete(self, NAME);
            m.complete(self, IFACE_FIELD);
            return;
        }
        if can_start_statement(k) {
            self.error(
                "an interface declaration holds method signatures (`fn name: Type`) and field \
                 requirements (`name: Type`), one per line",
            );
            let e = self.start();
            self.statement();
            e.complete(self, ERROR_NODE);
        }
    }

    /// `impl_member`: a method with a body.
    fn impl_member(&mut self) {
        let k = self.cur();
        if k == FN_KW || (k == ASYNC_KW && self.nth_at(1, FN_KW)) {
            let m = self.start();
            self.fn_decl(m);
            return;
        }
        if can_start_statement(k) || k.is_keyword() {
            self.error(
                "an implementation `interface … for …` holds method definitions only (`fn name … \
                 end`)",
            );
            let e = self.start();
            if (is_lower_word(k) || k == CNAME) && self.nth_at(1, COLON) {
                // A field requirement: fields belong in the struct.
                self.bump_word();
                self.bump();
                self.type_expected("after `:`");
            } else {
                self.statement();
            }
            e.complete(self, ERROR_NODE);
        }
    }

    /// `enum_decl`.
    fn enum_decl(&mut self, m: Marker) {
        self.in_construct(closer::END, |p| {
            p.bump();
            p.name(NameClass::Cap, "an enum name");
            if p.at(LT) && !p.nl() {
                p.error("an enum takes no type parameters");
                p.type_params();
            }
            p.member_block("enum", &[], |p| p.variant());
            p.expect_end("enum");
        });
        m.complete(self, ENUM_DECL);
    }

    /// `variant = CNAME [ "=" enum_value ]`.
    fn variant(&mut self) {
        let k = self.cur();
        if !matches!(k, CNAME | LNAME | ONAME) {
            if can_start_statement(k) {
                self.error("an enum body holds variants only, one `Name` or `Name = value` per line");
                let e = self.start();
                self.statement();
                e.complete(self, ERROR_NODE);
            }
            return;
        }
        let m = self.start();
        self.name(NameClass::Cap, "a variant name");
        if self.eat(EQ) {
            match self.cur() {
                MINUS if self.nth_at(1, NUMBER) => {
                    let e = self.start();
                    self.bump();
                    let l = self.start();
                    self.bump();
                    l.complete(self, LITERAL);
                    e.complete(self, PREFIX_EXPR);
                }
                NUMBER | STRING => {
                    let l = self.start();
                    self.bump();
                    l.complete(self, LITERAL);
                }
                STR_BEGIN => {
                    self.error("an enum value is a plain string, without interpolation");
                    self.interp_string();
                }
                _ => {
                    let found = self.describe_cur();
                    self.error(format!("expected a number or a string as the variant's value, found {found}"));
                }
            }
        }
        m.complete(self, VARIANT);
    }

    // ---- Type aliases, modules, imports -------------------------------------

    /// `type_decl = "type" CNAME [ type_params ] "=" type`.
    fn type_decl(&mut self, m: Marker) {
        self.bump();
        self.name(NameClass::Cap, "a type name");
        if self.at(LT) && !self.nl() {
            self.type_params();
        }
        if self.expect(EQ) {
            self.type_expected("after `=`");
        }
        m.complete(self, TYPE_DECL);
    }

    /// `module_decl = "module" qual_cname [ sep block ] "end"`.
    fn module_decl(&mut self, m: Marker) {
        self.in_construct(closer::END, |p| {
            p.bump();
            p.qual_name("a module name");
            if !p.at(END_KW) {
                p.header_end("module");
                p.block();
            }
            p.expect_end("module");
        });
        m.complete(self, MODULE_DECL);
    }

    /// `import_decl`: the three forms of `import`. Imports are the
    /// first statements of the file.
    fn import_decl(&mut self, m: Marker) {
        if !self.at_file_level() {
            self.error(
                "`import` is only allowed at the top of a file, before every other \
                 statement; to load a file while the program runs, call `require` or `include`",
            );
        } else if self.past_imports && !self.import_order_reported {
            self.import_order_reported = true;
            self.error(
                "`import` must come before every other statement of the file: move the \
                 imports to the top",
            );
        }
        self.bump();
        if self.at_word("from") {
            // `from` directly after `import` is always the whole-file form.
            self.bump_as(FROM_KW);
            self.import_path();
            if self.expect(AS_KW) {
                self.name(NameClass::Cap, "the name of the imported file");
            }
        } else if self.at(L_CURLY) {
            let l = self.start();
            self.in_bracket(R_CURLY, |p| {
                p.bump();
                p.comma_list(R_CURLY, |p| p.import_name());
                p.close_bracket(R_CURLY, "{");
            });
            l.complete(self, IMPORT_LIST);
            self.import_from();
        } else if matches!(self.cur(), LNAME | CNAME | ONAME) {
            self.import_name();
            self.import_from();
        } else {
            let found = self.describe_cur();
            self.error(format!(
                "expected what to import, found {found}: `import name from 'path'`, `import {{ a, \
                 B }} from 'path'` or `import from 'path' as Name`"
            ));
        }
        m.complete(self, IMPORT_DECL);
    }

    fn import_name(&mut self) {
        match self.cur() {
            LNAME | CNAME | ONAME => {
                if self.at(ONAME) {
                    let msg = oname_message(self.nth_text(0));
                    self.error(msg);
                }
                let n = self.start();
                self.bump();
                n.complete(self, NAME);
                if self.at(AS_KW) {
                    self.error("names cannot be renamed inside `import { … }`");
                    let e = self.start();
                    self.bump();
                    if matches!(self.cur(), LNAME | CNAME | ONAME) {
                        self.bump();
                    }
                    e.complete(self, ERROR_NODE);
                }
            }
            _ => {
                let found = self.describe_cur();
                self.error(format!("expected a name to import, found {found}"));
            }
        }
    }

    fn import_from(&mut self) {
        if self.at_word("from") {
            self.bump_as(FROM_KW);
            self.import_path();
        } else {
            let found = self.describe_cur();
            self.error(format!("expected `from` and the path of the file, found {found}"));
        }
    }

    fn import_path(&mut self) {
        match self.cur() {
            STRING => self.bump(),
            STR_BEGIN => {
                self.error("an import path is a plain string, without interpolation");
                self.interp_string();
            }
            _ => {
                let found = self.describe_cur();
                self.error(format!("expected the path of the file to import, as a string, found {found}"));
            }
        }
    }
}
