//! The stages of [`super::compile`], in order: bytes to text, parse,
//! resolve ([`nl_sema`]), analyze ([`nl_sema::analyze`]), check literal
//! regex patterns ([`super::regex_literal`]), lower ([`nl_ir`]), then print
//! ([`nl_luagen`]) or emit bytecode ([`nl_codegen`]) as
//! [`CompileOptions::emit`] says.
//!
//! A stage runs only when the stages before it found no error, so the
//! later stages may rely on a well-formed, resolved tree.
//!
//! # The compiler thread
//!
//! The resolver, the lowering and the printer recurse over the tree, and
//! the parser accepts chains of about a thousand operators (nl_syntax,
//! "Limits"), which is more than a 2 MiB thread stack holds in those
//! passes. [`compile`] therefore runs the stages on a thread of its own
//! with a large stack ([`STACK_SIZE`]; reserved, not committed, memory).
//! The [`Loader`] stays on the calling thread: the compiler thread sends it
//! its requests and waits for the answers, so a `Loader` need not be `Send`
//! or `Sync`. If the thread cannot be started, the stages run on the
//! calling thread. A panic in the stages becomes an `internal-error`
//! diagnostic.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc;

use nl_sema::{Declarations, ExportInfo};
use nl_syntax::{LineIndex, TextRange, TextSize};

use super::imports::DriverImports;
use super::{CompileOptions, CompileOutput, Diagnostic, Emit, Loader, Severity};

/// The file name diagnostics use: the chunk name without its `@` or `=`.
pub(crate) fn file_label(chunk_name: &str) -> String {
    chunk_name.strip_prefix('@').or_else(|| chunk_name.strip_prefix('=')).unwrap_or(chunk_name).to_owned()
}

/// Turns byte ranges into the line and column positions of [`Diagnostic`].
pub(crate) struct Positions<'a> {
    text: &'a str,
    index: LineIndex,
    file: String,
}

impl<'a> Positions<'a> {
    pub(crate) fn new(text: &'a str, file: String) -> Self {
        Positions { text, index: LineIndex::new(text), file }
    }

    pub(crate) fn diagnostic(&self, severity: Severity, code: &'static str, message: String, range: TextRange) -> Diagnostic {
        let start = self.index.line_col(self.text, range.start());
        let end = self.index.line_col(self.text, range.end());
        Diagnostic {
            severity,
            code,
            message,
            file: self.file.clone(),
            line: start.line,
            column: start.col,
            end_line: end.line,
            end_column: end.col,
        }
    }
}

/// Errors first, then the rest, each in source order (driver docs).
fn sorted(mut diags: Vec<Diagnostic>) -> Vec<Diagnostic> {
    diags.sort_by_key(|d| (d.severity != Severity::Error, d.line, d.column));
    diags
}

fn failed(diags: Vec<Diagnostic>) -> CompileOutput {
    CompileOutput { chunk: None, diagnostics: sorted(diags) }
}

/// Which file a run of [`stages`] compiles.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Mode<'a> {
    /// The file [`super::compile`] was asked for: printed, and the files it
    /// imports are checked (`imports.rs`).
    Main,
    /// A file imported by the main file `root` (its canonical name, if it
    /// has one), checked for errors only: not printed, and its own imports
    /// are only parsed.
    Imported { root: Option<&'a str> },
}

/// The stack of the compiler thread.
pub(crate) const STACK_SIZE: usize = 256 << 20;

/// A request from the compiler thread to the loader.
enum Request {
    Resolve { from: Option<String>, path: String, reply: mpsc::Sender<Option<String>> },
    Read { name: String, reply: mpsc::Sender<Option<Vec<u8>>> },
    Declarations { reply: mpsc::Sender<Vec<(String, Vec<u8>)>> },
}

/// The loader as the compiler thread sees it.
struct LoaderProxy {
    requests: mpsc::Sender<Request>,
}

impl Loader for LoaderProxy {
    fn resolve(&self, from: Option<&str>, path: &str) -> Option<String> {
        let (reply, answer) = mpsc::channel();
        let req = Request::Resolve { from: from.map(str::to_owned), path: path.to_owned(), reply };
        self.requests.send(req).ok()?;
        answer.recv().ok().flatten()
    }

    fn read(&self, name: &str) -> Option<Vec<u8>> {
        let (reply, answer) = mpsc::channel();
        self.requests.send(Request::Read { name: name.to_owned(), reply }).ok()?;
        answer.recv().ok().flatten()
    }

    fn declarations(&self) -> Vec<(String, Vec<u8>)> {
        let (reply, answer) = mpsc::channel();
        if self.requests.send(Request::Declarations { reply }).is_err() {
            return Vec::new();
        }
        answer.recv().unwrap_or_default()
    }
}

pub(crate) fn compile(source: &[u8], chunk_name: &str, options: &CompileOptions, loader: &dyn Loader) -> CompileOutput {
    let (requests, incoming) = mpsc::channel::<Request>();
    std::thread::scope(|scope| {
        let worker = std::thread::Builder::new().name("nl compiler".to_owned()).stack_size(STACK_SIZE).spawn_scoped(scope, move || {
            let proxy = LoaderProxy { requests };
            catch_unwind(AssertUnwindSafe(|| stages(source, chunk_name, options, &proxy, Mode::Main).output))
        });
        let Ok(worker) = worker else {
            // No thread: run here, with the stack there is.
            return catch_unwind(AssertUnwindSafe(|| stages(source, chunk_name, options, loader, Mode::Main).output)).unwrap_or_else(|_| internal_error(chunk_name));
        };
        // Serve the loader until the compiler thread drops its proxy.
        for req in incoming {
            match req {
                Request::Resolve { from, path, reply } => {
                    let _ = reply.send(loader.resolve(from.as_deref(), &path));
                }
                Request::Read { name, reply } => {
                    let _ = reply.send(loader.read(&name));
                }
                Request::Declarations { reply } => {
                    let _ = reply.send(loader.declarations());
                }
            }
        }
        match worker.join() {
            Ok(Ok(out)) => out,
            _ => internal_error(chunk_name),
        }
    })
}

fn internal_error(chunk_name: &str) -> CompileOutput {
    CompileOutput {
        chunk: None,
        diagnostics: vec![Diagnostic {
            severity: Severity::Error,
            code: "internal-error",
            message: "internal compiler error: the compiler panicked".to_owned(),
            file: file_label(chunk_name),
            line: 1,
            column: 1,
            end_line: 1,
            end_column: 1,
        }],
    }
}

/// What [`stages`] produced: the output of the compile, and the file's
/// exports as the resolver decided them (`None` when the file did not get
/// that far), which `imports.rs` hands to the importing file.
pub(crate) struct Staged {
    pub(crate) output: CompileOutput,
    pub(crate) exports: Option<Vec<ExportInfo>>,
    /// The `global` names the file and the files it imports declare
    /// (`imports.rs`, `DriverImports::globals`).
    pub(crate) globals: Vec<String>,
}

impl From<CompileOutput> for Staged {
    fn from(output: CompileOutput) -> Self {
        Staged { output, exports: None, globals: Vec::new() }
    }
}

pub(crate) fn stages(source: &[u8], chunk_name: &str, options: &CompileOptions, loader: &dyn Loader, mode: Mode<'_>) -> Staged {
    let file = file_label(chunk_name);
    let text = match nl_syntax::source_from_bytes(source) {
        Ok(t) => t,
        Err(d) => {
            // Point at the first invalid byte, located in the valid prefix.
            let start = usize::from(d.range.start());
            let prefix = String::from_utf8_lossy(source.get(..start).unwrap_or(&[])).into_owned();
            let pos = Positions::new(&prefix, file);
            let at = TextRange::empty(TextSize::of(prefix.as_str()));
            return failed(vec![pos.diagnostic(Severity::Error, "invalid-utf8", d.message, at)]).into();
        }
    };
    let pos = Positions::new(text, file.clone());

    // Parse.
    let parse = nl_syntax::parse(text);
    let mut diags: Vec<Diagnostic> = parse
        .diagnostics()
        .iter()
        .map(|d| {
            let sev = if d.is_error() { Severity::Error } else { Severity::Warning };
            pos.diagnostic(sev, "syntax", d.message.clone(), d.range)
        })
        .collect();
    if parse.has_errors() {
        return failed(diags).into();
    }
    let root = parse.syntax();

    // Declaration files: the built-in ones and the loader's.
    let (decls, decl_diags) = load_declarations(loader);
    diags.extend(decl_diags);

    // Resolve.
    let from = chunk_name.strip_prefix('@');
    let imports = DriverImports::new(loader, from, options, mode);
    let res = nl_sema::resolve_with(&root, &imports, &decls);
    let sema_severity = |d: &nl_sema::SemaDiagnostic| if d.error { Severity::Error } else if d.lint { Severity::Lint } else { Severity::Warning };
    diags.extend(res.diagnostics.iter().map(|d| pos.diagnostic(sema_severity(d), d.code, d.message.clone(), d.range)));
    if res.has_errors() || diags.iter().any(|d| d.severity == Severity::Error) {
        let mut out = failed(diags);
        out.diagnostics.extend(imports.into_diagnostics());
        return out.into();
    }
    let exports = Some(res.exports());
    // The `global` names of this file and of its imports, which
    // `Localize::All` leaves alone.
    let imported_globals = imports.globals();
    let mut globals = nl_ir::declared_globals(&root);
    globals.extend(imported_globals.iter().filter(|g| !globals.contains(g)).cloned().collect::<Vec<_>>());

    // Analyze. Without `options.analyze` the pass still runs for the
    // member keys of declared names, and reports nothing.
    let aopts = nl_sema::AnalyzeOptions { static_resolution: options.static_resolution, decls: Some(&decls), lints: options.analyze, imports: Some(&imports), record: false };
    let analysis = nl_sema::analyze(&root, &res, &aopts);
    // The errors of imported files follow this file's diagnostics.
    let imported = imports.into_diagnostics();
    if options.analyze {
        diags.extend(analysis.diagnostics.iter().map(|d| pos.diagnostic(sema_severity(d), d.code, d.message.clone(), d.range)));
        if analysis.has_errors() {
            let mut out = failed(diags);
            out.diagnostics.extend(imported);
            return Staged { output: out, exports, globals };
        }
    }
    let hints = if options.static_resolution { analysis.hints } else { analysis.hints.without_static_resolution() };

    // Literal patterns of Regex.new (regex_literal.rs).
    let bad_patterns = super::regex_literal::check(&root, &res);
    if !bad_patterns.is_empty() {
        diags.extend(bad_patterns.into_iter().map(|(range, msg)| pos.diagnostic(Severity::Error, super::regex_literal::CODE, msg, range)));
        let mut out = failed(diags);
        out.diagnostics.extend(imported);
        return Staged { output: out, exports, globals };
    }

    // Lower.
    // The Lua backend is level 0 whatever the option says: it runs on
    // stock LuaJIT.
    let level = if options.emit == Emit::Bytecode { options.level } else { 0 };
    let lopts = nl_ir::LowerOptions {
        checks: options.checks,
        chunk_name: chunk_name.to_owned(),
        file_label: file,
        hints,
        level,
        localize: options.localize,
        imported_globals,
    };
    let out = nl_ir::lower(&root, text, &res, &lopts);
    if !out.diagnostics.is_empty() {
        diags.extend(out.diagnostics.iter().map(|d| pos.diagnostic(Severity::Error, d.code, d.message.clone(), d.range)));
        let mut out = failed(diags);
        out.diagnostics.extend(imported);
        return Staged { output: out, exports, globals };
    }
    diags.extend(imported);

    // Print or emit.
    if let Mode::Imported { .. } = mode {
        return Staged { output: CompileOutput { chunk: None, diagnostics: sorted(diags) }, exports, globals };
    }
    let chunk = match options.emit {
        Emit::Lua => nl_luagen::print_chunk(&out.chunk).into_bytes(),
        Emit::Bytecode => match nl_codegen::emit(&out.chunk, &nl_codegen::Options { strip: options.strip, level }) {
            Ok(dump) => dump,
            Err(e) => {
                // A function over LuaJIT's limits that the
                // lowering's budget did not catch (`vm-limit`, as there),
                // at the line of its definition.
                let code = if e.message.starts_with("internal error") { "internal-error" } else { "vm-limit" };
                diags.push(Diagnostic {
                    severity: Severity::Error,
                    code,
                    message: e.message,
                    file: file_label(chunk_name),
                    line: e.line.max(1),
                    column: 1,
                    end_line: e.line.max(1),
                    end_column: 1,
                });
                return Staged { output: failed(diags), exports, globals };
            }
        },
    };
    Staged { output: CompileOutput { chunk: Some(chunk), diagnostics: sorted(diags) }, exports, globals }
}

/// The declaration files in scope: the built-in prelude declarations of
/// `decl/`, then the loader's. Syntax errors in
/// a loader's file, and a name declared twice, are diagnostics of that
/// file.
fn load_declarations(loader: &dyn Loader) -> (Declarations, Vec<Diagnostic>) {
    let mut diags = Vec::new();
    let mut files = nl_sema::decl::prelude_files();
    for (name, bytes) in loader.declarations() {
        let text = match nl_syntax::source_from_bytes(&bytes) {
            Ok(t) => t.to_owned(),
            Err(d) => {
                diags.push(Diagnostic { severity: Severity::Error, code: "invalid-utf8", message: d.message, file: name.clone(), line: 1, column: 1, end_line: 1, end_column: 1 });
                continue;
            }
        };
        let parse = nl_syntax::parse_decl_file(&text);
        let pos = Positions::new(&text, name.clone());
        for d in parse.diagnostics() {
            let sev = if d.is_error() { Severity::Error } else { Severity::Warning };
            diags.push(pos.diagnostic(sev, "syntax", d.message.clone(), d.range));
        }
        if parse.has_errors() {
            continue;
        }
        files.push((name, parse.syntax()));
    }
    let (decls, sema) = Declarations::load(&files);
    for (file, d) in sema {
        let text = loader.declarations().into_iter().find(|(n, _)| *n == file).and_then(|(_, b)| String::from_utf8(b).ok()).unwrap_or_default();
        let pos = Positions::new(&text, file.clone());
        diags.push(pos.diagnostic(Severity::Error, d.code, d.message, d.range));
    }
    (decls, diags)
}
