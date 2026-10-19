//! The NL compiler as a library.
//!
//! - [`driver`]: the compile entry point, in safe Rust. `nlc` and Rust hosts
//!   call it directly.
//! - The C ABI over it (`nl_compile`, declared in `nlc.h`) is what the VM
//!   links as `libnlc`.

/// The per-process cache of compiled files.
pub mod cache;
pub mod driver;
pub mod search;
/// Source snippets under diagnostics.
pub mod snippet;

// The regex and text libraries. Re-exported so that
// the crates are linked and their `nlregex_*` and `nltext_*` C ABI symbols
// (LuaJIT/src/nlregex.h, nltext.h) are in the staticlib: Cargo only links a
// dependency that is referenced.
pub use nl_regex;
pub use nl_text;

// The C ABI of nlc.h (LuaJIT/src/nlc.h). The only module allowed unsafe code.
#[allow(unsafe_code)]
pub mod capi;

pub use driver::{CompileOptions, CompileOutput, Diagnostic, Emit, Loader, Localize, Profile, Severity, compile};
