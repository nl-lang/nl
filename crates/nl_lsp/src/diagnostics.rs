//! Diagnostics: a document is compiled through
//! `nl_capi::compile` with the analyzer on, the chunk is discarded, and
//! every diagnostic becomes an LSP diagnostic under the URI of its own
//! file: the compile also reports the errors of the files the document
//! imports directly. Who publishes what is the
//! server's business (`lib.rs`): this module only converts.
//!
//! Severity follows `nlc check` (crates/nlc/src/run.rs): errors are
//! errors, warnings and lints are warnings. The compiler's positions are
//! 1-based lines and character columns; `position.rs` turns them into
//! LSP positions with the text of the file in question. Nothing here
//! touches the file system beyond reading.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use lsp_types::{Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range};
use nl_capi::{CompileOptions, Emit, Severity};

use crate::analysis::{decl_file_path, is_decl_file};
use crate::files::{DocLoader, Files};
use crate::position::{LineMap, path_to_uri};

/// Compiles the document `uri` (the file `path`, when it is one) with
/// text `text` and gives its diagnostics by URI, without duplicates. The
/// document itself is always present, with an empty list when it is
/// clean, so that stale diagnostics are cleared.
pub fn compile(files: &Files, uri: &str, path: Option<&Path>, text: &str) -> HashMap<String, Vec<Diagnostic>> {
    if is_decl_file(uri) {
        return HashMap::from([(uri.to_string(), dedup(decl_file(uri, text)))]);
    }
    let dir = path.and_then(Path::parent).map(Path::to_path_buf).unwrap_or_else(|| files.cwd().to_path_buf());
    let loader = DocLoader { files, dir: Some(dir) };
    let options = CompileOptions { analyze: true, emit: Emit::Lua, ..CompileOptions::default() };
    // The name the compiler reports the document's diagnostics under.
    let doc_name = path.map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|| uri.to_string());
    let out = nl_capi::compile(text.as_bytes(), &format!("@{doc_name}"), &options, &loader);
    let mut by_file: HashMap<String, Vec<Diagnostic>> = HashMap::new();
    by_file.insert(uri.to_string(), Vec::new());
    let mut texts: HashMap<String, Option<(String, LineMap)>> = HashMap::new();
    for d in out.diagnostics {
        let file = if d.file == doc_name {
            uri.to_string()
        } else {
            let p = decl_file_path(files, &d.file, false).unwrap_or_else(|| PathBuf::from(&d.file));
            match path_to_uri(&p) {
                Some(u) => u.as_str().to_string(),
                None => continue,
            }
        };
        let entry = texts.entry(d.file.clone()).or_insert_with(|| text_of(files, &d.file, &doc_name, text));
        let range = match entry {
            Some((t, lines)) => Range { start: lines.from_line_col(t, d.line, d.column), end: lines.from_line_col(t, d.end_line, d.end_column) },
            None => Range {
                start: Position::new(d.line.saturating_sub(1), d.column.saturating_sub(1)),
                end: Position::new(d.end_line.saturating_sub(1), d.end_column.saturating_sub(1)),
            },
        };
        let severity = match d.severity {
            Severity::Error => DiagnosticSeverity::ERROR,
            Severity::Warning | Severity::Lint => DiagnosticSeverity::WARNING,
        };
        by_file.entry(file).or_default().push(Diagnostic {
            range,
            severity: Some(severity),
            code: Some(NumberOrString::String(d.code.to_string())),
            source: Some("nl".to_string()),
            message: d.message,
            ..Diagnostic::default()
        });
    }
    by_file.into_iter().map(|(f, diags)| (f, dedup(diags))).collect()
}

/// Keeps the first of the diagnostics with one range, code and message
/// (the compile reports a declaration file's problems once per use).
pub fn dedup(diags: Vec<Diagnostic>) -> Vec<Diagnostic> {
    let mut seen = HashSet::new();
    diags
        .into_iter()
        .filter(|d| {
            let code = match &d.code {
                Some(NumberOrString::String(s)) => s.clone(),
                Some(NumberOrString::Number(n)) => n.to_string(),
                None => String::new(),
            };
            seen.insert((d.range.start.line, d.range.start.character, d.range.end.line, d.range.end.character, code, d.message.clone()))
        })
        .collect()
}

/// The diagnostics of a declaration file opened in the editor: its
/// syntax errors, and a name it declares twice.
fn decl_file(name: &str, text: &str) -> Vec<Diagnostic> {
    let parse = nl_syntax::parse_decl_file(text);
    let lines = LineMap::new(text);
    let mut out: Vec<Diagnostic> = parse
        .diagnostics()
        .iter()
        .map(|d| Diagnostic {
            range: lines.range(text, d.range),
            severity: Some(if d.is_error() { DiagnosticSeverity::ERROR } else { DiagnosticSeverity::WARNING }),
            code: Some(NumberOrString::String("syntax".to_string())),
            source: Some("nl".to_string()),
            message: d.message.clone(),
            ..Diagnostic::default()
        })
        .collect();
    if !parse.has_errors() {
        let (_, sema) = nl_sema::Declarations::load(&[(name.to_string(), parse.syntax())]);
        out.extend(sema.into_iter().map(|(_, d)| Diagnostic {
            range: lines.range(text, d.range),
            severity: Some(DiagnosticSeverity::ERROR),
            code: Some(NumberOrString::String(d.code.to_string())),
            source: Some("nl".to_string()),
            message: d.message,
            ..Diagnostic::default()
        }));
    }
    out
}

/// The text a diagnostic's positions refer to: the document, a built-in
/// declaration file, or a file read through [`Files`] (an open buffer
/// first).
fn text_of(files: &Files, name: &str, doc_name: &str, doc_text: &str) -> Option<(String, LineMap)> {
    let text = if name == doc_name {
        doc_text.to_owned()
    } else if let Some((_, src)) = nl_sema::decl::PRELUDE_SOURCES.iter().find(|(n, _)| *n == name) {
        (*src).to_owned()
    } else {
        files.read_text(Path::new(name))?
    };
    let lines = LineMap::new(&text);
    Some((text, lines))
}
