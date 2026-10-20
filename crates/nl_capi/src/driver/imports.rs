//! What the resolver learns about imported files (imported
//! names keep their binding kinds).
//!
//! The driver finds a file through the [`Loader`], parses and resolves it,
//! and hands the importing file the exports the resolver recorded for it
//! (`Resolution::exports`: the one place where the name and kind of an
//! export are decided). The file is compiled for real when it is loaded
//! at runtime (`rt.import_from`); at compile time the files a source
//! imports directly are also checked (parsed, resolved and lowered, not
//! printed), so that an error in one of them is reported when the
//! importing file is compiled, with the imported file's own name and
//! positions ([`DriverImports::into_diagnostics`]). Their own imports are
//! resolved without reading the files they import in turn ([`Unread`]),
//! so a compile reads each file of a deep import chain at most once and
//! the work does not grow with the depth of the chain.
//!
//! Import cycles (see `runtime/nl_70_module.lua`) are
//! errors. The runtime finds every cycle when it happens; the compiler
//! reports the ones it sees without following the whole graph: a file
//! that imports itself, and a directly imported file that imports the file
//! being compiled.

use std::cell::RefCell;
use std::collections::HashMap;

use nl_sema::{Declarations, ExportInfo, ExtensionInfo, ImportProvider};

use super::pipeline::{self, Mode};
use super::{CompileOptions, Diagnostic, Loader, Severity};

pub(crate) struct DriverImports<'a> {
    loader: &'a dyn Loader,
    /// The canonical name of the file being compiled, or `None`.
    from: Option<&'a str>,
    options: &'a CompileOptions,
    /// How this file is being compiled: the file asked for (its imports
    /// are checked) or a file it imports (its imports are only parsed).
    mode: Mode<'a>,
    /// What each imported file gave, by canonical name, so a file named by
    /// several imports is read and checked once: its exports, and the
    /// `global` names it and the files it imports declare.
    cache: RefCell<HashMap<String, Result<Imported, String>>>,
    /// The `global` names of the files imported so far ([`Self::globals`]).
    globals: RefCell<Vec<String>>,
    /// The errors of the imported files.
    diagnostics: RefCell<Vec<Diagnostic>>,
    /// The declaration files in scope, with which a file two imports away
    /// is resolved for the functions it adds to declared types.
    decls: &'a Declarations,
    /// The primitive methods and imports of each file looked at for
    /// [`ImportProvider::primitive_methods`], by canonical name (`None`:
    /// it cannot be read or parsed), so that each is parsed once.
    scans: RefCell<HashMap<String, Option<nl_sema::FileScan>>>,
}

impl<'a> DriverImports<'a> {
    pub(crate) fn new(loader: &'a dyn Loader, from: Option<&'a str>, options: &'a CompileOptions, mode: Mode<'a>, decls: &'a Declarations) -> Self {
        DriverImports {
            loader,
            from,
            options,
            mode,
            cache: RefCell::new(HashMap::new()),
            globals: RefCell::new(Vec::new()),
            diagnostics: RefCell::new(Vec::new()),
            decls,
            scans: RefCell::new(HashMap::new()),
        }
    }

    /// `nl_sema::scan_file` of a file: kept from `load_file` for the files
    /// this one imports, read and parsed once for the files further away.
    fn scan(&self, name: &str) -> Option<nl_sema::FileScan> {
        if let Some(done) = self.scans.borrow().get(name) {
            return done.clone();
        }
        let scanned = self.loader.read(name).and_then(|bytes| {
            let text = nl_syntax::source_from_bytes(&bytes).ok()?;
            let parse = nl_syntax::parse(text);
            (!parse.has_errors()).then(|| nl_sema::scan_file(&parse.syntax()))
        });
        self.scans.borrow_mut().insert(name.to_string(), scanned.clone());
        scanned
    }

    /// The names of the `global` declarations of the files
    /// imported so far, and of the files they import: what
    /// `Localize::All` leaves alone in the importing file.
    pub(crate) fn globals(&self) -> Vec<String> {
        self.globals.borrow().clone()
    }

    /// The errors found in the imported files, file by file in the order
    /// they were first imported.
    pub(crate) fn into_diagnostics(self) -> Vec<Diagnostic> {
        self.diagnostics.into_inner()
    }

    fn load(&self, path: &str) -> Result<Vec<ExportInfo>, String> {
        self.imported(path).map(|i| i.exports)
    }

    fn imported(&self, path: &str) -> Result<Imported, String> {
        let name = self.loader.resolve(self.from, path).ok_or_else(|| format!("cannot find the file '{path}' to import"))?;
        let cached = self.cache.borrow().get(&name).cloned();
        let result = match cached {
            Some(done) => done,
            None => {
                let result = self.load_file(&name);
                self.cache.borrow_mut().insert(name, result.clone());
                result
            }
        };
        let imported = result?;
        let mut globals = self.globals.borrow_mut();
        for g in &imported.globals {
            if !globals.contains(g) {
                globals.push(g.clone());
            }
        }
        Ok(imported)
    }

    fn load_file(&self, name: &str) -> Result<Imported, String> {
        if Some(name) == self.from {
            return Err(format!("'{name}' imports itself; an import cycle is an error"));
        }
        if let Mode::Imported { root: Some(root) } = self.mode {
            if name == root {
                let this = self.from.unwrap_or("this file");
                return Err(format!("import cycle: '{root}' imports '{this}', which imports '{root}'"));
            }
        }
        let bytes = self.loader.read(name).ok_or_else(|| format!("cannot read '{name}'"))?;
        if let Mode::Main = self.mode {
            let chunk_name = format!("@{name}");
            let staged = pipeline::stages(&bytes, &chunk_name, self.options, self.loader, Mode::Imported { root: self.from });
            let errors: Vec<Diagnostic> = staged.output.diagnostics.into_iter().filter(|d| d.severity == Severity::Error).collect();
            if let Some(first) = errors.first() {
                let more = match errors.len() {
                    1 => String::new(),
                    n => format!(" (and {} more)", n - 1),
                };
                let msg = format!("'{name}' does not compile: {}:{}:{}: {}{more}", first.file, first.line, first.column, first.message);
                self.diagnostics.borrow_mut().extend(errors);
                return Err(msg);
            }
            self.scans.borrow_mut().insert(name.to_string(), staged.scan);
            let exports = staged.exports.ok_or_else(|| format!("'{name}' does not compile"))?;
            return Ok(Imported { exports, globals: staged.globals, extensions: staged.extensions });
        }
        // A file imported by an imported file: resolved for its exports,
        // not checked (it is checked when the file importing it is
        // compiled, and at the latest when it is loaded).
        let text = String::from_utf8(bytes).map_err(|_| format!("'{name}' is not valid UTF-8"))?;
        let parse = nl_syntax::parse(&text);
        if parse.has_errors() {
            return Err(format!("'{name}' has syntax errors"));
        }
        let root = parse.syntax();
        self.scans.borrow_mut().insert(name.to_string(), Some(nl_sema::scan_file(&root)));
        let res = nl_sema::resolve_with(&root, &Unread, self.decls);
        Ok(Imported { exports: res.exports(), globals: nl_ir::declared_globals(&root), extensions: res.extensions() })
    }
}

/// What an imported file gives the importing one.
#[derive(Clone)]
struct Imported {
    exports: Vec<ExportInfo>,
    /// The `global` names it declares, and those of the files it imports
    /// (for a file the main file imports directly).
    globals: Vec<String>,
    /// The functions it adds to declared modules and types, and those of
    /// the files it imports (for a file the main file imports directly).
    extensions: Vec<ExtensionInfo>,
}

/// The imports of a file that is only resolved for its exports: the files
/// they name are not read. What such a file exports does not depend on
/// them (its `pub` items are declared in it), so the resolver's errors
/// about them are of no interest there.
struct Unread;

impl ImportProvider for Unread {
    fn exports(&self, path: &str) -> Result<Vec<ExportInfo>, String> {
        Err(format!("'{path}' is not read here"))
    }
}

impl ImportProvider for DriverImports<'_> {
    fn exports(&self, path: &str) -> Result<Vec<ExportInfo>, String> {
        self.load(path)
    }

    fn extensions(&self, path: &str) -> Vec<ExtensionInfo> {
        self.imported(path).map(|i| i.extensions).unwrap_or_default()
    }

    /// Follows the imports from the file `path` names, through every file
    /// of the chain (a parse of each; nothing is resolved or checked).
    fn primitive_methods(&self, path: &str) -> Vec<nl_sema::PrimMethod> {
        nl_sema::primitive_methods_through(self.from, path, &|from, p| self.loader.resolve(from, p), &|name| self.scan(name))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use std::cell::Cell;
    use std::collections::HashMap;

    use crate::driver::{CompileOptions, CompileOutput, Loader, Severity, compile};

    /// Files in memory; paths resolve next to the importing file, then at
    /// the root. Counts reads.
    struct Mem {
        files: HashMap<String, String>,
        reads: Cell<u32>,
    }

    impl Mem {
        fn new(files: &[(&str, &str)]) -> Self {
            Mem { files: files.iter().map(|(n, s)| (n.to_string(), s.to_string())).collect(), reads: Cell::new(0) }
        }
    }

    impl Loader for Mem {
        fn resolve(&self, from: Option<&str>, path: &str) -> Option<String> {
            let dir = from.and_then(|f| f.rsplit_once('/')).map(|(d, _)| format!("{d}/")).unwrap_or_default();
            [format!("{dir}{path}.nl"), format!("{path}.nl")].into_iter().find(|c| self.files.contains_key(c))
        }
        fn read(&self, name: &str) -> Option<Vec<u8>> {
            self.reads.set(self.reads.get() + 1);
            self.files.get(name).map(|s| s.as_bytes().to_vec())
        }
    }

    fn build(files: &Mem, main: &str) -> CompileOutput {
        let src = files.files[main].clone();
        compile(src.as_bytes(), &format!("@{main}"), &CompileOptions::default(), files)
    }

    fn errors(out: &CompileOutput) -> Vec<(String, u32, String)> {
        out.diagnostics.iter().filter(|d| d.severity == Severity::Error).map(|d| (d.file.clone(), d.line, d.message.clone())).collect()
    }

    #[test]
    fn imports_compile_and_keep_kinds() {
        let files = Mem::new(&[
            ("main.nl", "import { f, Mode, n } from 'lib/a'\nimport from 'lib/a' as A\nprint f\nprint Mode.X\nprint n\nprint A.f()\n"),
            ("lib/a.nl", "pub fn f\n  1\nend\npub enum Mode\n  X\nend\npub const n = 2\n"),
        ]);
        let out = build(&files, "main.nl");
        assert!(out.chunk.is_some(), "{:?}", out.diagnostics);
        let lua = String::from_utf8(out.chunk.unwrap()).unwrap();
        assert!(lua.contains(r#"__rt.import_from("main.nl", "lib/a", "f", "Mode", "n")"#), "{lua}");
        // `f` is a function: a bare name calls it.
        assert!(lua.contains("print(f())"), "{lua}");
        // lib/a.nl is read once although it is imported twice.
        assert_eq!(files.reads.get(), 1);
    }

    #[test]
    fn errors_of_an_imported_file_carry_its_name_and_positions() {
        let files = Mem::new(&[("main.nl", "import { f } from 'bad'\nprint f\n"), ("bad.nl", "pub fn f\n  1\nend\n\nprint nope\n")]);
        let out = build(&files, "main.nl");
        assert!(out.chunk.is_none());
        let errs = errors(&out);
        assert_eq!(errs.len(), 2, "{errs:?}");
        assert_eq!((errs[0].0.as_str(), errs[0].1), ("main.nl", 1));
        assert!(errs[0].2.contains("bad.nl:5:7"), "{}", errs[0].2);
        assert_eq!((errs[1].0.as_str(), errs[1].1), ("bad.nl", 5));
    }

    #[test]
    fn missing_files_and_names_are_errors_at_the_import() {
        let files = Mem::new(&[("main.nl", "import { f } from 'nowhere'\nimport { g } from 'lib'\n"), ("lib.nl", "fn g\n  1\nend\n")]);
        let errs = errors(&build(&files, "main.nl"));
        assert_eq!(errs.len(), 2, "{errs:?}");
        assert!(errs[0].2.contains("cannot find the file 'nowhere'"), "{}", errs[0].2);
        assert_eq!(errs[1].1, 2);
        assert!(errs[1].2.contains("not exported"), "{}", errs[1].2);
    }

    #[test]
    fn cycles_the_compiler_sees_are_errors() {
        let files = Mem::new(&[
            ("me.nl", "import { x } from 'me'\npub const x = 1\n"),
            ("a.nl", "import { y } from 'b'\npub const x = 1\n"),
            ("b.nl", "import { x } from 'a'\npub const y = 1\n"),
        ]);
        let me = errors(&build(&files, "me.nl"));
        assert!(me.iter().any(|e| e.2.contains("imports itself")), "{me:?}");
        let a = errors(&build(&files, "a.nl"));
        assert!(a.iter().any(|e| e.0 == "b.nl" && e.2.contains("import cycle")), "{a:?}");
    }

    /// The importer sees each export
    /// with the kind the exporting file's resolver gave it, and a name
    /// exported twice is an error in the exporting file.
    #[test]
    fn exports_are_what_the_exporting_file_resolved() {
        let files = Mem::new(&[
            ("main.nl", "import { f, g } from 'lib'\nprint f\nprint g\n"),
            ("lib.nl", "pub const f: Function? = fn() 1 end\npub const g = fn() 2 end\n"),
        ]);
        let out = build(&files, "main.nl");
        let lua = String::from_utf8(out.chunk.clone().unwrap_or_default()).unwrap();
        // `f` is a variable in lib.nl (`Function?`), so it is read;
        // `g` is function-kind, so it is called.
        assert!(lua.contains("print(f)") && lua.contains("print(g())"), "{lua}");

        let files = Mem::new(&[("main.nl", "import { f } from 'dup'\nprint f\n"), ("dup.nl", "pub fn f\n  1\nend\npub const f = 5\n")]);
        let errs = errors(&build(&files, "main.nl"));
        assert!(errs.iter().any(|e| e.0 == "dup.nl" && e.1 == 4 && e.2.contains("exported twice")), "{errs:?}");
    }

    /// The exports of a file two imports away come from its resolver too
    /// (without reading the files it imports): here they say that `plain`
    /// takes no `Function` parameter, so b.nl's block is an error.
    #[test]
    fn exports_of_files_two_imports_away() {
        let files = Mem::new(&[
            ("a.nl", "import { y } from 'b'\nprint y\n"),
            ("b.nl", "import { plain } from 'c'\npub const y = 1\nplain 1 do\nend\n"),
            ("c.nl", "import { z } from 'nowhere'\npub fn plain(x)\n  x\nend\n"),
        ]);
        let errs = errors(&build(&files, "a.nl"));
        assert!(errs.iter().any(|e| e.0 == "b.nl" && e.1 == 3 && e.2.contains("`Function` parameter")), "{errs:?}");
    }

    #[test]
    fn imports_of_imported_files_are_only_parsed() {
        // c.nl has a resolver error; a.nl imports b.nl, which imports c.nl.
        // Compiling a.nl checks b.nl, not c.nl (c.nl is checked when b.nl
        // is compiled).
        let files = Mem::new(&[
            ("a.nl", "import { y } from 'b'\nprint y\n"),
            ("b.nl", "import { z } from 'c'\npub const y = z\n"),
            ("c.nl", "pub const z = nope\n"),
        ]);
        let out = build(&files, "a.nl");
        assert!(out.chunk.is_some(), "{:?}", out.diagnostics);
        let errs = errors(&build(&files, "b.nl"));
        assert!(errs.iter().any(|e| e.0 == "c.nl"), "{errs:?}");
    }
}
