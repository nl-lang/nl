//! The Lua backend: prints the core IR as Lua source for stock LuaJIT 2.1
//! (kept as `nlc build --emit=lua` and as the test oracle).
//!
//! # The chunk
//!
//! ```lua
//! local __rt = ...; return function(...) <main body> end
//! ```
//!
//! When the chunk has an owner local ([`Chunk::owner`]), it is set between
//! the two: `local __rt = ...; local owner = __rt.owner("@name"); return
//! function(...) … end`, so that it is computed once per load.
//!
//! Loading the chunk gives a function `P`; `P(rt)` returns the file's main
//! function (`crates/nl_capi/src/driver.rs`). Generated code reaches the
//! runtime only through the upvalue `__rt`: an [`Expr::Intrinsic`] `name`
//! is printed as `__rt.name`, a member call as `__rt.mcall(…)`, an
//! auto-call read as `__rt.mget(…)`, a Lua method call `x:m(…)` as
//! `__rt.lmethod.m(x, …)`. It reads no global of its own; Lua
//! globals appear only where the IR names them (prelude tables, `global`
//! names, Lua paths).
//!
//! # Names and lines
//!
//! Every local gets a name unique in the chunk, made from its hint, so no
//! local shadows another and a Lua global named by a Lua path is never
//! hidden by a local: a global whose name is also a local's (or a Lua
//! keyword) is read through `__G`, the chunk's environment. The one
//! intended shadow: a closure of `try` lifted out of the code that runs it
//! (crates/nl_ir/src/lower/lift.rs) declares a variable it is passed again
//! as a parameter, under the same id and so the same name; inside the
//! closure the name means the parameter, which holds the same value.
//!
//! Each statement is printed on the line of the NL statement it comes
//! from, when that line is still ahead of the output, so runtime errors
//! and tracebacks point at NL lines without help. A statement can come
//! after its line has passed: the statements a function definition adds
//! after the function's body (registering a primitive method, the check
//! of a value that falls off the end), the value of a block expression
//! assigned after the block. When such a statement can run code (and so
//! raise, or be a frame of a traceback), it starts a new output line, and
//! the chunk records that line in a line map:
//!
//! ```lua
//! local __rt = ...; if __rt.linemap then __rt.linemap("@name", {out1, nl1, out2, nl2, ...}) end; return function(...)
//! ```
//!
//! The runtime applies the map where it shows a position: the frames of a
//! trace and the `chunk:line:` prefix of an error message
//! (runtime/nl_50_error.lua). A statement that runs no code follows on the
//! current line, as it cannot be seen there. Most chunks need no map and
//! have no call. The bytecode backend writes NL lines directly and never
//! needs one.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing))]

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use nl_ir::ir::*;

/// Prints a chunk as Lua source.
pub fn print_chunk(chunk: &Chunk) -> String {
    let mut p = Printer::new(chunk);
    p.assign_names(&chunk.main);
    p.out.push_str("local __rt = ...;");
    if p.needs_env(&chunk.main) {
        p.out.push_str(" local __G = getfenv(1);");
    }
    if let Some(o) = chunk.owner {
        p.name_local(o);
        let name = p.local(o).to_string();
        let _ = write!(p.out, " local {name} = __rt.owner({});", lua_string(chunk.name.as_bytes()));
    }
    p.out.push_str(" return function(...)");
    let prologue = p.out.len();
    p.block(&chunk.main.body, 1);
    p.out.push_str(" end\n");
    if !p.line_map.is_empty() {
        // On the first line, which keeps every other line where it is.
        let pairs: Vec<String> = p.line_map.iter().map(|(out, nl)| format!("{out}, {nl}")).collect();
        // Guarded, so that the chunk still runs on a runtime without it.
        let call = format!(" if __rt.linemap then __rt.linemap({}, {{{}}}) end;", lua_string(chunk.name.as_bytes()), pairs.join(", "));
        let at = prologue - " return function(...)".len();
        p.out.insert_str(at, &call);
    }
    p.out
}

/// Whether a statement can run code of its own (a call, an index, an
/// operator with a metamethod): what may raise an error, or be the line of
/// a frame. The parts of a nested block or function do not count; they are
/// statements of their own.
fn runs_code(s: &Stmt) -> bool {
    // A function value counts as code: its `linedefined` is a line of a
    // trace, which must be on a line of its own when mapped.
    let plain = |e: &Expr| e.is_constant() || matches!(e, Expr::Local(_));
    match &s.kind {
        StmtKind::Local { values, .. } => !values.iter().all(plain),
        StmtKind::Assign { targets, values } => targets.iter().any(|t| !matches!(t, Place::Local(_))) || !values.iter().all(plain),
        StmtKind::Expr(_) | StmtKind::GenericFor { .. } => true,
        StmtKind::Return(values) => !values.iter().all(plain),
        StmtKind::If { cond, .. } | StmtKind::While { cond, .. } => !plain(cond),
        StmtKind::NumericFor { start, limit, step, .. } => !plain(start) || !plain(limit) || step.as_ref().is_some_and(|e| !plain(e)),
        StmtKind::Do(_) | StmtKind::Break | StmtKind::Goto(_) | StmtKind::Label(_) | StmtKind::Hoist { .. } => false,
    }
}

const LUA_KEYWORDS: &[&str] = &[
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto", "if", "in", "local", "nil", "not", "or",
    "repeat", "return", "then", "true", "until", "while",
];

fn is_lua_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_') && !LUA_KEYWORDS.contains(&s)
}

struct Printer<'a> {
    chunk: &'a Chunk,
    out: String,
    line: u32,
    /// The NL line of the current output line: `line`, unless the output
    /// line is in the line map.
    line_nl: u32,
    /// Output lines that hold a statement of an earlier NL line, and that
    /// line: (output line, NL line), in order.
    line_map: Vec<(u32, u32)>,
    names: HashMap<LocalId, String>,
    used: HashSet<String>,
    hoist_id: u32,
}

// Lua operator precedences (Lua 5.1 manual 2.5.6).
const PREC_OR: u8 = 1;
const PREC_AND: u8 = 2;
const PREC_CMP: u8 = 3;
const PREC_CONCAT: u8 = 4;
const PREC_ADD: u8 = 5;
const PREC_MUL: u8 = 6;
const PREC_UNARY: u8 = 7;
const PREC_POW: u8 = 8;
const PREC_ATOM: u8 = 10;

fn binop_info(op: BinOp) -> (&'static str, u8, bool) {
    // (text, precedence, right-associative)
    match op {
        BinOp::Or => ("or", PREC_OR, false),
        BinOp::And => ("and", PREC_AND, false),
        BinOp::Eq => ("==", PREC_CMP, false),
        BinOp::Ne => ("~=", PREC_CMP, false),
        BinOp::Lt => ("<", PREC_CMP, false),
        BinOp::Le => ("<=", PREC_CMP, false),
        BinOp::Gt => (">", PREC_CMP, false),
        BinOp::Ge => (">=", PREC_CMP, false),
        BinOp::Concat => ("..", PREC_CONCAT, true),
        BinOp::Add => ("+", PREC_ADD, false),
        BinOp::Sub => ("-", PREC_ADD, false),
        BinOp::Mul => ("*", PREC_MUL, false),
        BinOp::Div => ("/", PREC_MUL, false),
        BinOp::Mod => ("%", PREC_MUL, false),
        BinOp::Pow => ("^", PREC_POW, true),
    }
}

impl<'a> Printer<'a> {
    fn new(chunk: &'a Chunk) -> Self {
        let mut used = HashSet::new();
        used.insert("__rt".to_string());
        used.insert("__G".to_string());
        Printer { chunk, out: String::new(), line: 1, line_nl: 1, line_map: Vec::new(), names: HashMap::new(), used, hoist_id: 0 }
    }

    // ---- names ---------------------------------------------------------------

    fn name_local(&mut self, id: LocalId) {
        if self.names.contains_key(&id) {
            return;
        }
        let hint = self.chunk.hint(id);
        let base: String = hint.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' }).collect();
        let base = if is_lua_identifier(&base) && !base.starts_with("__") { base } else { format!("_{base}_") };
        let mut name = base.clone();
        let mut n = 2;
        while self.used.contains(&name) {
            name = format!("{base}_{n}");
            n += 1;
        }
        self.used.insert(name.clone());
        self.names.insert(id, name);
    }

    fn assign_names(&mut self, f: &Function) {
        for p in &f.params {
            self.name_local(*p);
        }
        self.names_block(&f.body);
    }

    fn names_block(&mut self, b: &Block) {
        for s in &b.stmts {
            match &s.kind {
                StmtKind::Local { locals, values } => {
                    for v in values {
                        self.names_expr(v);
                    }
                    for l in locals {
                        self.name_local(*l);
                    }
                }
                StmtKind::Assign { targets, values } => {
                    for t in targets {
                        match t {
                            Place::Local(l) => self.name_local(*l),
                            Place::Index(o, k) => {
                                self.names_expr(o);
                                self.names_expr(k);
                            }
                            Place::Global(_) => {}
                        }
                    }
                    for v in values {
                        self.names_expr(v);
                    }
                }
                StmtKind::Expr(e) => self.names_expr(e),
                StmtKind::Do(b) => self.names_block(b),
                StmtKind::If { cond, then, otherwise } => {
                    self.names_expr(cond);
                    self.names_block(then);
                    if let Some(o) = otherwise {
                        self.names_block(o);
                    }
                }
                StmtKind::While { cond, body } => {
                    self.names_expr(cond);
                    self.names_block(body);
                }
                StmtKind::NumericFor { var, start, limit, step, body } => {
                    self.names_expr(start);
                    self.names_expr(limit);
                    if let Some(s) = step {
                        self.names_expr(s);
                    }
                    self.name_local(*var);
                    self.names_block(body);
                }
                StmtKind::GenericFor { vars, iter, body } => {
                    for e in iter {
                        self.names_expr(e);
                    }
                    for v in vars {
                        self.name_local(*v);
                    }
                    self.names_block(body);
                }
                StmtKind::Return(vals) => {
                    for v in vals {
                        self.names_expr(v);
                    }
                }
                StmtKind::Hoist { defs, rest } => {
                    self.names_block(defs);
                    self.names_block(rest);
                }
                StmtKind::Break | StmtKind::Goto(_) | StmtKind::Label(_) => {}
            }
        }
    }

    fn names_expr(&mut self, e: &Expr) {
        match e {
            Expr::Local(l) => self.name_local(*l),
            Expr::Index(o, k) => {
                self.names_expr(o);
                self.names_expr(k);
            }
            Expr::Call(f, args) => {
                self.names_expr(f);
                for a in args {
                    self.names_expr(a);
                }
            }
            Expr::MethodCall(o, _, args) => {
                self.names_expr(o);
                for a in args {
                    self.names_expr(a);
                }
            }
            Expr::Function(f) => self.assign_names(f),
            Expr::Binary(_, a, b) => {
                self.names_expr(a);
                self.names_expr(b);
            }
            Expr::Unary(_, a) | Expr::Paren(a) => self.names_expr(a),
            Expr::Table(entries) => {
                for en in entries {
                    match en {
                        TableEntry::Item(v) => self.names_expr(v),
                        TableEntry::Pair(k, v) => {
                            self.names_expr(k);
                            self.names_expr(v);
                        }
                    }
                }
            }
            Expr::MemberCall { receiver, key, args } => {
                self.names_expr(receiver);
                self.names_expr(key);
                for a in args {
                    self.names_expr(a);
                }
            }
            Expr::MemberApply { func, receiver, args } => {
                self.names_expr(func);
                self.names_expr(receiver);
                for a in args {
                    self.names_expr(a);
                }
            }
            Expr::AutoGet { receiver, key } => {
                self.names_expr(receiver);
                self.names_expr(key);
            }
            Expr::Nil | Expr::Bool(_) | Expr::Number(_) | Expr::Str(_) | Expr::Global(_) | Expr::Vararg | Expr::Intrinsic(_) => {}
        }
    }

    fn global_is_bare(&self, name: &str) -> bool {
        is_lua_identifier(name) && !self.used.contains(name)
    }

    /// True when some global must be read through the environment.
    fn needs_env(&self, f: &Function) -> bool {
        let mut found = false;
        visit_globals(&f.body, &mut |g| {
            if !self.global_is_bare(g) {
                found = true;
            }
        });
        found
    }

    fn local(&self, id: LocalId) -> &str {
        self.names.get(&id).map_or("_", String::as_str)
    }

    // ---- statements ------------------------------------------------------------

    /// Moves to the statement's line; or stays on the current one when it
    /// is that line, or when the statement runs no code; or starts a new
    /// output line, recorded in the line map (see the module doc).
    fn at_line(&mut self, s: &Stmt, indent: usize) {
        let line = s.line;
        if line > self.line {
            while self.line < line {
                self.out.push('\n');
                self.line += 1;
            }
            self.line_nl = line;
        } else if line == self.line_nl || line == 0 || !runs_code(s) {
            self.out.push(' ');
            return;
        } else {
            self.out.push('\n');
            self.line += 1;
            self.line_nl = line;
            self.line_map.push((self.line, line));
        }
        for _ in 0..indent {
            self.out.push_str("  ");
        }
    }

    fn newline_count(&mut self, s: &str) {
        self.line += u32::try_from(s.bytes().filter(|&b| b == b'\n').count()).unwrap_or(0);
    }

    fn push(&mut self, s: &str) {
        self.newline_count(s);
        self.out.push_str(s);
    }

    fn block(&mut self, b: &Block, indent: usize) {
        let n = b.stmts.len();
        for (i, s) in b.stmts.iter().enumerate() {
            let last = i + 1 == n;
            // Lua requires `return` and `break` to end their block.
            let wrap = !last && matches!(s.kind, StmtKind::Return(_) | StmtKind::Break);
            self.at_line(s, indent);
            if wrap {
                self.push("do ");
            }
            self.stmt(s, indent);
            if wrap {
                self.push(" end");
            }
        }
    }

    fn stmt(&mut self, s: &Stmt, indent: usize) {
        match &s.kind {
            StmtKind::Local { locals, values } => {
                let names: Vec<String> = locals.iter().map(|l| self.local(*l).to_string()).collect();
                self.push("local ");
                self.push(&names.join(", "));
                if !values.is_empty() {
                    self.push(" = ");
                    self.expr_list(values);
                }
                self.push(";");
            }
            StmtKind::Assign { targets, values } => {
                for (i, t) in targets.iter().enumerate() {
                    if i > 0 {
                        self.push(", ");
                    }
                    match t {
                        Place::Local(l) => {
                            let n = self.local(*l).to_string();
                            self.push(&n);
                        }
                        Place::Global(g) => self.global(g),
                        Place::Index(o, k) => self.index(o, k),
                    }
                }
                self.push(" = ");
                self.expr_list(values);
                self.push(";");
            }
            StmtKind::Expr(e) => {
                match e {
                    Expr::Call(..) | Expr::MethodCall(..) | Expr::MemberCall { .. } | Expr::MemberApply { .. } | Expr::AutoGet { .. } => self.expr(e, 0),
                    _ => {
                        self.push("local _ = ");
                        self.expr(e, 0);
                    }
                }
                self.push(";");
            }
            StmtKind::Do(b) => {
                self.push("do");
                self.block(b, indent + 1);
                self.push(" end");
            }
            StmtKind::If { cond, then, otherwise } => {
                self.push("if ");
                self.expr(cond, 0);
                self.push(" then");
                self.block(then, indent + 1);
                let mut otherwise = otherwise.as_ref();
                while let Some(o) = otherwise {
                    // `else if` with nothing else in the block is `elseif`.
                    if let [s @ Stmt { kind: StmtKind::If { cond, then, otherwise: next }, .. }] = o.stmts.as_slice() {
                        // On the line of its test, as any statement.
                        self.at_line(s, indent);
                        self.push("elseif ");
                        self.expr(cond, 0);
                        self.push(" then");
                        self.block(then, indent + 1);
                        otherwise = next.as_ref();
                    } else {
                        self.push(" else");
                        self.block(o, indent + 1);
                        otherwise = None;
                    }
                }
                self.push(" end");
            }
            StmtKind::While { cond, body } => {
                self.push("while ");
                self.expr(cond, 0);
                self.push(" do");
                self.block(body, indent + 1);
                self.push(" end");
            }
            StmtKind::NumericFor { var, start, limit, step, body } => {
                let v = self.local(*var).to_string();
                self.push("for ");
                self.push(&v);
                self.push(" = ");
                self.expr(start, 0);
                self.push(", ");
                self.expr(limit, 0);
                if let Some(st) = step {
                    self.push(", ");
                    self.expr(st, 0);
                }
                self.push(" do");
                self.block(body, indent + 1);
                self.push(" end");
            }
            StmtKind::GenericFor { vars, iter, body } => {
                let names: Vec<String> = vars.iter().map(|l| self.local(*l).to_string()).collect();
                self.push("for ");
                self.push(&names.join(", "));
                self.push(" in ");
                self.expr_list(iter);
                self.push(" do");
                self.block(body, indent + 1);
                self.push(" end");
            }
            StmtKind::Return(vals) => {
                self.push("return");
                match vals.as_slice() {
                    [] => {}
                    // LuaJIT keeps no frame for a tail call, so an error an
                    // intrinsic raises in one would name the caller's line,
                    // and the frame would be missing from a trace. An
                    // intrinsic is therefore called in an ordinary call
                    // (`return_form` of nl_ir, shared with the bytecode
                    // backend): in parentheses when it has one result,
                    // otherwise as the argument of `__rt.pass`, which
                    // returns all its arguments (runtime/nl_30_member.lua).
                    [v] if return_form(v) == ReturnForm::OneValue => {
                        self.push(" (");
                        self.expr(v, 0);
                        self.push(")");
                    }
                    [v] if return_form(v) == ReturnForm::AllValues => {
                        self.push(" __rt.pass(");
                        self.expr(v, 0);
                        self.push(")");
                    }
                    _ => {
                        self.push(" ");
                        self.expr_list(vals);
                    }
                }
            }
            StmtKind::Break => self.push("break"),
            StmtKind::Goto(l) => {
                let t = format!("goto continue_{}", l.0);
                self.push(&t);
            }
            StmtKind::Label(l) => {
                let t = format!("::continue_{}::", l.0);
                self.push(&t);
            }
            StmtKind::Hoist { defs, rest } => self.hoist(defs, rest, indent),
        }
    }

    /// Hoisted definitions run before the rest of their block, in their
    /// order, but are printed where they stand in the source (by line), so
    /// that every line keeps its NL line number. When they interleave with
    /// other statements, or their order is not the order of their lines,
    /// `goto` runs them first, in their order:
    ///
    /// ```lua
    /// goto h1_d1  ::h1_s0:: do <statements before the first def printed> end goto h1_s1
    /// ::h1_d2:: f = function ... end goto h1_d3 ...   -- the last to run goes to h1_s0
    /// ::h1_s1:: do <statements after it> end goto h1_s2 ...
    /// ```
    fn hoist(&mut self, defs: &Block, rest: &Block, indent: usize) {
        let n = defs.stmts.len();
        // The definitions in the order they are printed: by line.
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by_key(|&i| (defs.stmts.get(i).map_or(0, |s| s.line), i));
        let in_order = order.iter().enumerate().all(|(k, i)| k == *i);
        let def_lines: Vec<u32> = order.iter().filter_map(|&i| defs.stmts.get(i)).map(|s| s.line).collect();
        let last_def = def_lines.iter().copied().max().unwrap_or(0);
        let interleaved = rest.stmts.iter().any(|s| s.line < last_def);
        if !interleaved && in_order {
            self.block(defs, indent);
            self.block(rest, indent);
            return;
        }
        self.hoist_id += 1;
        let h = self.hoist_id;
        // Segment k holds the statements between printed def k and k+1.
        let mut segments: Vec<Vec<&Stmt>> = vec![Vec::new(); n + 1];
        for s in &rest.stmts {
            let k = def_lines.iter().filter(|&&l| l < s.line).count();
            if let Some(seg) = segments.get_mut(k) {
                seg.push(s);
            }
        }
        // Labels are numbered by the order the definitions run in.
        self.push(&format!(" goto h{h}_d1;"));
        for k in 0..=n {
            // Segment k.
            self.push(&format!(" ::h{h}_s{k}::"));
            let seg = segments.get(k).cloned().unwrap_or_default();
            if !seg.is_empty() {
                self.push(" do");
                let b = Block::new(seg.into_iter().cloned().collect());
                self.block(&b, indent + 1);
                self.push(" end");
            }
            if k == n {
                break;
            }
            self.push(&format!(" goto h{h}_s{};", k + 1));
            // The definition printed after segment k.
            let Some(&i) = order.get(k) else { continue };
            if let Some(d) = defs.stmts.get(i) {
                self.at_line(d, indent);
                self.push(&format!("::h{h}_d{}:: ", i + 1));
                self.stmt(d, indent);
                if i + 1 == n {
                    self.push(&format!(" goto h{h}_s0;"));
                } else {
                    self.push(&format!(" goto h{h}_d{};", i + 2));
                }
            }
        }
    }

    // ---- expressions -----------------------------------------------------------

    fn expr_list(&mut self, es: &[Expr]) {
        for (i, e) in es.iter().enumerate() {
            if i > 0 {
                self.push(", ");
            }
            self.expr(e, 0);
        }
    }

    fn global(&mut self, name: &str) {
        if self.global_is_bare(name) {
            self.push(name);
        } else {
            self.push("__G[");
            let s = lua_string(name.as_bytes());
            self.push(&s);
            self.push("]");
        }
    }

    fn index(&mut self, o: &Expr, k: &Expr) {
        self.prefix(o);
        match k {
            Expr::Str(s) if std::str::from_utf8(s).is_ok_and(is_lua_identifier) => {
                self.push(".");
                self.push(std::str::from_utf8(s).unwrap_or("_"));
            }
            _ => {
                self.push("[");
                self.expr(k, 0);
                self.push("]");
            }
        }
    }

    /// An expression used as the base of a call or index.
    fn prefix(&mut self, e: &Expr) {
        match e {
            Expr::Local(_)
            | Expr::Global(_)
            | Expr::Index(..)
            | Expr::Call(..)
            | Expr::MethodCall(..)
            | Expr::Paren(_)
            | Expr::Intrinsic(_)
            | Expr::MemberCall { .. }
            | Expr::MemberApply { .. }
            | Expr::AutoGet { .. } => self.expr(e, PREC_ATOM),
            _ => {
                self.push("(");
                self.expr(e, 0);
                self.push(")");
            }
        }
    }

    fn args(&mut self, args: &[Expr]) {
        self.push("(");
        self.expr_list(args);
        self.push(")");
    }

    fn intrinsic_call(&mut self, name: &str, args: &[&Expr]) {
        self.push("__rt.");
        self.push(name);
        self.push("(");
        for (i, a) in args.iter().enumerate() {
            if i > 0 {
                self.push(", ");
            }
            self.expr(a, 0);
        }
        self.push(")");
    }

    /// Prints `e` in a context of precedence `ctx`, adding parentheses
    /// when `e` binds less tightly.
    fn expr(&mut self, e: &Expr, ctx: u8) {
        match e {
            Expr::Nil => self.push("nil"),
            Expr::Bool(b) => self.push(if *b { "true" } else { "false" }),
            Expr::Number(n) => {
                let s = lua_number(*n);
                self.push(&s);
            }
            Expr::Str(s) => {
                let s = lua_string(s);
                self.push(&s);
            }
            Expr::Local(l) => {
                let n = self.local(*l).to_string();
                self.push(&n);
            }
            Expr::Global(g) => self.global(g),
            Expr::Index(o, k) => self.index(o, k),
            Expr::Call(f, args) => {
                self.prefix(f);
                self.args(args);
            }
            Expr::MethodCall(o, name, args) => {
                // `__rt.lmethod.name(o, args)` (runtime/nl_30_member.lua):
                // Lua's `o:name(args)`, except that a string receiver finds
                // `name` in NL's `String` library, as `o.name` does.
                // The receiver is evaluated once, and a name that is
                // a Lua keyword is spelled as a string key.
                self.push("__rt.lmethod");
                if is_lua_identifier(name) {
                    self.push(".");
                    self.push(name);
                } else {
                    let key = lua_string(name.as_bytes());
                    self.push(&format!("[{key}]"));
                }
                let mut all = vec![(**o).clone()];
                all.extend(args.iter().cloned());
                self.args(&all);
            }
            Expr::Function(f) => self.function(f),
            Expr::Binary(op, a, b) => {
                let (text, prec, right) = binop_info(*op);
                let paren = prec < ctx;
                if paren {
                    self.push("(");
                }
                self.expr(a, if right { prec + 1 } else { prec });
                self.push(" ");
                self.push(text);
                self.push(" ");
                self.expr(b, if right { prec } else { prec + 1 });
                if paren {
                    self.push(")");
                }
            }
            Expr::Unary(op, a) => {
                let paren = PREC_UNARY < ctx;
                if paren {
                    self.push("(");
                }
                self.push(match op {
                    UnOp::Neg => "- ",
                    UnOp::Not => "not ",
                    UnOp::Len => "#",
                });
                self.expr(a, PREC_UNARY);
                if paren {
                    self.push(")");
                }
            }
            Expr::Table(entries) => {
                self.push("{");
                for (i, en) in entries.iter().enumerate() {
                    if i > 0 {
                        self.push(", ");
                    }
                    match en {
                        TableEntry::Item(v) => self.expr(v, 0),
                        TableEntry::Pair(k, v) => {
                            match k {
                                Expr::Str(s) if std::str::from_utf8(s).is_ok_and(is_lua_identifier) => {
                                    self.push(std::str::from_utf8(s).unwrap_or("_"));
                                }
                                _ => {
                                    self.push("[");
                                    self.expr(k, 0);
                                    self.push("]");
                                }
                            }
                            self.push(" = ");
                            self.expr(v, 0);
                        }
                    }
                }
                self.push("}");
            }
            Expr::Vararg => self.push("..."),
            Expr::Paren(inner) => {
                self.push("(");
                self.expr(inner, 0);
                self.push(")");
            }
            Expr::Intrinsic(name) => {
                self.push("__rt.");
                self.push(name);
            }
            Expr::MemberCall { receiver, key, args } => match key.as_ref() {
                // `__rt.member.name(x, args)`: LuaJIT names a library
                // function that raises "bad argument" after the key it was
                // read with, so the message names the member, not `mcall`.
                Expr::Str(k) if std::str::from_utf8(k).is_ok_and(is_lua_identifier) => {
                    self.push("__rt.member.");
                    self.push(std::str::from_utf8(k).unwrap_or("_"));
                    let mut all: Vec<&Expr> = vec![receiver];
                    all.extend(args.iter());
                    self.push("(");
                    for (i, a) in all.iter().enumerate() {
                        if i > 0 {
                            self.push(", ");
                        }
                        self.expr(a, 0);
                    }
                    self.push(")");
                }
                _ => {
                    let mut all: Vec<&Expr> = vec![receiver, key];
                    all.extend(args.iter());
                    self.intrinsic_call("mcall", &all);
                }
            },
            Expr::AutoGet { receiver, key } => self.intrinsic_call("mget", &[receiver, key]),
            // `__rt.mapply(f, x, args)`: the member-call rule on a member
            // read before.
            Expr::MemberApply { func, receiver, args } => {
                let mut all: Vec<&Expr> = vec![func, receiver];
                all.extend(args.iter());
                self.intrinsic_call("mapply", &all);
            }
        }
    }

    fn function(&mut self, f: &Function) {
        let mut params: Vec<String> = f.params.iter().map(|p| self.local(*p).to_string()).collect();
        if f.vararg {
            params.push("...".to_string());
        }
        // `function` on the line where the NL function starts, when that
        // line is ahead: its `linedefined` is then the NL line, as the
        // bytecode backend writes it (a trace names unnamed functions by it).
        if f.line > self.line {
            while self.line < f.line {
                self.out.push('\n');
                self.line += 1;
            }
            self.line_nl = f.line;
            self.out.push_str("  ");
        }
        self.push("function(");
        self.push(&params.join(", "));
        self.push(")");
        let indent = 1;
        self.block(&f.body, indent);
        self.push(" end");
    }
}

/// Calls `f` with every global the block reads or writes.
fn visit_globals(b: &Block, f: &mut dyn FnMut(&str)) {
    fn expr(e: &Expr, f: &mut dyn FnMut(&str)) {
        match e {
            Expr::Global(g) => f(g),
            Expr::Index(o, k) => {
                expr(o, f);
                expr(k, f);
            }
            Expr::Call(c, args) => {
                expr(c, f);
                args.iter().for_each(|a| expr(a, f));
            }
            Expr::MethodCall(o, _, args) => {
                expr(o, f);
                args.iter().for_each(|a| expr(a, f));
            }
            Expr::Function(func) => visit_globals(&func.body, f),
            Expr::Binary(_, a, b) => {
                expr(a, f);
                expr(b, f);
            }
            Expr::Unary(_, a) | Expr::Paren(a) => expr(a, f),
            Expr::Table(entries) => {
                for en in entries {
                    match en {
                        TableEntry::Item(v) => expr(v, f),
                        TableEntry::Pair(k, v) => {
                            expr(k, f);
                            expr(v, f);
                        }
                    }
                }
            }
            Expr::MemberCall { receiver, key, args } => {
                expr(receiver, f);
                expr(key, f);
                args.iter().for_each(|a| expr(a, f));
            }
            Expr::MemberApply { func, receiver, args } => {
                expr(func, f);
                expr(receiver, f);
                args.iter().for_each(|a| expr(a, f));
            }
            Expr::AutoGet { receiver, key } => {
                expr(receiver, f);
                expr(key, f);
            }
            _ => {}
        }
    }
    for s in &b.stmts {
        match &s.kind {
            StmtKind::Local { values, .. } => values.iter().for_each(|v| expr(v, f)),
            StmtKind::Assign { targets, values } => {
                for t in targets {
                    match t {
                        Place::Global(g) => f(g),
                        Place::Index(o, k) => {
                            expr(o, f);
                            expr(k, f);
                        }
                        Place::Local(_) => {}
                    }
                }
                values.iter().for_each(|v| expr(v, f));
            }
            StmtKind::Expr(e) => expr(e, f),
            StmtKind::Do(b) => visit_globals(b, f),
            StmtKind::If { cond, then, otherwise } => {
                expr(cond, f);
                visit_globals(then, f);
                if let Some(o) = otherwise {
                    visit_globals(o, f);
                }
            }
            StmtKind::While { cond, body } => {
                expr(cond, f);
                visit_globals(body, f);
            }
            StmtKind::NumericFor { start, limit, step, body, .. } => {
                expr(start, f);
                expr(limit, f);
                if let Some(s) = step {
                    expr(s, f);
                }
                visit_globals(body, f);
            }
            StmtKind::GenericFor { iter, body, .. } => {
                iter.iter().for_each(|e| expr(e, f));
                visit_globals(body, f);
            }
            StmtKind::Return(vals) => vals.iter().for_each(|v| expr(v, f)),
            StmtKind::Hoist { defs, rest } => {
                visit_globals(defs, f);
                visit_globals(rest, f);
            }
            StmtKind::Break | StmtKind::Goto(_) | StmtKind::Label(_) => {}
        }
    }
}

/// A Lua number literal for `n`.
pub fn lua_number(n: f64) -> String {
    if n.is_nan() {
        return "(0/0)".to_string();
    }
    if n.is_infinite() {
        return if n > 0.0 { "(1/0)".to_string() } else { "(-1/0)".to_string() };
    }
    let text = if n == 0.0 {
        if n.is_sign_negative() { "-0".to_string() } else { "0".to_string() }
    } else if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else if n.abs() >= 1e15 || n.abs() < 1e-5 {
        format!("{n:e}")
    } else {
        format!("{n}")
    };
    if text.starts_with('-') { format!("({text})") } else { text }
}

/// A Lua string literal for `bytes`.
pub fn lua_string(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() + 2);
    s.push('"');
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        match b {
            b'"' => s.push_str("\\\""),
            b'\\' => s.push_str("\\\\"),
            b'\n' => s.push_str("\\n"),
            b'\r' => s.push_str("\\r"),
            b'\t' => s.push_str("\\t"),
            0x20..=0x7e => s.push(b as char),
            _ => {
                // Valid UTF-8 is kept as written; anything else is escaped
                // as a three-digit decimal escape.
                let len = bytes.get(i..).and_then(utf8_len);
                match len.and_then(|l| bytes.get(i..i + l)).and_then(|c| std::str::from_utf8(c).ok()) {
                    Some(ch) if b >= 0x80 => {
                        s.push_str(ch);
                        i += ch.len();
                        continue;
                    }
                    _ => {
                        let _ = write!(s, "\\{b:03}");
                    }
                }
            }
        }
        i += 1;
    }
    s.push('"');
    s
}

fn utf8_len(bytes: &[u8]) -> Option<usize> {
    let b = *bytes.first()?;
    match b {
        0xc2..=0xdf => Some(2),
        0xe0..=0xef => Some(3),
        0xf0..=0xf4 => Some(4),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        assert_eq!(lua_number(1.0), "1");
        assert_eq!(lua_number(-5.0), "(-5)");
        assert_eq!(lua_number(1.5), "1.5");
        assert_eq!(lua_number(5e20), "5e20");
        assert_eq!(lua_number(0.00457), "0.00457");
    }

    /// A Lua method call goes through `__rt.lmethod`, with the receiver
    /// evaluated once; a name that is a Lua keyword is a string key.
    #[test]
    fn keyword_method_calls() {
        let o = LocalId(0);
        let call_on = |receiver: Expr| Expr::MethodCall(Box::new(receiver), "end".to_string(), vec![Expr::Number(1.0)]);
        let main = Function {
            params: Vec::new(),
            vararg: true,
            body: Block::new(vec![
                Stmt { line: 1, kind: StmtKind::Local { locals: vec![o], values: vec![Expr::Table(Vec::new())] } },
                Stmt { line: 1, kind: StmtKind::Expr(call_on(Expr::Local(o))) },
                Stmt { line: 1, kind: StmtKind::Expr(call_on(Expr::call(Expr::Global("mk".to_string()), Vec::new()))) },
            ]),
            line: 1,
            name: None,
            method: false,
            transient: false,
        };
        let chunk = Chunk { locals: vec![LocalInfo { hint: "o".to_string() }], main, name: "=t".to_string(), owner: None };
        let lua = print_chunk(&chunk);
        assert!(lua.contains(r#"__rt.lmethod["end"](o, 1)"#), "{lua}");
        assert!(lua.contains(r#"__rt.lmethod["end"](mk(), 1)"#), "{lua}");
    }

    #[test]
    fn strings() {
        assert_eq!(lua_string(b"a\"b\n"), "\"a\\\"b\\n\"");
        assert_eq!(lua_string("é".as_bytes()), "\"é\"");
        assert_eq!(lua_string(&[0xff, b'1']), "\"\\2551\"");
    }
}
