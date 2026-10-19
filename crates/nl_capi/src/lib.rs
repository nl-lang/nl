//! The NL compiler as a library.
//!
//! - [`driver`]: the compile entry point, in safe Rust. `nlc` and Rust hosts
//!   call it directly.
//! - The C ABI over it (`nl_compile`, declared in `nlc.h`) is what the VM
//!   links as `libnlc`.
//! - [`bundle`], [`minify`] and [`sourcemap`]: the packager behind `nlc
//!   bundle`, for hosts as well: in Rust here, and in C through
//!   `include/nlbundle.h` ([`capi_bundle`]), which a host of the VM (Garry's
//!   Mod's `lua_shared`) calls to package NL for clients, minify Lua, and
//!   map the positions of errors back to their sources.

/// The packager (`nlc bundle`, `nlb_bundle`).
pub mod bundle;
/// The per-process cache of compiled files.
pub mod cache;
pub mod driver;
/// The Lua and GLua minifier (`nlb_minify`).
pub mod minify;
/// Bundle sourcemaps and the remapping of positions (`nlb_sourcemap_*`).
pub mod sourcemap;
pub mod search;
/// Source snippets under diagnostics.
pub mod snippet;

// The regex and text libraries. Re-exported so that
// the crates are linked and their `nlregex_*` and `nltext_*` C ABI symbols
// (LuaJIT/src/nlregex.h, nltext.h) are in the staticlib: Cargo only links a
// dependency that is referenced.
pub use nl_regex;
pub use nl_text;

// The C ABI of nlc.h (LuaJIT/src/nlc.h), and of nlbundle.h
// (include/nlbundle.h): the only modules allowed unsafe code.
#[allow(unsafe_code)]
pub mod capi;
#[allow(unsafe_code)]
pub mod capi_bundle;

pub use driver::{CompileOptions, CompileOutput, Diagnostic, Emit, Loader, Localize, Profile, Severity, compile};
