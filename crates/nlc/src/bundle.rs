//! `nlc bundle`: NL files compiled into one Lua file that runs on an
//! unmodified stock LuaJIT, 2.0 or 2.1, in Garry's Mod or anywhere else.
//!
//! The bundle holds the runtime library (`runtime/*.lua`), its Garry's
//! Mod host (`runtime/gmod/host.lua`), Unicode case tables for the String
//! library unless `--case-tables=ascii` leaves them out, and every file
//! the entries import, each compiled with the portable profile. Imports
//! are static, so the files are found by compiling: each compile asks the
//! loader to resolve its imports, and every file named that way is
//! compiled in turn. The resolutions are recorded in the bundle, so that
//! `import` at run time finds the same files without a search. A `.nl`
//! file that a bundled file passes to `include` as a literal path is
//! bundled too, so that `include` finds it. The host types of the
//! declaration files in scope (`declare type Player : Entity`) are listed
//! with their parents, for the host to register (runtime/gmod/host.lua).
//!
//! # Output
//!
//! ```lua
//! local rt = rawget(_G, "__nl_runtime")
//! if rt == nil then
//!   local function chunk(source, name) … end   -- loadstring or CompileString
//!   local host, start = chunk([==[<runtime/gmod/host.lua>]==], "nl host")()
//!   host.case_tables = { … }
//!   rt = chunk([==[<runtime>]==], "nl runtime")(host)
//!   start(rt)
//!   rt.bundle_interface = 1
//!   rawset(_G, "__nl_runtime", rt)
//! elseif rt.bundle_interface ~= 1 then
//!   error(…)
//! end
//! return rt.run_bundle({ ["a.nl"] = [==[<chunk>]==], … }, { ["a.nl\0lib"] = "lib.nl", … }, { "a.nl" },
//!   { {"Entity"}, {"Player", "Entity"}, … })
//! ```
//!
//! One runtime per Lua state: the first bundle to run installs it under
//! the global `__nl_runtime`, and every later bundle uses that one, if its
//! interface version is the same ([`INTERFACE`]). The runtime and its host
//! are loaded from text under the chunk names `nl runtime` and `nl host`,
//! which traces leave out as they do in `nlc run`. Each file is kept as
//! the text of its chunk and loaded by name when it is first imported,
//! with `@` and its canonical name as the chunk name, so that errors and
//! tracebacks name the NL file and line (the Lua backend prints each
//! statement on its NL line). Canonical names are relative to the working
//! directory, as for every command; build from the directory the game
//! sees as the root of the files (an addon's `lua/`).

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::fmt::Write as _;
use std::process::ExitCode;

use nl_capi::{CompileOptions, Emit, Loader, Profile, Severity};

use crate::host::{Files, RUNTIME};
use crate::run::{Sources, report};

/// The Garry's Mod host of the runtime.
const GMOD_HOST: &str = include_str!("../../../runtime/gmod/host.lua");

/// The version of the interface between a bundle's compiled files and the
/// runtime: the intrinsics they call and `rt.run_bundle`. A bundle refuses
/// to run on a runtime of another version that another bundle installed.
pub const INTERFACE: u32 = 1;

/// The global under which the runtime of a Lua state is published.
const RUNTIME_KEY: &str = "__nl_runtime";

struct Args {
    entries: Vec<String>,
    options: CompileOptions,
    output: Option<String>,
    decls: Vec<String>,
    case_tables: bool,
}

fn parse(args: &[String], usage: &str) -> Result<Args, ExitCode> {
    let usage_error = |msg: &str| {
        eprintln!("nlc bundle: {msg}\n\n{usage}");
        Err(ExitCode::from(2))
    };
    let mut options = CompileOptions { profile: Profile::Portable, emit: Emit::Lua, ..CompileOptions::default() };
    let mut entries = Vec::new();
    let mut output = None;
    let mut decls = Vec::new();
    let mut case_tables = true;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--profile=portable" => {}
            s if s.starts_with("--profile=") => return usage_error("a bundle runs on stock LuaJIT: the profile is `portable`"),
            "--unchecked" => options.checks = false,
            "--no-static-resolution" => options.static_resolution = false,
            "--no-analyzer" => options.analyze = false,
            "--case-tables=unicode" => case_tables = true,
            "--case-tables=ascii" => case_tables = false,
            s if s.starts_with("--case-tables=") => return usage_error("`--case-tables` is `unicode` or `ascii`"),
            s if s.starts_with("--localize=") => match nl_capi::Localize::from_name(s.trim_start_matches("--localize=")) {
                Some(l) => options.localize = l,
                None => return usage_error("`--localize` is `none`, `std` or `all`"),
            },
            "--decl" => match it.next() {
                Some(p) => decls.push(p.clone()),
                None => return usage_error("`--decl` takes a path"),
            },
            s if s.starts_with("--decl=") => decls.push(s.trim_start_matches("--decl=").to_string()),
            "-o" => match it.next() {
                Some(p) => output = Some(p.clone()),
                None => return usage_error("`-o` needs a path"),
            },
            "--minify" | "--map" | "--watch" => return usage_error(&format!("`{a}` is not available yet")),
            s if s.starts_with('-') => return usage_error(&format!("unknown option `{s}`")),
            s => entries.push(s.to_string()),
        }
    }
    if entries.is_empty() {
        return usage_error("no entry file");
    }
    Ok(Args { entries, options, output, decls, case_tables })
}

/// The loader of a bundle's compiles: the files of `nlc`, with every
/// resolution recorded.
struct Recording<'a> {
    files: &'a Files,
    resolved: RefCell<Vec<(Option<String>, String, String)>>,
}

impl Loader for Recording<'_> {
    fn resolve(&self, from: Option<&str>, path: &str) -> Option<String> {
        let name = self.files.resolve(from, path)?;
        self.resolved.borrow_mut().push((from.map(str::to_owned), path.to_owned(), name.clone()));
        Some(name)
    }
    fn read(&self, name: &str) -> Option<Vec<u8>> {
        self.files.read(name)
    }
    fn declarations(&self) -> Vec<(String, Vec<u8>)> {
        self.files.declarations()
    }
}

/// `nlc bundle [options] <entry.nl>...`.
pub fn cmd_bundle(args: &[String], usage: &str) -> ExitCode {
    let a = match parse(args, usage) {
        Ok(a) => a,
        Err(code) => return code,
    };
    let files = Files::new();
    for d in &a.decls {
        files.add_decl(d);
    }
    let loader = Recording { files: &files, resolved: RefCell::new(Vec::new()) };

    // The entries by canonical name, then every file they import.
    let mut entries = Vec::new();
    for path in &a.entries {
        if !std::path::Path::new(path).is_file() {
            eprintln!("{path}: error: cannot read the file");
            return ExitCode::FAILURE;
        }
        let name = files.canonical(std::path::Path::new(path));
        if !entries.contains(&name) {
            entries.push(name);
        }
    }
    let mut queue: VecDeque<String> = entries.iter().cloned().collect();
    let mut chunks: BTreeMap<String, String> = BTreeMap::new();
    let mut imports: BTreeMap<String, String> = BTreeMap::new();
    let mut failed = false;
    while let Some(name) = queue.pop_front() {
        if chunks.contains_key(&name) {
            continue;
        }
        let Some(source) = files.read(&name) else {
            eprintln!("{name}: error: cannot read the file");
            failed = true;
            chunks.insert(name, String::new());
            continue;
        };
        loader.resolved.borrow_mut().clear();
        let out = nl_capi::compile(&source, &format!("@{name}"), &a.options, &loader);
        let mut sources = Sources::new(&name, &source);
        for d in &out.diagnostics {
            report(d, &mut sources);
            failed |= d.severity == Severity::Error;
        }
        let text = out.chunk.map(|c| String::from_utf8_lossy(&c).into_owned()).unwrap_or_default();
        chunks.insert(name, text);
        for path in literal_includes(&String::from_utf8_lossy(&source)) {
            let p = std::path::Path::new(&path);
            if p.is_file() {
                let target = files.canonical(p);
                if !chunks.contains_key(&target) {
                    queue.push_back(target);
                }
            }
        }
        for (from, path, target) in loader.resolved.borrow_mut().drain(..) {
            imports.insert(format!("{}\0{path}", from.unwrap_or_default()), target.clone());
            if !chunks.contains_key(&target) {
                queue.push_back(target);
            }
        }
    }
    if failed {
        return ExitCode::FAILURE;
    }

    let text = bundle_text(&entries, &chunks, &imports, &host_types(&files), a.case_tables);
    match &a.output {
        Some(path) => match std::fs::write(path, text) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{path}: error: cannot write the file: {e}");
                ExitCode::FAILURE
            }
        },
        None => {
            use std::io::Write as _;
            let _ = std::io::stdout().write_all(text.as_bytes());
            ExitCode::SUCCESS
        }
    }
}

/// The host types of the declaration files in scope, parents first
/// (`nl_sema::Declarations::host_types`).
fn host_types(files: &Files) -> Vec<(String, Option<String>)> {
    let mut trees = nl_sema::decl::prelude_files();
    for (name, bytes) in files.declarations() {
        let text = String::from_utf8_lossy(&bytes).into_owned();
        trees.push((name, nl_syntax::parse_decl_file(&text).syntax()));
    }
    nl_sema::Declarations::load(&trees).0.host_types()
}

/// The paths of the `.nl` files that `source` passes to `include` as a
/// literal string: `include("a.nl")`, `include "a.nl"`.
fn literal_includes(source: &str) -> Vec<String> {
    use nl_syntax::SyntaxKind::*;
    let parse = nl_syntax::parse(source);
    let mut out = Vec::new();
    for call in parse.syntax().descendants().filter(|n| matches!(n.kind(), CALL_EXPR | COMMAND_CALL)) {
        let Some(callee) = call.children().next() else { continue };
        if callee.kind() != NAME_REF || callee.text().to_string().trim() != "include" {
            continue;
        }
        let Some(args) = call.children().find(|c| matches!(c.kind(), ARG_LIST | COMMAND_ARGS)) else { continue };
        let Some(arg) = args.children().next().filter(|a| a.kind() == LITERAL) else { continue };
        let Some(tok) = arg.first_token().filter(|t| t.kind() == STRING) else { continue };
        let path = nl_syntax::decode_string(&[tok.text()]).into_iter().next().map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
        if path.ends_with(".nl") && !out.contains(&path) {
            out.push(path);
        }
    }
    out
}

/// `text` as a Lua long string: `[==[text]==]` with a level that does not
/// occur in it. The text must not start with a newline, which a long
/// string drops.
fn long_string(text: &str) -> String {
    // With the closing bracket after it, so that a text ending in `]`
    // does not close early.
    let closed = format!("{text}]");
    let mut level = 0;
    while closed.contains(&format!("]{}]", "=".repeat(level))) {
        level += 1;
    }
    let eq = "=".repeat(level);
    format!("[{eq}[{text}]{eq}]")
}

/// `text` as a quoted Lua string; bytes outside printable ASCII are
/// three-digit decimal escapes.
fn quoted(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for b in text.bytes() {
        match b {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            0x20..=0x7e => out.push(char::from(b)),
            _ => {
                let _ = write!(out, "\\{b:03}");
            }
        }
    }
    out.push('"');
    out
}

fn bundle_text(entries: &[String], chunks: &BTreeMap<String, String>, imports: &BTreeMap<String, String>, types: &[(String, Option<String>)], case_tables: bool) -> String {
    let s = quoted;
    let mut out = String::new();
    out.push_str("-- Built by `nlc bundle`, portable profile: the NL runtime, its Garry's Mod\n");
    out.push_str("-- host, and the files below, for stock LuaJIT 2.0 and 2.1.\n");
    for name in chunks.keys() {
        let _ = writeln!(out, "--   {name}");
    }
    let _ = writeln!(out, "local rt = rawget(_G, {})", s(RUNTIME_KEY));
    out.push_str("if rt == nil then\n");
    // gmod has CompileString in place of loadstring; it names the chunk
    // after its identifier.
    out.push_str(
        "local function chunk(source, name)\n  local f, err\n  if loadstring ~= nil then\n    f, err = loadstring(source, \"=\" .. name)\n  else\n    f = CompileString(source, name, false)\n    if type(f) ~= \"function\" then f, err = nil, f end\n  end\n  if f == nil then error(err, 0) end\n  return f\nend\n",
    );
    let _ = writeln!(out, "local host, start = chunk({}, \"nl host\")()", long_string(GMOD_HOST));
    if case_tables {
        let t = nl_capi::nl_text::case_tables::case_tables();
        out.push_str("host.case_tables = {\n");
        for (k, v) in [("up", &t.up), ("upx", &t.upx), ("down", &t.down), ("downx", &t.downx), ("swapx", &t.swapx)] {
            let _ = writeln!(out, "  {k} = {},", s(v));
        }
        out.push_str("}\n");
    }
    let _ = writeln!(out, "rt = chunk({}, \"nl runtime\")(host)", long_string(RUNTIME));
    out.push_str("start(rt)\n");
    let _ = writeln!(out, "rt.bundle_interface = {INTERFACE}");
    let _ = writeln!(out, "rawset(_G, {}, rt)", s(RUNTIME_KEY));
    let _ = writeln!(out, "elseif rt.bundle_interface ~= {INTERFACE} then");
    let _ = writeln!(
        out,
        "  error(\"this bundle needs version {INTERFACE} of the NL runtime's interface, and the runtime already loaded in this Lua state by another bundle has version \" .. tostring(rt.bundle_interface), 0)"
    );
    out.push_str("end\n");
    out.push_str("return rt.run_bundle({\n");
    for (name, chunk) in chunks {
        let _ = writeln!(out, "[{}] = {},", s(name), long_string(chunk));
    }
    out.push_str("}, {\n");
    for (key, name) in imports {
        let _ = writeln!(out, "[{}] = {},", s(key), s(name));
    }
    out.push_str("}, {");
    out.push_str(&entries.iter().map(|e| s(e)).collect::<Vec<_>>().join(", "));
    out.push_str("}, {");
    for (name, parent) in types {
        match parent {
            Some(p) => {
                let _ = write!(out, "\n{{{}, {}}},", s(name), s(p));
            }
            None => {
                let _ = write!(out, "\n{{{}}},", s(name));
            }
        }
    }
    out.push_str("})\n");
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn long_strings_pick_a_free_level() {
        assert_eq!(long_string("a"), "[[a]]");
        assert_eq!(long_string("x]]y"), "[=[x]]y]=]");
        assert_eq!(long_string("]] ]=]"), "[==[]] ]=]]==]");
        assert_eq!(long_string("x]"), "[=[x]]=]");
    }

    #[test]
    fn includes_with_literal_paths() {
        let src = "include(\"a.nl\")\ninclude \"b/c.nl\"\ninclude(\"x.lua\")\nlet p = \"d.nl\"\ninclude(p)\nfoo(\"e.nl\")\ninclude(\"a.nl\")\n";
        assert_eq!(literal_includes(src), ["a.nl", "b/c.nl"]);
    }

    #[test]
    fn quoted_strings() {
        assert_eq!(quoted("a\0b\"\\é"), "\"a\\000b\\\"\\\\\\195\\169\"");
    }
}
