//! `nlc bundle`: the command line of the packager, `nl_capi::bundle`,
//! whose module docs say what a bundle is and holds.
//!
//! What is the command's own: the files come from the disk (host.rs), and
//! canonical names are relative to the working directory, so build from
//! the directory the game sees as the root of the files (an addon's
//! `lua/`); a chunk is checked to load on the stock LuaJIT that `nlc`
//! embeds (run.rs, `lua_check`); the bundle is written to `-o` (its parts
//! beside it, named after it: `-o client.lua` gives `client.lua`,
//! `client_2.lua`, …), or to stdout when it is one file; a part that an
//! earlier build left beyond the new count is removed; the sourcemap is
//! written to `--map`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use nl_capi::bundle::{BundleOptions, DEFAULT_PART_LIMIT, GMOD_FILE_LIMIT, MARK, MIN_PART_LIMIT};
use nl_capi::{CompileOptions, Loader, Profile, Severity};

use crate::host::Files;
use crate::run::{Sources, lua_check, report};

struct Args {
    entries: Vec<String>,
    compile: CompileOptions,
    profile: Profile,
    output: Option<String>,
    map: Option<String>,
    decls: Vec<String>,
    case_tables: bool,
    minify: bool,
    part_limit: usize,
    runtime_include: Option<String>,
    include_parts: Option<String>,
}

/// `--include-parts` without a path: the first file's Lua path is the `-o`
/// path (nlc runs from the addon's `lua/` folder).
const FROM_OUTPUT: &str = "";

fn parse(args: &[String], usage: &str, command: &str) -> Result<Args, ExitCode> {
    let usage_error = |msg: &str| {
        eprintln!("nlc {command}: {msg}\n\n{usage}");
        Err(ExitCode::from(2))
    };
    let mut compile = CompileOptions::default();
    let mut profile = Profile::Portable;
    let mut entries = Vec::new();
    let mut output = None;
    let mut map = None;
    let mut decls = Vec::new();
    let mut case_tables = true;
    let mut minify = false;
    let mut part_limit = None;
    let mut runtime_include = None;
    let mut include_parts = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--profile=portable" => profile = Profile::Portable,
            "--profile=gmod-client" => profile = Profile::GmodClient,
            s if s.starts_with("--profile=") => return usage_error("a bundle runs on stock LuaJIT: the profile is `portable` or `gmod-client`"),
            "--unchecked" => compile.checks = false,
            "--no-static-resolution" => compile.static_resolution = false,
            "--no-analyzer" => compile.analyze = false,
            "--minify" => minify = true,
            "--case-tables=unicode" => case_tables = true,
            "--case-tables=ascii" => case_tables = false,
            s if s.starts_with("--case-tables=") => return usage_error("`--case-tables` is `unicode` or `ascii`"),
            s if s.starts_with("--part-limit=") => match s.trim_start_matches("--part-limit=").parse::<usize>() {
                Ok(n) if (MIN_PART_LIMIT..=GMOD_FILE_LIMIT).contains(&n) => part_limit = Some(n),
                _ => return usage_error(&format!("`--part-limit` is a number of bytes from {MIN_PART_LIMIT} to {GMOD_FILE_LIMIT}")),
            },
            s if s.starts_with("--localize=") => match nl_capi::Localize::from_name(s.trim_start_matches("--localize=")) {
                Some(l) => compile.localize = l,
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
            "--map" => match it.next() {
                Some(p) => map = Some(p.clone()),
                None => return usage_error("`--map` needs a path"),
            },
            s if s.starts_with("--map=") => map = Some(s.trim_start_matches("--map=").to_string()),
            "--runtime-include" => match it.next() {
                Some(p) => runtime_include = Some(p.clone()),
                None => return usage_error("`--runtime-include` needs a Lua path"),
            },
            s if s.starts_with("--runtime-include=") => runtime_include = Some(s.trim_start_matches("--runtime-include=").to_string()),
            "--include-parts" => include_parts = Some(FROM_OUTPUT.to_owned()),
            s if s.starts_with("--include-parts=") => include_parts = Some(s.trim_start_matches("--include-parts=").to_string()),
            "--watch" => return usage_error("`--watch` is not available yet"),
            s if s.starts_with('-') => return usage_error(&format!("unknown option `{s}`")),
            s => entries.push(s.to_string()),
        }
    }
    if command == "runtime" {
        if !entries.is_empty() {
            return usage_error("the runtime has no entry files");
        }
        if runtime_include.is_some() {
            return usage_error("`--runtime-include` is for `nlc bundle`");
        }
    } else if entries.is_empty() {
        return usage_error("no entry file");
    }
    if part_limit.is_some() && !profile.is_gmod() {
        return usage_error("`--part-limit` is for the gmod-client profile");
    }
    if include_parts.is_some() && !profile.is_gmod() {
        return usage_error("`--include-parts` is for the gmod-client profile");
    }
    if include_parts.as_deref() == Some(FROM_OUTPUT) {
        match &output {
            Some(o) => include_parts = Some(o.replace('\\', "/")),
            None => return usage_error("`--include-parts` without a path takes the first file's Lua path from `-o`"),
        }
    }
    Ok(Args {
        entries,
        compile,
        profile,
        output,
        map,
        decls,
        case_tables,
        minify,
        part_limit: part_limit.unwrap_or(DEFAULT_PART_LIMIT),
        runtime_include,
        include_parts,
    })
}

/// The files of `nlc` as the packager's loader. A path of no file (an
/// entry, an `include` of a literal path) that names a file, relative to
/// the working directory, is that file; any other path is looked for as
/// `import` looks.
struct FilesLoader<'a>(&'a Files);

impl Loader for FilesLoader<'_> {
    fn resolve(&self, from: Option<&str>, path: &str) -> Option<String> {
        if from.is_none() && Path::new(path).is_file() {
            return Some(self.0.canonical(Path::new(path)));
        }
        self.0.resolve(from, path)
    }
    fn read(&self, name: &str) -> Option<Vec<u8>> {
        self.0.read(name)
    }
    fn declarations(&self) -> Vec<(String, Vec<u8>)> {
        self.0.declarations()
    }
}

/// `nlc bundle [options] <entry.nl>...`.
pub fn cmd_bundle(args: &[String], usage: &str) -> ExitCode {
    cmd(args, usage, "bundle")
}

/// `nlc runtime [options]`: the file that bundles made with
/// `--runtime-include` include.
pub fn cmd_runtime(args: &[String], usage: &str) -> ExitCode {
    cmd(args, usage, "runtime")
}

fn cmd(args: &[String], usage: &str, command: &str) -> ExitCode {
    let a = match parse(args, usage, command) {
        Ok(a) => a,
        Err(code) => return code,
    };
    let files = Files::new();
    for d in &a.decls {
        files.add_decl(d);
    }
    // The entries by canonical name.
    let mut entries = Vec::new();
    for path in &a.entries {
        if !Path::new(path).is_file() {
            eprintln!("{path}: error: cannot read the file");
            return ExitCode::FAILURE;
        }
        entries.push(files.canonical(Path::new(path)));
    }
    let default_name = if command == "runtime" { "runtime" } else { "bundle" };
    let name = a.output.as_deref().and_then(|o| Path::new(o).file_stem()).map_or_else(|| default_name.to_owned(), |s| s.to_string_lossy().into_owned());
    let lua = mlua::Lua::new();
    let check = lua_check(&lua);
    let options = BundleOptions {
        compile: a.compile,
        profile: a.profile,
        name: &name,
        minify: a.minify,
        case_tables: a.case_tables,
        part_limit: a.part_limit,
        sourcemap: a.map.is_some(),
        check: Some(&check),
        runtime_include: a.runtime_include.as_deref(),
        include_parts: a.include_parts.as_deref(),
    };
    let entry_refs: Vec<&str> = entries.iter().map(String::as_str).collect();
    let out = if command == "runtime" { nl_capi::bundle::runtime(&options) } else { nl_capi::bundle::bundle(&entry_refs, &options, &FilesLoader(&files)) };

    // Each diagnostic with its snippet (Sources reads each file once).
    let mut sources = Sources::new("", &[]);
    for d in &out.diagnostics {
        report(d, &mut sources);
    }
    if out.diagnostics.iter().any(|d| d.severity == Severity::Error) {
        return ExitCode::FAILURE;
    }

    let paths: Vec<PathBuf> = match &a.output {
        Some(o) => part_paths(Path::new(o), out.parts.len()),
        None => Vec::new(),
    };
    if let (Some(map_path), Some(map)) = (&a.map, &out.sourcemap) {
        if let Err(e) = std::fs::write(map_path, map.to_json()) {
            eprintln!("{map_path}: error: cannot write the file: {e}");
            return ExitCode::FAILURE;
        }
    }
    let Some(output) = &a.output else {
        if out.parts.len() > 1 {
            eprintln!("nlc {command}: error: the bundle is over {} bytes compressed, so it is written as several files: give their path with `-o`", a.part_limit);
            return ExitCode::FAILURE;
        }
        use std::io::Write as _;
        let _ = std::io::stdout().write_all(out.parts.iter().map(|p| p.text.as_str()).collect::<String>().as_bytes());
        return ExitCode::SUCCESS;
    };
    for (path, part) in paths.iter().zip(&out.parts) {
        if let Err(e) = std::fs::write(path, &part.text) {
            eprintln!("{}: error: cannot write the file: {e}", path.display());
            return ExitCode::FAILURE;
        }
    }
    remove_stale_parts(Path::new(output), out.parts.len());
    if out.parts.len() > 1 {
        let names: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
        let place = if a.include_parts.is_some() { "the first includes the others" } else { "put them all in one folder (lua/autorun/client/)" };
        eprintln!("nlc {command}: the bundle is {} files, each under {} bytes compressed: {}; {place}", out.parts.len(), a.part_limit, names.join(", "));
    }
    ExitCode::SUCCESS
}

/// The paths of a bundle's files: `-o` itself, then `<stem>_2.<ext>`, …
fn part_paths(output: &Path, n: usize) -> Vec<PathBuf> {
    (1..=n).map(|i| if i == 1 { output.to_path_buf() } else { part_path(output, i) }).collect()
}

fn part_path(output: &Path, i: usize) -> PathBuf {
    let stem = output.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let name = match output.extension() {
        Some(ext) => format!("{stem}_{i}.{}", ext.to_string_lossy()),
        None => format!("{stem}_{i}"),
    };
    output.with_file_name(name)
}

/// Removes the parts after the `n`th that an earlier build wrote, which
/// gmod would otherwise still run: the files `<stem>_<i>` from `n + 1` on
/// whose first line is the packager's mark, until one is missing.
fn remove_stale_parts(output: &Path, n: usize) {
    for i in (n + 1).max(2).. {
        let p = part_path(output, i);
        let Ok(text) = std::fs::read(&p) else { break };
        if text.starts_with(MARK.as_bytes()) {
            let _ = std::fs::remove_file(&p);
        } else {
            break;
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn part_names() {
        let p = part_paths(Path::new("out/client.lua"), 3);
        assert_eq!(p, [PathBuf::from("out/client.lua"), PathBuf::from("out/client_2.lua"), PathBuf::from("out/client_3.lua")]);
    }
}
