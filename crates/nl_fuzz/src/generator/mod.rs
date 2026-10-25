//! The program generator: a seed in, a well-formed NL program out.
//!
//! A generated program must mean the same thing on every backend and with
//! every option, so it stays inside the part of the language whose output
//! is defined exactly:
//!
//! - **Numbers stay exact and finite.** Every numeric expression carries a
//!   bound on its magnitude; integers stay far below 2^53, divisors and
//!   moduli are nonzero literals, and bitwise operands are integers
//!   (the `bit` library is only defined there). A variable has
//!   a bound of its own, and an assignment that could exceed it is wrapped
//!   in `% bound`.
//! - **Iteration order is defined.** Maps are only walked through sorted
//!   keys (`sorted_keys` in the program's prelude); lists never hold `nil`.
//! - **Programs end.** Loops count to small literal limits, the counter of
//!   a `while` loop is the first thing its body changes, nothing appends to
//!   a list while it is walked, and functions only call functions defined
//!   before them. A cost estimate keeps the trace short.
//! - **The resolver and the analyzer accept it**: only
//!   declared names, no discarded values (a statement that is not last is a
//!   call, an assignment or a construct whose branches end in one), fresh
//!   names so that nothing shadows, every binding read at least once, and
//!   one fixed type per variable.
//! - **Errors are raised on purpose.** `raise` appears only where a `try`
//!   catches it: in a `try` body, or in a function (or block) that is then
//!   only called inside one; always behind a condition. Programs that catch
//!   a failed runtime check (`TypeError`) are marked
//!   checked-only, since an unchecked build does not raise it.
//! - **Output does not depend on the sign of zero.** Every number that is
//!   printed or turned into text is written `x + 0`, which makes `-0`
//!   `0`. A dual-number LuaJIT (ARM64) keeps a value as an integer or as
//!   a double depending on whether the JIT compiled the code, and only a
//!   double can be `-0`: `x * 0` with a negative `x` prints `-0` or `0`
//!   on both backends. `//` itself is canonical; `*`, unary
//!   `-`, `Number.floor` and `Number.ceil` are not. Outputs are compared
//!   byte for byte.
//!
//! The program prints a trace of what it computes, and at its end the
//! value of every file-level variable, so that two runs can be compared
//! line by line. Code is built as text; each construct knows its own
//! indentation, and blocks are indented by their parent.

mod decl;
mod expr;
mod extra;
mod stmt;

use crate::rng::Rng;

/// A generated program.
#[derive(Clone, Debug)]
pub struct Program {
    pub seed: u64,
    pub source: String,
    /// The program catches a `TypeError` from a runtime check on purpose,
    /// so it is not run with `--unchecked` (tests/run/README.md).
    pub checked_only: bool,
}

/// The program of a seed. The same seed gives the same text on every
/// machine.
pub fn generate(seed: u64) -> Program {
    Gen::new(seed).program()
}

/// The largest magnitude an integer expression may reach: products of two
/// bounded values stay exact in a double (2^53).
const INT_CAP: f64 = 1.0e12;
/// The largest magnitude of a product's operands' bounds multiplied.
const MUL_CAP: f64 = 1.0e11;
/// The longest string, in characters, an expression may produce.
const STR_CAP: f64 = 48.0;
/// The longest a list may grow.
const LIST_CAP: f64 = 40.0;
/// The estimated number of trace lines and loop iterations a program may
/// cost; it keeps the output and the run time small.
const COST_CAP: f64 = 2500.0;
/// The same for one function call.
const FN_COST_CAP: f64 = 120.0;

/// The types the generator keeps track of. Every variable has one, fixed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
enum Ty {
    /// A number with an integer value.
    #[default]
    Int,
    /// Any finite number.
    Num,
    Str,
    Bool,
    IntList,
    StrList,
    /// A table with string keys and integer values.
    Map,
    Struct(usize),
    Enum(usize),
}

/// An expression: its text, its type and what is known of its value.
#[derive(Clone, Debug, Default)]
struct Ex {
    code: String,
    ty: Ty,
    /// Numbers: the largest magnitude. Strings: the most characters.
    /// Lists: the most items.
    bound: f64,
    /// Lists and maps: the bound of an item.
    elem: f64,
    /// Strings: every character is ASCII, so byte positions are character
    /// positions.
    ascii: bool,
    /// The text is a single token or a call, so it needs no parentheses
    /// as an operand.
    atomic: bool,
    /// Lists: the fewest items (a literal's count).
    min_len: usize,
}

impl Ex {
    fn new(code: impl Into<String>, ty: Ty, bound: f64) -> Ex {
        Ex { code: code.into(), ty, bound, elem: 0.0, ascii: true, atomic: false, min_len: 0 }
    }
    fn atom(mut self) -> Ex {
        self.atomic = true;
        self
    }
    fn elem(mut self, e: f64) -> Ex {
        self.elem = e;
        self
    }
    fn ascii(mut self, a: bool) -> Ex {
        self.ascii = a;
        self
    }
    /// The text as an operand of a binary operator.
    fn operand(&self) -> String {
        if self.atomic && !self.code.starts_with('-') { self.code.clone() } else { format!("({})", self.code) }
    }
}

/// A variable in scope.
#[derive(Clone, Debug, Default)]
struct Var {
    name: String,
    ty: Ty,
    bound: f64,
    elem: f64,
    ascii: bool,
    /// The fewest items a list has: `l[i] = v` stays inside it.
    min_len: usize,
    /// `let`, a parameter: may be assigned. `const` and pattern
    /// variables of a `case` are never assigned here.
    mutable: bool,
    used: bool,
    /// The function depth it was declared at (0: file level).
    fn_depth: usize,
    /// Declared with a type: what is assigned to it is checked, so a
    /// string the analyzer thinks may be `nil` is given a default.
    typed: bool,
}

/// A parameter of a generated function.
#[derive(Clone, Debug, Default)]
struct Param {
    name: String,
    ty: Ty,
    bound: f64,
    default: Option<String>,
}

/// What a function returns.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Ret {
    Nothing,
    One(Ty, f64),
    /// Two integers, as a value list.
    Pair(f64),
}

/// How a function is reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FnKind {
    /// `fn name`: a function-kind name.
    Decl,
    /// `let name = fn ... end`: also function-kind.
    Let,
    /// A variable that holds a function (a closure from a factory, a loop
    /// variable over a list of functions, `let g = f!`).
    Value,
    /// A function of a module, `Mod.f`: a method of the module, so it is
    /// never passed with `!`.
    Module,
}

/// A function the program can call.
#[derive(Clone, Debug)]
struct Func {
    name: String,
    params: Vec<Param>,
    /// `*rest`: extra integer arguments.
    rest: bool,
    /// `**opts` and the option keys the body reads.
    opts: Vec<String>,
    ret: Ret,
    /// Trace lines and iterations of one call.
    cost: f64,
    /// It may raise, so it is only called inside a `try` body.
    raises: bool,
    /// File-level variables it assigns: it is not called while one of
    /// them is frozen.
    assigns: Vec<String>,
    /// It takes a trailing block (a `Function` parameter):
    /// the block gets an integer within `.0` and must return one within
    /// `.1`.
    block: Option<(f64, f64)>,
    kind: FnKind,
    /// Its parameters are annotated (all or none).
    typed: bool,
}

/// A field of a generated struct.
#[derive(Clone, Debug)]
struct Field {
    name: String,
    ty: Ty,
    bound: f64,
    typed: bool,
    default: Option<String>,
}

/// A method of a generated struct.
#[derive(Clone, Debug)]
struct Method {
    name: String,
    /// Integer parameters, with their bounds.
    params: Vec<f64>,
    /// `None`: it changes a field and returns nothing.
    ret: Option<f64>,
    /// Trace lines and iterations of one call.
    cost: f64,
}

#[derive(Clone, Debug)]
struct StructDef {
    name: String,
    parent: Option<usize>,
    fields: Vec<Field>,
    methods: Vec<Method>,
    /// `interface To<String> for` it: interpolation and `as String` work.
    to_string: bool,
    /// `fn S.+`: adds two instances field by field.
    plus: bool,
    /// `fn S.len`.
    len: Option<f64>,
    /// `fn S.<` and `fn S.<=`.
    lt: bool,
    /// `fn S.==`.
    eq: bool,
    /// `fn S.()(k)`: an instance can be called.
    call_op: bool,
    /// `fn S.make(k): Self`.
    make: Option<String>,
}

/// An interface with one method returning a string.
#[derive(Clone, Debug, Default)]
struct Iface {
    name: String,
    method: String,
    /// The structs it is implemented for.
    impls: Vec<usize>,
    /// Implemented for `Number` too.
    number: bool,
}

#[derive(Clone, Debug)]
enum EnumVal {
    /// The variant's name, as a string.
    Name,
    Str(String),
    Num(i64),
}

#[derive(Clone, Debug)]
struct EnumDef {
    name: String,
    variants: Vec<(String, EnumVal)>,
}

/// One lexical scope.
#[derive(Debug, Default)]
struct Scope {
    vars: Vec<Var>,
    funcs: Vec<Func>,
}

/// Where a statement is being generated.
#[derive(Clone, Debug)]
struct Ctx {
    /// How deeply blocks nest here; it limits recursion.
    depth: u32,
    /// Loops around this point inside the current function.
    loops: u32,
    /// How many times this point runs per run of the enclosing function
    /// (or of the file).
    mult: f64,
    /// `raise` is allowed here, and so are calls of functions that may
    /// raise: inside a `try` body, or in a function allowed to raise.
    raise_ok: bool,
    /// A `try` of the current function encloses this point: what is
    /// raised here does not leave the function.
    caught: bool,
    /// Inside a method of this struct: `@field` is allowed.
    method_of: Option<usize>,
}

impl Frame {
    fn new(ret: Ret, ret_cap: f64) -> Frame {
        Frame { ret, ret_cap, ret_bound: 0.0, cost: 0.0, raises: false, assigns: Vec::new() }
    }
}

impl Ctx {
    fn top() -> Ctx {
        Ctx { depth: 0, loops: 0, mult: 1.0, raise_ok: false, caught: false, method_of: None }
    }
    fn nested(&self) -> Ctx {
        Ctx { depth: self.depth + 1, ..self.clone() }
    }
    fn in_loop(&self, iterations: f64) -> Ctx {
        Ctx { depth: self.depth + 1, loops: self.loops + 1, mult: self.mult * iterations.max(1.0), ..self.clone() }
    }
}

/// A function being generated: what its `return`s give and what it
/// costs.
#[derive(Debug)]
struct Frame {
    ret: Ret,
    /// Every returned integer is wrapped to stay within this.
    ret_cap: f64,
    /// The largest bound of a returned value so far.
    ret_bound: f64,
    cost: f64,
    raises: bool,
    assigns: Vec<String>,
}

struct Gen {
    rng: Rng,
    seed: u64,
    names: u32,
    scopes: Vec<Scope>,
    structs: Vec<StructDef>,
    /// Exception structs (`struct E : Exception` with a `code` field).
    errors: Vec<String>,
    enums: Vec<EnumDef>,
    /// The function being generated, innermost last.
    frames: Vec<Frame>,
    /// Trace lines and iterations of the file's own code.
    cost: f64,
    /// Variables that must not change here: loop counters and the
    /// collections being walked.
    frozen: Vec<String>,
    checked_only: bool,
    /// Methods defined on `Number` (one integer parameter) and on
    /// `String` (none).
    prim_num: Vec<String>,
    prim_str: Vec<String>,
    /// Modules: name, constant, its value.
    modules: Vec<(String, String, i64)>,
    ifaces: Vec<Iface>,
    /// Generic functions of two parameters of one type.
    generics: Vec<String>,
}

impl Gen {
    fn new(seed: u64) -> Gen {
        Gen {
            rng: Rng::new(seed),
            seed,
            names: 0,
            scopes: vec![Scope::default()],
            structs: Vec::new(),
            errors: Vec::new(),
            enums: Vec::new(),
            frames: Vec::new(),
            cost: 0.0,
            frozen: Vec::new(),
            checked_only: false,
            prim_num: Vec::new(),
            prim_str: Vec::new(),
            modules: Vec::new(),
            ifaces: Vec::new(),
            generics: Vec::new(),
        }
    }

    /// A name no other binding of the program has, so that nothing
    /// shadows.
    fn fresh(&mut self, prefix: &str) -> String {
        self.names += 1;
        format!("{prefix}{}", self.names)
    }

    /// An anonymous function whose body is one expression: `fn(a, b) e
    /// end`, or one of the short forms that mean the same:
    /// `|a, b| -> e`, `a -> e`, `|| -> e`. `stmt` says it is the last
    /// statement of a function: there a line that starts with `|` or `||`
    /// would continue the statement before it, so the
    /// forms with pipes are returned with `return`.
    fn fn_literal(&mut self, params: &[String], body: &str, stmt: bool) -> String {
        if self.rng.chance(50) {
            return format!("fn({}) {body} end", params.join(", "));
        }
        let ret = if stmt { "return " } else { "" };
        match params {
            [] => format!("{ret}|| -> {body}"),
            [p] if self.rng.chance(60) => format!("{p} -> {body}"),
            _ => format!("{ret}|{}| -> {body}", params.join(", ")),
        }
    }

    fn fn_depth(&self) -> usize {
        self.frames.len()
    }

    // ---- scopes ----

    fn push_scope(&mut self) {
        self.scopes.push(Scope::default());
    }

    /// Closes a scope; returns the statements that read the variables
    /// nothing read, so that no unused-binding lint fires.
    fn pop_scope(&mut self, ctx: &Ctx) -> Vec<String> {
        let scope = self.scopes.pop().unwrap_or_default();
        let mut out = Vec::new();
        for v in scope.vars.iter().filter(|v| !v.used) {
            out.push(self.show_stmt(&v.name, v.ty));
            self.add_cost(ctx.mult);
        }
        out
    }

    fn declare(&mut self, v: Var) {
        if let Some(s) = self.scopes.last_mut() {
            s.vars.push(v);
        }
    }

    fn declare_fn(&mut self, f: Func) {
        if let Some(s) = self.scopes.last_mut() {
            s.funcs.push(f);
        }
    }

    /// A visible variable of the type, marked used. `assign` asks for one
    /// that may be assigned here.
    fn pick_var(&mut self, ty: Ty, assign: bool) -> Option<Var> {
        let mut found: Vec<(usize, usize)> = Vec::new();
        for (si, s) in self.scopes.iter().enumerate() {
            for (vi, v) in s.vars.iter().enumerate() {
                if v.ty != ty {
                    continue;
                }
                if assign && (!v.mutable || self.frozen.contains(&v.name)) {
                    continue;
                }
                // A function assigns only its own variables and scalars
                // of the file: lists and maps of outer scopes keep their
                // length, so that walking them always ends.
                if assign && v.fn_depth < self.fn_depth() && !matches!(ty, Ty::Int | Ty::Num | Ty::Str | Ty::Bool) {
                    continue;
                }
                found.push((si, vi));
            }
        }
        let (si, vi) = found.get(self.rng.below(found.len())).copied()?;
        let v = self.scopes.get_mut(si)?.vars.get_mut(vi)?;
        v.used = true;
        let v = v.clone();
        if assign && v.fn_depth < self.fn_depth() {
            for f in &mut self.frames {
                if !f.assigns.contains(&v.name) {
                    f.assigns.push(v.name.clone());
                }
            }
        }
        Some(v)
    }

    /// Updates what is known of a variable after an assignment.
    fn update_var(&mut self, name: &str, f: impl FnOnce(&mut Var)) {
        for s in self.scopes.iter_mut().rev() {
            if let Some(v) = s.vars.iter_mut().find(|v| v.name == name) {
                f(v);
                return;
            }
        }
    }

    /// The functions visible here that may be called here.
    fn callable(&self, ctx: &Ctx, pred: impl Fn(&Func) -> bool) -> Vec<Func> {
        let mut out = Vec::new();
        for s in &self.scopes {
            for f in &s.funcs {
                if f.raises && !ctx.raise_ok {
                    continue;
                }
                if f.assigns.iter().any(|a| self.frozen.contains(a)) {
                    continue;
                }
                if f.cost * ctx.mult + self.current_cost() > self.cost_cap() {
                    continue;
                }
                if pred(f) {
                    out.push(f.clone());
                }
            }
        }
        out
    }

    // ---- cost ----

    fn current_cost(&self) -> f64 {
        self.frames.last().map_or(self.cost, |f| f.cost)
    }

    fn cost_cap(&self) -> f64 {
        if self.frames.is_empty() { COST_CAP } else { FN_COST_CAP }
    }

    fn add_cost(&mut self, c: f64) {
        match self.frames.last_mut() {
            Some(f) => f.cost += c,
            None => self.cost += c,
        }
    }

    /// Room for `c` more.
    fn affordable(&self, c: f64) -> bool {
        self.current_cost() + c <= self.cost_cap()
    }

    /// A block runs inside the call it is passed to: what it raises
    /// arrives here.
    fn block_raises(&mut self, ctx: &Ctx, block: &Frame) {
        if block.raises && !ctx.caught {
            if let Some(f) = self.frames.last_mut() {
                f.raises = true;
            }
        }
        let assigns = block.assigns.clone();
        if let Some(f) = self.frames.last_mut() {
            for a in assigns {
                if !f.assigns.contains(&a) {
                    f.assigns.push(a);
                }
            }
        }
    }

    /// Notes the effects of calling `f` here.
    fn note_call(&mut self, f: &Func, ctx: &Ctx) {
        self.add_cost(f.cost * ctx.mult + ctx.mult);
        let raises = f.raises && !ctx.caught;
        let assigns = f.assigns.clone();
        if let Some(fr) = self.frames.last_mut() {
            fr.raises |= raises;
            for a in assigns {
                if !fr.assigns.contains(&a) {
                    fr.assigns.push(a);
                }
            }
        }
    }

    // ---- the program ----

    fn program(mut self) -> Program {
        let mut body: Vec<String> = Vec::new();
        let ctx = Ctx::top();
        // Declarations first: they are hoisted anyway, and
        // methods (`fn S.m`, not hoisted) must run before any use.
        let n_enums = self.rng.below(3);
        for _ in 0..n_enums {
            body.push(self.enum_decl());
        }
        let n_structs = self.rng.below(4);
        for _ in 0..n_structs {
            body.extend(self.struct_decl());
        }
        for i in 0..self.structs.len() {
            body.extend(self.struct_extras(i));
        }
        let n_errors = self.rng.below(3);
        for _ in 0..n_errors {
            body.push(self.error_decl());
        }
        for _ in 0..self.rng.below(3) {
            body.push(self.prim_method_decl());
        }
        if self.rng.chance(30) {
            body.push(self.module_decl());
        }
        if self.rng.chance(30) {
            body.extend(self.iface_decl());
        }
        if self.rng.chance(25) {
            body.push(self.generic_decl());
        }
        body.push(PRELUDE.trim_end().to_string());
        let items = 12 + self.rng.below(24);
        for _ in 0..items {
            if self.rng.chance(30) {
                if let Some(f) = self.fn_decl(&ctx) {
                    body.push(f);
                    // Call what was just declared, so that its body runs.
                    if self.rng.chance(60) {
                        if let Some(s) = self.call_latest(&ctx) {
                            body.push(s);
                        }
                    }
                    continue;
                }
            }
            body.push(self.stmt(&ctx));
        }
        if self.rng.chance(12) {
            body.push(self.type_error_probe(&ctx));
        }
        // The final trace: every file-level variable. It is also what
        // keeps them from being unused.
        let vars: Vec<(String, Ty)> = self.scopes.first().map(|s| s.vars.iter().map(|v| (v.name.clone(), v.ty)).collect()).unwrap_or_default();
        for (name, ty) in vars {
            body.push(self.show_stmt(&name, ty));
        }
        let mut source = format!("# nl-fuzz program, seed {}\n", self.seed);
        if self.checked_only {
            source.push_str("# checked-only\n");
        }
        for b in body {
            source.push_str(&b);
            source.push('\n');
        }
        Program { seed: self.seed, source, checked_only: self.checked_only }
    }

    /// A statement that prints a value of any type readably: never a table
    /// address (tests/run/README.md).
    fn show_stmt(&mut self, name: &str, ty: Ty) -> String {
        format!("print({})", self.show(name, ty))
    }

    fn show(&self, code: &str, ty: Ty) -> String {
        let simple = code.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        let code = if simple { code.to_string() } else { format!("({code})") };
        let code = code.as_str();
        match ty {
            Ty::IntList => format!("show_list({code})"),
            Ty::StrList => format!("\"[\" <> Table.concat({code}, \",\") <> \"]\""),
            Ty::Map => format!("show_map({code})"),
            Ty::Struct(i) => {
                let fields = self.all_fields(i);
                if fields.is_empty() {
                    format!("typeof({code})")
                } else {
                    let parts: Vec<String> = fields.iter().map(|f| if f.ty == Ty::Str { format!("#{{{code}.{}}}", f.name) } else { format!("#{{{code}.{} + 0}}", f.name) }).collect();
                    format!("\"{} {}\"", self.structs.get(i).map_or("?", |s| s.name.as_str()), parts.join(" "))
                }
            }
            Ty::Int | Ty::Num => format!("{code} + 0"),
            _ => code.to_string(),
        }
    }

    /// The fields of a struct and of its ancestors, the ancestors' first.
    fn all_fields(&self, i: usize) -> Vec<Field> {
        let mut chain = Vec::new();
        let mut cur = Some(i);
        while let Some(c) = cur {
            if chain.contains(&c) {
                break;
            }
            chain.push(c);
            cur = self.structs.get(c).and_then(|s| s.parent);
        }
        let mut out = Vec::new();
        for c in chain.iter().rev() {
            if let Some(s) = self.structs.get(*c) {
                out.extend(s.fields.iter().cloned());
            }
        }
        out
    }

    /// The methods of a struct and of its ancestors.
    fn all_methods(&self, i: usize) -> Vec<Method> {
        let mut out = Vec::new();
        let mut cur = Some(i);
        let mut seen = Vec::new();
        while let Some(c) = cur {
            if seen.contains(&c) {
                break;
            }
            seen.push(c);
            if let Some(s) = self.structs.get(c) {
                out.extend(s.methods.iter().cloned());
                cur = s.parent;
            } else {
                cur = None;
            }
        }
        out
    }
}

/// Helpers every program has. `sorted_keys` is how a map is walked in a
/// defined order; `show_map`, `show_list` and `show_pair` print a map, a
/// list of numbers and the two values of a call, each number as `x + 0`.
const PRELUDE: &str = r##"
fn sorted_keys(m)
  let ks = []
  for k, _v in m
    Table.insert(ks, k)
  end
  Table.sort(ks)
  ks
end

fn show_map(m)
  let parts = []
  for k in sorted_keys(m)
    Table.insert(parts, "#{k}=#{m[k] + 0}")
  end
  "{" <> Table.concat(parts, ",") <> "}"
end

fn show_list(l)
  let parts = []
  for v in l
    Table.insert(parts, "#{v + 0}")
  end
  "[" <> Table.concat(parts, ",") <> "]"
end

fn show_pair(a, b)
  "#{a + 0}\t#{b + 0}"
end
"##;

/// The bound of a bitwise result: a signed 32-bit integer.
const BIT_BOUND: f64 = 2_147_483_648.0;

/// `e` wrapped in `% b` when its bound exceeds `b`.
fn fit(e: Ex, b: f64) -> Ex {
    if e.bound <= b {
        return e;
    }
    let m = b.max(1.0).floor();
    Ex::new(format!("{} % {}", e.operand(), m as i64), e.ty, m)
}

/// A string for a slot declared `String`: the analyzer types `s[a..b]`
/// as `String?`, so the value gets an `or ""` that never applies.
fn non_nil(e: &Ex) -> String {
    if e.code.contains('\n') { format!("({}) or \"\"", e.code) } else { format!("{} or \"\"", e.operand()) }
}

/// Indents every line of `code` by two spaces.
fn indent(code: &str) -> String {
    let mut out = String::with_capacity(code.len() + 16);
    for (i, line) in code.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        if !line.is_empty() {
            out.push_str("  ");
        }
        out.push_str(line);
    }
    out
}

/// The statements of a block, indented, one per line.
fn block_text(stmts: &[String]) -> String {
    stmts.iter().map(|s| indent(s)).collect::<Vec<_>>().join("\n")
}

/// A float literal for a bound: the integer part, at least 1.
fn bound_lit(b: f64) -> String {
    format!("{}", b.max(1.0).floor() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_program() {
        for seed in 0..20 {
            assert_eq!(generate(seed).source, generate(seed).source);
        }
    }

    #[test]
    fn programs_differ() {
        assert_ne!(generate(1).source, generate(2).source);
    }

    #[test]
    fn indents() {
        assert_eq!(indent("a\n\nb"), "  a\n\n  b");
    }

    /// A map key that names a function of the Table library (`count`) is
    /// never assigned through a dot: on a map, `m.count` is the function,
    /// and the analyzer rejects the assignment (seed 500206 did it).
    #[test]
    fn table_function_names_are_assigned_in_brackets() {
        for seed in (0..300).chain([500206]) {
            let src = generate(seed).source;
            for line in src.lines() {
                // `m.count = …`, `m.count += …`: a dotted target, then an
                // assignment operator.
                let t = line.trim_start();
                let target = t.split_whitespace().next().unwrap_or("");
                let assigns = target.ends_with(".count") && t.get(target.len()..).unwrap_or("").trim_start().starts_with(|c: char| "=+-|*%".contains(c));
                assert!(!assigns, "seed {seed}: {line}");
            }
        }
    }
}
