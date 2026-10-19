//! Rust declarations of `nl.h`, on the `lua_State` of `mlua-sys`:
//! everything `LuaJIT/src/nl.h` declares today. States and options
//! (`nl_sandboxenv`, `NL_OPT_LOCALIZE`), loading, the loader and reporter,
//! types and values, methods and operators, exceptions, events, tasks, the
//! thread functions `nl_post`, `nl_pump`, `nl_setwakeup`, and the workers
//! (`nl_drain`, `nl_setworkers`, `nl_setworkerinit`, `nl_settransfer`,
//! `nl_spawn`). A unit test compares the declarations with `nl.h` (names,
//! arities, constants).
//!
//! Everything here is `unsafe` and follows the C API's rules, plus Rust's:
//!
//! - Functions marked `e` or `v` in `nl.h` raise Lua errors. Call them inside
//!   a protected call (mlua's `Lua::exec_raw`), never with Rust frames that
//!   must not be unwound between the call and the protected call.
//! - Callbacks that run outside any Lua call (the loader, the reporter, the
//!   wakeup) are `extern "C"`: a panic in them aborts. Callbacks that run
//!   inside the state (`nl_Callback`, `nl_Timer`) are `extern "C-unwind"`,
//!   like `lua_CFunction`, so that they may raise Lua errors.
//!
//! # Linking
//!
//! The VM is the fork of LuaJIT that `mlua-sys` builds when the host patches
//! the `luajit-src` crate. This crate references
//! [`nl_version_0_1`], so a program built against stock LuaJIT fails to link
//! instead of misbehaving at run time. It also depends on `nl_capi`, the
//! compiler library, and holds a reference to each of its symbols that the
//! VM calls (`nl_compile` and the `nlc_*` accessors), so that the linker
//! keeps them for the VM's references.

#![allow(non_camel_case_types, non_snake_case)]

use std::ffi::{c_char, c_double, c_int, c_void};

pub use mlua_sys::{lua_CFunction, lua_Integer, lua_Number, lua_State};

/// `NL_VERSION`.
pub const NL_VERSION: &str = "NL 0.1";
/// `NL_VERSION_NUM`: major*10000 + minor*100 + patch.
pub const NL_VERSION_NUM: c_int = 100;

pub const NL_OPT_CHECKS: c_int = 1;
pub const NL_OPT_STRIP: c_int = 2;
pub const NL_OPT_MAXWORKERS: c_int = 3;
pub const NL_OPT_WORKERMEM: c_int = 4;
pub const NL_OPT_GLUA: c_int = 5;
pub const NL_OPT_SANDBOX: c_int = 6;
pub const NL_OPT_LOCALIZE: c_int = 7;

pub const NL_DIAG_ERROR: c_int = 0;
pub const NL_DIAG_WARNING: c_int = 1;
pub const NL_DIAG_LINT: c_int = 2;

/// The operators of `nl_arith`.
pub const NL_OPADD: c_int = 0;
pub const NL_OPSUB: c_int = 1;
pub const NL_OPMUL: c_int = 2;
pub const NL_OPDIV: c_int = 3;
pub const NL_OPMOD: c_int = 4;
pub const NL_OPPOW: c_int = 5;
pub const NL_OPIDIV: c_int = 6;
pub const NL_OPBAND: c_int = 7;
pub const NL_OPBOR: c_int = 8;
pub const NL_OPBXOR: c_int = 9;
pub const NL_OPSHL: c_int = 10;
pub const NL_OPSHR: c_int = 11;
pub const NL_OPUSHR: c_int = 12;
pub const NL_OPCONCAT: c_int = 13;
/// Unary: one operand.
pub const NL_OPUNM: c_int = 14;
/// Unary: one operand.
pub const NL_OPBNOT: c_int = 15;

/// The comparisons of `nl_compare`.
pub const NL_OPEQ: c_int = 0;
pub const NL_OPLT: c_int = 1;
pub const NL_OPLE: c_int = 2;
pub const NL_OPGT: c_int = 3;
pub const NL_OPGE: c_int = 4;

pub const NL_TASK_PENDING: c_int = 0;
pub const NL_TASK_DONE: c_int = 1;
pub const NL_TASK_FAILED: c_int = 2;

/// `NL_EVENTLIBNAME`.
pub const NL_EVENTLIBNAME: &str = "event";

/// `nl_Loader`. Its callbacks may be called from any thread.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct nl_Loader {
    pub resolve: Option<
        unsafe extern "C" fn(
            data: *mut c_void,
            from: *const c_char,
            path: *const c_char,
            out: *mut c_char,
            size: usize,
        ) -> c_int,
    >,
    pub read: Option<
        unsafe extern "C" fn(data: *mut c_void, name: *const c_char, size: *mut usize) -> *const c_char,
    >,
    pub release: Option<unsafe extern "C" fn(data: *mut c_void, name: *const c_char, buf: *const c_char)>,
    pub data: *mut c_void,
}

/// `nl_Diagnostic`.
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct nl_Diagnostic {
    pub severity: c_int,
    pub code: *const c_char,
    pub message: *const c_char,
    pub file: *const c_char,
    pub line: c_int,
    pub column: c_int,
    pub endline: c_int,
    pub endcolumn: c_int,
}

/// `nl_Reporter`. May be called from any thread.
pub type nl_Reporter = unsafe extern "C" fn(data: *mut c_void, d: *const nl_Diagnostic);

/// `nl_Callback`: a posted callback, or the wake of a timer. Runs on the
/// state's thread in a protected call; `L` is NULL when the state closed
/// first.
pub type nl_Callback = unsafe extern "C-unwind" fn(L: *mut lua_State, data: *mut c_void);

/// `nl_Timer`.
pub type nl_Timer = unsafe extern "C-unwind" fn(
    data: *mut c_void,
    L: *mut lua_State,
    seconds: c_double,
    wake: nl_Callback,
    arg: *mut c_void,
);

/// `nl_Wakeup`. Called from any thread.
pub type nl_Wakeup = unsafe extern "C" fn(data: *mut c_void);

#[allow(unsafe_code)] // Foreign declarations.
unsafe extern "C-unwind" {
    /// `NL_VERSION_SYM`: a link error when the VM is another version, or
    /// stock LuaJIT.
    pub fn nl_version_0_1();

    // States and options.
    pub fn luaopen_nl(L: *mut lua_State) -> c_int;
    pub fn nl_setoption(L: *mut lua_State, opt: c_int, value: lua_Integer) -> c_int;
    pub fn nl_getoption(L: *mut lua_State, opt: c_int) -> lua_Integer;
    pub fn nl_isworker(L: *mut lua_State) -> c_int;
    pub fn nl_workersstarted(L: *mut lua_State) -> c_int;
    pub fn nl_sandboxenv(L: *mut lua_State);

    // Loading.
    pub fn nl_loadbuffer(L: *mut lua_State, buf: *const c_char, size: usize, name: *const c_char) -> c_int;
    pub fn nl_loadfile(L: *mut lua_State, filename: *const c_char) -> c_int;
    pub fn nl_import(L: *mut lua_State, path: *const c_char);
    pub fn nl_addpath(L: *mut lua_State, dir: *const c_char);
    pub fn nl_adddecl(L: *mut lua_State, name: *const c_char);
    pub fn nl_setloader(L: *mut lua_State, loader: *const nl_Loader);
    pub fn nl_setreporter(L: *mut lua_State, f: Option<nl_Reporter>, data: *mut c_void);

    // Types and values.
    pub fn nl_typeof(L: *mut lua_State, idx: c_int) -> *const c_char;
    pub fn nl_typename(L: *mut lua_State, idx: c_int) -> *const c_char;
    pub fn nl_isa(L: *mut lua_State, idx: c_int, ty: c_int) -> c_int;
    pub fn nl_gettype(L: *mut lua_State, name: *const c_char);
    pub fn nl_settype(L: *mut lua_State, idx: c_int, name: *const c_char, parent: c_int);
    pub fn nl_keyed(L: *mut lua_State, idx: c_int);
    pub fn nl_new(L: *mut lua_State, ty: c_int);
    pub fn nl_pushrange(L: *mut lua_State, first: lua_Number, last: lua_Number, exclusive: c_int);
    pub fn nl_torange(
        L: *mut lua_State,
        idx: c_int,
        first: *mut lua_Number,
        last: *mut lua_Number,
        exclusive: *mut c_int,
    ) -> c_int;
    pub fn nlL_checkisa(L: *mut lua_State, narg: c_int, ty: c_int);
    pub fn nlL_typeerror(L: *mut lua_State, narg: c_int, expected: *const c_char) -> c_int;
    pub fn nlL_tolstring(L: *mut lua_State, idx: c_int, len: *mut usize) -> *const c_char;

    // Methods and member calls.
    pub fn nl_markmethod(L: *mut lua_State, idx: c_int);
    pub fn nl_ismethod(L: *mut lua_State, idx: c_int) -> c_int;
    pub fn nlL_setmethods(L: *mut lua_State, l: *const mlua_sys::luaL_Reg);
    pub fn nlL_callmember(L: *mut lua_State, name: *const c_char, nargs: c_int, nresults: c_int);

    // Operators.
    pub fn nl_arith(L: *mut lua_State, op: c_int);
    pub fn nl_compare(L: *mut lua_State, idx1: c_int, idx2: c_int, op: c_int) -> c_int;
    pub fn nl_len(L: *mut lua_State, idx: c_int);

    // Exceptions.
    pub fn nl_pcall(L: *mut lua_State, nargs: c_int, nresults: c_int) -> c_int;
    pub fn nl_raise(L: *mut lua_State) -> c_int;
    pub fn nlL_raise(L: *mut lua_State, ty: *const c_char, fmt: *const c_char, ...) -> c_int;
    pub fn nl_settranslator(L: *mut lua_State);

    // Events.
    pub fn luaopen_event(L: *mut lua_State) -> c_int;
    pub fn nl_runevent(L: *mut lua_State, name: *const c_char, nargs: c_int, nresults: c_int);
    pub fn nl_prunevent(L: *mut lua_State, name: *const c_char, nargs: c_int, nresults: c_int) -> c_int;
    pub fn nl_onevent(L: *mut lua_State, name: *const c_char, handler: *const c_char);
    pub fn nl_offevent(L: *mut lua_State, name: *const c_char, handler: *const c_char);

    // Tasks.
    pub fn nl_newtask(L: *mut lua_State);
    pub fn nl_resolve(L: *mut lua_State, idx: c_int);
    pub fn nl_reject(L: *mut lua_State, idx: c_int);
    pub fn nl_taskstate(L: *mut lua_State, idx: c_int) -> c_int;
    pub fn nl_settimer(L: *mut lua_State, f: Option<nl_Timer>, data: *mut c_void);
    pub fn nl_setunhandled(L: *mut lua_State);

    // Threads and workers.
    pub fn nl_pump(L: *mut lua_State) -> c_int;
    /// Pumps, waiting, until every job and call `L` started has been
    /// delivered.
    pub fn nl_drain(L: *mut lua_State) -> c_int;
    pub fn nl_setwakeup(L: *mut lua_State, f: Option<nl_Wakeup>, data: *mut c_void);
    pub fn nl_setworkers(L: *mut lua_State, n: c_int);
    /// `f` runs in each new worker state as `lua_cpcall(W, f, data)` does, on
    /// the worker's thread: `data` is the light userdata at index 1.
    pub fn nl_setworkerinit(L: *mut lua_State, f: Option<lua_CFunction>, data: *mut c_void);
    /// Pops `encode` (below) and `decode` (on top).
    pub fn nl_settransfer(L: *mut lua_State, ty: *const c_char);
    pub fn nl_spawn(L: *mut lua_State, nargs: c_int);
}

#[allow(unsafe_code)] // A foreign declaration.
unsafe extern "C" {
    /// `nl_post`: callable from any thread. It raises nothing
    /// and never unwinds.
    pub fn nl_post(L: *mut lua_State, f: nl_Callback, data: *mut c_void) -> c_int;
}

/// `nlL_loadstring(L, s)`: `s` is the source and the chunk name.
///
/// # Safety
///
/// As for [`nl_loadbuffer`]; `s` is NUL-terminated.
#[allow(unsafe_code)] // The macro of nl.h, over foreign functions.
pub unsafe fn nlL_loadstring(L: *mut lua_State, s: *const c_char) -> c_int {
    // SAFETY: the caller passes a valid state and a NUL-terminated string.
    unsafe { nl_loadbuffer(L, s, std::ffi::CStr::from_ptr(s).to_bytes().len(), s) }
}

/// `nlL_dostring(L, s)`.
///
/// # Safety
///
/// As for [`nlL_loadstring`] and [`nl_pcall`].
#[allow(unsafe_code)] // The macro of nl.h, over foreign functions.
pub unsafe fn nlL_dostring(L: *mut lua_State, s: *const c_char) -> c_int {
    // SAFETY: the caller's guarantees; both functions raise nothing.
    unsafe { (nlL_loadstring(L, s) != 0 || nl_pcall(L, 0, mlua_sys::LUA_MULTRET) != 0) as c_int }
}

/// `nlL_dofile(L, filename)`.
///
/// # Safety
///
/// As for [`nl_loadfile`] and [`nl_pcall`].
#[allow(unsafe_code)] // The macro of nl.h, over foreign functions.
pub unsafe fn nlL_dofile(L: *mut lua_State, filename: *const c_char) -> c_int {
    // SAFETY: the caller's guarantees; both functions raise nothing.
    unsafe { (nl_loadfile(L, filename) != 0 || nl_pcall(L, 0, mlua_sys::LUA_MULTRET) != 0) as c_int }
}

/// Holds the symbols that must be linked: the version symbol, so that a
/// program built against stock LuaJIT fails to link, and the compiler entry
/// points that the VM calls, so that the linker keeps them from `nl_capi`.
///
/// Left out of this crate's own unit tests: the workspace builds mlua-sys
/// with stock LuaJIT (for `nlc`), and the tests only check layouts.
#[cfg(not(test))]
#[used]
static LINK_REFS: LinkRefs = LinkRefs {
    version: nl_version_0_1,
    compile: nl_capi::capi::nl_compile,
    chunk: nl_capi::capi::nlc_chunk,
    ndiagnostics: nl_capi::capi::nlc_ndiagnostics,
    diagnostic: nl_capi::capi::nlc_diagnostic,
    free: nl_capi::capi::nlc_free,
};

#[cfg(not(test))]
#[allow(dead_code)]
struct LinkRefs {
    version: unsafe extern "C-unwind" fn(),
    compile: unsafe extern "C" fn(
        *const c_char,
        usize,
        *const c_char,
        *const nl_capi::capi::NlcOptions,
        *const nl_capi::capi::NlcLoader,
    ) -> *mut nl_capi::capi::NlcResult,
    chunk: unsafe extern "C" fn(*const nl_capi::capi::NlcResult, *mut usize) -> *const c_char,
    ndiagnostics: unsafe extern "C" fn(*const nl_capi::capi::NlcResult) -> usize,
    diagnostic: unsafe extern "C" fn(*const nl_capi::capi::NlcResult, usize) -> *const nl_capi::capi::NlcDiagnostic,
    free: unsafe extern "C" fn(*mut nl_capi::capi::NlcResult),
}

/// Every C ABI function of the text and regex libraries (`nltext.h`,
/// `nlregex.h`, the `#[no_mangle]` functions of `nl_text/src/abi.rs` and
/// `nl_regex/src/abi.rs`), which only `libluajit.a` calls. A linker that
/// searches each archive once in command-line order (GNU ld) meets
/// `libluajit.a` after those crates, so without these references it would
/// not have pulled the functions in. A unit test checks that the list is
/// complete.
#[cfg(not(test))]
#[used]
static ABI_REFS: AbiRefs = AbiRefs([
    nl_capi::nl_text::abi::nltext_available as *const (),
    nl_capi::nl_text::abi::nltext_invalid_at as *const (),
    nl_capi::nl_text::abi::nltext_count as *const (),
    nl_capi::nl_text::abi::nltext_offset as *const (),
    nl_capi::nl_text::abi::nltext_charpos as *const (),
    nl_capi::nl_text::abi::nltext_slice as *const (),
    nl_capi::nl_text::abi::nltext_charat as *const (),
    nl_capi::nl_text::abi::nltext_case as *const (),
    nl_capi::nl_text::abi::nltext_free as *const (),
    nl_capi::nl_text::abi::nltext_decode as *const (),
    nl_capi::nl_text::abi::nltext_encode as *const (),
    nl_capi::nl_regex::abi::nlregex_available as *const (),
    nl_capi::nl_regex::abi::nlregex_compile as *const (),
    nl_capi::nl_regex::abi::nlregex_free as *const (),
    nl_capi::nl_regex::abi::nlregex_freemsg as *const (),
    nl_capi::nl_regex::abi::nlregex_memory as *const (),
    nl_capi::nl_regex::abi::nlregex_ngroups as *const (),
    nl_capi::nl_regex::abi::nlregex_groupname as *const (),
    nl_capi::nl_regex::abi::nlregex_pattern as *const (),
    nl_capi::nl_regex::abi::nlregex_test as *const (),
    nl_capi::nl_regex::abi::nlregex_find as *const (),
    nl_capi::nl_regex::abi::nlregex_captures as *const (),
]);

#[cfg(not(test))]
#[allow(dead_code)] // Kept, never read.
struct AbiRefs([*const (); 22]);

// SAFETY: the pointers are only kept, never dereferenced or called.
#[cfg(not(test))]
#[allow(unsafe_code)]
unsafe impl Sync for AbiRefs {}

/// Forces the references of `LINK_REFS` and `ABI_REFS` into a program that
/// uses nothing else of this crate. Calling it has no effect.
#[cfg(not(test))]
pub fn link() {
    std::hint::black_box(&LINK_REFS);
    std::hint::black_box(&ABI_REFS);
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use std::mem::{align_of, offset_of, size_of};

    // The layouts must match nl.h; nl_Loader and nl_Diagnostic must also
    // match nlc.h, which libnlc implements (crates/nl_capi/src/capi.rs).
    #[test]
    fn layouts_match_nlc_h() {
        assert_eq!(size_of::<nl_Loader>(), size_of::<nl_capi::capi::NlcLoader>());
        assert_eq!(align_of::<nl_Loader>(), align_of::<nl_capi::capi::NlcLoader>());
        assert_eq!(offset_of!(nl_Loader, data), offset_of!(nl_capi::capi::NlcLoader, data));
        assert_eq!(size_of::<nl_Diagnostic>(), size_of::<nl_capi::capi::NlcDiagnostic>());
        assert_eq!(offset_of!(nl_Diagnostic, line), offset_of!(nl_capi::capi::NlcDiagnostic, line));
        assert_eq!(offset_of!(nl_Diagnostic, endcolumn), offset_of!(nl_capi::capi::NlcDiagnostic, endcolumn));
    }

    // The declarations against nl.h: every `LUA_API` and `LUALIB_API`
    // function of the header is declared here with the same number of
    // parameters (a C `...` counts as one, as Rust's does), nothing else is
    // declared, and every numeric `NL_*` constant has the header's value.
    // The header is read from the repository, so a function added to the
    // fork without a declaration here fails `cargo test --workspace`.

    /// Splits a parameter list at the commas outside brackets; `void` and
    /// an empty list have no parameters.
    fn arity(params: &str) -> usize {
        let p = params.trim().trim_end_matches(',').trim_end();
        if p.is_empty() || p == "void" {
            return 0;
        }
        let mut depth = 0i32;
        let mut n = 1;
        for c in p.chars() {
            match c {
                '(' | '<' | '[' => depth += 1,
                ')' | '>' | ']' => depth -= 1,
                ',' if depth == 0 => n += 1,
                _ => {}
            }
        }
        n
    }

    /// The text from just after the `(` at `open` to its matching `)`.
    fn parenthesized(s: &str, open: usize) -> Option<(&str, usize)> {
        let mut depth = 0i32;
        for (i, c) in s.get(open..)?.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some((s.get(open + 1..open + i)?, open + i));
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn strip_c_comments(src: &str) -> String {
        let mut out = String::new();
        let mut rest = src;
        while let Some(i) = rest.find("/*") {
            out.push_str(rest.get(..i).unwrap_or_default());
            rest = match rest.get(i..).and_then(|r| r.find("*/")) {
                Some(j) => rest.get(i + j + 2..).unwrap_or_default(),
                None => "",
            };
            out.push(' ');
        }
        out.push_str(rest);
        out
    }

    /// (name, arity) of every function nl.h declares, and its numeric
    /// `#define NL_*` constants.
    fn header() -> (std::collections::BTreeMap<String, usize>, std::collections::BTreeMap<String, i64>) {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../LuaJIT/src/nl.h");
        let src = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"));
        let src = strip_c_comments(&src);
        let mut defines = std::collections::BTreeMap::new();
        let mut aliases = std::collections::BTreeMap::new();
        for line in src.lines() {
            let mut w = line.split_whitespace();
            if w.next() != Some("#define") {
                continue;
            }
            let (Some(name), Some(value)) = (w.next(), w.next()) else { continue };
            if let Ok(v) = value.parse::<i64>() {
                defines.insert(name.to_string(), v);
            } else if !name.contains('(') {
                aliases.insert(name.to_string(), value.to_string());
            }
        }
        let flat: String = src
            .lines()
            .filter(|l| !l.trim_start().starts_with('#'))
            .collect::<Vec<_>>()
            .join(" ");
        let mut funcs = std::collections::BTreeMap::new();
        for decl in flat.split(';') {
            let decl = decl.trim();
            let Some(start) = ["LUA_API", "LUALIB_API"].iter().filter_map(|k| decl.find(k)).min() else {
                continue;
            };
            let decl = decl.get(start..).unwrap_or_default();
            let Some(open) = decl.find('(') else { continue };
            let head = decl.get(..open).unwrap_or_default();
            let name = head.rsplit(|c: char| c.is_whitespace() || c == '*').next().unwrap_or_default();
            let name = aliases.get(name).cloned().unwrap_or_else(|| name.to_string());
            let (params, _) = parenthesized(decl, open).unwrap_or_else(|| panic!("unbalanced: {decl}"));
            funcs.insert(name, arity(params));
        }
        (funcs, defines)
    }

    /// (name, arity) of every function declared in the extern blocks of
    /// this file, and its `pub const NAME: c_int = N` constants.
    fn declared() -> (std::collections::BTreeMap<String, usize>, std::collections::BTreeMap<String, i64>) {
        let src = include_str!("lib.rs");
        let src = src.get(..src.find("\nmod tests {").unwrap_or(src.len())).unwrap_or_default();
        let mut funcs = std::collections::BTreeMap::new();
        let mut rest = src;
        while let Some(i) = rest.find("unsafe extern \"C") {
            let block = rest.get(i..).unwrap_or_default();
            let Some(b) = block.find('{') else { break };
            // The block ends at the first line that is a lone `}`.
            let end = block.find("\n}").unwrap_or(block.len());
            let body = block.get(b..end).unwrap_or_default();
            let mut at = 0;
            while let Some(j) = body.get(at..).and_then(|r| r.find("pub fn ")) {
                let s = at + j + "pub fn ".len();
                let Some(open) = body.get(s..).and_then(|r| r.find('(')) else { break };
                let name = body.get(s..s + open).unwrap_or_default().trim().to_string();
                let (params, close) = parenthesized(body, s + open).unwrap_or_else(|| panic!("unbalanced: {name}"));
                funcs.insert(name, arity(params));
                at = close;
            }
            rest = block.get(end..).unwrap_or_default();
        }
        let mut consts = std::collections::BTreeMap::new();
        for line in src.lines() {
            let Some(rest) = line.trim().strip_prefix("pub const ") else { continue };
            let Some((name, rest)) = rest.split_once(": c_int = ") else { continue };
            if let Ok(v) = rest.trim_end_matches(';').trim().parse::<i64>() {
                consts.insert(name.to_string(), v);
            }
        }
        (funcs, consts)
    }

    #[test]
    fn declarations_match_nl_h() {
        let (want, defines) = header();
        let (have, consts) = declared();
        assert!(want.len() > 40, "nl.h parsed to only {} functions: {want:?}", want.len());
        let missing: Vec<_> = want.keys().filter(|k| !have.contains_key(*k)).collect();
        let extra: Vec<_> = have.keys().filter(|k| !want.contains_key(*k)).collect();
        assert!(missing.is_empty(), "declared in nl.h, not in nl_sys: {missing:?}");
        assert!(extra.is_empty(), "declared in nl_sys, not in nl.h: {extra:?}");
        for (name, n) in &want {
            assert_eq!(have.get(name), Some(n), "{name}: {n} parameters in nl.h");
        }
        let nl_defines: Vec<_> = defines.iter().filter(|(k, _)| k.starts_with("NL_")).collect();
        assert!(nl_defines.len() > 30, "nl.h has only {} numeric NL_ defines", nl_defines.len());
        for (name, v) in nl_defines {
            assert_eq!(consts.get(name), Some(v), "{name} is {v} in nl.h");
        }
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../LuaJIT/src/nl.h"))
            .unwrap_or_default();
        assert!(text.contains(&format!("#define NL_VERSION\t\"{NL_VERSION}\"")), "NL_VERSION");
        assert!(text.contains(&format!("#define NL_EVENTLIBNAME\t\"{NL_EVENTLIBNAME}\"")), "NL_EVENTLIBNAME");
    }

    // ABI_REFS names every `#[no_mangle]` function of the text and regex
    // libraries' ABI modules.
    #[test]
    fn abi_refs_are_complete() {
        let own = include_str!("lib.rs");
        let own = own.get(..own.find("\nmod tests {").unwrap_or(own.len())).unwrap_or_default();
        for (krate, file) in [("nl_text", "nl_text/src/abi.rs"), ("nl_regex", "nl_regex/src/abi.rs")] {
            let path = format!("{}/../{file}", env!("CARGO_MANIFEST_DIR"));
            let src = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"));
            let mut n = 0;
            let mut lines = src.lines();
            while let Some(line) = lines.next() {
                if !line.contains("no_mangle") {
                    continue;
                }
                let next = lines.next().unwrap_or_default();
                let name = next.split("fn ").nth(1).and_then(|r| r.split('(').next()).unwrap_or_default();
                assert!(!name.is_empty(), "no function after no_mangle in {file}: {next}");
                let reference = format!("nl_capi::{krate}::abi::{name} as *const ()");
                assert!(own.contains(&reference), "ABI_REFS lacks {name} ({file})");
                n += 1;
            }
            assert!(n >= 10, "{file}: only {n} no_mangle functions found");
        }
    }

    // luajit-src-rs, which hosts outside the repository patch luajit-src
    // with, must build the fork and the runtime of this commit. Not compared
    // where the submodule is not checked out.
    #[test]
    fn luajit_src_rs_is_in_step() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
        if !std::path::Path::new(&format!("{root}/luajit-src-rs/Cargo.toml")).exists() {
            eprintln!("luajit-src-rs is not checked out; not compared");
            return;
        }
        let out = std::process::Command::new("sh")
            .arg(format!("{root}/tests/vm/support/sync_luajit_src_rs.sh"))
            .arg("--check")
            .output()
            .expect("cannot run tests/vm/support/sync_luajit_src_rs.sh");
        assert!(
            out.status.success(),
            "luajit-src-rs is not in step with LuaJIT/ and runtime/; run \
             tests/vm/support/sync_luajit_src_rs.sh and commit it:\n{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    #[test]
    fn arity_counts_parameters() {
        assert_eq!(arity("void"), 0);
        assert_eq!(arity(""), 0);
        assert_eq!(arity("a: i32,\n b: i32,\n"), 2);
        assert_eq!(arity("lua_State *L, const char *fmt, ..."), 3);
        assert_eq!(arity("L: *mut lua_State, f: Option<unsafe extern \"C\" fn(a: i32, b: i32)>"), 2);
    }
}
