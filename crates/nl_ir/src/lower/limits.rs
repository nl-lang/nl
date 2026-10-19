//! LuaJIT's limits: 200 active locals and 60
//! upvalues per function.
//!
//! The lowering keeps its own temporaries scoped to the statement that
//! needs them, so ordinary code stays far from both limits. What a
//! program's own declarations can still reach is handled here, on the
//! finished chunk, for every backend:
//!
//! - the main function's top-level locals (the file-level declarations,
//!   more than about 200 of them), and the upvalues they cost the
//!   functions that use many of them (a function that calls 60 file-level
//!   functions): when either limit is exceeded, every top-level local of
//!   the main function moves into one hidden table, `local file = {}`,
//!   and is read and written as a field of it;
//! - before that, the locals that hold localized Lua paths (localize.rs)
//!   are dropped where they put a function over a limit, and the locals
//!   that cache runtime values for the inline checks
//!   (`Lowerer::rt_cached`) are dropped, which is enough when only they
//!   went over;
//! - anything still over a limit after that (more than about 180 locals
//!   in one function, or more than 56 names of enclosing functions used in
//!   one function) is reported as an error naming the function and the
//!   limit.
//!
//! The budgets leave room for what the Lua backend adds: `__rt` and `__G`
//! are upvalues of every function (and so is the chunk's owner local,
//! `Chunk::owner`, in the functions that use it), and a statement that is
//! not a call is printed as `local _ = …`.

use std::collections::{HashMap, HashSet};

use crate::ir::*;

/// Active locals per function: LuaJIT allows 200, and expressions need
/// registers beside them (250 slots).
pub(crate) const LOCALS_BUDGET: usize = 180;

/// Upvalues per function: LuaJIT allows 60; the Lua backend adds `__rt`
/// and `__G`.
pub(crate) const UPVALUES_BUDGET: usize = 56;

/// A function over a limit: its line, and the message.
pub(crate) struct Overflow {
    pub line: u32,
    pub message: String,
}

/// Fits `chunk` into the limits (see the module documentation). Returns
/// the functions that still do not fit.
///
/// `rt_cache` lists the locals of the main function that hold runtime
/// values for the inline checks (`Lowerer::rt_cached`). They are a
/// speed-up only: before anything else moves, they are dropped and their
/// uses read `rt` again (`__rt.type`), so they never make a file move to
/// the hidden table that would fit without them.
///
/// `localized` lists the locals of the main function that hold localized
/// Lua paths (localize.rs), with the root and field each holds. They are
/// a speed-up too, and go first: a function over its upvalue budget, or
/// the main function over its budget of locals, gets back the ordinary
/// read of as many of the paths it uses as it is over by
/// ([`drop_localized_over`]); when something is still over after the
/// runtime values are dropped, the remaining paths are dropped before the
/// file's locals move.
pub(crate) fn fit(chunk: &mut Chunk, rt_cache: &[(&'static str, LocalId)], localized: &[(LocalId, String, Option<String>)]) -> Vec<Overflow> {
    if over_limits(&chunk.main).is_empty() {
        return Vec::new();
    }
    let mut kept: Vec<(LocalId, Expr)> = localized.iter().map(|(l, r, n)| (*l, super::localize::path_value(r, n.as_deref()))).collect();
    if !kept.is_empty() && drop_localized_over(chunk, &mut kept) && over_limits(&chunk.main).is_empty() {
        return Vec::new();
    }
    if drop_rt_cache(chunk, rt_cache) && over_limits(&chunk.main).is_empty() {
        return Vec::new();
    }
    if !kept.is_empty() {
        let all: HashSet<LocalId> = kept.iter().map(|(l, _)| *l).collect();
        drop_localized(chunk, &mut kept, &all);
        if over_limits(&chunk.main).is_empty() {
            return Vec::new();
        }
    }
    demote_top_level(chunk);
    over_limits(&chunk.main)
}

/// Drops the localized paths that put a function over a budget: for a
/// function with more upvalues than [`UPVALUES_BUDGET`], as many of the
/// localized locals it uses as it has upvalues too many (the ones made
/// last first); for a main function with more active locals than
/// [`LOCALS_BUDGET`], as many localized locals as it is over by. Repeats
/// while that drops something. False when nothing was dropped.
fn drop_localized_over(chunk: &mut Chunk, kept: &mut Vec<(LocalId, Expr)>) -> bool {
    let mut dropped_any = false;
    loop {
        let candidates: Vec<LocalId> = kept.iter().rev().map(|(l, _)| *l).collect();
        let mut drop: HashSet<LocalId> = HashSet::new();
        let mut sets = Vec::new();
        upvalue_sets(&chunk.main, &mut sets);
        for free in sets.iter().filter(|f| f.len() > UPVALUES_BUDGET) {
            let excess = free.len() - UPVALUES_BUDGET;
            drop.extend(candidates.iter().filter(|l| free.contains(l)).take(excess));
        }
        let active = max_active(&chunk.main.body, chunk.main.params.len());
        if active > LOCALS_BUDGET {
            drop.extend(candidates.iter().take(active - LOCALS_BUDGET));
        }
        if drop.is_empty() {
            return dropped_any;
        }
        drop_localized(chunk, kept, &drop);
        dropped_any = true;
        if kept.is_empty() {
            return true;
        }
    }
}

/// The upvalues of every function of `f` (itself included, innermost
/// first); returns those of `f`.
fn upvalue_sets(f: &Function, out: &mut Vec<HashSet<LocalId>>) -> HashSet<LocalId> {
    let mut declared: HashSet<LocalId> = f.params.iter().copied().collect();
    let mut used = HashSet::new();
    let mut nested = Vec::new();
    scan_block(&f.body, &mut declared, &mut used, &mut nested);
    for g in nested {
        used.extend(upvalue_sets(g, out));
    }
    let free: HashSet<LocalId> = used.difference(&declared).copied().collect();
    out.push(free.clone());
    free
}

/// Removes the localized locals `drop` from their declaration (one
/// statement of the main function's top level; localize.rs) and from
/// `kept`, and reads their paths where they were used.
fn drop_localized(chunk: &mut Chunk, kept: &mut Vec<(LocalId, Expr)>, drop: &HashSet<LocalId>) {
    let values: HashMap<LocalId, Expr> = kept.iter().filter(|(l, _)| drop.contains(l)).cloned().collect();
    kept.retain(|(l, _)| !drop.contains(l));
    let stmts = &mut chunk.main.body.stmts;
    if let Some(at) = stmts.iter().position(|s| matches!(&s.kind, StmtKind::Local { locals, .. } if locals.iter().any(|l| drop.contains(l)))) {
        if let Some(Stmt { kind: StmtKind::Local { locals, values: init }, .. }) = stmts.get_mut(at) {
            let pairs: Vec<(LocalId, Expr)> = std::mem::take(locals).into_iter().zip(std::mem::take(init)).filter(|(l, _)| !drop.contains(l)).collect();
            (*locals, *init) = pairs.into_iter().unzip();
            if locals.is_empty() {
                stmts.remove(at);
            }
        }
    }
    let field = |l: &LocalId| values.get(l).cloned();
    rewrite_block(&mut chunk.main.body, &field, false);
}

/// Removes the declaration of the `rt_cache` locals (a statement at the
/// start of the main function, `lift` may have put its table before it)
/// and reads the intrinsic wherever one was used. False when there was
/// nothing to drop.
fn drop_rt_cache(chunk: &mut Chunk, rt_cache: &[(&'static str, LocalId)]) -> bool {
    let cached: Vec<LocalId> = rt_cache.iter().map(|(_, l)| *l).collect();
    let at = chunk.main.body.stmts.iter().position(|s| matches!(&s.kind, StmtKind::Local { locals, .. } if *locals == cached));
    let Some(at) = at.filter(|_| !cached.is_empty()) else { return false };
    chunk.main.body.stmts.remove(at);
    let field = |l: &LocalId| rt_cache.iter().find(|(_, c)| c == l).map(|(n, _)| Expr::Intrinsic(n));
    rewrite_block(&mut chunk.main.body, &field, false);
    true
}

/// The functions of `main` (itself included) that exceed a limit.
fn over_limits(main: &Function) -> Vec<Overflow> {
    let mut out = Vec::new();
    check_function(main, &mut out);
    out
}

/// Checks `f` and the functions inside it; returns the locals `f` uses
/// that it does not declare (its upvalues).
fn check_function(f: &Function, out: &mut Vec<Overflow>) -> HashSet<LocalId> {
    let mut declared: HashSet<LocalId> = f.params.iter().copied().collect();
    let mut used = HashSet::new();
    let mut nested = Vec::new();
    scan_block(&f.body, &mut declared, &mut used, &mut nested);
    for g in nested {
        for l in check_function(g, out) {
            used.insert(l);
        }
    }
    let free: HashSet<LocalId> = used.difference(&declared).copied().collect();
    let name = f.name.clone().unwrap_or_else(|| "a function".to_string());
    let active = max_active(&f.body, f.params.len());
    if active > LOCALS_BUDGET {
        out.push(Overflow {
            line: f.line,
            message: format!("{name} needs {active} local variables at once, more than the {LOCALS_BUDGET} the VM allows in one function (LuaJIT's limit is 200); split it into smaller functions"),
        });
    }
    if free.len() > UPVALUES_BUDGET {
        out.push(Overflow {
            line: f.line,
            message: format!(
                "{name} uses {} variables of enclosing functions, more than the {UPVALUES_BUDGET} the VM allows in one function (LuaJIT's limit is 60 upvalues); pass some of them as arguments or group them in a table",
                free.len()
            ),
        });
    }
    free
}

/// Collects what a block declares and uses, and the functions inside it
/// (which are checked on their own).
fn scan_block<'a>(b: &'a Block, declared: &mut HashSet<LocalId>, used: &mut HashSet<LocalId>, nested: &mut Vec<&'a Function>) {
    for s in &b.stmts {
        match &s.kind {
            StmtKind::Local { locals, values } => {
                declared.extend(locals.iter().copied());
                values.iter().for_each(|v| scan_expr(v, used, nested));
            }
            StmtKind::Assign { targets, values } => {
                for t in targets {
                    match t {
                        Place::Local(l) => {
                            used.insert(*l);
                        }
                        Place::Index(o, k) => {
                            scan_expr(o, used, nested);
                            scan_expr(k, used, nested);
                        }
                        Place::Global(_) => {}
                    }
                }
                values.iter().for_each(|v| scan_expr(v, used, nested));
            }
            StmtKind::Expr(e) => scan_expr(e, used, nested),
            StmtKind::Do(b) => scan_block(b, declared, used, nested),
            StmtKind::If { cond, then, otherwise } => {
                scan_expr(cond, used, nested);
                scan_block(then, declared, used, nested);
                if let Some(o) = otherwise {
                    scan_block(o, declared, used, nested);
                }
            }
            StmtKind::While { cond, body } => {
                scan_expr(cond, used, nested);
                scan_block(body, declared, used, nested);
            }
            StmtKind::NumericFor { var, start, limit, step, body } => {
                declared.insert(*var);
                scan_expr(start, used, nested);
                scan_expr(limit, used, nested);
                if let Some(st) = step {
                    scan_expr(st, used, nested);
                }
                scan_block(body, declared, used, nested);
            }
            StmtKind::GenericFor { vars, iter, body } => {
                declared.extend(vars.iter().copied());
                iter.iter().for_each(|e| scan_expr(e, used, nested));
                scan_block(body, declared, used, nested);
            }
            StmtKind::Return(vals) => vals.iter().for_each(|v| scan_expr(v, used, nested)),
            StmtKind::Hoist { defs, rest } => {
                scan_block(defs, declared, used, nested);
                scan_block(rest, declared, used, nested);
            }
            StmtKind::Break | StmtKind::Goto(_) | StmtKind::Label(_) => {}
        }
    }
}

fn scan_expr<'a>(e: &'a Expr, used: &mut HashSet<LocalId>, nested: &mut Vec<&'a Function>) {
    match e {
        Expr::Local(l) => {
            used.insert(*l);
        }
        Expr::Function(f) => nested.push(f),
        Expr::Index(a, b) | Expr::Binary(_, a, b) => {
            scan_expr(a, used, nested);
            scan_expr(b, used, nested);
        }
        Expr::Call(f, args) => {
            scan_expr(f, used, nested);
            args.iter().for_each(|a| scan_expr(a, used, nested));
        }
        Expr::MethodCall(o, _, args) => {
            scan_expr(o, used, nested);
            args.iter().for_each(|a| scan_expr(a, used, nested));
        }
        Expr::Unary(_, a) | Expr::Paren(a) => scan_expr(a, used, nested),
        Expr::Table(entries) => {
            for en in entries {
                match en {
                    TableEntry::Item(v) => scan_expr(v, used, nested),
                    TableEntry::Pair(k, v) => {
                        scan_expr(k, used, nested);
                        scan_expr(v, used, nested);
                    }
                }
            }
        }
        Expr::MemberCall { receiver, key, args } => {
            scan_expr(receiver, used, nested);
            scan_expr(key, used, nested);
            args.iter().for_each(|a| scan_expr(a, used, nested));
        }
        Expr::MemberApply { func, receiver, args } => {
            scan_expr(func, used, nested);
            scan_expr(receiver, used, nested);
            args.iter().for_each(|a| scan_expr(a, used, nested));
        }
        Expr::AutoGet { receiver, key } => {
            scan_expr(receiver, used, nested);
            scan_expr(key, used, nested);
        }
        Expr::Nil | Expr::Bool(_) | Expr::Number(_) | Expr::Str(_) | Expr::Global(_) | Expr::Vararg | Expr::Intrinsic(_) => {}
    }
}

/// The most locals a block has active at once, `base` being active when it
/// starts. A `for` loop uses three hidden locals besides its variables, and
/// a statement that is not a call one (`local _ = …`, nl_luagen).
pub(crate) fn max_active(b: &Block, base: usize) -> usize {
    let mut active = base;
    let mut max = base;
    for s in &b.stmts {
        match &s.kind {
            StmtKind::Local { locals, .. } => active += locals.len(),
            StmtKind::Expr(e) if !matches!(e, Expr::Call(..) | Expr::MethodCall(..) | Expr::MemberCall { .. } | Expr::MemberApply { .. } | Expr::AutoGet { .. }) => active += 1,
            StmtKind::Do(b) => max = max.max(max_active(b, active)),
            StmtKind::If { then, otherwise, .. } => {
                max = max.max(max_active(then, active));
                if let Some(o) = otherwise {
                    max = max.max(max_active(o, active));
                }
            }
            StmtKind::While { body, .. } => max = max.max(max_active(body, active)),
            StmtKind::NumericFor { body, .. } => max = max.max(max_active(body, active + 4)),
            StmtKind::GenericFor { vars, body, .. } => max = max.max(max_active(body, active + 3 + vars.len())),
            StmtKind::Hoist { defs, rest } => {
                // Printed one after the other, or in segments that are
                // blocks of their own: counting them in sequence is an
                // upper bound.
                let d = max_active(defs, active);
                max = max.max(d);
                max = max.max(max_active(rest, d));
            }
            _ => {}
        }
        max = max.max(active);
    }
    max
}

/// Moves every local the main function declares at its top level into a
/// hidden table.
fn demote_top_level(chunk: &mut Chunk) {
    let mut top: Vec<LocalId> = Vec::new();
    collect_top_level(&chunk.main.body, &mut top);
    if top.is_empty() {
        return;
    }
    let table = new_local(&mut chunk.locals, "file");
    // Each local becomes the field named after it (`file.score`), made
    // unique.
    let mut keys: HashMap<LocalId, String> = HashMap::new();
    let mut taken: HashSet<String> = HashSet::new();
    for l in &top {
        let base = chunk.hint(*l).to_string();
        let mut key = base.clone();
        let mut n = 2;
        while taken.contains(&key) {
            key = format!("{base}#{n}");
            n += 1;
        }
        taken.insert(key.clone());
        keys.insert(*l, key);
    }
    let field = |l: &LocalId| keys.get(l).map(|k| Expr::index(Expr::Local(table), Expr::str(k)));
    rewrite_block(&mut chunk.main.body, &field, true);
    let line = chunk.main.line;
    chunk.main.body.stmts.insert(0, Stmt { line, kind: StmtKind::Local { locals: vec![table], values: vec![Expr::Table(Vec::new())] } });
}

/// The locals declared by the top-level statements of a block (a `Hoist`
/// is part of the same level).
fn collect_top_level(b: &Block, out: &mut Vec<LocalId>) {
    for s in &b.stmts {
        match &s.kind {
            StmtKind::Local { locals, .. } => out.extend(locals.iter().copied()),
            StmtKind::Hoist { defs, rest } => {
                collect_top_level(defs, out);
                collect_top_level(rest, out);
            }
            _ => {}
        }
    }
}

pub(crate) type Field<'a> = dyn Fn(&LocalId) -> Option<Expr> + 'a;

/// Replaces every use of a demoted local by its field; at the top level
/// (`top`), their declarations become assignments. (lift.rs uses it to
/// make a block read the copies of the variables it captured.)
pub(crate) fn rewrite_block(b: &mut Block, field: &Field<'_>, top: bool) {
    let stmts = std::mem::take(&mut b.stmts);
    for mut s in stmts {
        match &mut s.kind {
            StmtKind::Local { locals, values } if top && locals.iter().any(|l| field(l).is_some()) => {
                values.iter_mut().for_each(|v| rewrite_expr(v, field));
                if values.is_empty() {
                    // `local a, b`: a field that was never set is nil.
                    continue;
                }
                let targets = locals.iter().map(|l| field(l).map_or(Place::Local(*l), place_of)).collect();
                s.kind = StmtKind::Assign { targets, values: std::mem::take(values) };
            }
            StmtKind::Local { values, .. } => values.iter_mut().for_each(|v| rewrite_expr(v, field)),
            StmtKind::Assign { targets, values } => {
                for t in targets.iter_mut() {
                    match t {
                        Place::Local(l) => {
                            if let Some(f) = field(l) {
                                *t = place_of(f);
                            }
                        }
                        Place::Index(o, k) => {
                            rewrite_expr(o, field);
                            rewrite_expr(k, field);
                        }
                        Place::Global(_) => {}
                    }
                }
                values.iter_mut().for_each(|v| rewrite_expr(v, field));
            }
            StmtKind::Expr(e) => rewrite_expr(e, field),
            StmtKind::Do(b) => rewrite_block(b, field, false),
            StmtKind::If { cond, then, otherwise } => {
                rewrite_expr(cond, field);
                rewrite_block(then, field, false);
                if let Some(o) = otherwise {
                    rewrite_block(o, field, false);
                }
            }
            StmtKind::While { cond, body } => {
                rewrite_expr(cond, field);
                rewrite_block(body, field, false);
            }
            StmtKind::NumericFor { start, limit, step, body, .. } => {
                rewrite_expr(start, field);
                rewrite_expr(limit, field);
                if let Some(st) = step {
                    rewrite_expr(st, field);
                }
                rewrite_block(body, field, false);
            }
            StmtKind::GenericFor { iter, body, .. } => {
                iter.iter_mut().for_each(|e| rewrite_expr(e, field));
                rewrite_block(body, field, false);
            }
            StmtKind::Return(vals) => vals.iter_mut().for_each(|v| rewrite_expr(v, field)),
            StmtKind::Hoist { defs, rest } => {
                rewrite_block(defs, field, top);
                rewrite_block(rest, field, top);
            }
            StmtKind::Break | StmtKind::Goto(_) | StmtKind::Label(_) => {}
        }
        b.stmts.push(s);
    }
}

fn place_of(e: Expr) -> Place {
    match e {
        Expr::Index(o, k) => Place::Index(*o, *k),
        Expr::Local(l) => Place::Local(l),
        _ => Place::Global(String::new()),
    }
}

fn rewrite_expr(e: &mut Expr, field: &Field<'_>) {
    match e {
        Expr::Local(l) => {
            if let Some(f) = field(l) {
                *e = f;
            }
        }
        Expr::Function(f) => rewrite_block(&mut f.body, field, false),
        Expr::Index(a, b) | Expr::Binary(_, a, b) => {
            rewrite_expr(a, field);
            rewrite_expr(b, field);
        }
        Expr::Call(f, args) => {
            rewrite_expr(f, field);
            args.iter_mut().for_each(|a| rewrite_expr(a, field));
        }
        Expr::MethodCall(o, _, args) => {
            rewrite_expr(o, field);
            args.iter_mut().for_each(|a| rewrite_expr(a, field));
        }
        Expr::Unary(_, a) | Expr::Paren(a) => rewrite_expr(a, field),
        Expr::Table(entries) => {
            for en in entries.iter_mut() {
                match en {
                    TableEntry::Item(v) => rewrite_expr(v, field),
                    TableEntry::Pair(k, v) => {
                        rewrite_expr(k, field);
                        rewrite_expr(v, field);
                    }
                }
            }
        }
        Expr::MemberCall { receiver, key, args } => {
            rewrite_expr(receiver, field);
            rewrite_expr(key, field);
            args.iter_mut().for_each(|a| rewrite_expr(a, field));
        }
        Expr::MemberApply { func, receiver, args } => {
            rewrite_expr(func, field);
            rewrite_expr(receiver, field);
            args.iter_mut().for_each(|a| rewrite_expr(a, field));
        }
        Expr::AutoGet { receiver, key } => {
            rewrite_expr(receiver, field);
            rewrite_expr(key, field);
        }
        Expr::Nil | Expr::Bool(_) | Expr::Number(_) | Expr::Str(_) | Expr::Global(_) | Expr::Vararg | Expr::Intrinsic(_) => {}
    }
}
