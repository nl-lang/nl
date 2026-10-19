//! Lua paths and the restricted path of a pipe
//! target (`pipe_lua_path`).
//!
//! The tree of a path is one flat `LUA_PATH` node: the `:`, the first name,
//! then one node per step (`LUA_FIELD`, `LUA_INDEX`, `LUA_CALL`,
//! `LUA_METHOD`), so that every step's Lua semantics is in its kind.

use super::is_lua_name;
use crate::SyntaxKind::*;
use crate::parser::event::CompletedMarker;
use crate::parser::{LuaLast, Parser, Shape};

impl Parser<'_> {
    /// `lua_path = PATH_COLON lua_name { lua_step }`. The path takes
    /// every step that follows it, in any order, and ends at the first token
    /// that is not a step. Each step is a link of a chain (nesting
    /// limit).
    pub(crate) fn lua_path(&mut self) -> (CompletedMarker, Shape) {
        let start = self.pos();
        let m = self.start();
        self.bump();
        self.bump_word();
        let mut last = LuaLast::Name;
        let mut links = 0;
        loop {
            let k = self.cur();
            let same_line = !self.nl();
            let callee = matches!(last, LuaLast::Name | LuaLast::Field | LuaLast::Method);
            let step = match k {
                // Only a `.name` or a nil-safe step may start a new line.
                DOT | QUESTION_DOT => true,
                L_BRACK | L_PAREN => same_line,
                COLON => self.at_method_colon(),
                DO_KW | ASYNC_KW => self.at_trailing_do(),
                BANG_POSTFIX => true,
                BANG => self.nth_touches(0),
                // After a callee path, `{` starts a table argument.
                L_CURLY => same_line && !callee,
                _ => false,
            };
            if !step || !self.chain_link() {
                break;
            }
            links += 1;
            match k {
                DOT => {
                    self.lua_field();
                    last = LuaLast::Field;
                }
                // A nil-safe step: the `?.` goes into the
                // step's node.
                QUESTION_DOT => {
                    self.safe_chain_starts.insert(start);
                    match self.nth(1) {
                        L_BRACK => {
                            self.lua_index(true);
                            last = LuaLast::Index;
                        }
                        L_PAREN => {
                            if last == LuaLast::Method {
                                self.error(super::postfix::METHOD_SAFE_CALL);
                            }
                            self.lua_call(true);
                            last = LuaLast::Call;
                        }
                        COLON if self.nth_touches(2) && is_lua_name(self.nth(2)) => last = self.lua_method(true),
                        _ => {
                            self.lua_field();
                            last = LuaLast::Field;
                        }
                    }
                }
                L_BRACK => {
                    self.lua_index(false);
                    last = LuaLast::Index;
                }
                L_PAREN => {
                    self.lua_call(false);
                    last = LuaLast::Call;
                }
                COLON => last = self.lua_method(false),
                DO_KW | ASYNC_KW => match last {
                    LuaLast::Name | LuaLast::Field => {
                        // The block is the only argument of a call.
                        let s = self.start();
                        self.trailing_block();
                        s.complete(self, LUA_CALL);
                        last = LuaLast::Call;
                    }
                    _ => {
                        let msg = if last == LuaLast::Index {
                            "a `do … end` block cannot follow an index in a Lua path: it is passed \
                             to a call, so it follows a name, a `.name`, a `:name` or a call"
                        } else {
                            "this call already has a block"
                        };
                        self.error(msg);
                        let e = self.start();
                        self.trailing_block();
                        e.complete(self, ERROR_NODE);
                    }
                },
                L_CURLY => {
                    self.error(
                        "`{` cannot follow this Lua path on the same line: a table is passed as an \
                         argument after a path that names a function (`:f {a: 1}`) or in \
                         parentheses (`:f(x, {a: 1})`); a Lua path never starts a struct literal",
                    );
                    let e = self.start();
                    self.table();
                    e.complete(self, ERROR_NODE);
                }
                _ => {
                    // Postfix `!`: reported, and the path goes on as if
                    // it were not there.
                    self.err_bump("a Lua path is never called automatically, so it takes no `!`");
                }
            }
        }
        self.chain_done(links);
        (m.complete(self, LUA_PATH), Shape::LuaPath { last })
    }

    /// A `[ expr ]` step, after `?.` when `safe`.
    fn lua_index(&mut self, safe: bool) {
        let s = self.start();
        if safe {
            self.bump();
        }
        self.in_bracket(R_BRACK, |p| {
            p.bump();
            if p.at(R_BRACK) {
                p.error("expected an index between `[` and `]`");
            } else {
                p.expr();
            }
            p.close_bracket(R_BRACK, "[");
        });
        s.complete(self, LUA_INDEX);
    }

    /// A call step with its arguments and block, after `?.` when `safe`.
    fn lua_call(&mut self, safe: bool) {
        let s = self.start();
        if safe {
            self.bump();
        }
        self.arg_list();
        if self.at_trailing_do() {
            self.trailing_block();
        }
        s.complete(self, LUA_CALL);
    }

    /// A `. name` (or `?. name`) step: any word or Lua-style name.
    fn lua_field(&mut self) {
        let s = self.start();
        self.bump();
        if is_lua_name(self.cur()) {
            self.bump_word();
        } else {
            let found = self.describe_cur();
            self.error(format!("expected a name after `.` in a Lua path, found {found}"));
        }
        s.complete(self, LUA_FIELD);
    }

    /// A `: name` step with its arguments and block, if any: one method call;
    /// after `?.` when `safe`.
    fn lua_method(&mut self, safe: bool) -> LuaLast {
        let s = self.start();
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
        s.complete(self, LUA_METHOD);
        if args { LuaLast::Call } else { LuaLast::Method }
    }

    /// The Lua path of a pipe target: `pipe_lua_path = PATH_COLON lua_name {
    /// "." lua_name } [ ":" lua_name ]`, then at most one argument
    /// list and a trailing block, all inside the `LUA_PATH`. Steps after that
    /// are left to the pipe, which reports them (`a |> :f().g`).
    pub(crate) fn pipe_lua_path(&mut self) -> (CompletedMarker, Shape) {
        let m = self.start();
        self.bump();
        self.bump_word();
        let mut last = LuaLast::Name;
        let mut links = 0;
        while self.at(DOT) && is_lua_name(self.nth(1)) && self.chain_link() {
            links += 1;
            self.lua_field();
            last = LuaLast::Field;
        }
        if self.at_method_colon() {
            last = self.lua_method(false);
        } else if (self.at(L_PAREN) && !self.nl()) || self.at_trailing_do() {
            let s = self.start();
            if self.at(L_PAREN) {
                self.arg_list();
            }
            if self.at_trailing_do() {
                self.trailing_block();
            }
            s.complete(self, LUA_CALL);
            last = LuaLast::Call;
        }
        self.chain_done(links);
        (m.complete(self, LUA_PATH), Shape::LuaPath { last })
    }
}
