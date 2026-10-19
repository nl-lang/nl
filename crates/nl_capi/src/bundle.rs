//! The packager: NL files compiled into Lua that runs on an unmodified
//! stock LuaJIT, 2.0 or 2.1: in Garry's Mod's client realm with the
//! gmod-client profile, or anywhere with the portable one. `nlc bundle`
//! is its command line; hosts call [`bundle`], or `nlb_bundle` from C
//! (nlbundle.h, capi_bundle.rs).
//!
//! It is pure, like the compiler: files are read and resolved only through
//! the [`Loader`], nothing is written, and no Lua runs. Whether a chunk
//! loads on LuaJIT is asked of the host ([`BundleOptions::check`]), which
//! has a VM.
//!
//! The bundle holds the runtime library (`runtime/*.lua`), its Garry's
//! Mod host (`runtime/gmod/host.lua`), Unicode case tables for the String
//! library unless [`BundleOptions::case_tables`] leaves them out, and every
//! file the entries import, each compiled with the profile. Imports are
//! static, so the files are found by compiling: each compile asks the
//! loader to resolve its imports, and every file named that way is
//! compiled in turn. The resolutions are recorded in the bundle, so that
//! `import` at run time finds the same files without a search. A `.nl`
//! file that a bundled file passes to `include` as a literal path is
//! bundled too (resolved as the loader resolves a path of no file), so
//! that `include` finds it. The host types of the declaration files in
//! scope (`declare type Player : Entity`) are listed with their parents,
//! for the host to register (runtime/gmod/host.lua).
//!
//! # Garry's Mod (the gmod-client profile)
//!
//! What the profile checks in the source is nl_sema::profile's (gmod's
//! declarations are in scope for it: the driver adds them). The bundle
//! itself is held to what the stock game takes from a client file:
//!
//! - every file's Lua, and the runtime's, is minified (minify.rs: lines
//!   stay where they are); it must be free of the text gmod replaces in
//!   scripts, and load ([`BundleOptions::check`]);
//! - the bundle starts with `if SERVER then AddCSLuaFile() return end`: it
//!   runs on clients only, and a server that runs it (`lua/autorun/`, or
//!   an `include`) sends it to them;
//! - each file of the bundle is at most [`BundleOptions::part_limit`]
//!   bytes compressed ([`DEFAULT_PART_LIMIT`], under gmod's 64 KiB). A
//!   bundle that does not fit is split into parts (Parts, below), named
//!   `<name>.lua`, `<name>_2.lua`, …, which go in one folder,
//!   `lua/autorun/client/` being the usual one: gmod sends and runs those
//!   files by itself.
//!
//! [`BundleOptions::minify`] minifies a portable bundle too, and
//! [`BundleOptions::sourcemap`] makes the sourcemap of any bundle
//! (sourcemap.rs).
//!
//! # Output
//!
//! ```lua
//! if SERVER then AddCSLuaFile() return end      -- gmod-client
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
//! which traces leave out. Each file is kept as the text of its chunk and
//! loaded by name when it is first imported, with `@` and its canonical
//! name as the chunk name, so that errors and tracebacks name the NL file
//! and line (the Lua backend prints each statement on its NL line). The
//! canonical names are the loader's; they should be the addon's own
//! (`myaddon/main.nl`), so that they do not meet another addon's in the
//! one runtime of the state.
//!
//! # Parts
//!
//! A bundle that is too large for one file is its text (Output, without
//! the guard) cut in pieces. Each part keeps its piece in a table shared by
//! the parts, under the bundle's name and build; the part that completes
//! the set joins the pieces, compiles them (`CompileString`, chunk name
//! `nl bundle <name>`, which traces leave out) and runs the bundle. The
//! parts run in any order: gmod runs the files of a folder in its own. A
//! part of another build starts the set over, so a set mixed from two
//! builds runs neither.
//!
//! ```lua
//! -- Built by `nlc bundle` (gmod-client): part 2 of 3 of `name`, build 5f0c2b7e9a14d3c8
//! if SERVER then AddCSLuaFile() return end
//! local all = rawget(_G, "__nl_bundle_parts")
//! …
//! b[2] = [==[
//! <piece>]==]
//! if b.n < 3 then return end
//! …
//! ```

use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::fmt::Write as _;

use crate::driver::{CompileOptions, Diagnostic, Emit, Loader, Profile, Severity};
use crate::minify::{GMOD_MACRO, MinifyOptions, minify};
use crate::sourcemap::{SourceMap, build_hash};

/// The runtime library, `runtime/*.lua` concatenated (build.rs).
pub const RUNTIME: &str = include_str!(concat!(env!("OUT_DIR"), "/runtime.lua"));

mod runtime_files {
    include!(concat!(env!("OUT_DIR"), "/runtime_files.rs"));
}
pub use runtime_files::RUNTIME_FILES;

/// The Garry's Mod host of the runtime, and its name in sourcemaps.
pub const GMOD_HOST: &str = include_str!("../../../runtime/gmod/host.lua");
const GMOD_HOST_FILE: &str = "runtime/gmod/host.lua";

/// The version of the interface between a bundle's compiled files and the
/// runtime: the intrinsics they call and `rt.run_bundle`. A bundle refuses
/// to run on a runtime of another version that another bundle installed.
pub const INTERFACE: u32 = 1;

/// The global under which the runtime of a Lua state is published.
const RUNTIME_KEY: &str = "__nl_runtime";

/// The first line of every file the packager writes: how a part that an
/// earlier build left behind is recognized before it is removed.
pub const MARK: &str = "-- Built by `nlc bundle`";

/// gmod's limit on a file it sends to clients, compressed (LZMA; the
/// wiki's `AddCSLuaFile`).
pub const GMOD_FILE_LIMIT: usize = 64 * 1024;

/// The compressed size a bundle file is held to by default: below
/// [`GMOD_FILE_LIMIT`] by a margin, as gmod's encoder settings are not
/// these and the same text can come out a few percent larger.
pub const DEFAULT_PART_LIMIT: usize = 60_000;

/// The smallest [`BundleOptions::part_limit`]: the runtime alone takes
/// several parts of this size.
pub const MIN_PART_LIMIT: usize = 4096;

/// Asks the host whether a chunk loads on LuaJIT: `(chunk name, text)`,
/// and the error message (`name:line: message`) when it does not.
pub type Check<'a> = &'a dyn Fn(&str, &str) -> Result<(), String>;

/// What [`bundle`] makes.
#[derive(Clone, Copy)]
pub struct BundleOptions<'a> {
    /// The compile's options; the profile is [`Self::profile`] and the
    /// backend Lua source, whatever these say.
    pub compile: CompileOptions,
    /// [`Profile::Portable`] or [`Profile::GmodClient`]; the full profile
    /// is taken as the portable one, as a bundle runs on stock LuaJIT.
    pub profile: Profile,
    /// The bundle's name: the key of its parts, the names of their files
    /// (`<name>.lua`, `<name>_2.lua`), the sourcemap's `bundle`.
    pub name: &'a str,
    /// Minify; always for gmod.
    pub minify: bool,
    /// The String library's Unicode case tables; without them, case
    /// mapping is ASCII only.
    pub case_tables: bool,
    /// gmod: the most bytes a file of the bundle may take, compressed.
    pub part_limit: usize,
    /// Make the sourcemap; the chunks then carry no line maps of their own
    /// (sourcemap.rs).
    pub sourcemap: bool,
    /// Whether a chunk loads on LuaJIT; `None` takes it on trust.
    pub check: Option<Check<'a>>,
}

impl Default for BundleOptions<'_> {
    fn default() -> Self {
        BundleOptions {
            compile: CompileOptions::default(),
            profile: Profile::Portable,
            name: "bundle",
            minify: false,
            case_tables: true,
            part_limit: DEFAULT_PART_LIMIT,
            sourcemap: false,
            check: None,
        }
    }
}

/// One file of a bundle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    /// `<name>.lua`, then `<name>_2.lua`, …
    pub name: String,
    pub text: String,
}

/// What [`bundle`] made.
#[derive(Clone, Debug, Default)]
pub struct Bundle {
    /// The files of the bundle, in order; none when a diagnostic is an
    /// error.
    pub parts: Vec<Part>,
    /// Every diagnostic of every file, then the bundle's own.
    pub diagnostics: Vec<Diagnostic>,
    /// With [`BundleOptions::sourcemap`], when the bundle was made.
    pub sourcemap: Option<SourceMap>,
    /// A hash of the bundle's content (sourcemap.rs, `build_hash`).
    pub build: String,
}

impl Bundle {
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.severity == Severity::Error)
    }
}

/// The loader of a bundle's compiles: the host's, with every resolution
/// recorded.
struct Recording<'a> {
    inner: &'a dyn Loader,
    resolved: RefCell<Vec<(Option<String>, String, String)>>,
}

impl Loader for Recording<'_> {
    fn resolve(&self, from: Option<&str>, path: &str) -> Option<String> {
        let name = self.inner.resolve(from, path)?;
        self.resolved.borrow_mut().push((from.map(str::to_owned), path.to_owned(), name.clone()));
        Some(name)
    }
    fn read(&self, name: &str) -> Option<Vec<u8>> {
        self.inner.read(name)
    }
    fn declarations(&self) -> Vec<(String, Vec<u8>)> {
        self.inner.declarations()
    }
}

/// A file of the bundle once compiled: its chunk's text, and the line map
/// taken out of it for the sourcemap.
struct Compiled {
    text: String,
    moved: Vec<(u32, u32)>,
}

fn diagnostic(file: &str, line: u32, code: &'static str, message: String) -> Diagnostic {
    Diagnostic { severity: Severity::Error, code, message, file: file.to_owned(), line, column: 1, end_line: line, end_column: 1 }
}

/// Bundles `entries`, each a path the loader resolves as a path of no file
/// (`loader.resolve(None, path)`), with everything they import (module
/// docs).
pub fn bundle(entries: &[&str], options: &BundleOptions<'_>, loader: &dyn Loader) -> Bundle {
    let profile = if options.profile == Profile::GmodClient { Profile::GmodClient } else { Profile::Portable };
    let gmod = profile.is_gmod();
    let compile = CompileOptions { profile, emit: Emit::Lua, ..options.compile };
    let minify_all = options.minify || gmod;
    let recording = Recording { inner: loader, resolved: RefCell::new(Vec::new()) };
    let mut out = Bundle::default();
    // The name goes in file names and in a comment of every part.
    if options.name.is_empty() || options.name.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | '`')) {
        out.diagnostics.push(diagnostic(options.name, 1, "bundle-name", "a bundle's name is not empty and has no control character, `/`, `\\` or backquote: it names the bundle's files".to_owned()));
        return out;
    }

    // The entries by canonical name, then every file they import.
    let mut names: Vec<String> = Vec::new();
    for path in entries {
        match loader.resolve(None, path) {
            Some(name) if !names.contains(&name) => names.push(name),
            Some(_) => {}
            None => out.diagnostics.push(diagnostic(path, 1, "cannot-read", "cannot find the file".to_owned())),
        }
    }
    if names.is_empty() && out.diagnostics.is_empty() {
        out.diagnostics.push(diagnostic("", 1, "no-entry", "no entry file".to_owned()));
    }
    let mut queue: VecDeque<String> = names.iter().cloned().collect();
    let mut chunks: BTreeMap<String, Compiled> = BTreeMap::new();
    let mut imports: BTreeMap<String, String> = BTreeMap::new();
    while let Some(name) = queue.pop_front() {
        if chunks.contains_key(&name) {
            continue;
        }
        let Some(source) = loader.read(&name) else {
            out.diagnostics.push(diagnostic(&name, 1, "cannot-read", "cannot read the file".to_owned()));
            chunks.insert(name, Compiled { text: String::new(), moved: Vec::new() });
            continue;
        };
        recording.resolved.borrow_mut().clear();
        let compiled = crate::driver::compile(&source, &format!("@{name}"), &compile, &recording);
        let ok = compiled.chunk.is_some();
        out.diagnostics.extend(compiled.diagnostics);
        let source_text = String::from_utf8_lossy(&source).into_owned();
        let text = compiled.chunk.map(|c| String::from_utf8_lossy(&c).into_owned()).unwrap_or_default();
        let c = if ok {
            match finish_chunk(&name, &source_text, text, options.sourcemap, minify_all, gmod, options.check) {
                Ok(c) => c,
                Err(d) => {
                    out.diagnostics.extend(d);
                    Compiled { text: String::new(), moved: Vec::new() }
                }
            }
        } else {
            Compiled { text: String::new(), moved: Vec::new() }
        };
        chunks.insert(name, c);
        for path in literal_includes(&source_text) {
            if let Some(target) = loader.resolve(None, &path) {
                if !chunks.contains_key(&target) {
                    queue.push_back(target);
                }
            }
        }
        for (from, path, target) in recording.resolved.borrow_mut().drain(..) {
            imports.insert(format!("{}\0{path}", from.unwrap_or_default()), target.clone());
            if !chunks.contains_key(&target) {
                queue.push_back(target);
            }
        }
    }
    if out.has_errors() {
        return out;
    }

    // The runtime and its host, minified like the files.
    let (runtime, host) = if minify_all {
        match (minify(RUNTIME, MinifyOptions::COMPILED), minify(GMOD_HOST, MinifyOptions::COMPILED)) {
            (Ok(r), Ok(h)) => (r, h),
            (Err(e), _) | (_, Err(e)) => {
                out.diagnostics.push(diagnostic("nl runtime", 1, "internal-error", format!("internal error: the runtime does not minify: {e}")));
                return out;
            }
        }
    } else {
        (RUNTIME.to_owned(), GMOD_HOST.to_owned())
    };
    let texts: BTreeMap<String, String> = chunks.iter().map(|(n, c)| (n.clone(), c.text.clone())).collect();
    let types = host_types(loader, profile);
    let payload = bundle_text(&names, &texts, &imports, &types, options.case_tables, &runtime, &host, profile);
    out.build = build_hash(payload.as_bytes());

    // One file, or the parts of a gmod bundle too large for one.
    let limit = options.part_limit.clamp(MIN_PART_LIMIT, GMOD_FILE_LIMIT);
    let texts: Vec<String> = if gmod {
        let single = format!("{MARK} (gmod-client)\n{GUARD}{payload}");
        if lzma_size(single.as_bytes()) <= limit {
            vec![single]
        } else {
            match split(&payload, options.name, &out.build, limit) {
                Ok(parts) => parts,
                Err(e) => {
                    out.diagnostics.push(diagnostic(options.name, 1, "bundle-too-large", e));
                    return out;
                }
            }
        }
    } else {
        vec![payload]
    };
    out.parts = texts.into_iter().enumerate().map(|(i, text)| Part { name: part_name(options.name, i + 1), text }).collect();
    if options.sourcemap {
        let parts = out.parts.iter().map(|p| p.name.clone()).collect();
        out.sourcemap = Some(source_map(options.name, &out.build, parts, &chunks, &runtime, &host));
    }
    out
}

/// The name of the `i`th file of the bundle `name` (from 1).
pub fn part_name(name: &str, i: usize) -> String {
    if i == 1 { format!("{name}.lua") } else { format!("{name}_{i}.lua") }
}

/// The errors of the Lua text of the file `name` (its canonical name;
/// `source` is its NL source) for a stock LuaJIT in gmod: `check`'s, and
/// the text gmod replaces in scripts. `nlc check --profile=gmod-client`
/// asks this of each file it compiles.
pub fn check_output(name: &str, source: &str, chunk: &str, check: Option<Check<'_>>) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    if let Some(check) = check {
        if let Err(message) = check(&format!("@{name}"), chunk) {
            // `name:12: function at line 3 has more than 200 local variables`
            let (line, text) = match message.strip_prefix(&format!("{name}:")).and_then(|rest| rest.split_once(": ")) {
                Some((l, text)) => (l.parse().unwrap_or(1), text.to_owned()),
                None => (1, message.clone()),
            };
            out.push(diagnostic(name, line, "lua-limit", format!("the Lua this file compiles to does not load on LuaJIT: {text}")));
        }
    }
    if chunk.contains(GMOD_MACRO) {
        let line = source.lines().position(|l| l.contains(GMOD_MACRO)).map_or(1, |i| u32::try_from(i + 1).unwrap_or(1));
        out.push(diagnostic(
            name,
            line,
            "not-in-gmod",
            format!("the Lua this file compiles to holds the text `{GMOD_MACRO}` outside a quoted string; Garry's Mod replaces that text with `local BaseClass = baseclass.Get` in every script it loads, so the file would not run as written"),
        ));
    }
    out
}

/// The text of a compiled file as the bundle carries it: without its line
/// map when the sourcemap takes it, minified when asked, and checked. The
/// diagnostics when it does not do for the bundle.
fn finish_chunk(name: &str, source: &str, text: String, sourcemap: bool, minify_it: bool, gmod: bool, check: Option<Check<'_>>) -> Result<Compiled, Vec<Diagnostic>> {
    let (text, moved) = if sourcemap { nl_luagen::take_line_map(&text) } else { (text, Vec::new()) };
    let text = if minify_it {
        minify(&text, MinifyOptions::COMPILED).map_err(|e| vec![diagnostic(name, 1, "internal-error", format!("internal error: the Lua of this file does not minify: {e}"))])?
    } else {
        text
    };
    let diags = if gmod {
        check_output(name, source, &text, check)
    } else {
        // Every bundle runs on LuaJIT; the macro is gmod's alone.
        check_output(name, source, &text, check).into_iter().filter(|d| d.code != "not-in-gmod").collect()
    };
    if !diags.is_empty() {
        return Err(diags);
    }
    Ok(Compiled { text, moved })
}

/// The sourcemap of a bundle (sourcemap.rs).
fn source_map(name: &str, build: &str, parts: Vec<String>, chunks: &BTreeMap<String, Compiled>, runtime: &str, host: &str) -> SourceMap {
    let lines = |t: &str| u32::try_from(t.lines().count()).unwrap_or(u32::MAX);
    let mut map = SourceMap { bundle: name.to_owned(), build: build.to_owned(), parts, ..SourceMap::default() };
    for (file, c) in chunks {
        map.add_file_chunk(file, file, lines(&c.text), &c.moved);
    }
    // The minifier keeps every line, so the runtime's lines are its files'.
    debug_assert_eq!(lines(runtime), RUNTIME_FILES.iter().map(|(_, n)| n).sum::<u32>());
    map.add_concatenation("nl runtime", RUNTIME_FILES);
    map.add_concatenation("nl host", &[(GMOD_HOST_FILE, lines(host))]);
    map
}

/// The size of `bytes` compressed as gmod compresses the Lua files it
/// sends (LZMA, `.lzma` header), at preset 6.
pub fn lzma_size(bytes: &[u8]) -> usize {
    use lzma_rust2::{LzmaOptions, LzmaWriter};
    use std::io::Write as _;
    let mut out = Vec::with_capacity(bytes.len() / 3);
    let compressed = (|| -> std::io::Result<usize> {
        let options = LzmaOptions::with_preset(6);
        let mut w = LzmaWriter::new_use_header(&mut out, &options, Some(bytes.len() as u64)).map_err(std::io::Error::other)?;
        w.write_all(bytes)?;
        w.finish().map_err(std::io::Error::other)?;
        Ok(out.len())
    })();
    // A compressor that fails is a bug; the raw size bounds the result.
    compressed.unwrap_or(bytes.len() + 64)
}

/// What a gmod bundle runs first: on the server, it only sends itself to
/// clients.
const GUARD: &str = "if SERVER then AddCSLuaFile() return end\n";

/// The parts of a bundle whose text is `payload` (module docs, Parts),
/// each at most `limit` bytes compressed.
fn split(payload: &str, name: &str, build: &str, limit: usize) -> Result<Vec<String>, String> {
    let total = lzma_size(payload.as_bytes()).max(1);
    // Bytes of text per compressed byte, from the whole; each part is
    // then measured, and the pieces made smaller until every part fits.
    let ratio = payload.len() as f64 / total as f64;
    let mut piece = ((limit as f64) * 0.9 * ratio) as usize;
    for _ in 0..16 {
        let pieces = cut(payload, piece.max(1024));
        let n = pieces.len();
        let parts: Vec<String> = pieces.iter().enumerate().map(|(i, p)| part_text(name, build, i + 1, n, p)).collect();
        if parts.iter().all(|p| lzma_size(p.as_bytes()) <= limit) {
            return Ok(parts);
        }
        piece = piece * 85 / 100;
    }
    Err(format!("cannot split the bundle into files of at most {limit} bytes compressed"))
}

/// `text` in pieces of about `size` bytes, cut at character boundaries.
fn cut(text: &str, size: usize) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        let mut at = size.min(rest.len());
        while !rest.is_char_boundary(at) {
            at -= 1;
        }
        let (piece, tail) = rest.split_at(at);
        out.push(piece);
        rest = tail;
    }
    out
}

fn part_text(name: &str, build: &str, i: usize, n: usize, piece: &str) -> String {
    let key = quoted(name);
    let mut out = String::new();
    let _ = writeln!(out, "{MARK} (gmod-client): part {i} of {n} of `{name}`, build {build}");
    out.push_str("-- Every part goes in one folder (lua/autorun/client/); they run in any order, and the last to arrive runs the bundle.\n");
    out.push_str(GUARD);
    out.push_str("local all = rawget(_G, \"__nl_bundle_parts\")\n");
    out.push_str("if all == nil then all = {} rawset(_G, \"__nl_bundle_parts\", all) end\n");
    let _ = writeln!(out, "local b = all[{key}]");
    let _ = writeln!(out, "if b == nil or b.build ~= \"{build}\" then b = {{ build = \"{build}\", n = 0 }} all[{key}] = b end");
    let _ = writeln!(out, "if b[{i}] == nil then b.n = b.n + 1 end");
    let _ = writeln!(out, "b[{i}] = {}", long_string(piece));
    let _ = writeln!(out, "if b.n < {n} then return end");
    let _ = writeln!(out, "all[{key}] = nil");
    let _ = writeln!(out, "local f = CompileString(table.concat(b, \"\", 1, {n}), {}, false)", quoted(&format!("nl bundle {name}")));
    out.push_str("if type(f) ~= \"function\" then error(f, 0) end\n");
    out.push_str("return f()\n");
    out
}

/// The host types of the declaration files in scope, parents first
/// (`nl_sema::Declarations::host_types`).
fn host_types(loader: &dyn Loader, profile: Profile) -> Vec<(String, Option<String>)> {
    crate::driver::declarations_in_scope(loader, profile).host_types()
}

/// The paths of the `.nl` files that `source` passes to `include` as a
/// literal string: `include("a.nl")`, `include "a.nl"`.
pub fn literal_includes(source: &str) -> Vec<String> {
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

/// The `=` signs of a long bracket that does not occur in `text`.
fn long_level(text: &str) -> String {
    // With the closing bracket after it, so that a text ending in `]`
    // does not close early.
    let closed = format!("{text}]");
    let mut level = 0;
    while closed.contains(&format!("]{}]", "=".repeat(level))) {
        level += 1;
    }
    "=".repeat(level)
}

/// `text` as a Lua long string, `[==[` and a line break, the text, `]==]`,
/// with a level that does not occur in it: Lua drops a line break right
/// after the opening bracket, so the text keeps a line break it starts
/// with (a minified chunk whose first line was a comment), and each of
/// its lines keeps its number.
fn long_string(text: &str) -> String {
    let eq = long_level(text);
    format!("[{eq}[\n{text}]{eq}]")
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

#[allow(clippy::too_many_arguments)]
fn bundle_text(
    entries: &[String],
    chunks: &BTreeMap<String, String>,
    imports: &BTreeMap<String, String>,
    types: &[(String, Option<String>)],
    case_tables: bool,
    runtime: &str,
    host: &str,
    profile: Profile,
) -> String {
    let s = quoted;
    let mut out = String::new();
    // The first line is the caller's for gmod (MARK, then the guard).
    if !profile.is_gmod() {
        let _ = writeln!(out, "{MARK}, {} profile: the NL runtime, its Garry's Mod", profile.name());
        out.push_str("-- host, and the files below, for stock LuaJIT 2.0 and 2.1.\n");
    } else {
        out.push_str("-- The NL runtime, its Garry's Mod host, and the files below, for gmod's client realm.\n");
    }
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
    let _ = writeln!(out, "local host, start = chunk({}, \"nl host\")()", long_string(host));
    if case_tables {
        let t = nl_text::case_tables::case_tables();
        out.push_str("host.case_tables = {\n");
        for (k, v) in [("up", &t.up), ("upx", &t.upx), ("down", &t.down), ("downx", &t.downx), ("swapx", &t.swapx)] {
            let _ = writeln!(out, "  {k} = {},", s(v));
        }
        out.push_str("}\n");
    }
    let _ = writeln!(out, "rt = chunk({}, \"nl runtime\")(host)", long_string(runtime));
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
        assert_eq!(long_string("a"), "[[\na]]");
        assert_eq!(long_string("x]]y"), "[=[\nx]]y]=]");
        assert_eq!(long_string("]] ]=]"), "[==[\n]] ]=]]==]");
        assert_eq!(long_string("x]"), "[=[\nx]]=]");
        assert_eq!(long_string("\nx]"), "[=[\n\nx]]=]");
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

    #[test]
    fn part_names() {
        assert_eq!([part_name("client", 1), part_name("client", 2)], ["client.lua", "client_2.lua"]);
    }

    #[test]
    fn compressed_sizes() {
        let text = "local x = 1\n".repeat(2000);
        let n = lzma_size(text.as_bytes());
        assert!(n > 13 && n < 1000, "{n}");
    }

    #[test]
    fn output_checks() {
        let fail = |_: &str, _: &str| Err("a.nl:7: function at line 3 has more than 200 local variables".to_owned());
        let d = check_output("a.nl", "", "x = 1", Some(&fail));
        assert_eq!((d.len(), d[0].line, d[0].code), (1, 7, "lua-limit"));
        assert!(check_output("a.nl", "", "local x = 'DEFINE_\\066ASECLASS'", None).is_empty());
        let d = check_output("a.nl", "x\nDEFINE_BASECLASS(\"b\")\n", "DEFINE_BASECLASS('b')", None);
        assert_eq!((d.len(), d[0].line, d[0].code), (1, 2, "not-in-gmod"));
    }

    /// The parts of a split give back the text, and run it once, when the
    /// last arrives, whatever the order (run.rs of nlc and
    /// crates/nlc/tests/gmod_client.rs run them on LuaJIT; here the
    /// pieces are checked to join).
    #[test]
    fn parts_hold_the_text_in_pieces() {
        let mut payload = String::from("local t = {}\n");
        let mut x: u64 = 1;
        for i in 1..=6000 {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let _ = writeln!(payload, "t[{i}] = \"{:016x}é]]\"", x);
        }
        let parts = split(&payload, "test", "abc", 20_000).unwrap();
        assert!(parts.len() >= 3, "{}", parts.len());
        let mut joined = String::new();
        for p in &parts {
            assert!(lzma_size(p.as_bytes()) <= 20_000);
            // The piece: after `b[i] = [==[` and its line break, up to `]==]`.
            let open = p.find("] = [").unwrap() + 4;
            let level = p[open + 1..].find('[').unwrap();
            let start = open + 1 + level + 2;
            let close = format!("]{}]", "=".repeat(level));
            joined.push_str(&p[start..start + p[start..].find(&close).unwrap()]);
        }
        assert_eq!(joined, payload);
    }
}
