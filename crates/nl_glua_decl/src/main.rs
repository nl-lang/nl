//! `glua-decl`: generates `decl/glua.d.nl`, the declarations of Garry's
//! Mod's Lua API, from the wiki data that glua-api-snippets publishes
//! with each release (`<date>.json.zip`, `fetch.sh` beside this crate).
//!
//! ```text
//! glua-decl <json-dir> [--lua <lua-dir>] [-o <file>] [--realm server|client|menu]
//! ```
//!
//! `<json-dir>` is the release's `<date>.json.zip` unpacked, and
//! `<lua-dir>` its `<date>.lua.zip`: the Lua annotations give back the
//! functions whose JSON pages overwrote each other (`source.rs`).
//!
//! The output is the same for the same data. Before it is written it is
//! checked: it must parse as a declaration file, be formatted as
//! `nlc fmt` formats it, and load beside the built-in declarations
//! without a diagnostic. What was left out, and why, goes to stderr.

mod emit;
mod extra;
mod names;
mod source;
mod types;

use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: glua-decl <json-dir> [--lua <lua-dir>] [-o <file>] [--realm server|client|menu]

  <json-dir>       the unpacked <date>.json.zip of a glua-api-snippets release
  --lua <lua-dir>  the unpacked <date>.lua.zip of the same release
  -o <file>        where to write (default: standard output)
  --realm <realm>  declare only what exists in one realm";

/// The override list and the hand-written declarations.
const NAMES: &str = include_str!("../names.txt");
const EXTRA: &str = include_str!("../extra.d.nl");

fn main() -> ExitCode {
    let mut data: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut lua: Option<PathBuf> = None;
    let mut side = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "-o" => out = args.next().map(PathBuf::from),
            "--lua" => lua = args.next().map(PathBuf::from),
            "--realm" => match args.next().as_deref().and_then(emit::Side::parse) {
                Some(s) => side = Some(s),
                None => return usage(),
            },
            "-h" | "--help" => {
                println!("{USAGE}");
                return ExitCode::SUCCESS;
            }
            s if s.starts_with('-') => return usage(),
            _ if data.is_none() => data = Some(PathBuf::from(a)),
            _ => return usage(),
        }
    }
    let Some(data) = data else { return usage() };
    match run(&data, lua.as_deref(), out.as_ref(), side) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("glua-decl: {e}");
            ExitCode::FAILURE
        }
    }
}

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

fn run(data: &std::path::Path, lua: Option<&std::path::Path>, out: Option<&PathBuf>, side: Option<emit::Side>) -> Result<(), String> {
    let mut api = source::Api::load(data)?;
    let recovered = match lua {
        Some(dir) => api.recover_from_lua(dir)?,
        None => Vec::new(),
    };
    let overrides = names::Overrides::parse(NAMES)?;
    let extra = extra::Extra::parse(EXTRA)?;
    let generated = emit::generate(&api, &overrides, &extra, side);
    let text = match check(&generated.text) {
        Ok(t) => t,
        Err(e) => {
            // Keep what was generated, to see what went wrong.
            if let Some(path) = out {
                let rejected = path.with_extension("rejected");
                let _ = std::fs::write(&rejected, &generated.text);
                return Err(format!("{e}\n(the output is in {})", rejected.display()));
            }
            return Err(e);
        }
    };
    match out {
        Some(path) => std::fs::write(path, &text).map_err(|e| format!("{}: {e}", path.display()))?,
        None => print!("{text}"),
    }
    let s = &generated.stats;
    eprintln!(
        "glua-decl: {} classes ({} methods), {} derma panel classes ({} methods), {} tables of hooks ({} hooks), {} libraries ({} functions), {} global functions, {} enums ({} constants); {} structures typed `Table`; {} bytes, {} lines",
        s.classes,
        s.class_methods,
        s.panels,
        s.panel_methods,
        s.hook_tables,
        s.hooks,
        s.libraries,
        s.library_functions,
        s.globals,
        s.enums,
        s.enum_constants,
        s.structures,
        text.len(),
        text.lines().count()
    );
    if !recovered.is_empty() {
        eprintln!("glua-decl: {} functions from the Lua annotations: {}", recovered.len(), recovered.join(", "));
    }
    for skipped in &s.skipped {
        eprintln!("glua-decl: left out: {skipped}");
    }
    if !s.unknown_types.is_empty() {
        eprintln!("glua-decl: wiki types with no declaration, typed `any`: {}", s.unknown_types.iter().cloned().collect::<Vec<_>>().join(", "));
    }
    Ok(())
}

/// The formatted text, once it parses and loads without a diagnostic.
fn check(text: &str) -> Result<String, String> {
    let parse = nl_syntax::parse_decl_file(text);
    if !parse.diagnostics().is_empty() {
        return Err(format!("the output does not parse:\n{}", nl_syntax::dump_diagnostics(text, parse.diagnostics())));
    }
    let formatted = nl_syntax::format_decl(text, nl_syntax::FormatOptions::default()).map_err(|d| format!("the output cannot be formatted: {d:?}"))?.text;
    let mut files = nl_sema::decl::prelude_files();
    files.push(("decl/glua.d.nl".to_string(), nl_syntax::parse_decl_file(&formatted).syntax()));
    let (decls, diags) = nl_sema::Declarations::load(&files);
    if let Some((file, d)) = diags.first() {
        return Err(format!("{file}: {} ({} diagnostics)", d.message, diags.len()));
    }
    // The analyzer takes a type name it does not know for `any`; every one
    // the generator writes must be declared.
    let builtin: Vec<&str> = nl_sema::prelude::all().iter().map(|p| p.name).collect();
    let root = files.last().map(|(_, r)| r.clone());
    for n in root.iter().flat_map(|r| r.descendants()).filter(|n| n.kind() == nl_syntax::SyntaxKind::NAMED_TYPE) {
        let name = n.text().to_string();
        let name = name.trim();
        if name.starts_with(|c: char| c.is_ascii_uppercase()) && name != "Self" && decls.lookup(name).is_none() && !builtin.contains(&name) {
            return Err(format!("the output names the type `{name}`, which nothing declares"));
        }
    }
    Ok(formatted)
}
