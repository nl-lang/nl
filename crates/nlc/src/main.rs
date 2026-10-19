//! `nlc`, the NL command-line tool.
//!
//! ```text
//! nlc parse <file>...                   print the syntax tree of each file
//! nlc check [--decl <path>]... <file>...
//!                                       report every diagnostic of each file
//! nlc build --emit=lua|bc [--unchecked] [--strip] [-o path] [--decl <path>]... [--no-static-resolution] [--localize=none|std|all] [--profile=full|portable] <file>
//!                                       the Lua source or the bytecode dump
//!                                       the file compiles to
//! nlc run [--backend=lua|bc] [--jit=on|off|hot] [--unchecked] [--decl <path>]... [--no-static-resolution] [--localize=none|std|all] [--profile=full|portable] <file>
//!                                       run the file on stock LuaJIT
//! nlc bundle [--profile=portable] [-o path] [--case-tables=unicode|ascii] [--unchecked] [--decl <path>]... <entry.nl>...
//!                                       one Lua file with the runtime and
//!                                       every file the entries import, for
//!                                       stock LuaJIT (Garry's Mod)
//! nlc fmt [--check] [--stdout] [--fix] <file>...
//!                                       format each file in place
//! ```
//!
//! `nlc check` compiles each file through the
//! resolver and the analyzer and prints its errors, warnings and lints;
//! the exit status is 1 when any file has an error. `--decl` names a
//! declaration file (`.d.nl`) or a directory of them, for
//! every command that compiles; `--no-static-resolution` leaves the
//! analyzer's proven checks and static resolution out of the generated
//! code, so that a program can be run both ways.
//! `--localize` says which Lua paths a file reads once, when it loads, into
//! hidden locals: `none`, `std`
//! (the fields of the standard libraries, the default) or `all`.
//!
//! `nlc run` embeds stock LuaJIT through `mlua`,
//! opens the runtime library (`runtime/*.lua`) with a host table that
//! compiles through `nl_capi::compile` and finds files in the order of
//! `import` (the search path starts from `NL_PATH`), runs the file, and
//! then pumps until no task is pending. An uncaught error is
//! reported on stderr and the exit status is 1. The backend (Lua
//! source, the default, or bytecode) compiles the file and every file
//! it imports; `--jit` is for debugging the bytecode backend: `off` is
//! `jit.off()`, `hot` is `jit.opt.start("hotloop=1")`.
//!
//! `--profile=portable` holds a file and its imports to the portable
//! profile (`nl_sema::profile`): what runs on an unmodified stock LuaJIT
//! 2.0 or 2.1, through the Lua backend, so `build` and `run` refuse the
//! bytecode backend with it. `nlc bundle` always uses it (bundle.rs).
//!
//! `nlc fmt` formats each file on its lossless
//! syntax tree (`nl_syntax::format`, which states the rules): spacing and
//! indentation are normalized, line breaks are kept. `--check` only
//! reports the files that would change, `--stdout` prints the result
//! instead of writing it, and `--fix` also rewrites each `"…"` string
//! continued with a backslash as a heredoc; without it such a string is
//! a warning. A file with syntax errors is reported and left alone.
//!
//! Diagnostics go to stderr as `path:line:column: error: message` (1-based;
//! the column counts characters), each followed by the source line with a
//! caret underline (`nl_capi::snippet`). The exit
//! status is 0 when every file was read and has no error, 1 otherwise, and
//! 2 for a usage error.

use std::io::Write as _;
use std::process::ExitCode;

use nl_syntax::{Diagnostic, DumpOptions, LineIndex, Severity, dump_tree, parse, source_from_bytes};

mod bundle;
mod fmt;
mod host;
mod run;

const USAGE: &str = "usage: nlc <command> [arguments]

commands:
  parse <file>...                          print the syntax tree of each file; diagnostics go to stderr
  check [--decl <path>]... <file>...       report the errors, warnings and lints of each file
  build --emit=lua|bc [--unchecked] [--strip] [-o path] <file>
                                           print the Lua source or write the bytecode dump the file compiles to
  run [--backend=lua|bc] [--jit=on|off|hot] [--unchecked] <file>
                                           run the file on stock LuaJIT
  bundle [-o path] [--case-tables=unicode|ascii] <entry.nl>...
                                           write one Lua file for stock LuaJIT (Garry's Mod): the runtime, its
                                           gmod host and every file the entries import, compiled with the
                                           portable profile; --case-tables=ascii leaves out the Unicode case
                                           tables of the String library
  fmt [--check] [--stdout] [--fix] <file>...
                                           format each file in place; --check only reports the files that would
                                           change, --stdout prints the result, --fix rewrites continued strings
                                           as heredocs

options of check, build and run (and bundle, except --profile):
  --decl <path>                            a declaration file (.d.nl) or a directory of them; repeatable
  --profile=full|portable                  the subset of the language the target supports: everything (the
                                           default), or what runs on stock LuaJIT 2.0 and 2.1 (no Regex,
                                           Worker.new, load_nl or add_path; Lua source only)
  --no-static-resolution                   test everything at runtime: no proven checks, no static resolution
  --no-analyzer                            run only the resolver's checks
  --localize=none|std|all                  which Lua paths a file reads once, when it loads, into hidden locals:
                                           none, the fields of the standard libraries (:math.floor; the
                                           default), or every Lua global (:print, :hook.Add)";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some((command, rest)) = args.split_first() else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    match command.as_str() {
        "parse" => cmd_parse(rest),
        "check" => run::cmd_check(rest, USAGE),
        "build" => run::cmd_build(rest, USAGE),
        "run" => run::cmd_run(rest, USAGE),
        "bundle" => bundle::cmd_bundle(rest, USAGE),
        "fmt" => fmt::cmd_fmt(rest, USAGE),
        "help" | "--help" | "-h" => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("nlc: unknown command `{other}`; the commands are `parse`, `check`, `build`, `run`, `bundle` and `fmt`\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// `nlc parse <file>...`.
fn cmd_parse(files: &[String]) -> ExitCode {
    if files.is_empty() {
        eprintln!("nlc parse: no input files\n\n{USAGE}");
        return ExitCode::from(2);
    }
    let mut failed = false;
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for path in files {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("{path}: error: cannot read the file: {e}");
                failed = true;
                continue;
            }
        };
        let source = match source_from_bytes(&bytes) {
            Ok(s) => s,
            Err(d) => {
                // The valid prefix locates the bad byte.
                let start = usize::from(d.range.start());
                let prefix = String::from_utf8_lossy(bytes.get(..start).unwrap_or(&[]));
                report(path, &prefix, &LineIndex::new(&prefix), &d, true);
                failed = true;
                continue;
            }
        };
        let parse = parse(source);
        if files.len() > 1 {
            let _ = writeln!(out, "# {path}");
        }
        let _ = out.write_all(dump_tree(&parse.syntax(), DumpOptions::default()).as_bytes());
        // One index per file: every position is then found in log time.
        let index = LineIndex::new(source);
        for d in parse.diagnostics() {
            report(path, source, &index, d, false);
            failed |= d.is_error();
        }
    }
    let _ = out.flush();
    if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}

/// Prints `path:line:col: error: message` and the snippet of the range.
/// With `at_end`, the position is the end of `text` (the valid prefix of a
/// file that is not UTF-8).
fn report(path: &str, text: &str, index: &LineIndex, d: &Diagnostic, at_end: bool) {
    let (start, end) = if at_end { (nl_syntax::TextSize::of(text), nl_syntax::TextSize::of(text)) } else { (d.range.start(), d.range.end()) };
    let lc = index.line_col(text, start);
    let end = index.line_col(text, end);
    let severity = match d.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    };
    eprintln!("{path}:{}:{}: {severity}: {}", lc.line, lc.col, d.message);
    let snippet = nl_capi::snippet::snippet(text, lc.line, lc.col, end.line, end.col);
    if !snippet.is_empty() {
        eprintln!("{snippet}");
    }
}
