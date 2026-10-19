//! The per-process cache of compiled files: every state of a process, the main
//! state and each of its workers, gets the same bytecode for a file, and a
//! file is compiled once.
//!
//! An entry is keyed by the chunk name, the source bytes, the options that
//! change the output (checks, strip, backend, feature level, localized
//! paths, the analyzer and static resolution) and the declaration files
//! named. A compile also reads other files through the loader: the files
//! it imports, for their declarations, and the declaration files. So an
//! entry keeps what each call of the loader answered, and is used only when
//! the loader answers the same again: the files are read again, which is
//! cheap next to a compile, and a changed import makes a new compile.
//!
//! The cache is one `Mutex`; nothing is compiled while it is held, so two
//! threads that compile the same file at once both compile it, and the
//! second entry replaces the first. It holds at most [`MAX_ENTRIES`]
//! entries and about [`MAX_BYTES`] bytes, and forgets the least recently
//! used first.

use std::cell::RefCell;
use std::sync::Mutex;

use crate::driver::{CompileOptions, CompileOutput, Emit, Loader};

/// The most entries the cache holds.
pub const MAX_ENTRIES: usize = 512;
/// About the most bytes the entries hold (sources, chunks, the files read).
pub const MAX_BYTES: usize = 64 << 20;

/// What a compile depends on besides its key: one call of the loader and
/// its answer.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Dep {
    Resolve(Option<String>, String, Option<String>),
    Read(String, Option<Vec<u8>>),
    Declarations(Vec<(String, Vec<u8>)>),
}

impl Dep {
    fn bytes(&self) -> usize {
        match self {
            Dep::Resolve(f, p, r) => f.as_ref().map_or(0, String::len) + p.len() + r.as_ref().map_or(0, String::len),
            Dep::Read(n, b) => n.len() + b.as_ref().map_or(0, Vec::len),
            Dep::Declarations(list) => list.iter().map(|(n, b)| n.len() + b.len()).sum(),
        }
    }

    /// Whether `loader` answers the same now.
    fn holds(&self, loader: &dyn Loader) -> bool {
        match self {
            Dep::Resolve(from, path, r) => loader.resolve(from.as_deref(), path) == *r,
            Dep::Read(name, b) => loader.read(name) == *b,
            Dep::Declarations(list) => loader.declarations() == *list,
        }
    }
}

/// The options that change what a compile produces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct OptKey {
    checks: bool,
    strip: bool,
    bytecode: bool,
    analyze: bool,
    static_resolution: bool,
    level: u8,
    localize: crate::driver::Localize,
}

impl OptKey {
    fn of(o: &CompileOptions) -> OptKey {
        OptKey {
            checks: o.checks,
            strip: o.strip,
            bytecode: o.emit == Emit::Bytecode,
            analyze: o.analyze,
            static_resolution: o.static_resolution,
            level: o.level,
            localize: o.localize,
        }
    }
}

/// What a compile is looked up by.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    chunk_name: String,
    source: Vec<u8>,
    options: OptKey,
    decls: Vec<String>,
}

impl Key {
    pub fn new(chunk_name: &str, source: &[u8], options: &CompileOptions, decls: &[String]) -> Key {
        Key { chunk_name: chunk_name.to_owned(), source: source.to_vec(), options: OptKey::of(options), decls: decls.to_vec() }
    }

    fn bytes(&self) -> usize {
        self.chunk_name.len() + self.source.len() + self.decls.iter().map(String::len).sum::<usize>()
    }
}

struct Entry {
    key: Key,
    deps: Vec<Dep>,
    out: CompileOutput,
    bytes: usize,
    used: u64,
}

struct Cache {
    entries: Vec<Entry>,
    bytes: usize,
    clock: u64,
    hits: u64,
    misses: u64,
}

static CACHE: Mutex<Cache> = Mutex::new(Cache { entries: Vec::new(), bytes: 0, clock: 0, hits: 0, misses: 0 });

fn with_cache<T>(f: impl FnOnce(&mut Cache) -> T) -> T {
    // A panic while the lock was held leaves consistent data (every update
    // is a push or a swap_remove): the poison is ignored.
    let mut guard = CACHE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    f(&mut guard)
}

/// A loader that notes every answer of the one it wraps.
struct Recorder<'a> {
    inner: &'a dyn Loader,
    deps: RefCell<Vec<Dep>>,
}

impl Loader for Recorder<'_> {
    fn resolve(&self, from: Option<&str>, path: &str) -> Option<String> {
        let r = self.inner.resolve(from, path);
        self.deps.borrow_mut().push(Dep::Resolve(from.map(str::to_owned), path.to_owned(), r.clone()));
        r
    }

    fn read(&self, name: &str) -> Option<Vec<u8>> {
        let b = self.inner.read(name);
        self.deps.borrow_mut().push(Dep::Read(name.to_owned(), b.clone()));
        b
    }

    fn declarations(&self) -> Vec<(String, Vec<u8>)> {
        let list = self.inner.declarations();
        self.deps.borrow_mut().push(Dep::Declarations(list.clone()));
        list
    }
}

/// The cached output for `key`, when `loader` still answers as it did for
/// that compile. `before_compile` runs when the compile is needed after a
/// look-up that read files (the caller resets what reading did to its
/// loader); then `compile` runs with a loader that notes the answers, and
/// its output is kept when `keep` says so.
pub fn compile_cached(
    key: Key,
    loader: &dyn Loader,
    before_compile: impl FnOnce(),
    compile: impl FnOnce(&dyn Loader) -> CompileOutput,
    keep: impl FnOnce(&CompileOutput) -> bool,
) -> CompileOutput {
    let found = with_cache(|c| {
        c.entries.iter().position(|e| e.key == key).and_then(|i| c.entries.get(i)).map(|e| (e.deps.clone(), e.out.clone()))
    });
    let mut looked = false;
    if let Some((deps, out)) = found {
        looked = !deps.is_empty();
        if deps.iter().all(|d| d.holds(loader)) {
            with_cache(|c| {
                c.clock += 1;
                c.hits += 1;
                let now = c.clock;
                if let Some(e) = c.entries.iter_mut().find(|e| e.key == key) {
                    e.used = now;
                }
            });
            return out;
        }
    }
    if looked {
        before_compile();
    }
    let rec = Recorder { inner: loader, deps: RefCell::new(Vec::new()) };
    let out = compile(&rec);
    if keep(&out) {
        let deps = rec.deps.into_inner();
        let bytes = key.bytes()
            + deps.iter().map(Dep::bytes).sum::<usize>()
            + out.chunk.as_ref().map_or(0, Vec::len)
            + out.diagnostics.iter().map(|d| d.message.len() + d.file.len() + 64).sum::<usize>();
        if bytes <= MAX_BYTES / 4 {
            let entry_out = out.clone();
            with_cache(|c| insert(c, Entry { key, deps, out: entry_out, bytes, used: 0 }));
        }
    } else {
        with_cache(|c| c.misses += 1);
    }
    out
}

fn insert(c: &mut Cache, mut e: Entry) {
    c.clock += 1;
    c.misses += 1;
    e.used = c.clock;
    if let Some(i) = c.entries.iter().position(|x| x.key == e.key) {
        let old = c.entries.swap_remove(i);
        c.bytes -= old.bytes;
    }
    while !c.entries.is_empty() && (c.entries.len() >= MAX_ENTRIES || c.bytes + e.bytes > MAX_BYTES) {
        let Some(oldest) = c.entries.iter().enumerate().min_by_key(|(_, x)| x.used).map(|(i, _)| i) else { break };
        let old = c.entries.swap_remove(oldest);
        c.bytes -= old.bytes;
    }
    c.bytes += e.bytes;
    c.entries.push(e);
}

/// Entries, bytes held, hits and misses so far (for tests and measurements).
pub fn stats() -> (usize, usize, u64, u64) {
    with_cache(|c| (c.entries.len(), c.bytes, c.hits, c.misses))
}

/// Forgets every entry.
pub fn clear() {
    with_cache(|c| {
        c.entries.clear();
        c.bytes = 0;
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::driver::{Diagnostic, Severity};
    use std::cell::Cell;
    use std::collections::HashMap;

    /// The tests share the process's cache: one at a time.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    struct Files(RefCell<HashMap<String, Vec<u8>>>);

    impl Loader for Files {
        fn resolve(&self, _from: Option<&str>, path: &str) -> Option<String> {
            let name = format!("{path}.nl");
            self.0.borrow().contains_key(&name).then_some(name)
        }
        fn read(&self, name: &str) -> Option<Vec<u8>> {
            self.0.borrow().get(name).cloned()
        }
    }

    fn output(text: &str) -> CompileOutput {
        CompileOutput {
            chunk: Some(text.as_bytes().to_vec()),
            diagnostics: vec![Diagnostic {
                severity: Severity::Warning,
                code: "w",
                message: text.to_owned(),
                file: "a.nl".to_owned(),
                line: 1,
                column: 1,
                end_line: 1,
                end_column: 1,
            }],
        }
    }

    #[test]
    fn a_file_is_compiled_once_until_an_import_changes() {
        let _one = TEST_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let files = Files(RefCell::new(HashMap::from([("b.nl".to_owned(), b"pub fn f end".to_vec())])));
        let compiles = Cell::new(0);
        let opts = CompileOptions::default();
        let name = "@cache_test_a.nl";
        let run = |files: &Files| {
            compile_cached(
                Key::new(name, b"import { f } from 'b'", &opts, &[]),
                files,
                || {},
                |l| {
                    compiles.set(compiles.get() + 1);
                    let b = l.resolve(Some("a.nl"), "b").and_then(|n| l.read(&n)).unwrap_or_default();
                    output(&String::from_utf8_lossy(&b))
                },
                |_| true,
            )
        };
        let first = run(&files);
        let second = run(&files);
        assert_eq!(compiles.get(), 1);
        assert_eq!(first, second);
        files.0.borrow_mut().insert("b.nl".to_owned(), b"pub fn g end".to_vec());
        let third = run(&files);
        assert_eq!(compiles.get(), 2);
        assert_eq!(third.chunk.as_deref(), Some(&b"pub fn g end"[..]));
        // Other options are another entry.
        let unchecked = CompileOptions { checks: false, ..CompileOptions::default() };
        compile_cached(Key::new(name, b"import { f } from 'b'", &unchecked, &[]), &files, || {}, |_| output("u"), |_| true);
        let again = run(&files);
        assert_eq!(compiles.get(), 2);
        assert_eq!(again, third);
    }

    #[test]
    fn the_cache_is_bounded() {
        let _one = TEST_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let files = Files(RefCell::new(HashMap::new()));
        let opts = CompileOptions::default();
        for i in 0..(MAX_ENTRIES + 10) {
            let name = format!("@cache_bound_{i}.nl");
            compile_cached(Key::new(&name, b"x", &opts, &[]), &files, || {}, |_| output("x"), |_| true);
        }
        let (entries, bytes, _, _) = stats();
        assert!(entries <= MAX_ENTRIES);
        assert!(bytes <= MAX_BYTES);
    }
}
