//! The core IR: a small Lua-like language plus the NL nodes the backends
//! must know.
//!
//! Locals are numbered ([`LocalId`]) and carry a name hint; the backend
//! picks the final names. Control flow is structured. Every statement
//! carries the NL source line it came from.
//!
//! NL nodes:
//!
//! - [`Expr::MemberCall`]: `x.m(args)` under the member-call rule:
//!   `x` is passed as `self` when the value of `m` is a method;
//! - [`Expr::AutoGet`]: `x.m` where the value is used: called with `self`
//!   when it is a method, otherwise the value;
//! - [`Expr::MemberApply`]: the call of a member already read,
//!   `x.m?.(args)`: the member-call rule without a second lookup;
//! - [`Expr::Intrinsic`]: a runtime-library function (`rt.name`), what
//!   `KINTR` loads in the bytecode backend;
//! - type checks and protected regions are calls of intrinsics
//!   (`check`; `xpcall`, `rethrow` and `try`), so a backend needs nothing
//!   more for them.
//!
//! Patterns, default parameters, enums, operator definitions, `async`, Lua
//! paths, `.len`, `on` and the nil-safe operators (`?.`, `??`, `??=`) are
//! lowered away before the IR.

/// Intrinsics with at most one result whose `return` the backends do not
/// make a tail call, because the calling line matters: those that can
/// raise an error that names it; `raise`, which never returns and whose
/// line belongs in the trace; `spawn`, which reports the line that started
/// the job (runtime/nl_64_worker.lua) and returns a task at once. LuaJIT
/// keeps no frame for a tail call, so an error raised in one would name
/// the caller's line.
pub const SINGLE_RESULT_INTRINSICS: &[&str] = &[
    "index", "len", "new", "as", "idiv", "band", "bor", "bxor", "shl", "shr", "ushr", "bnot", "lt", "le", "gt", "ge",
    "raise", "spawn",
];

/// Intrinsics whose `return` stays a tail call: they cannot raise, and
/// only pass values on.
pub const TAIL_INTRINSICS: &[&str] = &["unpack"];

/// How a backend returns the single expression of `return e`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReturnForm {
    /// As a tail call when `e` is a call: a call of user code.
    Tail,
    /// A call of one of [`SINGLE_RESULT_INTRINSICS`]: an ordinary call
    /// whose one value is returned.
    OneValue,
    /// Any other intrinsic call, and an auto-call read (`rt.mget`): an
    /// ordinary call whose values are all returned, so that the frame
    /// that applied the intrinsic is in a trace and an error it raises
    /// names this line (LuaJIT keeps no frame for
    /// a tail call).
    AllValues,
}

/// The [`ReturnForm`] of `return e`.
pub fn return_form(e: &Expr) -> ReturnForm {
    match e {
        Expr::Call(f, _) => match f.as_ref() {
            Expr::Intrinsic(n) if SINGLE_RESULT_INTRINSICS.contains(n) => ReturnForm::OneValue,
            Expr::Intrinsic(n) if TAIL_INTRINSICS.contains(n) => ReturnForm::Tail,
            Expr::Intrinsic(_) => ReturnForm::AllValues,
            _ => ReturnForm::Tail,
        },
        Expr::AutoGet { .. } => ReturnForm::AllValues,
        _ => ReturnForm::Tail,
    }
}

/// A local variable of the chunk. Ids are unique in the chunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LocalId(pub u32);

/// A label, for `continue` (a `goto` to the end of a loop body).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LabelId(pub u32);

/// Information about one local.
#[derive(Clone, Debug)]
pub struct LocalInfo {
    /// The NL name, or a short hint for a temporary.
    pub hint: String,
}

/// A new local of a chunk whose locals are `locals` ([`Chunk::locals`],
/// or the lowering's list before the chunk exists).
pub fn new_local(locals: &mut Vec<LocalInfo>, hint: &str) -> LocalId {
    let id = LocalId(u32::try_from(locals.len()).unwrap_or(u32::MAX));
    locals.push(LocalInfo { hint: hint.to_string() });
    id
}

/// A compiled file.
#[derive(Clone, Debug)]
pub struct Chunk {
    /// Every local of every function, indexed by [`LocalId`].
    pub locals: Vec<LocalInfo>,
    /// The file's main function. It takes no parameters and returns the
    /// export table.
    pub main: Function,
    /// The chunk name (`@path` or `=text`).
    pub name: String,
    /// A local that holds `rt.owner(name)`, set once when the chunk is
    /// loaded, before the main function is made: the owner that the
    /// chunk's definitions of primitive methods and operators pass to the
    /// runtime. `None` when nothing uses it.
    pub owner: Option<LocalId>,
}

impl Chunk {
    /// The name hint of a local.
    pub fn hint(&self, id: LocalId) -> &str {
        self.locals.get(id.0 as usize).map_or("_", |l| l.hint.as_str())
    }
}

/// A function.
#[derive(Clone, Debug, Default)]
pub struct Function {
    pub params: Vec<LocalId>,
    /// Takes `...` after its parameters.
    pub vararg: bool,
    pub body: Block,
    /// The NL line of the definition.
    pub line: u32,
    /// A descriptive name for tracebacks (`Player.damage`).
    pub name: Option<String>,
    /// A method: a function with a `self` parameter, which
    /// the runtime marks (`rt.method`, `rt.opdef`, …). The bytecode
    /// backend at level 1 marks the prototype instead, so its closures are
    /// methods from the moment they are made.
    pub method: bool,
    /// A block or `fn` literal written as an argument that its callee only
    /// calls (a call-only parameter, `nl_sema` callonly.rs): no reference
    /// to it survives the call and nothing can compare it, so lift.rs may
    /// make it once rather than at every evaluation. The backends ignore
    /// it.
    pub transient: bool,
}

/// A sequence of statements; also a scope.
#[derive(Clone, Debug, Default)]
pub struct Block {
    pub stmts: Vec<Stmt>,
}

impl Block {
    pub fn new(stmts: Vec<Stmt>) -> Self {
        Block { stmts }
    }
}

/// A statement and its NL source line.
#[derive(Clone, Debug)]
pub struct Stmt {
    pub line: u32,
    pub kind: StmtKind,
}

#[derive(Clone, Debug)]
pub enum StmtKind {
    /// `local a, b = values` (no values: all nil).
    Local { locals: Vec<LocalId>, values: Vec<Expr> },
    /// `targets = values`.
    Assign { targets: Vec<Place>, values: Vec<Expr> },
    /// An expression evaluated for its effect (normally a call).
    Expr(Expr),
    /// `do ... end`.
    Do(Block),
    /// `if cond then ... else ... end`.
    If { cond: Expr, then: Block, otherwise: Option<Block> },
    /// `while cond do ... end`.
    While { cond: Expr, body: Block },
    /// `for var = start, limit, step do ... end`.
    NumericFor { var: LocalId, start: Expr, limit: Expr, step: Option<Expr>, body: Block },
    /// `for vars in iter do ... end`.
    GenericFor { vars: Vec<LocalId>, iter: Vec<Expr>, body: Block },
    /// `return values`.
    Return(Vec<Expr>),
    /// `break`.
    Break,
    /// `goto label`.
    Goto(LabelId),
    /// `::label::`.
    Label(LabelId),
    /// A block's hoisted function definitions and the rest of
    /// the block. `defs` run first, then `rest`; a backend may lay them out
    /// in source order (by their lines) as long as that holds. Every name
    /// either part assigns is declared before this statement, and `rest`
    /// declares no local that outlives one of its statements.
    Hoist { defs: Block, rest: Block },
}

/// An assignment target.
#[derive(Clone, Debug)]
pub enum Place {
    Local(LocalId),
    /// A Lua global (`global` names, Lua paths).
    Global(String),
    /// `object[key] = …`, a raw Lua index assignment.
    Index(Expr, Expr),
}

/// Binary operators of the IR: Lua's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Mod,
    Pow,
    Concat,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
}

/// Unary operators of the IR: Lua's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
    Len,
}

/// An entry of a table constructor.
#[derive(Clone, Debug)]
pub enum TableEntry {
    /// A positional item.
    Item(Expr),
    /// `[key] = value`.
    Pair(Expr, Expr),
}

#[derive(Clone, Debug)]
pub enum Expr {
    Nil,
    Bool(bool),
    Number(f64),
    /// A string: bytes, as in Lua.
    Str(Vec<u8>),
    Local(LocalId),
    /// A Lua global.
    Global(String),
    /// `object[key]`, a raw Lua index (metamethods apply).
    Index(Box<Expr>, Box<Expr>),
    /// `f(args)`. The last argument may produce several values.
    Call(Box<Expr>, Vec<Expr>),
    /// Lua's `object:name(args)`.
    MethodCall(Box<Expr>, String, Vec<Expr>),
    Function(Box<Function>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Unary(UnOp, Box<Expr>),
    Table(Vec<TableEntry>),
    /// `...` of a vararg function.
    Vararg,
    /// Truncates a multi-value expression to one value: `(f())`.
    Paren(Box<Expr>),
    /// A function of the runtime library, `rt.name`.
    Intrinsic(&'static str),
    /// `x.m(args)` under the member-call rule.
    MemberCall { receiver: Box<Expr>, key: Box<Expr>, args: Vec<Expr> },
    /// `x.m` whose value is used: auto-called when it is a method.
    AutoGet { receiver: Box<Expr>, key: Box<Expr> },
    /// `func(receiver, args)` when `func`, a member of `receiver` read
    /// before, is a method, else `func(args)`: the member-call rule
    /// applied to a value a nil-safe guard holds, so the member
    /// is read once.
    MemberApply { func: Box<Expr>, receiver: Box<Expr>, args: Vec<Expr> },
}

impl Expr {
    /// A string constant.
    pub fn str(s: &str) -> Expr {
        Expr::Str(s.as_bytes().to_vec())
    }

    /// `rt.name(args)`.
    pub fn intrinsic_call(name: &'static str, args: Vec<Expr>) -> Expr {
        Expr::Call(Box::new(Expr::Intrinsic(name)), args)
    }

    /// `f(args)`.
    pub fn call(f: Expr, args: Vec<Expr>) -> Expr {
        Expr::Call(Box::new(f), args)
    }

    pub fn binary(op: BinOp, a: Expr, b: Expr) -> Expr {
        Expr::Binary(op, Box::new(a), Box::new(b))
    }

    pub fn lnot(e: Expr) -> Expr {
        Expr::Unary(UnOp::Not, Box::new(e))
    }

    pub fn index(o: Expr, k: Expr) -> Expr {
        Expr::Index(Box::new(o), Box::new(k))
    }

    /// True for expressions without side effects whose value cannot be
    /// changed by other code: constants. (Locals can be reassigned, so they
    /// are not constant for evaluation-order purposes.)
    pub fn is_constant(&self) -> bool {
        matches!(self, Expr::Nil | Expr::Bool(_) | Expr::Number(_) | Expr::Str(_) | Expr::Intrinsic(_))
    }

    /// True for expressions that may produce several values in a list
    /// position (calls and `...`).
    pub fn is_multi(&self) -> bool {
        matches!(self, Expr::Call(..) | Expr::MethodCall(..) | Expr::MemberCall { .. } | Expr::MemberApply { .. } | Expr::AutoGet { .. } | Expr::Vararg)
    }
}
