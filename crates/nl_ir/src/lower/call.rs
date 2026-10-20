//! Calls: plain calls, command calls, member calls under the
//! member-call rule, Lua method calls `x:m()`, the pipe and trailing
//! blocks.

use nl_sema::util::*;
use nl_sema::{BindingKind, DeclKind, MemberHint, Ref};
use nl_syntax::{SyntaxKind, SyntaxNode};

use super::Lowerer;
use super::expr::{is_len, single, table_fn};
use crate::ir::*;

use SyntaxKind::*;

/// One written argument.
pub(crate) enum Arg {
    Expr(SyntaxNode),
    /// `*list`: the elements of a table as the final arguments.
    Spread(SyntaxNode),
    /// `key: value`.
    Keyed(String, SyntaxNode),
    /// `**table`.
    KeyedSpread(SyntaxNode),
}

/// The arguments of an `ARG_LIST` or `COMMAND_ARGS`.
pub(crate) fn arg_nodes(list: Option<SyntaxNode>) -> Vec<Arg> {
    let Some(list) = list else { return Vec::new() };
    list.children()
        .filter_map(|c| match c.kind() {
            SPREAD_ARG => expr_child(&c).map(Arg::Spread),
            KEYED_ARG => {
                let key = first_token(&c).map(|t| t.text().to_string()).unwrap_or_default();
                expr_child(&c).map(|e| Arg::Keyed(key, e))
            }
            KEYED_SPREAD_ARG => expr_child(&c).map(Arg::KeyedSpread),
            k if is_expr(k) => Some(Arg::Expr(c)),
            _ => None,
        })
        .collect()
}

impl Lowerer<'_> {
    /// `CALL_EXPR`, `COMMAND_CALL` or `METHOD_CALL`.
    pub(crate) fn call(&mut self, node: &SyntaxNode) -> Expr {
        match node.kind() {
            METHOD_CALL => self.method_call(node, Vec::new(), &[], None),
            CALL_EXPR | COMMAND_CALL => {
                let Some(callee) = expr_child(node) else { return self.internal(node, "call without callee") };
                let list = child(node, ARG_LIST).or_else(|| child(node, COMMAND_ARGS));
                let args = arg_nodes(list);
                let block = child(node, TRAILING_BLOCK);
                self.call_target(&callee, Vec::new(), &args, block.as_ref())
            }
            _ => self.internal(node, "call"),
        }
    }

    /// A call of `callee` with the already lowered values `pre` (the piped
    /// value) followed by `args` and the trailing block.
    pub(crate) fn call_target(&mut self, callee: &SyntaxNode, pre: Vec<Expr>, args: &[Arg], block: Option<&SyntaxNode>) -> Expr {
        match callee.kind() {
            NAME_REF => {
                let mut vals = Vec::new();
                // `f?.(…)`: the function its guard tested (nilsafe.rs).
                let f = match self.preset(callee) {
                    Some(f) => f,
                    None => self.name_raw(callee),
                };
                vals.push(f);
                if let Some(m) = self.module_self_of(callee) {
                    vals.push(m);
                }
                // The piped value is the first written argument.
                let call_only = u32::try_from(pre.len()).ok().and_then(|n| self.call_only_mask(callee).checked_shr(n)).unwrap_or(0);
                vals.extend(pre);
                self.push_args(&mut vals, args, block, call_only);
                let mut it = vals.into_iter();
                let f = it.next().unwrap_or(Expr::Nil);
                Expr::call(f, it.collect())
            }
            FIELD_EXPR if !is_len(callee) => self.member_call_of(callee, pre, args, block),
            METHOD_CALL if child(callee, ARG_LIST).is_none() && child(callee, TRAILING_BLOCK).is_none() => {
                self.method_call(callee, pre, args, block)
            }
            LUA_PATH => self.lua_path_call(callee, pre, args, block),
            AT_EXPR => {
                let f = match self.preset(callee) {
                    Some(f) => f,
                    None => self.at_expr(callee),
                };
                let mut vals = vec![f];
                vals.extend(pre);
                self.push_args(&mut vals, args, block, 0);
                let mut it = vals.into_iter();
                let f = it.next().unwrap_or(Expr::Nil);
                Expr::call(f, it.collect())
            }
            _ => {
                let mut vals = Vec::new();
                self.push_operand(&mut vals, |l| l.expr1(callee));
                vals.extend(pre);
                self.push_args(&mut vals, args, block, 0);
                let mut it = vals.into_iter();
                let f = it.next().unwrap_or(Expr::Nil);
                Expr::call(f, it.collect())
            }
        }
    }

    /// The parameters that the function a bare name calls only calls
    /// (`FnParams::call_only`, one bit per written parameter), when the
    /// name is a `fn` declaration or an imported `pub fn`: an exact callee,
    /// since neither can be reassigned. Zero otherwise.
    fn call_only_mask(&self, callee: &SyntaxNode) -> u64 {
        let Some(Ref::Binding(id)) = self.res.reference(callee.text_range()) else { return 0 };
        let Some(b) = self.res.binding(*id) else { return 0 };
        let exact = b.decl == DeclKind::Fn || (b.decl == DeclKind::Import && self.res.declared_as(*id) == Some(DeclKind::Fn));
        if b.kind == BindingKind::Function && exact { b.params.map_or(0, |p| p.call_only) } else { 0 }
    }

    /// Lowers written arguments and the trailing block onto `vals`.
    /// `call_only` has a bit for each written position whose parameter
    /// the callee only calls (see [`Self::push_arg_refs`]).
    pub(crate) fn push_args(&mut self, vals: &mut Vec<Expr>, args: &[Arg], block: Option<&SyntaxNode>, call_only: u64) {
        let refs: Vec<&Arg> = args.iter().collect();
        self.push_arg_refs(vals, &refs, block, 0, call_only);
    }

    /// Lowers the arguments `args` and the trailing block onto `vals`, whose
    /// first `keep` values (the callee of a Lua path) may stay multi-valued.
    /// The positional arguments come first, then the block, then the keyed
    /// arguments as one marked table; a spread that is
    /// followed only by the keyed table keeps all its values through
    /// `rt.unpackkw`.
    ///
    /// A block or `fn` literal written at a position whose bit is set in
    /// `call_only` is marked transient ([`Function::transient`]): the callee
    /// only calls it, so lift.rs may make it once. After a spread the
    /// positions are unknown.
    pub(crate) fn push_arg_refs(&mut self, vals: &mut Vec<Expr>, args: &[&Arg], block: Option<&SyntaxNode>, keep: usize, call_only: u64) {
        let mut spread_at = None;
        // The spread is a rest parameter, whose count is its `n`.
        let mut counted = None;
        let mut call_only = call_only;
        let mut pos = 0u32;
        let mark = |vals: &mut [Expr], call_only: u64, pos: u32| {
            if call_only.checked_shr(pos).is_some_and(|m| m & 1 != 0) {
                if let Some(Expr::Function(f)) = vals.last_mut() {
                    f.transient = true;
                }
            }
        };
        for a in args {
            match a {
                Arg::Expr(e) => {
                    self.push_operand(vals, |l| l.expr(e));
                    mark(vals, call_only, pos);
                    pos += 1;
                }
                Arg::Spread(e) => {
                    spread_at = Some(vals.len());
                    counted = self.counted_rest(e);
                    self.push_operand(vals, |l| l.expr1(e));
                    call_only = 0;
                }
                Arg::Keyed(..) | Arg::KeyedSpread(_) => {}
            }
        }
        // The keyed values, evaluated in their written order after the
        // positional ones.
        let mut pairs = Vec::new();
        let mut sources = Vec::new();
        for a in args {
            match a {
                Arg::Keyed(k, e) => {
                    let mut v = Vec::new();
                    self.push_operand(&mut v, |l| l.expr1(e));
                    pairs.push(TableEntry::Pair(Expr::str(k), v.pop().unwrap_or(Expr::Nil)));
                }
                Arg::KeyedSpread(e) => {
                    let mut v = Vec::new();
                    self.push_operand(&mut v, |l| l.expr1(e));
                    sources.push(v.pop().unwrap_or(Expr::Nil));
                }
                _ => {}
            }
        }
        let mut keyed = if pairs.is_empty() && sources.is_empty() {
            None
        } else {
            let mut kargs = vec![Expr::Table(pairs)];
            kargs.extend(sources);
            Some(Expr::intrinsic_call("keyed", kargs))
        };
        if let Some(b) = block {
            self.push_operand(vals, |l| l.block_fn(b));
            mark(vals, call_only, pos);
        }
        if let Some(i) = spread_at {
            let last = i + 1 == vals.len();
            if let Some(slot) = vals.get_mut(i) {
                let list = std::mem::replace(slot, Expr::Nil);
                *slot = match keyed.take() {
                    // The keyed table rides along with the spread values.
                    Some(kw) if last => match counted {
                        // A rest parameter whose count is a local.
                        Some((c, n)) if matches!(list, Expr::Local(l) if l == c) => Expr::intrinsic_call("unpackkw", vec![list, kw, n]),
                        _ => Expr::intrinsic_call("unpackkw", vec![list, kw]),
                    },
                    other => {
                        keyed = other;
                        match (counted, &list) {
                            (Some((c, n)), Expr::Local(l)) if *l == c => Expr::intrinsic_call("unpack", vec![list, Expr::Number(1.0), n]),
                            _ => Expr::intrinsic_call("spread", vec![list]),
                        }
                    }
                };
            }
        }
        if let Some(kw) = keyed {
            vals.push(kw);
        }
        // Only the last value of an argument list may expand.
        let n = vals.len();
        for (j, v) in vals.iter_mut().enumerate() {
            if j >= keep && j + 1 < n && v.is_multi() {
                let old = std::mem::replace(v, Expr::Nil);
                *v = single(old);
            }
        }
    }

    /// The module to pass as `self` when `name` is called by its bare name
    /// inside its module.
    fn module_self_of(&mut self, name: &SyntaxNode) -> Option<Expr> {
        let Some(Ref::Binding(id)) = self.res.reference(name.text_range()).cloned() else { return None };
        let m = self.res.binding(id)?.module_owner?;
        Some(Expr::Local(self.local_of(m)))
    }

    /// A call of a name with lowered arguments: a function-kind name used as
    /// a value, or any name in statement position.
    pub(crate) fn call_name(&mut self, node: &SyntaxNode, args: Vec<Expr>) -> Expr {
        let f = self.name_raw(node);
        let mut all = Vec::new();
        if let Some(m) = self.module_self_of(node) {
            all.push(m);
        }
        all.extend(args);
        Expr::call(f, all)
    }

    /// `x.m(args)`: the member-call rule, decided at runtime.
    pub(crate) fn member_call_of(&mut self, field: &SyntaxNode, pre: Vec<Expr>, args: &[Arg], block: Option<&SyntaxNode>) -> Expr {
        let Some(recv) = expr_child(field) else { return self.internal(field, "member call") };
        let name = field_name(field).map(|t| t.text().to_string()).unwrap_or_default();
        let mut vals = Vec::new();
        self.push_operand(&mut vals, |l| l.expr1(&recv));
        vals.extend(pre);
        self.push_args(&mut vals, args, block, 0);
        let mut it = vals.into_iter();
        let r = it.next().unwrap_or(Expr::Nil);
        // `String.shout(s)`: a member of a primitive type's library, named
        // directly, is called as a plain function, also when it is a
        // method; `s` is its `self`.
        if self.res.plain_primitive_member(&recv, &name).is_some() {
            let f = match self.preset(field) {
                Some(f) => f,
                None => {
                    let key = self.opts.hints.member(field.text_range()).map_or(name, |h| h.key().to_string());
                    Expr::index(r, Expr::str(&key))
                }
            };
            return Expr::call(f, it.collect());
        }
        // `x.m?.(args)` (nilsafe.rs): the guard holds the member it tested,
        // which is called, not read again.
        if let Some(f) = self.preset(field) {
            return match self.opts.hints.member(field.text_range()) {
                Some(MemberHint::Method { .. } | MemberHint::TableFn { .. }) => Expr::call(f, std::iter::once(r).chain(it).collect()),
                Some(MemberHint::Field { .. } | MemberHint::Function { .. }) => Expr::call(f, it.collect()),
                None => Expr::MemberApply { func: Box::new(f), receiver: Box::new(r), args: it.collect() },
            };
        }
        // A member the analyzer resolved from the receiver's declared
        // type, or a declared member with a Lua key.
        match self.opts.hints.member(field.text_range()).cloned() {
            Some(MemberHint::Field { key } | MemberHint::Function { key }) => Expr::call(Expr::index(r, Expr::str(&key)), it.collect()),
            Some(MemberHint::Method { key }) => Expr::MethodCall(Box::new(r), key, it.collect()),
            Some(MemberHint::TableFn { key }) => Expr::call(table_fn(&key), std::iter::once(r).chain(it).collect()),
            None => Expr::MemberCall { receiver: Box::new(r), key: Box::new(Expr::str(&name)), args: it.collect() },
        }
    }

    /// `x:m(args)`: Lua's method call. `pre` and `extra` come
    /// from a pipe or command arguments around a method call without its
    /// own argument list.
    pub(crate) fn method_call(&mut self, node: &SyntaxNode, pre: Vec<Expr>, extra: &[Arg], extra_block: Option<&SyntaxNode>) -> Expr {
        let Some(recv) = expr_child(node) else { return self.internal(node, "method call") };
        let name = method_name(node).unwrap_or_default();
        let own = arg_nodes(child(node, ARG_LIST).or_else(|| child(node, COMMAND_ARGS)));
        let own_block = child(node, TRAILING_BLOCK);
        let mut vals = Vec::new();
        self.push_operand(&mut vals, |l| l.expr1(&recv));
        vals.extend(pre);
        let mut args: Vec<&Arg> = own.iter().collect();
        args.extend(extra.iter());
        self.push_arg_refs(&mut vals, &args, own_block.as_ref().or(extra_block), 0, 0);
        let mut it = vals.into_iter();
        let r = it.next().unwrap_or(Expr::Nil);
        Expr::MethodCall(Box::new(r), name, it.collect())
    }

    /// `a |> target`: the left value becomes the first
    /// argument; the target is never auto-called first.
    pub(crate) fn pipe(&mut self, node: &SyntaxNode) -> Expr {
        let parts: Vec<SyntaxNode> = expr_children(node).collect();
        let (Some(lhs), Some(target)) = (parts.first(), parts.get(1)) else { return self.internal(node, "pipe") };
        let value = self.expr1(lhs);
        match target.kind() {
            CALL_EXPR => {
                let Some(callee) = expr_child(target) else { return self.internal(target, "pipe target") };
                let args = arg_nodes(child(target, ARG_LIST));
                let block = child(target, TRAILING_BLOCK);
                self.call_target(&callee, vec![value], &args, block.as_ref())
            }
            METHOD_CALL => self.method_call(target, vec![value], &[], None),
            _ => self.call_target(target, vec![value], &[], None),
        }
    }
}

/// The method name of a `METHOD_CALL`: the word after its `:`.
pub(crate) fn method_name(node: &SyntaxNode) -> Option<String> {
    let mut after_colon = false;
    for t in tokens(node) {
        if t.kind() == COLON {
            after_colon = true;
        } else if after_colon && (t.kind().is_name() || t.kind().is_keyword()) {
            return Some(t.text().to_string());
        }
    }
    None
}
