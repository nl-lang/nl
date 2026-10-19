//! Lifting the closures of `try` out of the code that runs them
//! (bench/README.md, "Opcode decisions"), on the finished chunk.
//!
//! error.rs lowers a `try` to protected calls of closures written where
//! the `try` stands: `rt.xpcall(function() … end, rt.handler)`, or
//! `rt.try(body, catch, finally)` when the outcome is packed. Made there,
//! the closures are created every time the `try` runs, and LuaJIT 2.1
//! cannot compile closure creation (`FNEW`), so a loop that contains a
//! `try` never gets a trace. This pass makes each closure once, in a
//! place that runs less often, and passes it what it reads as arguments:
//!
//! ```lua
//! try_fns[1] = function(i) … end      -- once, in the main function
//! …
//! local ok, code, v = __rt.xpcall(try_fns[1], __rt.handler, i)
//! ```
//!
//! A variable `x` that a closure uses and does not declare is *passed*
//! (copied into a parameter at the call) when it is not a top-level local
//! of the main function, no function other than the one that declares it
//! assigns it, and either it is never assigned at all, or it belongs to
//! the function that runs the `try` (which waits for the call, so nothing
//! changes `x` meanwhile) and no function inside the closure uses it (such
//! a function could outlive the call and must see later assignments). The
//! parameter has the id of `x` (the IR declares it a second time, which
//! both backends allow as shadowing), so the closure's uses need no
//! change and error messages name the variable as before. Every other
//! variable is *kept*: the closure still captures it, so it can only be
//! made where `x` is in scope.
//!
//! Where a closure is made:
//!
//! - nothing kept but top-level locals of the main function: in the main
//!   function, before its top-level statement that contains the `try`, as
//!   an entry of one table (`try_fns`), so that the closures of a file
//!   cost one local and one upvalue;
//! - otherwise: right after the latest declaration of a kept variable (at
//!   the start of the function or loop body when that is a parameter or
//!   loop variable), in a local of its own for the first [`OWN_LOCALS`]
//!   closures of the function that declares it, then as an entry of that
//!   function's table (made when the function starts). That runs once per
//!   scope of that variable, never more often than the `try`.
//!
//! When the place is a block whose statement containing the `try` is a
//! [`StmtKind::Hoist`], the closure is assigned first among its
//! definitions instead (a local is declared before it), which runs before
//! anything the `try` is in, and lets the Lua backend print the closure
//! where the `try` is. The closures of one `rt.try` are lifted together
//! and take the same arguments.
//!
//! Lifting keeps within the budgets of limits.rs: a local it adds (a
//! closure's own local, a table) costs the function at most one more
//! active local at its peak, and a function between the one that holds the
//! closure and the one running the `try` uses it as one more upvalue. A
//! closure goes to the table when a local of its own would not fit, and
//! stays where it is when the table would not fit either (the main
//! function's table is exempt from the budget of locals: limits.rs moves
//! the main function's top-level locals into a table when they are too
//! many). A closure also stays where it is when a kept variable is
//! declared in a part of a `Hoist` or not found.
//!
//! The statements and closures keep their lines: a lifted closure starts
//! on the line of its `try`, `catch` or `finally`, as a trace shows it.
//!
//! # `spawn do` blocks
//!
//! The hidden function of a `spawn do` block (spawn.rs) is made in the
//! main function's table the same way, before the top-level statement
//! that holds the `spawn` (first among the definitions of a `Hoist`), and
//! registered there under its name, so that it exists, registered, once
//! the file has loaded: a worker that loads the file to run
//! the block finds it by name. It reads its captures as parameters and
//! everything else as top-level locals of the main function, so nothing
//! constrains where it is made.
//!
//! # Blocks
//!
//! The same pass makes a block or `fn` literal once when it is written
//! where its callee only calls it ([`Function::transient`], set by call.rs
//! from the call-only parameters of `nl_sema`). Such a
//! block is referenced by nothing once the call returns and nothing can
//! compare it, so one function object serves every evaluation, as the
//! closure of a `try` does. A block cannot be passed what it reads, since
//! the callee calls it with arguments of its own; a variable it captures
//! that is declared where the block runs more than once (a loop variable,
//! a local of the loop body, a parameter of an enclosing block) is instead
//! *rebound*: a copy with its name, declared where the closure is made, is
//! assigned the variable's value right before the statement that holds
//! the block, and the block reads the copy:
//!
//! ```lua
//! local k, block = nil, function(v) return v * 2 + k end   -- once
//! for i = 1, n do
//!   k = i
//!   s = s + apply(i, block)
//! end
//! ```
//!
//! The copy is exact when nothing can change the variable while the call
//! runs: the variable is not assigned by a function other than its own,
//! not used by a function inside the block (which could outlive the call),
//! and either never assigned or declared by a function that is suspended
//! in the call. Those are the function that holds the block and, when the
//! block stands inside other transient blocks, each of them: a transient
//! block only runs while its own call does, and nothing can call it
//! again while it runs, so the copies of a block inside a block can live
//! in the function around them both. Loop variables and the locals of a
//! loop body qualify; a variable the block assigns (an accumulator
//! declared inside the loop) does not, and keeps the block where it is.
//!
//! The closure is made right after the latest declaration of a variable
//! it keeps, at the start of the function that holds the block (before
//! the statement of its body that holds the block) when the copies need a
//! home there, or in the main function when nothing constrains it, as
//! for `try`: in a local of its own for the first [`OWN_LOCALS`] closures
//! of that function (an immutable upvalue, which a trace treats as a
//! constant), then in the function's table. Nothing is moved unless that
//! place runs less often than the block is written: it is outside a loop
//! that holds the block, or in an enclosing function. The copy has the
//! variable's name, so the block's error messages name it as before; the
//! Lua backend, whose names are unique in a chunk, then prints the
//! variable itself under a fresh name (`k_2`), and a VM error message
//! about it in the loop body differs there from the bytecode backend's.

use std::collections::{HashMap, HashSet};

use super::limits::{LOCALS_BUDGET, UPVALUES_BUDGET, max_active, rewrite_block};
use crate::ir::*;

/// The closures lifted into locals of their own in one function; the
/// next ones go to the function's table.
const OWN_LOCALS: usize = 4;

/// Lifts the closures of the `try` statements of `chunk` (see the module
/// documentation).
/// Returns the `spawn do` blocks that cannot be made when the file loads,
/// as (line, message): compile errors.
pub(crate) fn lift(chunk: &mut Chunk) -> Vec<(u32, String)> {
    let facts = Facts::of(&chunk.main);
    let mut l = Lifter {
        locals: &mut chunk.locals,
        facts,
        frames: Vec::new(),
        fns: Vec::new(),
        scope: HashMap::new(),
        owner: chunk.owner,
        made: HashSet::new(),
        cond_depth: 0,
        errors: Vec::new(),
        try_closure: None,
    };
    l.function(&mut chunk.main);
    l.errors
}

/// Which locals are assigned (beyond their declaration), and which are
/// assigned by a function other than the one that declares them.
struct Facts {
    assigned: HashSet<LocalId>,
    assigned_elsewhere: HashSet<LocalId>,
}

impl Facts {
    fn of(main: &Function) -> Facts {
        struct V {
            next: usize,
            stack: Vec<usize>,
            decl: HashMap<LocalId, usize>,
            assigns: Vec<(LocalId, usize)>,
        }
        impl Visitor for V {
            fn enter(&mut self) {
                self.next += 1;
                self.stack.push(self.next);
            }
            fn leave(&mut self) {
                self.stack.pop();
            }
            fn decl(&mut self, id: LocalId) {
                self.decl.insert(id, self.stack.last().copied().unwrap_or(0));
            }
            fn used(&mut self, _id: LocalId) {}
            fn assigned(&mut self, id: LocalId) {
                self.assigns.push((id, self.stack.last().copied().unwrap_or(0)));
            }
        }
        let mut v = V { next: 0, stack: Vec::new(), decl: HashMap::new(), assigns: Vec::new() };
        visit_fn(main, &mut v);
        let mut facts = Facts { assigned: HashSet::new(), assigned_elsewhere: HashSet::new() };
        for (id, f) in v.assigns {
            facts.assigned.insert(id);
            if v.decl.get(&id) != Some(&f) {
                facts.assigned_elsewhere.insert(id);
            }
        }
        facts
    }
}

/// The variables a function uses outside the scope of their declarations
/// in it, each with whether a function inside it uses it. (A lifted
/// closure declares the variables it is passed again, as parameters, so
/// one id can be declared in two places.)
fn free_vars(f: &Function, out: &mut HashMap<LocalId, bool>) {
    struct V<'a> {
        depth: usize,
        active: Vec<LocalId>,
        marks: Vec<usize>,
        out: &'a mut HashMap<LocalId, bool>,
    }
    impl Visitor for V<'_> {
        fn enter(&mut self) {
            self.depth += 1;
        }
        fn leave(&mut self) {
            self.depth -= 1;
        }
        fn scope_begin(&mut self) {
            self.marks.push(self.active.len());
        }
        fn scope_end(&mut self) {
            let n = self.marks.pop().unwrap_or(0);
            self.active.truncate(n);
        }
        fn decl(&mut self, id: LocalId) {
            self.active.push(id);
        }
        fn used(&mut self, id: LocalId) {
            if !self.active.contains(&id) {
                let nested = self.depth > 1;
                let e = self.out.entry(id).or_insert(false);
                *e |= nested;
            }
        }
        fn assigned(&mut self, id: LocalId) {
            self.used(id);
        }
    }
    visit_fn(f, &mut V { depth: 0, active: Vec::new(), marks: Vec::new(), out });
}

/// What the read-only walks of this module see.
trait Visitor {
    /// A function starts; its parameters follow as declarations.
    fn enter(&mut self);
    fn leave(&mut self);
    /// A scope starts or ends: the declarations in it go out of scope.
    fn scope_begin(&mut self) {}
    fn scope_end(&mut self) {}
    fn decl(&mut self, id: LocalId);
    fn used(&mut self, id: LocalId);
    fn assigned(&mut self, id: LocalId);
}

fn visit_fn(f: &Function, v: &mut dyn Visitor) {
    v.enter();
    v.scope_begin();
    f.params.iter().for_each(|p| v.decl(*p));
    visit_block(&f.body, v);
    v.scope_end();
    v.leave();
}

fn visit_block(b: &Block, v: &mut dyn Visitor) {
    v.scope_begin();
    visit_stmts(b, v);
    v.scope_end();
}

fn visit_stmts(b: &Block, v: &mut dyn Visitor) {
    for s in &b.stmts {
        match &s.kind {
            StmtKind::Local { locals, values } => {
                values.iter().for_each(|e| visit_expr(e, v));
                locals.iter().for_each(|l| v.decl(*l));
            }
            StmtKind::Assign { targets, values } => {
                for t in targets {
                    match t {
                        Place::Local(l) => v.assigned(*l),
                        Place::Index(o, k) => {
                            visit_expr(o, v);
                            visit_expr(k, v);
                        }
                        Place::Global(_) => {}
                    }
                }
                values.iter().for_each(|e| visit_expr(e, v));
            }
            StmtKind::Expr(e) => visit_expr(e, v),
            StmtKind::Do(b) => visit_block(b, v),
            StmtKind::If { cond, then, otherwise } => {
                visit_expr(cond, v);
                visit_block(then, v);
                if let Some(o) = otherwise {
                    visit_block(o, v);
                }
            }
            StmtKind::While { cond, body } => {
                visit_expr(cond, v);
                visit_block(body, v);
            }
            StmtKind::NumericFor { var, start, limit, step, body } => {
                visit_expr(start, v);
                visit_expr(limit, v);
                if let Some(st) = step {
                    visit_expr(st, v);
                }
                v.scope_begin();
                v.decl(*var);
                visit_block(body, v);
                v.scope_end();
            }
            StmtKind::GenericFor { vars, iter, body } => {
                iter.iter().for_each(|e| visit_expr(e, v));
                v.scope_begin();
                vars.iter().for_each(|l| v.decl(*l));
                visit_block(body, v);
                v.scope_end();
            }
            StmtKind::Return(vals) => vals.iter().for_each(|e| visit_expr(e, v)),
            StmtKind::Hoist { defs, rest } => {
                // Both parts belong to the block of the `Hoist`.
                visit_stmts(defs, v);
                visit_stmts(rest, v);
            }
            StmtKind::Break | StmtKind::Goto(_) | StmtKind::Label(_) => {}
        }
    }
}

fn visit_expr(e: &Expr, v: &mut dyn Visitor) {
    match e {
        Expr::Local(l) => v.used(*l),
        Expr::Function(f) => visit_fn(f, v),
        Expr::Index(a, b) | Expr::Binary(_, a, b) => {
            visit_expr(a, v);
            visit_expr(b, v);
        }
        Expr::Call(f, args) => {
            visit_expr(f, v);
            args.iter().for_each(|a| visit_expr(a, v));
        }
        Expr::MethodCall(o, _, args) => {
            visit_expr(o, v);
            args.iter().for_each(|a| visit_expr(a, v));
        }
        Expr::Unary(_, a) | Expr::Paren(a) => visit_expr(a, v),
        Expr::Table(entries) => {
            for en in entries {
                match en {
                    TableEntry::Item(x) => visit_expr(x, v),
                    TableEntry::Pair(k, x) => {
                        visit_expr(k, v);
                        visit_expr(x, v);
                    }
                }
            }
        }
        Expr::MemberCall { receiver, key, args } => {
            visit_expr(receiver, v);
            visit_expr(key, v);
            args.iter().for_each(|a| visit_expr(a, v));
        }
        Expr::MemberApply { func, receiver, args } => {
            visit_expr(func, v);
            visit_expr(receiver, v);
            args.iter().for_each(|a| visit_expr(a, v));
        }
        Expr::AutoGet { receiver, key } => {
            visit_expr(receiver, v);
            visit_expr(key, v);
        }
        Expr::Nil | Expr::Bool(_) | Expr::Number(_) | Expr::Str(_) | Expr::Global(_) | Expr::Vararg | Expr::Intrinsic(_) => {}
    }
}

/// A block being walked.
struct Frame {
    /// The depth of the function the block belongs to (the main function
    /// is 1).
    fn_depth: usize,
    /// A part of a `Hoist`, where no local is inserted.
    hoist_part: bool,
    /// How many times the block runs for one run of the function's body,
    /// counted as the loops and function bodies entered: the main
    /// function's body is 1, a loop body in it 2, the body of a function
    /// written in that loop 3. A part of a `Hoist` has its block's count.
    iter: usize,
    /// The index of the statement being walked, its line, and whether it
    /// is a `Hoist`.
    cur: usize,
    cur_line: u32,
    cur_is_hoist: bool,
    /// The locals the block declares, removed from the scope at its end.
    declared: Vec<LocalId>,
    /// Statements to insert before the statement at an index (the length:
    /// at the end), in order.
    inserts: Vec<(usize, Stmt)>,
    /// Statements to put first among the definitions of the `Hoist` at an
    /// index, in order.
    defs_inserts: Vec<(usize, Stmt)>,
    /// The block runs once per load of the file: the main function's body,
    /// a block in it outside loops, or the body of a `try`, `catch` or
    /// `finally` there.
    once: bool,
    /// The block is in a branch of a conditional (`if`, `unless`, `case`,
    /// …), which a state that loads the file may not run.
    branch: bool,
}

/// Where a local is declared: its block, the index a statement inserted
/// right after the declaration would have, and the declaration's line.
#[derive(Clone, Copy)]
struct Decl {
    /// An index into [`Lifter::frames`]. It names the right block because
    /// a block's declarations leave [`Lifter::scope`] when the block ends
    /// (`Lifter::block`): while a local is in scope, the frames up to its
    /// own are those of the blocks that enclose the walk.
    frame: usize,
    pos: usize,
    line: u32,
}

/// A function being walked.
struct FnInfo {
    /// The most locals it has active at once, and the locals lifting gave
    /// it: each costs at most one more at the peak.
    base_locals: usize,
    added_locals: usize,
    /// The closures lifted into locals of their own.
    own_locals: usize,
    /// Its upvalues before lifting, and the locals of enclosing functions
    /// that lifting made it use.
    upvalues: usize,
    added_upvalues: HashSet<LocalId>,
    /// Its table of lifted closures, once it has one, and the entries.
    table: Option<LocalId>,
    slots: usize,
    /// A transient block ([`Function::transient`]): it runs only while
    /// the call it is written in does.
    transient: bool,
    /// Its body runs once per load ([`Frame::once`]).
    once: bool,
    /// The `catch` closure of a `try`: it runs only on an error, like a
    /// branch ([`Frame::branch`]).
    catch: bool,
}

impl FnInfo {
    fn locals_fit(&self, more: usize) -> bool {
        self.base_locals + self.added_locals + more <= LOCALS_BUDGET
    }
}

/// Where a lifted closure is kept: a local of its own, or an entry of the
/// table of the function at an index of [`Lifter::fns`].
#[derive(Clone, Copy, PartialEq, Eq)]
enum Store {
    Own,
    Table(usize),
}

struct Lifter<'a> {
    locals: &'a mut Vec<LocalInfo>,
    facts: Facts,
    frames: Vec<Frame>,
    fns: Vec<FnInfo>,
    scope: HashMap<LocalId, Decl>,
    /// [`Chunk::owner`].
    owner: Option<LocalId>,
    /// The locals this pass made: closures, tables and the copies of
    /// rebound variables. A block captures them as they are.
    made: HashSet<LocalId>,
    /// Inside the condition of a `while`, which runs before each
    /// iteration: a statement inserted before the loop would not.
    cond_depth: usize,
    /// `spawn do` blocks that cannot be made when the file loads.
    errors: Vec<(u32, String)>,
    /// The function walked next is a closure of a `try` (rt.xpcall, rt.try)
    /// whose call runs where this is the current frame: `Some(true)` for
    /// the `catch` closure of `rt.try`, which runs only on an error.
    try_closure: Option<bool>,
}

impl Lifter<'_> {
    fn new_local(&mut self, hint: &str) -> LocalId {
        new_local(self.locals, hint)
    }

    fn declare(&mut self, id: LocalId, pos: usize, line: u32) {
        let frame = self.frames.len().saturating_sub(1);
        self.scope.insert(id, Decl { frame, pos, line });
        if let Some(f) = self.frames.last_mut() {
            f.declared.push(id);
        }
    }

    fn function(&mut self, f: &mut Function) {
        let mut free = HashMap::new();
        free_vars(f, &mut free);
        // The main function runs once per load, and so does a closure of a
        // `try` written where a block runs once (error.rs calls it once).
        let try_closure = std::mem::take(&mut self.try_closure);
        let once = self.fns.is_empty() || (try_closure.is_some() && self.frames.last().is_some_and(|fr| fr.once));
        let catch = try_closure == Some(true);
        self.fns.push(FnInfo {
            base_locals: max_active(&f.body, f.params.len()),
            added_locals: 0,
            own_locals: 0,
            upvalues: free.len(),
            added_upvalues: HashSet::new(),
            table: None,
            slots: 0,
            transient: f.transient,
            once,
            catch,
        });
        let params = f.params.clone();
        self.block(&mut f.body, false, &params, f.line, true);
        if let Some(t) = self.fns.pop().and_then(|i| i.table) {
            // Made when the function starts, before anything that fills it.
            f.body.stmts.insert(0, Stmt { line: f.line, kind: StmtKind::Local { locals: vec![t], values: vec![Expr::Table(Vec::new())] } });
        }
    }

    /// Walks a block whose first declarations are `vars` (parameters, loop
    /// variables), then applies the insertions it collected. `repeats`:
    /// the block runs more than once for one run of the enclosing block (a
    /// loop or function body).
    fn block(&mut self, b: &mut Block, hoist_part: bool, vars: &[LocalId], line: u32, repeats: bool) {
        self.block_in(b, hoist_part, vars, line, repeats, false);
    }

    /// [`Self::block`], for a branch of a conditional when `branch`.
    fn block_in(&mut self, b: &mut Block, hoist_part: bool, vars: &[LocalId], line: u32, repeats: bool, branch: bool) {
        let iter = self.frames.last().map_or(0, |f| f.iter) + usize::from(repeats);
        let parent = self.frames.last();
        // A function's body (the first frame of its function) runs once
        // when the function does; a loop body does not.
        let fn_body = parent.is_none_or(|p| p.fn_depth != self.fns.len());
        let once = if fn_body { self.fns.last().is_some_and(|i| i.once) } else { !repeats && parent.is_some_and(|p| p.once) };
        let branch = branch || parent.is_some_and(|p| p.branch) || (fn_body && self.fns.last().is_some_and(|i| i.catch));
        self.frames.push(Frame {
            fn_depth: self.fns.len(),
            hoist_part,
            iter,
            cur: 0,
            cur_line: line,
            cur_is_hoist: false,
            declared: Vec::new(),
            inserts: Vec::new(),
            defs_inserts: Vec::new(),
            once,
            branch,
        });
        for v in vars {
            self.declare(*v, 0, line);
        }
        for (i, s) in b.stmts.iter_mut().enumerate() {
            if let Some(f) = self.frames.last_mut() {
                f.cur = i;
                f.cur_line = s.line;
                f.cur_is_hoist = matches!(s.kind, StmtKind::Hoist { .. });
            }
            self.stmt(s);
            if let StmtKind::Local { locals, .. } = &s.kind {
                for l in locals {
                    self.declare(*l, i + 1, s.line);
                }
            }
        }
        let Some(frame) = self.frames.pop() else { return };
        for l in &frame.declared {
            self.scope.remove(l);
        }
        let mut defs: Vec<(usize, Vec<Stmt>)> = Vec::new();
        for (at, s) in frame.defs_inserts {
            match defs.iter_mut().find(|(i, _)| *i == at) {
                Some((_, list)) => list.push(s),
                None => defs.push((at, vec![s])),
            }
        }
        for (at, list) in defs {
            if let Some(Stmt { kind: StmtKind::Hoist { defs, .. }, .. }) = b.stmts.get_mut(at) {
                defs.stmts.splice(0..0, list);
            }
        }
        if !frame.inserts.is_empty() {
            let mut inserts = frame.inserts;
            inserts.sort_by_key(|(at, _)| *at);
            let mut inserts = inserts.into_iter().peekable();
            let old = std::mem::take(&mut b.stmts);
            for (i, s) in old.into_iter().enumerate() {
                while let Some((_, ins)) = inserts.next_if(|(at, _)| *at <= i) {
                    b.stmts.push(ins);
                }
                b.stmts.push(s);
            }
            b.stmts.extend(inserts.map(|(_, s)| s));
        }
    }

    fn stmt(&mut self, s: &mut Stmt) {
        match &mut s.kind {
            StmtKind::Local { values, .. } => values.iter_mut().for_each(|e| self.expr(e)),
            StmtKind::Assign { targets, values } => {
                for t in targets {
                    if let Place::Index(o, k) = t {
                        self.expr(o);
                        self.expr(k);
                    }
                }
                values.iter_mut().for_each(|e| self.expr(e));
            }
            StmtKind::Expr(e) => self.expr(e),
            StmtKind::Do(b) => self.block(b, false, &[], s.line, false),
            StmtKind::If { cond, then, otherwise } => {
                self.expr(cond);
                self.block_in(then, false, &[], s.line, false, true);
                if let Some(o) = otherwise {
                    self.block_in(o, false, &[], s.line, false, true);
                }
            }
            StmtKind::While { cond, body } => {
                self.cond_depth += 1;
                self.expr(cond);
                self.cond_depth -= 1;
                self.block(body, false, &[], s.line, true);
            }
            StmtKind::NumericFor { var, start, limit, step, body } => {
                self.expr(start);
                self.expr(limit);
                if let Some(st) = step {
                    self.expr(st);
                }
                let var = *var;
                self.block(body, false, &[var], s.line, true);
            }
            StmtKind::GenericFor { vars, iter, body } => {
                iter.iter_mut().for_each(|e| self.expr(e));
                let vars = vars.clone();
                self.block(body, false, &vars, s.line, true);
            }
            StmtKind::Return(vals) => vals.iter_mut().for_each(|e| self.expr(e)),
            StmtKind::Hoist { defs, rest } => {
                self.block(defs, true, &[], s.line, false);
                self.block(rest, true, &[], s.line, false);
            }
            StmtKind::Break | StmtKind::Goto(_) | StmtKind::Label(_) => {}
        }
    }

    fn expr(&mut self, e: &mut Expr) {
        if let Expr::Function(f) = e {
            self.function(f);
            let transient = f.transient;
            if transient {
                self.lift_transient(e);
            }
            return;
        }
        match e {
            Expr::Function(_) => {}
            Expr::Call(callee, args) => {
                // The closures of `try` (error.rs): the first argument of
                // `rt.xpcall`, the first three of `rt.try`.
                let group: &[usize] = match callee.as_ref() {
                    Expr::Intrinsic("xpcall") if matches!(args.first(), Some(Expr::Function(_))) => &[0],
                    Expr::Intrinsic("try") => &[0, 1, 2],
                    _ => &[],
                };
                let spawn_block = matches!(callee.as_ref(), Expr::Intrinsic("spawn")) && matches!((args.first(), args.get(1)), (Some(Expr::Table(_)), Some(Expr::Function(_))));
                self.expr(callee);
                for (i, a) in args.iter_mut().enumerate() {
                    let catch = matches!(callee.as_ref(), Expr::Intrinsic("try")) && i == 1;
                    self.try_closure = (group.contains(&i) && matches!(a, Expr::Function(_))).then_some(catch);
                    self.expr(a);
                    self.try_closure = None;
                }
                if !group.is_empty() {
                    self.lift(args, group);
                }
                if spawn_block {
                    self.lift_spawn(args);
                }
            }
            Expr::Index(a, b) | Expr::Binary(_, a, b) => {
                self.expr(a);
                self.expr(b);
            }
            Expr::MethodCall(o, _, args) => {
                self.expr(o);
                args.iter_mut().for_each(|a| self.expr(a));
            }
            Expr::Unary(_, a) | Expr::Paren(a) => self.expr(a),
            Expr::Table(entries) => {
                for en in entries {
                    match en {
                        TableEntry::Item(x) => self.expr(x),
                        TableEntry::Pair(k, x) => {
                            self.expr(k);
                            self.expr(x);
                        }
                    }
                }
            }
            Expr::MemberCall { receiver, key, args } => {
                self.expr(receiver);
                self.expr(key);
                args.iter_mut().for_each(|a| self.expr(a));
            }
            Expr::MemberApply { func, receiver, args } => {
                self.expr(func);
                self.expr(receiver);
                args.iter_mut().for_each(|a| self.expr(a));
            }
            Expr::AutoGet { receiver, key } => {
                self.expr(receiver);
                self.expr(key);
            }
            Expr::Nil | Expr::Bool(_) | Expr::Number(_) | Expr::Str(_) | Expr::Local(_) | Expr::Global(_) | Expr::Vararg | Expr::Intrinsic(_) => {}
        }
    }

    /// Lifts the closures among `args` at the indices of `group`, already
    /// walked, and appends the variables they are passed to `args`.
    fn lift(&mut self, args: &mut Vec<Expr>, group: &[usize]) {
        let mut free: HashMap<LocalId, bool> = HashMap::new();
        for &g in group {
            if let Some(Expr::Function(f)) = args.get(g) {
                free_vars(f, &mut free);
            }
        }
        // In scope everywhere in the main function: its table, declared
        // first in it, and the chunk's owner, declared before it.
        for t in [self.fns.first().and_then(|f| f.table), self.owner].into_iter().flatten() {
            free.remove(&t);
        }
        let depth = self.fns.len();
        let mut free: Vec<(LocalId, bool)> = free.into_iter().collect();
        free.sort_unstable();
        let mut passed = Vec::new();
        let mut kept = Vec::new();
        for (x, nested) in free {
            let decl = self.scope.get(&x);
            // A top-level local of the main function never constrains
            // where the closure is made: it stays an upvalue.
            let top = decl.is_some_and(|d| d.frame == 0);
            let own = decl.and_then(|d| self.frames.get(d.frame)).is_some_and(|f| f.fn_depth == depth);
            let pass = !top && !self.facts.assigned_elsewhere.contains(&x) && (!self.facts.assigned.contains(&x) || (own && !nested));
            if pass {
                passed.push(x);
            } else {
                kept.push(x);
            }
        }
        // The latest declaration of a kept variable, unless they are all
        // top-level locals of the main function.
        let mut place: Option<Decl> = None;
        for x in &kept {
            let Some(d) = self.scope.get(x).copied() else { return };
            if self.frames.get(d.frame).is_none_or(|f| f.hoist_part) {
                return;
            }
            if d.frame != 0 && place.is_none_or(|p| (d.frame, d.pos) > (p.frame, p.pos)) {
                place = Some(d);
            }
        }
        let count = group.iter().filter(|&&g| matches!(args.get(g), Some(Expr::Function(_)))).count();
        // Where the closures go: before the top-level statement of the
        // main function being walked, or after the latest declaration.
        let at = match place {
            Some(p) => p,
            None => Decl { frame: 0, pos: self.frames.first().map_or(0, |f| f.cur), line: 0 },
        };
        let Some(owner) = self.frames.get(at.frame).map(|f| f.fn_depth.saturating_sub(1)) else { return };
        let Some(store) = self.store(owner, place.is_some(), count, false) else { return };
        for &g in group {
            let Some(slot) = args.get_mut(g) else { continue };
            if !matches!(slot, Expr::Function(_)) {
                continue;
            }
            let Expr::Function(mut f) = std::mem::replace(slot, Expr::Nil) else { continue };
            // The passed variables become parameters of the same ids,
            // which the uses in the closure then mean (and which keep the
            // names that error messages show).
            f.params.extend(passed.iter().copied());
            let line = f.line;
            let value = Expr::Function(f);
            *slot = match store {
                Store::Table(t) => self.make_in_table(value, line, t, at),
                Store::Own => self.make_in_local(value, line, owner, at),
            };
        }
        args.extend(passed.into_iter().map(Expr::Local));
    }

    /// Makes the hidden function of a `spawn do` block (spawn.rs) when the
    /// file loads, in the main function's table and registered under its
    /// name there (`rt.register(file, name, f)`), so that a
    /// worker that loads the file finds it without the `spawn` ever
    /// running there. `args` are those of `rt.spawn({file, name, …}, f,
    /// values…)`, already walked. The block may read, besides its
    /// parameters, only declarations made once per load and outside the
    /// branches of conditionals; anything else is an error (`errors`).
    /// Over LuaJIT's limits the table is fitted by limits.rs.
    fn lift_spawn(&mut self, args: &mut [Expr]) {
        let (Some(Expr::Table(info)), Some(Expr::Function(f))) = (args.first(), args.get(1)) else { return };
        let (Some(TableEntry::Item(file)), Some(TableEntry::Item(name))) = (info.first(), info.get(1)) else { return };
        let (file, name, line) = (file.clone(), name.clone(), f.line);
        let mut free: HashMap<LocalId, bool> = HashMap::new();
        free_vars(f, &mut free);
        for t in [self.fns.first().and_then(|f| f.table), self.owner].into_iter().flatten() {
            free.remove(&t);
        }
        // Made when the file loads: right after the latest declaration of
        // what it reads, in a block that runs once per load (the file's
        // own block, a module body, the body or `finally` of a top-level
        // `try`) and in no branch of a conditional, or before the
        // top-level statement that holds the `spawn` when it reads only
        // top-level locals.
        let mut place: Option<Decl> = None;
        // The latest declaration first: an error names what the block's
        // code names (`sib`, not the module `M` passed as its `self`).
        let mut free: Vec<LocalId> = free.into_keys().collect();
        free.sort_unstable_by_key(|x| (self.scope.get(x).map(|d| std::cmp::Reverse((d.frame, d.pos))), *x));
        for x in &free {
            let what = self.locals.get(x.0 as usize).map_or_else(|| "a variable".to_string(), |l| format!("`{}`", l.hint));
            let Some(d) = self.scope.get(x).copied() else {
                self.errors.push((line, format!("this `spawn do` block reads {what}, which the compiler cannot place where the file loads")));
                return;
            };
            let Some(frame) = self.frames.get(d.frame) else { return };
            if frame.branch && frame.once && !frame.hoist_part {
                // A state that loads the file may take another branch (the
                // `unless Worker.parent` guard): the block would be missing
                // there.
                self.errors.push((
                    line,
                    format!(
                        "this `spawn do` block reads {what}, which is declared in a branch of a top-level conditional (or a `catch`), which a worker that loads the file may not run: declare it outside the branch"
                    ),
                ));
                return;
            }
            if !frame.once || frame.hoist_part {
                self.errors.push((
                    line,
                    format!(
                        "this `spawn do` block reads {what}, which is declared where the block cannot be made once when the file loads: declare it at file level, in a module body or in a top-level `try`, outside loops and functions"
                    ),
                ));
                return;
            }
            if d.frame != 0 && place.is_none_or(|p| (d.frame, d.pos) > (p.frame, p.pos)) {
                place = Some(d);
            }
        }
        let at = match place {
            Some(p) => p,
            None => {
                let Some(cur) = self.frames.first().map(|f| f.cur) else { return };
                Decl { frame: 0, pos: cur, line: 0 }
            }
        };
        // Over the upvalue budget, limits.rs fits the file as it does any
        // other (the table is one of its top-level locals).
        let Some(Store::Table(t)) = self.store(0, false, 1, true) else { return };
        let Some(slot) = args.get_mut(1) else { return };
        let Expr::Function(f) = std::mem::replace(slot, Expr::Nil) else { return };
        let value = Expr::intrinsic_call("register", vec![file, name, Expr::Function(f)]);
        *slot = self.make_in_table(value, line, t, at);
    }

    /// Makes a transient block once (see "Blocks" in the module
    /// documentation): `e` is its `Expr::Function`, already walked, and is
    /// replaced by what reads the closure. Nothing changes when the block
    /// cannot be moved somewhere that runs less often, or would not fit.
    fn lift_transient(&mut self, e: &mut Expr) {
        let Expr::Function(f) = e else { return };
        let mut free: HashMap<LocalId, bool> = HashMap::new();
        free_vars(f, &mut free);
        for t in [self.fns.first().and_then(|f| f.table), self.owner].into_iter().flatten() {
            free.remove(&t);
        }
        let Some(call_iter) = self.frames.last().map(|fr| fr.iter) else { return };
        // The nearest enclosing function that is not a transient block,
        // and the frame of its body: the copies live there or deeper.
        let Some(home) = self.fns.iter().rposition(|i| !i.transient) else { return };
        let Some(home_frame) = self.frames.iter().position(|fr| fr.fn_depth == home + 1) else { return };
        // The variables the block captures, by declaration, latest first.
        // A top-level local of the main function never constrains where
        // the closure is made.
        let mut vars: Vec<(LocalId, bool, Decl)> = Vec::new();
        for (x, nested) in free {
            let Some(d) = self.scope.get(&x).copied() else { return };
            if d.frame == 0 {
                continue;
            }
            if self.frames.get(d.frame).is_none_or(|fr| fr.hoist_part) {
                return;
            }
            vars.push((x, nested, d));
        }
        vars.sort_by_key(|(x, _, d)| std::cmp::Reverse((d.frame, d.pos, *x)));
        // Rebind the variables declared deeper than the body of the
        // function that holds the block (in its loops, or in transient
        // blocks inside it), latest first, down to the first that cannot
        // be: the closure then goes right after that one's declaration.
        let home_iter = self.frames.get(home_frame).map_or(0, |fr| fr.iter);
        let mut rebinds: Vec<LocalId> = Vec::new();
        let mut place: Option<Decl> = None;
        for (x, nested, d) in &vars {
            let Some(frame) = self.frames.get(d.frame) else { return };
            // Exact only when nothing can assign `x` during the call: it
            // is never assigned, or its function is suspended in the call.
            let suspended = frame.fn_depth.saturating_sub(1) >= home;
            let exact = !self.facts.assigned_elsewhere.contains(x) && (!self.facts.assigned.contains(x) || suspended);
            if frame.iter <= home_iter || *nested || self.cond_depth > 0 || self.made.contains(x) || !exact {
                place = Some(*d);
                break;
            }
            rebinds.push(*x);
        }
        let line = f.line;
        // Before the statement of a function's body that holds the block,
        // on that statement's line.
        let before_cur = |frames: &[Frame], frame: usize| frames.get(frame).map(|fr| Decl { frame, pos: fr.cur, line: fr.cur_line });
        let Some(mut at) = (match place {
            Some(p) => Some(p),
            None if rebinds.is_empty() => before_cur(&self.frames, 0),
            None => before_cur(&self.frames, home_frame),
        }) else {
            return;
        };
        // The copies cannot go above the function that holds the block.
        if !rebinds.is_empty() && at.frame < home_frame {
            let Some(d) = before_cur(&self.frames, home_frame) else { return };
            at = d;
        }
        let Some(at_frame) = self.frames.get(at.frame) else { return };
        if at_frame.iter >= call_iter {
            return;
        }
        let owner = at_frame.fn_depth.saturating_sub(1);
        // The copies are locals of `owner` and upvalues of every function
        // between it and this one; the closure costs one more of each.
        let more = rebinds.len() + 1;
        if !self.fns.get(owner).is_some_and(|i| i.locals_fit(more)) || !self.upvalues_fit(owner, &[], more) {
            return;
        }
        let Some(store) = self.store(owner, true, 1, false) else { return };
        if !rebinds.is_empty() {
            let mut copies = Vec::new();
            for x in &rebinds {
                let hint = self.locals.get(x.0 as usize).map_or_else(|| "v".to_string(), |l| l.hint.clone());
                let h = self.new_local(&hint);
                self.made_local(h, at);
                self.facts.assigned.insert(h);
                self.facts.assigned_elsewhere.insert(h);
                copies.push(h);
            }
            if let Some(info) = self.fns.get_mut(owner) {
                info.added_locals += copies.len();
            }
            self.add_upvalues(owner, &copies);
            // Declared where the closure is made, before it.
            let decl = Stmt { line: at.line, kind: StmtKind::Local { locals: copies.clone(), values: Vec::new() } };
            if let Some(frame) = self.frames.get_mut(at.frame) {
                frame.inserts.push((at.pos, decl));
            }
            // Inside the block, the variables mean their copies.
            let map: HashMap<LocalId, LocalId> = rebinds.iter().copied().zip(copies.iter().copied()).collect();
            rewrite_block(&mut f.body, &|l| map.get(l).map(|h| Expr::Local(*h)), false);
            // Assigned right before the statement that holds the block.
            let copy = StmtKind::Assign { targets: copies.iter().map(|h| Place::Local(*h)).collect(), values: rebinds.iter().map(|x| Expr::Local(*x)).collect() };
            if let Some(frame) = self.frames.last_mut() {
                frame.inserts.push((frame.cur, Stmt { line: frame.cur_line, kind: copy }));
            }
        }
        let value = std::mem::replace(e, Expr::Nil);
        *e = match store {
            Store::Table(t) => self.make_in_table(value, line, t, at),
            Store::Own => self.make_in_local(value, line, owner, at),
        };
    }

    /// How `count` closures lifted into the function at index `owner` of
    /// [`Self::fns`] are kept, within LuaJIT's limits (limits.rs) for that
    /// function and for the functions between it and the one running the
    /// `try`, which use the closures as upvalues; `None` when they do not
    /// fit and stay where they are. `own` allows locals of their own.
    /// `force` keeps them in the table even over the upvalue budget, which
    /// limits.rs then fits (the main function's table moves with the
    /// file's other top-level locals).
    fn store(&mut self, owner: usize, own: bool, count: usize, force: bool) -> Option<Store> {
        let info = self.fns.get(owner)?;
        let own = own && info.own_locals + count <= OWN_LOCALS && info.locals_fit(count) && self.upvalues_fit(owner, &[], count);
        let store = if own {
            Store::Own
        } else {
            // The main function's table is one of its top-level locals,
            // which limits.rs moves into a table of their own when there
            // are too many.
            let fits = match info.table {
                Some(t) => self.upvalues_fit(owner, &[t], 0),
                None => (owner == 0 || info.locals_fit(1)) && self.upvalues_fit(owner, &[], 1),
            };
            if !fits && !force {
                return None;
            }
            Store::Table(owner)
        };
        if let Store::Table(t) = store {
            if self.fns.get(t).is_some_and(|i| i.table.is_none()) {
                let id = self.new_local("try_fns");
                if let Some(info) = self.fns.get_mut(t) {
                    info.table = Some(id);
                    info.added_locals += 1;
                }
                // Declared first in the body of that function (`function`).
                if let Some(frame) = self.frames.iter().position(|f| f.fn_depth == t + 1) {
                    self.made_local(id, Decl { frame, pos: 0, line: 0 });
                }
            }
            let id = self.fns.get(t).and_then(|i| i.table);
            self.add_upvalues(owner, id.as_slice());
        }
        Some(store)
    }

    /// Records a local this pass declared at `at`, so that a block lifted
    /// later finds its declaration and keeps it as it is.
    fn made_local(&mut self, id: LocalId, at: Decl) {
        self.made.insert(id);
        self.scope.insert(id, at);
        if let Some(frame) = self.frames.get_mut(at.frame) {
            frame.declared.push(id);
        }
    }

    /// Whether the functions inside the one at index `owner` down to the
    /// current one can take `known` (locals of `owner`) and `new` more
    /// locals of `owner` as upvalues.
    fn upvalues_fit(&self, owner: usize, known: &[LocalId], new: usize) -> bool {
        self.fns.iter().skip(owner + 1).all(|f| {
            let more = known.iter().filter(|l| !f.added_upvalues.contains(l)).count() + new;
            f.upvalues + f.added_upvalues.len() + more <= UPVALUES_BUDGET
        })
    }

    fn add_upvalues(&mut self, owner: usize, ids: &[LocalId]) {
        for f in self.fns.iter_mut().skip(owner + 1) {
            f.added_upvalues.extend(ids.iter().copied());
        }
    }

    /// Stores a closure in the table of the function at index `t` of
    /// [`Self::fns`], at `at`; returns the expression that reads it.
    fn make_in_table(&mut self, value: Expr, line: u32, t: usize, at: Decl) -> Expr {
        let Some(info) = self.fns.get_mut(t) else { return value };
        let Some(table) = info.table else { return value };
        info.slots += 1;
        let key = Expr::Number(info.slots as f64);
        let stmt = Stmt { line, kind: StmtKind::Assign { targets: vec![Place::Index(Expr::Local(table), key.clone())], values: vec![value] } };
        if let Some(frame) = self.frames.get_mut(at.frame) {
            if frame.cur_is_hoist {
                frame.defs_inserts.push((frame.cur, stmt));
            } else {
                frame.inserts.push((at.pos, stmt));
            }
        }
        Expr::index(Expr::Local(table), key)
    }

    /// Stores a closure in a new local of the function at index `owner` of
    /// [`Self::fns`], declared at `at`; returns the expression that reads
    /// it.
    fn make_in_local(&mut self, value: Expr, line: u32, owner: usize, at: Decl) -> Expr {
        let hint = if matches!(&value, Expr::Function(f) if f.transient) { "block" } else { "try" };
        let h = self.new_local(hint);
        if let Some(info) = self.fns.get_mut(owner) {
            info.own_locals += 1;
            info.added_locals += 1;
        }
        self.add_upvalues(owner, &[h]);
        self.made_local(h, at);
        if let Some(frame) = self.frames.get_mut(at.frame) {
            if frame.cur_is_hoist {
                let decl = Stmt { line: at.line, kind: StmtKind::Local { locals: vec![h], values: Vec::new() } };
                frame.inserts.push((at.pos, decl));
                let def = Stmt { line, kind: StmtKind::Assign { targets: vec![Place::Local(h)], values: vec![value] } };
                frame.defs_inserts.push((frame.cur, def));
            } else {
                frame.inserts.push((at.pos, Stmt { line, kind: StmtKind::Local { locals: vec![h], values: vec![value] } }));
            }
        }
        Expr::Local(h)
    }
}
