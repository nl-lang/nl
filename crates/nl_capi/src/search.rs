//! The file search of `import`, written once for `nlc` (crates/nlc,
//! `host::Files`) and the language server (crates/nl_lsp, `files::Files`).
//!
//! An `import 'path'` names `path.nl`, tried relative to the importing
//! file's directory, then to the working directory, then to each directory
//! of the search path ([`candidates`]). The declaration files in scope
//! are the files named one by one, then the
//! `.d.nl` files of a list of directories ([`decl_files`]).
//!
//! The callers differ only in what exists and in how a path becomes a
//! canonical name: `nlc` asks the file system and canonicalizes through
//! it, relative to the working directory; the language server also counts
//! the editor's open buffers and normalizes absolute paths lexically.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// The files an `import path` may name, in the order they are
/// tried: the directory of `from` (the canonical name of the importing
/// file, if any), `cwd`, then each directory of `search_path`. The first
/// one that exists is the file.
pub fn candidates(from: Option<&str>, path: &str, cwd: &Path, search_path: &[PathBuf]) -> Vec<PathBuf> {
    let file = format!("{path}.nl");
    let mut out = Vec::with_capacity(search_path.len() + 2);
    if let Some(dir) = from.and_then(|f| Path::new(f).parent()) {
        out.push(dir.join(&file));
    }
    out.push(cwd.join(&file));
    out.extend(search_path.iter().map(|d| d.join(&file)));
    out
}

/// Splits paths that name declaration files or directories of them
/// (`nlc --decl`, the language server's `declarations`) into
/// `(directories, files)`, by what is on disk.
pub fn split_decl_paths(paths: &[PathBuf]) -> (Vec<PathBuf>, Vec<PathBuf>) {
    paths.iter().cloned().partition(|p| p.is_dir())
}

/// Whether `p` names a declaration file.
pub fn is_decl_name(p: &Path) -> bool {
    p.to_string_lossy().ends_with(".d.nl")
}

/// The files on disk in `dir`, unsorted; none when it cannot be read.
pub fn dir_files(dir: &Path) -> Vec<PathBuf> {
    match std::fs::read_dir(dir) {
        Ok(rd) => rd.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_file()).collect(),
        Err(_) => Vec::new(),
    }
}

/// The canonical names of the declaration files in scope, in the order
/// they are read: `files` as given, then the `.d.nl` files of each
/// directory of `dirs`, by name within a directory. `entries(dir)` lists
/// the files of a directory ([`dir_files`], plus whatever else the caller
/// knows of); `canonical` names a file. Each name is given once, at its
/// first place.
pub fn decl_files(dirs: &[PathBuf], files: &[PathBuf], entries: &dyn Fn(&Path) -> Vec<PathBuf>, canonical: &dyn Fn(&Path) -> String) -> Vec<String> {
    let mut paths: Vec<PathBuf> = files.to_vec();
    for dir in dirs {
        let mut found: Vec<PathBuf> = entries(dir).into_iter().filter(|p| is_decl_name(p)).collect();
        found.sort();
        paths.extend(found);
    }
    let mut seen = HashSet::new();
    paths.iter().map(|p| canonical(p)).filter(|name| seen.insert(name.clone())).collect()
}
