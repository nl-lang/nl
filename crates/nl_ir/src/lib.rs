//! The core IR of NL and the lowering from the syntax tree.
//!
//! - [`ir`]: the IR itself, shared by every backend.
//! - [`lower()`]: syntax tree plus [`nl_sema::Resolution`] to IR.
//!
//! # How lowering is organized
//!
//! The [`lower`] module holds the context ([`lower::Lowerer`]): the
//! statement buffer, locals, temporaries, the function and loop stacks, and
//! the two dispatchers, `stmt` (statements) and `expr` (expressions). Each
//! feature area has a module of its own beside it, with `impl Lowerer`
//! blocks; the dispatchers only call into them:
//!
//! | Module | Area |
//! |---|---|
//! | `expr` | literals, names, tables, strings, indexing, `.len` |
//! | `operator` | unary and binary operators, ranges, `and`/`or` |
//! | `nilsafe` | nil-safe chains `?.`, `??` and `??=` |
//! | `call` | calls, member calls, auto-call, pipe, trailing blocks |
//! | `func` | functions, parameters, defaults, `*args`, returns |
//! | `control` | `if`, `unless`, `cond`, `case` on values, loops, `for` |
//! | `assign` | `let`, `const`, `global`, assignment, compound assignment |
//! | `types` | runtime type values, type pools and checks, `is`, `as` |
//! | `lua_path` | Lua paths |
//! | `localize` | Lua paths read once into hidden locals (`--localize`) |
//! | `structs` | structs, enums, methods on types |
//! | `module` | `module` declarations |
//! | `pattern` | patterns |
//! | `error` | `try`/`catch`/`finally` |
//! | `event` | `on` |
//! | `task` | `async` |
//! | `spawn` | `spawn` |
//! | `iface` | interfaces |
//! | `opdef` | operator definitions (`fn Vec.+`) |
//! | `import` | `import` and the export table |
//! | `lift` | the closures of `try` made once, on the finished chunk |
//! | `limits` | LuaJIT's limits on the finished chunk (locals, upvalues) |
//!
//! # Conventions
//!
//! - Expressions may need statements (an `if` used as a value, a pattern
//!   test). `expr` returns an [`ir::Expr`] and may emit statements into the
//!   current buffer before it; lists of operands are lowered with
//!   `operands`, which spills earlier operands to temporaries when a later
//!   one emits statements, so evaluation order is kept; a
//!   local that a closure assigns is also copied before a later operand
//!   that calls code (`Lowerer::pin`), since the IR reads a local where
//!   the operation happens, as Lua does.
//! - Temporaries are scoped to the statement that needs them, so they do
//!   not accumulate in a block; what a program's own declarations make
//!   exceed LuaJIT's limits is handled by `limits`.
//! - Whether an expression statement's value is discarded is the
//!   resolver's decision (`Resolution::value_discarded`); so is whether a
//!   dotted name in a pattern is an enum variant.
//! - Every runtime-library function is reached through
//!   [`ir::Expr::Intrinsic`]; the names are those of the `rt` table in
//!   `runtime/` (see its header). The few values the inline type checks
//!   read (`rt.type`, `rt.getmetatable`, `rt.setmetatable`, `rt.KW`) are
//!   copied from there once, into locals of the main function, when the
//!   chunk starts (`Lowerer::rt_cached`).
//! - Lua paths of the standard libraries (`:string.format`) are likewise
//!   read once, after the file's imports, into hidden locals of the main
//!   function, as `LowerOptions::localize` says (`localize`).
//! - Lowering runs only on files the resolver accepted, so it may assume
//!   names are resolved; where something is still missing it reports an
//!   internal diagnostic instead of panicking.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing))]

pub mod ir;
pub mod lower;

pub use lower::{LUA_LIBRARY_FIELDS, Localize, LowerDiagnostic, LowerOptions, LowerOutput, STD_LIBRARIES, declared_globals, directive, lower};
