//! The compiler (`nl_capi::compile`) on every program of the repository and
//! on hostile variants of them: prefixes, deleted lines, deep nesting and
//! long chains. A compile must finish without panicking and return either a
//! chunk or an error diagnostic, through both backends (the bytecode
//! backend reports a function over LuaJIT's limits as a `limit` error).
//!
//! Deep inputs run on a thread with exactly 2 MiB of stack, the default of
//! `std::thread::spawn`, like the parser's robustness tests: the compiler
//! is called from host threads.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::path::{Path, PathBuf};

use nl_capi::{CompileOptions, Emit, Loader, Severity, compile};

struct NoFiles;

impl Loader for NoFiles {
    fn resolve(&self, _from: Option<&str>, _path: &str) -> Option<String> {
        None
    }
    fn read(&self, _name: &str) -> Option<Vec<u8>> {
        None
    }
}

fn check(src: &str) {
    for emit in [Emit::Lua, Emit::Bytecode] {
        let out = compile(src.as_bytes(), "=test", &CompileOptions { emit, ..CompileOptions::default() }, &NoFiles);
        let has_error = out.diagnostics.iter().any(|d| d.severity == Severity::Error);
        let head: String = src.chars().take(80).collect();
        assert!(out.chunk.is_some() || has_error, "no chunk and no error for {head:?} ({emit:?})");
        assert!(out.chunk.is_none() || !has_error, "a chunk despite an error ({emit:?})");
        assert!(!out.diagnostics.iter().any(|d| d.code == "internal-error"), "the compiler panicked on {head:?} ({emit:?}): {:?}", out.diagnostics);
    }
}

fn on_small_stack(src: String) {
    std::thread::Builder::new().stack_size(2 << 20).spawn(move || check(&src)).unwrap().join().unwrap();
}

fn nl_files() -> Vec<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/run");
    let mut out = Vec::new();
    let mut dirs = vec![root];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).unwrap().filter_map(Result::ok) {
            let p = e.path();
            if p.is_dir() {
                dirs.push(p);
            } else if p.extension().is_some_and(|x| x == "nl") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

#[test]
fn every_program() {
    for p in nl_files() {
        check(&std::fs::read_to_string(&p).unwrap());
    }
    check(&std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../example.nl")).unwrap());
}

#[test]
fn prefixes_and_deleted_lines() {
    for p in nl_files().iter().step_by(3) {
        let src = std::fs::read_to_string(p).unwrap();
        let mut cut = 0;
        while cut < src.len() {
            if src.is_char_boundary(cut) {
                check(&src[..cut]);
            }
            cut += 7;
        }
        let lines: Vec<&str> = src.lines().collect();
        for i in 0..lines.len() {
            let mut v = lines.clone();
            v.remove(i);
            check(&v.join("\n"));
        }
    }
}

/// `DEEP=<tag>` runs one of these alone.
#[test]
fn deep_inputs() {
    let only = std::env::var("DEEP").ok();
    let on_small_stack = |s: String, tag: &str| { if only.as_deref().is_none_or(|o| o == tag) { on_small_stack(s) } };
    let n = 120;
    on_small_stack(format!("{}1{}\n", "(".repeat(n), ")".repeat(n)), "paren");
    on_small_stack(format!("{}print 1\n{}", "if true\n".repeat(n), "end\n".repeat(n)), "ifs");
    on_small_stack(format!("let f = {}1{}\n", "fn() ".repeat(n), " end".repeat(n)), "fns");
    on_small_stack(format!("let t = {}1{}\n", "{".repeat(n), "}".repeat(n)), "tables");
    on_small_stack(format!("let x = {}\n", vec!["1"; 990].join(" + ")), "plus");
    on_small_stack(format!("let x = {}\n", vec!["\"a\""; 990].join(" <> ")), "concat");
    on_small_stack(format!("let x = 1\nlet y = x{}\n", ".a".repeat(990)), "field");
    on_small_stack(format!("let x = {}\n", vec!["true"; 990].join(" and ")), "and");
    on_small_stack(format!("fn f(x) x end\nlet y = f{}\n", "(1)".repeat(990)), "calls");
    on_small_stack(format!("let x = {}\n", "-".repeat(500) + "1"), "neg");
    on_small_stack(format!("let t = {{}}\nlet y = t{}\n", "[1]".repeat(990)), "index");
}

#[test]
fn many_locals() {
    // More file-level names than LuaJIT allows locals in one function: the
    // compile must still answer (with a chunk; running it is another
    // matter, see the crate notes).
    let mut src = String::new();
    for i in 0..400 {
        src.push_str(&format!("let v{i} = {i}\n"));
    }
    check(&src);
}
