//! `nlc`, the NL command-line tool.
//!
//! ```text
//! nlc parse <file>...                   print the syntax tree of each file
//! nlc check [--decl <path>]... <file>...
//!                                       report every diagnostic of each file
//! nlc build --emit=lua|bc [--minify] [--unchecked] [--strip] [-o path] [--decl <path>]... [--no-static-resolution] [--localize=none|std|all] [--profile=full|portable|gmod-client] <file>
//!                                       the Lua source or the bytecode dump
//!                                       the file compiles to
//! nlc run [--backend=lua|bc] [--jit=on|off|hot] [--unchecked] [--decl <path>]... [--no-static-resolution] [--localize=none|std|all] [--profile=full|portable] <file>
//!                                       run the file on stock LuaJIT
//! nlc bundle [--profile=portable|gmod-client] [-o path] [--map path] [--minify] [--part-limit=bytes] [--case-tables=unicode|ascii] [--runtime-include <lua path>] [--include-parts[=lua path]] [--unchecked] [--decl <path>]... <entry.nl>...
//! nlc runtime [--profile=portable|gmod-client] [-o path] [--map path] [--minify] [--part-limit=bytes] [--case-tables=unicode|ascii] [--include-parts[=lua path]]
//!                                       the runtime and every file the
//!                                       entries import, as Lua for stock
//!                                       LuaJIT; for Garry's Mod's client
//!                                       realm with gmod-client
//! nlc minify [--glua] [--escape-macro] [-o path] <file.lua>
//!                                       a Lua or GLua file minified
//!                                       (`nl_capi::minify`)
//! nlc remap <map.json>...               stdin to stdout, with the positions
//!                                       of a bundle's chunks mapped to
//!                                       their source files and lines
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
//! (the fields Lua's standard libraries have or the declarations list, the
//! default) or `all`. A file's `#!localize` directive overrides it.
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
//! bytecode backend with it. `--profile=gmod-client` holds them to what
//! runs in Garry's Mod's client realm on the stock game: the portable
//! profile, nothing gmod's Lua state leaves out (`io`, `loadstring`, most
//! of `os`, …), and nothing of gmod's API that the client realm lacks.
//! Code under `if SERVER` (and the other conditions gmod's realm
//! constants decide) is not checked and not compiled. gmod's declarations
//! (decl/glua.d.nl) are in scope for it, and `check` and `build` also
//! check that the Lua output loads on LuaJIT (`nl_capi::bundle::check_output`).
//! `run` refuses it: such a program runs in the game. `nlc bundle` is the
//! command line of the packager, `nl_capi::bundle`, which hosts call too
//! (nl_capi's include/nlbundle.h): the portable profile unless told
//! otherwise; with gmod-client it minifies, checks and measures the output,
//! splitting it into parts when one file would be over gmod's limit.
//! `nlc remap` reads the sourcemap of `nlc bundle --map`
//! (`nl_capi::sourcemap`).
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
  build --emit=lua|bc [--minify] [--unchecked] [--strip] [-o path] <file>
                                           print the Lua source or write the bytecode dump the file compiles to;
                                           --minify: the Lua as a bundle carries it
  run [--backend=lua|bc] [--jit=on|off|hot] [--unchecked] <file>
                                           run the file on stock LuaJIT
  bundle [--profile=portable|gmod-client] [-o path] [--map path] [--minify] <entry.nl>...
                                           write Lua for stock LuaJIT: the runtime, its gmod host and every file
                                           the entries import, compiled with the profile (portable by default).
                                           gmod-client: for Garry's Mod's client realm; minified, checked to
                                           load, and split into <name>.lua, <name>_2.lua, ... when one file would
                                           be over --part-limit bytes compressed (default 60000; gmod takes
                                           64 KiB); put the files in lua/autorun/client/
                                           --map path: write the sourcemap (positions of the bundle's chunks to
                                           source files and lines; see `remap`)
                                           --minify: minify a portable bundle too
                                           --case-tables=ascii: leave out the String library's Unicode case tables
                                           --runtime-include <lua path>: leave the runtime out; the bundle runs
                                           gmod's include of that path when the state has none (see `runtime`)
                                           --include-parts[=lua path]: the first file includes the other parts,
                                           by the first file's Lua path (default: the -o path), so that it alone
                                           runs the bundle
  runtime [--profile=portable|gmod-client] [-o path] [--include-parts[=lua path]]
                                           write the runtime alone, the file that bundles made with
                                           --runtime-include include; bundle's --map, --minify, --part-limit and
                                           --case-tables apply
  remap <map.json>...                      copy stdin to stdout with the positions of the bundles' chunks
                                           (name.nl:12, nl runtime:345) replaced by their source files and lines
  minify [--glua] [--escape-macro] [-o path] <file.lua>
                                           the Lua file minified as bundles are (line breaks stay); --glua reads
                                           Garry's Mod's dialect (// and /* */ comments, !=, &&, ||);
                                           --escape-macro spells DEFINE_BASECLASS in quoted strings with an escape
  fmt [--check] [--stdout] [--fix] <file>...
                                           format each file in place; --check only reports the files that would
                                           change, --stdout prints the result, --fix rewrites continued strings
                                           as heredocs

options of check, build and run (and bundle, whose --profile is portable or gmod-client):
  --decl <path>                            a declaration file (.d.nl) or a directory of them; repeatable
  --profile=full|portable|gmod-client      the subset of the language the target supports: everything (the
                                           default); what runs on stock LuaJIT 2.0 and 2.1 (no Regex,
                                           Worker.new, load_nl or add_path; Lua source only); or that, in
                                           Garry's Mod's client realm (nothing gmod removes from Lua, nothing
                                           server-side; code under `if SERVER` is left out). Not for `run`
  --no-static-resolution                   test everything at runtime: no proven checks, no static resolution
  --no-analyzer                            run only the resolver's checks
  --localize=none|std|all                  which Lua paths a file reads once, when it loads, into hidden locals:
                                           none, the fields the standard libraries have or the declarations
                                           list (:math.floor, not a field a file adds; the default), or every
                                           Lua global (:print, :hook.Add); a file's `#!localize` comment wins";

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
        "runtime" => bundle::cmd_runtime(rest, USAGE),
        "remap" => cmd_remap(rest),
        "minify" => cmd_minify(rest),
        "fmt" => fmt::cmd_fmt(rest, USAGE),
        "help" | "--help" | "-h" => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("nlc: unknown command `{other}`; the commands are `parse`, `check`, `build`, `run`, `bundle`, `remap`, `minify` and `fmt`\n\n{USAGE}");
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

/// `nlc remap <map.json>...`: stdin to stdout, remapped
/// (`nl_capi::sourcemap::remap_text`).
fn cmd_remap(maps: &[String]) -> ExitCode {
    use std::io::Read as _;
    if maps.is_empty() {
        eprintln!("nlc remap: no sourcemap\n\n{USAGE}");
        return ExitCode::from(2);
    }
    let mut loaded = Vec::new();
    for path in maps {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("{path}: error: cannot read the file: {e}");
                return ExitCode::FAILURE;
            }
        };
        match nl_capi::sourcemap::SourceMap::from_json(&text) {
            Ok(m) => loaded.push(m),
            Err(e) => {
                eprintln!("{path}: error: {e}");
                return ExitCode::FAILURE;
            }
        }
    }
    let mut input = Vec::new();
    if let Err(e) = std::io::stdin().read_to_end(&mut input) {
        eprintln!("nlc remap: cannot read stdin: {e}");
        return ExitCode::FAILURE;
    }
    let text = String::from_utf8_lossy(&input);
    let _ = std::io::stdout().write_all(nl_capi::sourcemap::remap_text(&loaded.iter().collect::<Vec<_>>(), &text).as_bytes());
    ExitCode::SUCCESS
}

/// `nlc minify [--glua] [--escape-macro] [-o path] <file.lua>`.
fn cmd_minify(args: &[String]) -> ExitCode {
    let mut options = nl_capi::minify::MinifyOptions::default();
    let mut output = None;
    let mut file = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--glua" => options.glua = true,
            "--escape-macro" => options.escape_gmod_macro = true,
            "-o" => match it.next() {
                Some(p) => output = Some(p.clone()),
                None => {
                    eprintln!("nlc minify: `-o` needs a path\n\n{USAGE}");
                    return ExitCode::from(2);
                }
            },
            s if s.starts_with('-') => {
                eprintln!("nlc minify: unknown option `{s}`\n\n{USAGE}");
                return ExitCode::from(2);
            }
            s if file.is_none() => file = Some(s.to_string()),
            _ => {
                eprintln!("nlc minify: one input file only\n\n{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let Some(file) = file else {
        eprintln!("nlc minify: no input file\n\n{USAGE}");
        return ExitCode::from(2);
    };
    let bytes = match std::fs::read(&file) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{file}: error: cannot read the file: {e}");
            return ExitCode::FAILURE;
        }
    };
    let text = match nl_capi::minify::minify_bytes(&bytes, options) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{file}: error: {e}");
            return ExitCode::FAILURE;
        }
    };
    match output {
        Some(path) => match std::fs::write(&path, text) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{path}: error: cannot write the file: {e}");
                ExitCode::FAILURE
            }
        },
        None => {
            let _ = std::io::stdout().write_all(&text);
            ExitCode::SUCCESS
        }
    }
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
