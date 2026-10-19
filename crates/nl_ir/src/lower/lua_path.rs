//! Lua paths: `:name` followed by field, index, call and
//! method steps, all with Lua's semantics. Nothing in a path is
//! auto-called, no `self` is added, and `.len` is an ordinary key.
//!
//! A path whose root (or root and first field) the file localizes starts
//! from the hidden local that holds it (localize.rs,
//! [`Lowerer::lua_path_head`]); the steps after it are the same.

use nl_sema::util::*;
use nl_syntax::{SyntaxKind, SyntaxNode};

use super::Lowerer;
use super::call::{Arg, arg_nodes};
use super::expr::single;
use super::localize::{field_step_name, path_steps};
use crate::ir::*;

use SyntaxKind::*;

impl Lowerer<'_> {
    /// The first name of a path: an NL local in scope, or the Lua global.
    fn lua_path_root(&mut self, node: &SyntaxNode) -> Expr {
        match nl_sema::lua_path_root(node) {
            Some(tok) => {
                let r = self.res.reference(tok.text_range()).cloned();
                self.ref_value(node, r)
            }
            None => self.internal(node, "Lua path without a name"),
        }
    }

    /// Applies one step to the value so far.
    fn lua_step(&mut self, prefix: Expr, step: &SyntaxNode, pre: Vec<Expr>, extra: &[Arg], block: Option<&SyntaxNode>) -> Expr {
        match step.kind() {
            LUA_FIELD => {
                let name = name_token(step).map(|t| t.text().to_string()).unwrap_or_default();
                Expr::index(prefix, Expr::str(&name))
            }
            LUA_INDEX => {
                let mut vals = vec![prefix];
                if let Some(k) = expr_child(step) {
                    self.push_operand(&mut vals, |l| l.expr1(&k));
                }
                let mut it = vals.into_iter();
                let (o, k) = (it.next().unwrap_or(Expr::Nil), it.next().unwrap_or(Expr::Nil));
                Expr::index(o, k)
            }
            LUA_CALL => {
                let mut vals = vec![prefix];
                vals.extend(pre);
                let args = arg_nodes(child(step, ARG_LIST));
                let tb = child(step, TRAILING_BLOCK);
                let mut all: Vec<&Arg> = args.iter().collect();
                all.extend(extra.iter());
                self.push_arg_refs(&mut vals, &all, tb.as_ref().or(block), 1, 0);
                let mut it = vals.into_iter();
                let f = it.next().unwrap_or(Expr::Nil);
                Expr::call(f, it.collect())
            }
            LUA_METHOD => {
                let name = method_step_name(step).unwrap_or_default();
                let mut vals = vec![prefix];
                vals.extend(pre);
                let args = arg_nodes(child(step, ARG_LIST));
                let tb = child(step, TRAILING_BLOCK);
                let mut all: Vec<&Arg> = args.iter().collect();
                all.extend(extra.iter());
                self.push_arg_refs(&mut vals, &all, tb.as_ref().or(block), 1, 0);
                let mut it = vals.into_iter();
                let o = it.next().unwrap_or(Expr::Nil);
                Expr::MethodCall(Box::new(single(o)), name, it.collect())
            }
            _ => self.internal(step, "Lua path step"),
        }
    }


    fn steps(node: &SyntaxNode) -> Vec<SyntaxNode> {
        path_steps(node)
    }

    /// The value of a path's root, or of its root and first step when the
    /// file localizes them (localize.rs), and how many steps that took.
    /// `keep_last`: the last step is not taken (an assignment target).
    fn lua_path_head(&mut self, node: &SyntaxNode, steps: &[SyntaxNode], keep_last: bool) -> (Expr, usize) {
        if !self.localize.is_empty() {
            let global = nl_sema::lua_path_root(node).and_then(|tok| match self.res.reference(tok.text_range()) {
                Some(nl_sema::Ref::LuaGlobal(name)) => Some(name.clone()),
                _ => None,
            });
            if let Some(global) = global {
                let available = if keep_last { steps.len().saturating_sub(1) } else { steps.len() };
                // A nil-safe first step (`:string?.upper`) tests the root
                // before reading the field, so the pair is not taken as one
                // value; the root alone may be (nilsafe.rs).
                if let Some(name) = steps.first().filter(|s| available >= 1 && !is_safe_step(s)).and_then(field_step_name) {
                    if self.localize.has_path(&global, &name) {
                        return (self.localized_path(&global, Some(&name)), 1);
                    }
                }
                if self.localize.has_root(&global) {
                    return (self.localized_path(&global, None), 0);
                }
            }
        }
        (self.lua_path_root(node), 0)
    }

    /// The value a path starts from and the steps still to apply: the
    /// first name (or the hidden local of a localized root or root and
    /// field, localize.rs, [`Self::lua_path_head`]) and the steps after
    /// it, or, under the guard of a `?.` step (nilsafe.rs), the value the
    /// guard tested and the steps from that one on. `keep_last`: the last
    /// step is not taken by a localized head (an assignment target).
    fn lua_path_start(&mut self, node: &SyntaxNode, keep_last: bool) -> (Expr, Vec<SyntaxNode>) {
        let mut steps = Self::steps(node);
        if let Some(i) = steps.iter().rposition(|s| self.preset(s).is_some()) {
            let rest = steps.split_off(i);
            if let Some(v) = rest.first().and_then(|s| self.preset(s)) {
                return (v, rest);
            }
        }
        let (e, from) = self.lua_path_head(node, &steps, keep_last);
        (e, steps.split_off(from))
    }

    /// The value of a path up to `step`, without it: what a `?.` before
    /// `step` tests (nilsafe.rs).
    pub(crate) fn lua_path_before(&mut self, node: &SyntaxNode, step: &SyntaxNode) -> Expr {
        let (mut e, steps) = self.lua_path_start(node, false);
        for s in steps {
            if &s == step {
                break;
            }
            e = self.lua_step(e, &s, Vec::new(), &[], None);
        }
        e
    }

    /// A path whose value is used.
    pub(crate) fn lua_path(&mut self, node: &SyntaxNode) -> Expr {
        let (mut e, steps) = self.lua_path_start(node, false);
        for s in steps {
            e = self.lua_step(e, &s, Vec::new(), &[], None);
        }
        e
    }

    /// A path on a line of its own: a call.
    pub(crate) fn lua_path_statement(&mut self, node: &SyntaxNode) -> Expr {
        let ends_in_call = Self::steps(node).last().is_some_and(|s| matches!(s.kind(), LUA_CALL | LUA_METHOD));
        let e = self.lua_path(node);
        if ends_in_call { e } else { Expr::call(e, Vec::new()) }
    }

    /// A path as the callee of a command call or pipe: the arguments go to
    /// a final `:name` step without arguments, or to a call of the path.
    pub(crate) fn lua_path_call(&mut self, node: &SyntaxNode, pre: Vec<Expr>, args: &[Arg], block: Option<&SyntaxNode>) -> Expr {
        let (mut e, steps) = self.lua_path_start(node, false);
        let last_is_bare_method = steps.last().is_some_and(|s| {
            s.kind() == LUA_METHOD && child(s, ARG_LIST).is_none() && child(s, TRAILING_BLOCK).is_none()
        });
        let n = steps.len();
        for (i, s) in steps.iter().enumerate() {
            if i + 1 == n && last_is_bare_method {
                return self.lua_step(e, s, pre, args, block);
            }
            e = self.lua_step(e, s, Vec::new(), &[], None);
        }
        let mut vals = vec![e];
        vals.extend(pre);
        let refs: Vec<&Arg> = args.iter().collect();
        self.push_arg_refs(&mut vals, &refs, block, 1, 0);
        let mut it = vals.into_iter();
        let f = it.next().unwrap_or(Expr::Nil);
        Expr::call(f, it.collect())
    }

    /// A path as an assignment target: its last step is `.name` or `[k]`.
    pub(crate) fn lua_path_place(&mut self, node: &SyntaxNode) -> Place {
        let (mut e, steps) = self.lua_path_start(node, true);
        let n = steps.len();
        for (i, s) in steps.iter().enumerate() {
            if i + 1 == n {
                return match s.kind() {
                    LUA_FIELD => {
                        let name = name_token(s).map(|t| t.text().to_string()).unwrap_or_default();
                        Place::Index(e, Expr::str(&name))
                    }
                    LUA_INDEX => {
                        let mut vals = vec![e];
                        if let Some(k) = expr_child(s) {
                            self.push_operand(&mut vals, |l| l.expr1(&k));
                        }
                        let mut it = vals.into_iter();
                        Place::Index(it.next().unwrap_or(Expr::Nil), it.next().unwrap_or(Expr::Nil))
                    }
                    _ => {
                        self.internal(node, "Lua path target");
                        Place::Global(String::new())
                    }
                };
            }
            e = self.lua_step(e, s, Vec::new(), &[], None);
        }
        self.internal(node, "Lua path target");
        Place::Global(String::new())
    }
}

/// The name of a `LUA_METHOD` step.
fn method_step_name(step: &SyntaxNode) -> Option<String> {
    let mut after_colon = false;
    for t in tokens(step) {
        if t.kind() == COLON {
            after_colon = true;
        } else if after_colon && (t.kind().is_name() || t.kind().is_keyword()) {
            return Some(t.text().to_string());
        }
    }
    None
}
