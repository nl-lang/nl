//! Call-only parameters: for
//! every `fn` declaration of the file, which of its parameters the function
//! does nothing with but call. A block or `fn` literal passed in such a
//! position is referenced by nothing once the call returns, and nothing
//! can tell one function object from another there, so the lowering may
//! make it once instead of at every call (crates/nl_ir/src/lower/lift.rs).
//! Recorded in [`FnParams::call_only`], one bit per written parameter, and
//! carried to the files that import the function with its export.
//!
//! A parameter `p` of `fn f` is call-only when `f` is not `async` (an
//! `async fn` runs the rest of its body after the call returned) and every
//! use of `p` in `f`, the defaults of later parameters included, is one of:
//!
//! - a call: `p(x)`, `x |> p`, and for a function-kind `p` (one annotated
//!   `Function`) any bare use, which calls it; not `spawn p()`,
//!   which copies `p` to a worker;
//! - the value `p!` (or a variable-kind `p` itself) written as the `k`th
//!   positional argument of a call of a `fn` declaration or an imported
//!   `pub fn` whose `k`th parameter is call-only;
//! - for a variable-kind `p` (an optional callback, `cb: Function?`), a test
//!   of whether it is set: the condition of an `if`, `unless`, `elseif`,
//!   `while`, `until`, `when` or `cond` clause or a modifier, `not p`,
//!   `p == nil`, `p != nil`, the left operand of `and`;
//!
//! and every function literal around the use, up to `f`, is a trailing
//! block or `fn` literal, not `async`, that is itself the `k`th positional
//! argument of such a call to a call-only parameter, so that it runs only
//! while that call does. Anything else (storing `p` in a variable, a table
//! or a field, returning it, comparing it, passing it to a Lua function or
//! a method, using it in a named inner function, an `on` handler or a
//! `spawn` block) keeps a reference that could outlive the call or shows
//! the identity of the value, and the parameter is not call-only.
//!
//! The functions of a file that pass their parameters to each other,
//! recursive ones included, are solved together: every parameter starts
//! call-only and loses the property when a use of it depends on a parameter
//! that is not, until nothing changes. Imported functions are known from
//! their exports.

use std::collections::HashMap;

use nl_syntax::{SyntaxKind, SyntaxNode};

use crate::binding::{BindingId, BindingKind, DeclKind, Ref, Resolution};
use crate::util::*;

use SyntaxKind::*;

/// A use of a parameter depends on the `k`th parameter of the function `g`
/// being call-only.
type Needs = (BindingId, usize);

/// What is known about one written parameter.
struct ParamUses {
    /// A use that is neither a call nor a passing on, or a pattern
    /// parameter: never call-only.
    leak: bool,
    needs: Vec<Needs>,
}

/// Records [`FnParams::call_only`] for every `fn` declaration of the file
/// (see the module documentation).
pub(crate) fn compute(root: &SyntaxNode, res: &mut Resolution) {
    let mut fns: Vec<(BindingId, Vec<ParamUses>)> = Vec::new();
    for decl in root.descendants().filter(|n| n.kind() == FN_DECL && child(n, QUAL_NAME).is_none()) {
        let Some(id) = child(&decl, NAME).and_then(|n| res.decls.get(&n.text_range()).copied()) else { continue };
        let params = written_params(&decl, res);
        let is_async = has_token(&decl, ASYNC_KW);
        let mut uses: Vec<ParamUses> = params.iter().map(|p| ParamUses { leak: p.is_none() || is_async, needs: Vec::new() }).collect();
        let index: HashMap<BindingId, usize> = params.iter().enumerate().filter_map(|(k, p)| p.map(|p| (p, k))).collect();
        if !is_async {
            for n in decl.descendants().filter(|n| n.kind() == NAME_REF) {
                let Some(Ref::Binding(p)) = res.refs.get(&n.text_range()) else { continue };
                let Some(u) = index.get(p).and_then(|k| uses.get_mut(*k)) else { continue };
                if u.leak {
                    continue;
                }
                let kind = res.binding(*p).map_or(BindingKind::Variable, |b| b.kind);
                match classify(&n, &decl, kind, res) {
                    Some(needs) => u.needs.extend(needs),
                    None => u.leak = true,
                }
            }
        }
        fns.push((id, uses));
    }
    let mut masks: HashMap<BindingId, u64> = fns
        .iter()
        .map(|(id, uses)| (*id, uses.iter().enumerate().take(64).filter(|(_, u)| !u.leak).fold(0u64, |m, (k, _)| m | (1 << k))))
        .collect();
    loop {
        let mut changed = false;
        for (id, uses) in &fns {
            let before = masks.get(id).copied().unwrap_or(0);
            let mut mask = before;
            for (k, u) in uses.iter().enumerate().take(64) {
                if mask & (1 << k) != 0 && !u.needs.iter().all(|(g, j)| is_call_only(*g, *j, &masks, res)) {
                    mask &= !(1 << k);
                }
            }
            if mask != before {
                masks.insert(*id, mask);
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    for (id, mask) in masks {
        if let Some(p) = res.bindings.get_mut(id.0 as usize).and_then(|b| b.params.as_mut()) {
            p.call_only = mask;
        }
    }
}

/// Whether the `k`th parameter of `g` is call-only: by the masks being
/// solved for the functions of this file, by its export for an imported one.
fn is_call_only(g: BindingId, k: usize, masks: &HashMap<BindingId, u64>, res: &Resolution) -> bool {
    if k >= 64 {
        return false;
    }
    let mask = match masks.get(&g) {
        Some(m) => *m,
        None => res.binding(g).and_then(|b| b.params).map_or(0, |p| p.call_only),
    };
    mask & (1 << k) != 0
}

/// The bindings of a function's written parameters, in order; `None` for a
/// pattern parameter.
fn written_params(decl: &SyntaxNode, res: &Resolution) -> Vec<Option<BindingId>> {
    let Some(list) = child(decl, PARAM_LIST) else { return Vec::new() };
    children(&list, PARAM)
        .map(|p| {
            let pat = pattern_children(&p).next()?;
            let bind = match pat.kind() {
                BIND_PAT => pat,
                TYPED_PAT => pattern_children(&pat).next().filter(|b| b.kind() == BIND_PAT)?,
                _ => return None,
            };
            child(&bind, NAME).and_then(|n| res.decls.get(&n.text_range()).copied())
        })
        .collect()
}

/// What one use of a parameter of `f` requires: `None` when the use lets
/// the value escape or be compared, otherwise the call-only parameters it
/// depends on.
fn classify(n: &SyntaxNode, f: &SyntaxNode, kind: BindingKind, res: &Resolution) -> Option<Vec<Needs>> {
    let mut needs = Vec::new();
    // The function literals between the use and `f` each run only during
    // a call that holds them in a call-only position.
    for a in n.ancestors().skip(1).take_while(|a| a != f) {
        match a.kind() {
            TRAILING_BLOCK | FN_EXPR | LAMBDA_EXPR => {
                if has_token(&a, ASYNC_KW) {
                    return None;
                }
                needs.push(literal_position(&a, res)?);
            }
            FN_DECL | ON_EXPR | SPAWN_EXPR | INTERFACE_IMPL => return None,
            _ => {}
        }
    }
    let parent = n.parent()?;
    match parent.kind() {
        // `p!`: the function itself, passed on or leaked.
        BANG_EXPR => needs.push(pass_position(&parent, res)?),
        SPAWN_EXPR => return None,
        CALL_EXPR | COMMAND_CALL if expr_child(&parent).as_ref() == Some(n) => {
            if parent.parent().is_some_and(|c| c.kind() == SPAWN_EXPR) {
                return None;
            }
        }
        PIPE_EXPR if expr_children(&parent).nth(1).as_ref() == Some(n) => {}
        // A function-kind name is called wherever its value is used;
        // as an assignment target it is written, not read.
        _ if kind == BindingKind::Function => {}
        _ => match pass_position(n, res) {
            Some(c) => needs.push(c),
            None if is_test(n, &parent) => {}
            None => return None,
        },
    }
    Some(needs)
}

/// The function and parameter index that `arg`, written as a positional
/// argument, is passed to, when the callee is a `fn` declaration or an
/// imported `pub fn` and no spread precedes it.
fn pass_position(arg: &SyntaxNode, res: &Resolution) -> Option<Needs> {
    let list = arg.parent().filter(|l| matches!(l.kind(), ARG_LIST | COMMAND_ARGS))?;
    let call = list.parent()?;
    let g = callee_fn(&call, res)?;
    let mut k = pipe_offset(&call);
    for c in list.children() {
        if &c == arg {
            return Some((g, k));
        }
        match c.kind() {
            SPREAD_ARG | KEYED_ARG | KEYED_SPREAD_ARG => return None,
            kind if is_expr(kind) => k += 1,
            _ => {}
        }
    }
    None
}

/// The function and parameter index a trailing block or `fn` literal is
/// passed to (the block is the final positional argument).
fn literal_position(lit: &SyntaxNode, res: &Resolution) -> Option<Needs> {
    if matches!(lit.kind(), FN_EXPR | LAMBDA_EXPR) {
        return pass_position(lit, res);
    }
    let call = lit.parent().filter(|c| matches!(c.kind(), CALL_EXPR | COMMAND_CALL))?;
    let g = callee_fn(&call, res)?;
    let mut k = pipe_offset(&call);
    if let Some(list) = child(&call, ARG_LIST).or_else(|| child(&call, COMMAND_ARGS)) {
        for c in list.children() {
            match c.kind() {
                SPREAD_ARG => return None,
                kind if is_expr(kind) => k += 1,
                _ => {}
            }
        }
    }
    Some((g, k))
}

/// The `fn` declaration or imported `pub fn` that a call names, when it
/// names one by a bare name (an exact callee: neither can be reassigned)
/// and is not the job of a `spawn`.
fn callee_fn(call: &SyntaxNode, res: &Resolution) -> Option<BindingId> {
    if call.parent().is_some_and(|p| p.kind() == SPAWN_EXPR) {
        return None;
    }
    let callee = expr_child(call).filter(|c| c.kind() == NAME_REF)?;
    let Ref::Binding(g) = res.refs.get(&callee.text_range())? else { return None };
    let b = res.binding(*g)?;
    let is_fn = b.kind == BindingKind::Function && (b.decl == DeclKind::Fn || (b.decl == DeclKind::Import && res.imported.get(g) == Some(&DeclKind::Fn)));
    is_fn.then_some(*g)
}

/// One, when the call is the target of a pipe: the piped
/// value is its first argument.
fn pipe_offset(call: &SyntaxNode) -> usize {
    let is_target = call.parent().is_some_and(|p| p.kind() == PIPE_EXPR && expr_children(&p).nth(1).as_ref() == Some(call));
    usize::from(is_target)
}

/// A use of a variable-kind parameter that only asks whether it is set.
fn is_test(n: &SyntaxNode, parent: &SyntaxNode) -> bool {
    match parent.kind() {
        IF_EXPR | UNLESS_EXPR | ELSEIF_CLAUSE | WHILE_STMT | UNTIL_STMT | MODIFIER | WHEN_GUARD | COND_CLAUSE => expr_child(parent).as_ref() == Some(n),
        PREFIX_EXPR => has_token(parent, NOT_KW),
        BIN_EXPR => match bin_op(parent).map(|t| t.kind()) {
            Some(EQ2 | NEQ) => expr_children(parent).any(|e| e.kind() == LITERAL && has_token(&e, NIL_KW)),
            Some(AND_KW) => expr_child(parent).as_ref() == Some(n),
            _ => false,
        },
        _ => false,
    }
}
