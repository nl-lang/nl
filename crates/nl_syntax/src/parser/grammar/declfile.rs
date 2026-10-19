//! Declaration files: `declare` items, their members
//! and signatures. Lexically an NL file; nothing in it has a body.
//!
//! The tree reuses the nodes of NL declarations where the shape is the
//! same (`NAME`, `TYPE_PARAMS`, `PARAM_LIST` with `PARAM`/`REST_PARAM`/
//! `KEYED_PARAM`, `RETURN_TYPE`, `STRUCT_PARENT`, `OP_NAME`, `LUA_PATH`)
//! and adds `DECL_MODULE`, `DECL_STRUCT`, `DECL_TYPE`, `DECL_FN`,
//! `DECL_VALUE`, `DECL_FIELD`, `SELF_PARAM`, `LUA_ROOT` and `LUA_KEY`.

use super::{NameClass, is_word, oname_message};
use crate::SyntaxKind::{self, *};
use crate::parser::event::Marker;
use crate::parser::{Parser, closer};

impl Parser<'_> {
    /// `decl_file = { sep } { "declare" decl { sep } } EOF`.
    pub(crate) fn decl_file(&mut self) {
        let m = self.start();
        loop {
            while self.eat(SEMI) {}
            if self.at(EOF) {
                break;
            }
            if self.at(DECLARE_KW) {
                self.top_level_statement();
                self.declare_item();
                continue;
            }
            let found = self.describe_cur();
            self.error(format!("expected `declare`, found {found}: a declaration file holds `declare` items only, and no statements"));
            let e = self.start();
            self.bump();
            while !self.at(EOF) && !(self.at(DECLARE_KW) && self.nl()) {
                self.bump();
            }
            e.complete(self, ERROR_NODE);
        }
        self.finish_file();
        m.complete(self, SOURCE_FILE);
    }

    /// `"declare" decl`.
    fn declare_item(&mut self) {
        let m = self.start();
        self.bump();
        match self.cur() {
            MODULE_KW => self.decl_container(m, DECL_MODULE),
            STRUCT_KW => self.decl_container(m, DECL_STRUCT),
            TYPE_KW => self.decl_container(m, DECL_TYPE),
            FN_KW => self.decl_fn(m, true),
            LET_KW | CONST_KW => self.decl_value(m, true),
            _ => {
                let found = self.describe_cur();
                self.error(format!("expected `module`, `struct`, `type`, `fn`, `let` or `const` after `declare`, found {found}"));
                m.complete(self, ERROR_NODE);
            }
        }
    }

    /// `decl_module`, `decl_struct`, `decl_type`: a header, members
    /// one per line, `end`.
    fn decl_container(&mut self, m: Marker, kind: SyntaxKind) {
        let keyword = match kind {
            DECL_MODULE => "module",
            DECL_STRUCT => "struct",
            _ => "type",
        };
        self.in_construct(closer::END, |p| {
            p.bump();
            let what = match kind {
                DECL_MODULE => "a module name",
                DECL_STRUCT => "a struct name",
                _ => "a type name",
            };
            p.name(NameClass::Cap, what);
            match kind {
                DECL_TYPE => {
                    if p.at(COLON) {
                        let s = p.start();
                        p.bump();
                        p.type_ref("after `:`", "the parent of a host type");
                        s.complete(p, STRUCT_PARENT);
                    }
                    if p.at(EQ) {
                        p.error("a host type is found by its registered name and takes no `= :path`");
                        let e = p.start();
                        p.opt_lua_root();
                        e.complete(p, ERROR_NODE);
                    }
                }
                _ => p.opt_lua_root(),
            }
            p.member_block(keyword, &[COLON], |p| p.decl_member());
            p.expect_end(keyword);
        });
        m.complete(self, kind);
    }

    /// `[ "=" lua_path ]` as a `LUA_ROOT`.
    fn opt_lua_root(&mut self) {
        if !self.at(EQ) {
            return;
        }
        let r = self.start();
        self.bump();
        if self.at(COLON) && self.nth_touches(1) {
            self.lua_path();
        } else {
            let found = self.describe_cur();
            self.error(format!("expected a Lua path after `=` (`= :name`, `= :require('mod')`), found {found}"));
        }
        r.complete(self, LUA_ROOT);
    }

    /// `[ "as" plain_string ]` as a `LUA_KEY`.
    fn opt_lua_key(&mut self) {
        if !self.at(AS_KW) || self.nl() {
            return;
        }
        let k = self.start();
        self.bump();
        if self.at(STRING) {
            self.bump();
        } else if self.at(STR_BEGIN) {
            self.error("the Lua key after `as` is a plain string, without interpolation");
            self.interp_string();
        } else {
            let found = self.describe_cur();
            self.error(format!("expected a string after `as` (the member's Lua key), found {found}"));
        }
        k.complete(self, LUA_KEY);
    }

    /// `member`: a field, a `const`, an `fn`, or a nested module,
    /// struct or type.
    fn decl_member(&mut self) {
        let k = self.cur();
        match k {
            DECLARE_KW => {
                self.error("a nested declaration is written without `declare`");
                let e = self.start();
                self.bump();
                e.complete(self, ERROR_NODE);
                self.decl_member();
            }
            FN_KW => {
                let m = self.start();
                self.decl_fn(m, false);
            }
            CONST_KW => {
                let m = self.start();
                self.decl_value(m, false);
            }
            LET_KW => {
                self.error("a member cannot be `let`: a field is written `name: Type`, a read-only member `const name: Type`");
                let m = self.start();
                self.decl_value(m, false);
            }
            MODULE_KW => {
                let m = self.start();
                self.decl_container(m, DECL_MODULE);
            }
            STRUCT_KW => {
                let m = self.start();
                self.decl_container(m, DECL_STRUCT);
            }
            TYPE_KW => {
                let m = self.start();
                self.decl_container(m, DECL_TYPE);
            }
            _ if (is_word(k) || k == ONAME) && self.nth_at(1, COLON) && !self.nth_raw_nl(1) => self.decl_field(),
            _ if is_word(k) || k == ONAME => {
                let text = self.nth_text(0).to_string();
                self.error(format!("a field needs a type: `{text}: Type`"));
                let m = self.start();
                let n = self.start();
                self.bump_word();
                n.complete(self, NAME);
                m.complete(self, DECL_FIELD);
            }
            _ => {}
        }
    }

    /// `field_member = word_any ":" type [ lua_key ]`.
    fn decl_field(&mut self) {
        let m = self.start();
        if self.at(ONAME) {
            let msg = oname_message(self.nth_text(0));
            self.error(msg);
        }
        let n = self.start();
        self.bump_word();
        n.complete(self, NAME);
        self.expect(COLON);
        self.type_expected("after `:`");
        self.opt_lua_key();
        m.complete(self, DECL_FIELD);
    }

    /// `decl_fn` (top level) or `fn_member`.
    fn decl_fn(&mut self, m: Marker, top: bool) {
        self.bump();
        if top {
            self.name(NameClass::Lower, "a function name");
        } else {
            // `fn +(right: Point): Point`, `fn [](key: Number): Number`:
            // the operator names of `fn` declarations.
            self.method_name();
        }
        if self.at(LT) && !self.nl() {
            self.type_params();
        }
        if self.at(L_PAREN) && !self.nl() {
            self.sig_params();
        }
        // No body follows, so every `:` is the return type.
        if self.at(COLON) && !self.nl() {
            let r = self.start();
            self.bump();
            self.type_expected("after `:`");
            r.complete(self, RETURN_TYPE);
        }
        if top {
            self.opt_lua_root();
            if self.at(AS_KW) && !self.nl() {
                self.error("a top-level `declare fn` names its Lua value with `= :path`, not `as`");
                let e = self.start();
                self.opt_lua_key();
                e.complete(self, ERROR_NODE);
            }
        } else {
            self.opt_lua_key();
            if self.at(EQ) && !self.nl() {
                self.error("a member names its Lua key with `as 'luaName'`, not `= :path`");
                let e = self.start();
                self.opt_lua_root();
                e.complete(self, ERROR_NODE);
            }
        }
        m.complete(self, DECL_FN);
    }

    /// `sig_params = "(" [ sig_param { "," sig_param } [ "," ] ] ")"`, as a
    /// `PARAM_LIST`.
    fn sig_params(&mut self) {
        let m = self.start();
        self.in_bracket(R_PAREN, |p| {
            p.bump();
            let mut first = true;
            let mut keyed_seen = false;
            p.comma_list(R_PAREN, |p| {
                if keyed_seen {
                    p.error("`**opts` must be the last parameter");
                }
                keyed_seen |= p.sig_param(first);
                first = false;
            });
            p.close_bracket(R_PAREN, "(");
        });
        m.complete(self, PARAM_LIST);
    }

    /// `sig_param`: `self` (first only), `name: type`, `*name:
    /// type` or `**name: type`. Returns true for `**opts`.
    fn sig_param(&mut self, first: bool) -> bool {
        let k = self.cur();
        match k {
            SELF_KW => {
                if !first {
                    self.error("`self` can only be the first parameter");
                }
                let m = self.start();
                self.bump();
                m.complete(self, SELF_PARAM);
                false
            }
            STAR | STAR2 => {
                let keyed = k == STAR2;
                let m = self.start();
                self.bump();
                self.name(NameClass::Lower, if keyed { "the name of the keyed parameter" } else { "the name of the rest parameter" });
                if self.eat(COLON) {
                    self.type_expected("after `:`");
                } else {
                    self.error("a parameter in a declaration is annotated: `*name: Type`");
                }
                m.complete(self, if keyed { KEYED_PARAM } else { REST_PARAM });
                keyed
            }
            _ if is_word(k) || k == ONAME => {
                let m = self.start();
                let t = self.start();
                let b = self.start();
                if k == SELF_TYPE_KW {
                    self.error("`Self` is a type; the receiver parameter is `self`");
                }
                self.name(NameClass::Lower, "a parameter name");
                b.complete(self, BIND_PAT);
                if self.eat(COLON) {
                    self.type_expected("after `:`");
                } else {
                    self.error("a parameter in a declaration is annotated: `name: Type`");
                }
                if self.at(EQ) {
                    self.error("a declaration holds no expression, so a parameter has no default; an optional parameter is typed `T?`");
                    let e = self.start();
                    self.bump();
                    self.expr();
                    e.complete(self, ERROR_NODE);
                }
                t.complete(self, TYPED_PAT);
                m.complete(self, PARAM);
                false
            }
            _ => false,
        }
    }

    /// `decl_value` (top level) or `const_member`.
    fn decl_value(&mut self, m: Marker, top: bool) {
        let is_const = self.at(CONST_KW);
        self.bump();
        if is_const && self.at(CNAME) {
            let n = self.start();
            self.bump();
            n.complete(self, NAME);
        } else {
            self.name(NameClass::Lower, if is_const { "a constant name" } else { "a variable name" });
        }
        if self.eat(COLON) {
            self.type_expected("after `:`");
        } else {
            let what = if is_const { "const" } else { "let" };
            self.error(format!("a declared `{what}` is annotated: `{what} name: Type`"));
        }
        if self.at(EQ) && !self.nl() && !self.nth_at(1, COLON) {
            self.error("a declaration holds no value; `=` names the Lua value with a path: `= :name`");
            let e = self.start();
            self.bump();
            self.expr();
            e.complete(self, ERROR_NODE);
        } else if top {
            self.opt_lua_root();
        } else {
            self.opt_lua_key();
        }
        m.complete(self, DECL_VALUE);
    }
}
