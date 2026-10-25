//! The files the server sees: the open documents, which take precedence
//! over the disk, and the `Loader` the compiler reads imported files and
//! declaration files through.
//!
//! The search order is that of `nlc`, written once in `nl_capi::search`:
//! the importing file's directory, the working directory, then each
//! directory of the search path, which starts as `NL_PATH` and gains the
//! `searchPath` of the client's initialization options. Declaration files
//! are every `.d.nl` in those directories, in the workspace roots, and the
//! `declarations` of the initialization options (files or directories),
//! open buffers included; the prelude of `decl/` is built into the
//! compiler.
//!
//! Open documents are kept by URI, since a document need not be a file
//! (`untitled:` buffers); one that is a file is also reachable by its
//! path, which is what an import resolves to. Canonical names are absolute
//! paths, normalized lexically and never through the file system, so that
//! the name of an open document is the spelling the editor gave and
//! diagnostics return under the same URI.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use nl_capi::{Loader, search};

use crate::position::normalize;

/// An open document.
#[derive(Debug)]
pub struct OpenDoc {
    /// Its path, for a `file:` URI.
    pub path: Option<PathBuf>,
    pub text: String,
}

#[derive(Debug, Default)]
pub struct Files {
    search: Vec<PathBuf>,
    /// `declarations` of the initialization options: files or directories.
    decl_paths: Vec<PathBuf>,
    roots: Vec<PathBuf>,
    cwd: PathBuf,
    /// The open documents, by URI.
    open: HashMap<String, OpenDoc>,
    /// The URI of each open document that is a file, by canonical path.
    by_path: HashMap<PathBuf, String>,
}

impl Files {
    pub fn new() -> Files {
        let search = std::env::var_os("NL_PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        Files { search, cwd, ..Files::default() }
    }

    /// The workspace roots of the client.
    pub fn add_root(&mut self, dir: PathBuf) {
        self.roots.push(self.canonical(&dir));
    }

    pub fn add_search(&mut self, dir: PathBuf) {
        self.search.push(dir);
    }

    pub fn add_decl(&mut self, path: PathBuf) {
        self.decl_paths.push(path);
    }

    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// The canonical name of a path: absolute and normalized.
    pub fn canonical(&self, p: &Path) -> PathBuf {
        if p.is_absolute() { normalize(p) } else { normalize(&self.cwd.join(p)) }
    }

    pub fn open(&mut self, uri: &str, path: Option<PathBuf>, text: String) {
        if let Some(p) = &path {
            self.by_path.insert(p.clone(), uri.to_string());
        }
        self.open.insert(uri.to_string(), OpenDoc { path, text });
    }

    pub fn close(&mut self, uri: &str) {
        if let Some(d) = self.open.remove(uri) {
            if let Some(p) = d.path {
                self.by_path.remove(&p);
            }
        }
    }

    pub fn is_open(&self, uri: &str) -> bool {
        self.open.contains_key(uri)
    }

    /// The URI of the open document that is the file at `path` (a
    /// canonical name), as the editor spelled it.
    pub fn uri_of(&self, path: &Path) -> Option<&str> {
        self.by_path.get(path).map(String::as_str)
    }

    pub fn doc(&self, uri: &str) -> Option<&OpenDoc> {
        self.open.get(uri)
    }

    /// The URIs of the open documents, in a fixed order.
    pub fn open_uris(&self) -> Vec<String> {
        let mut uris: Vec<String> = self.open.keys().cloned().collect();
        uris.sort();
        uris
    }

    /// The open buffer of a file, if the file is open.
    fn overlay(&self, path: &Path) -> Option<&String> {
        self.by_path.get(path).and_then(|uri| self.open.get(uri)).map(|d| &d.text)
    }

    /// The text of a file: the open buffer, else the disk.
    pub fn read_text(&self, path: &Path) -> Option<String> {
        if let Some(t) = self.overlay(path) {
            return Some(t.clone());
        }
        let bytes = std::fs::read(path).ok()?;
        nl_syntax::source_from_bytes(&bytes).ok().map(str::to_owned)
    }

    fn exists(&self, p: &Path) -> bool {
        self.by_path.contains_key(p) || p.is_file()
    }

    /// The import search order: the importing file's directory, the working directory,
    /// the search path.
    pub fn resolve(&self, from: Option<&str>, path: &str) -> Option<String> {
        let candidates = search::candidates(from, path, &self.cwd, &self.search);
        candidates.into_iter().map(|c| self.canonical(&c)).find(|c| self.exists(c)).map(|c| c.to_string_lossy().into_owned())
    }

    pub fn read(&self, name: &str) -> Option<Vec<u8>> {
        let path = Path::new(name);
        if let Some(t) = self.overlay(path) {
            return Some(t.as_bytes().to_vec());
        }
        std::fs::read(path).ok()
    }

    /// The declaration files in scope for a document in `doc_dir`,
    /// each read once, by canonical name: the `.d.nl`
    /// files of the directories, on disk or open in the editor only.
    pub fn declarations(&self, doc_dir: Option<&Path>) -> Vec<(String, Vec<u8>)> {
        let mut dirs: Vec<PathBuf> = Vec::new();
        if let Some(d) = doc_dir {
            dirs.push(self.canonical(d));
        }
        dirs.push(self.cwd.clone());
        dirs.extend(self.search.iter().map(|d| self.canonical(d)));
        dirs.extend(self.roots.iter().cloned());
        let (decl_dirs, files) = search::split_decl_paths(&self.decl_paths);
        dirs.extend(decl_dirs.iter().map(|d| self.canonical(d)));
        let entries = |dir: &Path| {
            let mut found = search::dir_files(dir);
            // Open buffers in the directory that are not on disk yet.
            found.extend(self.by_path.keys().filter(|p| p.parent() == Some(dir)).cloned());
            found
        };
        let canonical = |p: &Path| self.canonical(p).to_string_lossy().into_owned();
        let names = search::decl_files(&dirs, &files, &entries, &canonical);
        names.into_iter().filter_map(|name| self.read(&name).map(|bytes| (name, bytes))).collect()
    }
}

/// The compiler's view of [`Files`] while one document is compiled.
pub struct DocLoader<'a> {
    pub files: &'a Files,
    /// The document's directory, for its declaration files.
    pub dir: Option<PathBuf>,
}

impl Loader for DocLoader<'_> {
    fn resolve(&self, from: Option<&str>, path: &str) -> Option<String> {
        self.files.resolve(from, path)
    }

    fn read(&self, name: &str) -> Option<Vec<u8>> {
        self.files.read(name)
    }

    fn declarations(&self) -> Vec<(String, Vec<u8>)> {
        self.files.declarations(self.dir.as_deref())
    }
}
