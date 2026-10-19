//! The NL language server: diagnostics, hover, go-to-definition and
//! completion over the Language Server Protocol.
//!
//! The server is the `nl-lsp` binary (`main.rs`), which speaks LSP over
//! stdio through `lsp-server`; [`run`] drives any `Connection`, so tests
//! use an in-memory pair. Everything happens on one thread, in the order
//! the client sends it, which keeps the state simple: the open documents
//! ([`files::Files`], whose buffers take precedence over the disk when a
//! compile loads an imported file or a declaration file), one analysis
//! per open document, built on first use and dropped on any change
//! ([`analysis::Doc`]), and what each document published.
//!
//! - [`diagnostics`]: the real compile (`nl_capi::compile`). Any open,
//!   change or save compiles every open document again, since any of them
//!   may import the one that changed. An open document owns its URI: only
//!   its own compile publishes there. What a compile reports about a file
//!   that is not open (an imported file, a declaration file) is published
//!   under that file's URI as the union of what every open document
//!   reports about it, so that closing one document removes only its
//!   contribution.
//! - [`analysis`]: the tree, the resolver's `Resolution` and the analyzer's
//!   `TypeInfo` of one document, and the symbol under a position.
//! - [`hover`], [`definition`], [`completion`]: the three requests.
//! - [`position`]: byte offsets to LSP positions (UTF-16), paths to URIs.
//!
//! Nothing in the compiler panics on input, and neither does the server:
//! every request and notification is handled under `catch_unwind` as the
//! last line of defence; a panic is logged to stderr and answered with an
//! error response (a compile that panics publishes one `internal-error`
//! diagnostic), and the server goes on. The compiler recurses deeply on
//! deep input, so `main` runs [`run`] on a thread with [`STACK_SIZE`] of
//! stack, as the compile pipeline does.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing))]

pub mod analysis;
pub mod completion;
pub mod definition;
pub mod diagnostics;
pub mod files;
pub mod hover;
pub mod position;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::error::Error;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::rc::Rc;
use std::str::FromStr;

use lsp_server::{Connection, ErrorCode, Message, Notification, Request, Response};
use lsp_types::notification::Notification as _;
use lsp_types::request::Request as _;
use lsp_types::{
    CompletionOptions, CompletionResponse, Diagnostic, DiagnosticSeverity, GotoDefinitionResponse, HoverProviderCapability, InitializeParams, NumberOrString, OneOf, Position, PublishDiagnosticsParams,
    Range, SaveOptions, ServerCapabilities, TextDocumentSyncCapability, TextDocumentSyncKind, TextDocumentSyncOptions, TextDocumentSyncSaveOptions, Uri,
};

use crate::analysis::Doc;
use crate::files::Files;
use crate::position::uri_to_path;

/// The stack of the thread that runs the server: the compiler's own
/// (`nl_capi::driver::pipeline::STACK_SIZE`), reserved, not committed.
pub const STACK_SIZE: usize = 256 << 20;

/// The server's capabilities, as announced to the client.
pub fn capabilities() -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Options(TextDocumentSyncOptions {
            open_close: Some(true),
            change: Some(TextDocumentSyncKind::FULL),
            save: Some(TextDocumentSyncSaveOptions::SaveOptions(SaveOptions { include_text: Some(false) })),
            ..TextDocumentSyncOptions::default()
        })),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        completion_provider: Some(CompletionOptions { trigger_characters: Some(vec![".".to_string(), ":".to_string()]), ..CompletionOptions::default() }),
        ..ServerCapabilities::default()
    }
}

/// Runs the server on `conn` until the client asks it to exit. An error
/// when the client exits without `shutdown`, or the connection closes.
pub fn run(conn: Connection) -> Result<(), Box<dyn Error + Send + Sync>> {
    let (id, params) = conn.initialize_start()?;
    let init: InitializeParams = serde_json::from_value(params).unwrap_or_default();
    let result = serde_json::json!({
        "capabilities": capabilities(),
        "serverInfo": { "name": "nl-lsp", "version": env!("CARGO_PKG_VERSION") },
    });
    conn.initialize_finish(id, result)?;
    let mut server = Server::new(conn, &init);
    server.main_loop()
}

/// The server's state (module docs).
pub struct Server {
    conn: Connection,
    files: Files,
    /// The analyses of open documents, by URI.
    docs: HashMap<String, Rc<Doc>>,
    /// For each file that is not open: what each open document's compile
    /// reported about it, by the document's URI.
    foreign: BTreeMap<String, BTreeMap<String, Vec<Diagnostic>>>,
    /// The files each open document contributed to, by URI.
    contributed: HashMap<String, BTreeSet<String>>,
    /// What was last published for each file that is not open, so that an
    /// unchanged union is not published again.
    last_foreign: HashMap<String, Vec<Diagnostic>>,
}

/// An error answer to a request.
type RequestError = (ErrorCode, String);

impl Server {
    fn new(conn: Connection, init: &InitializeParams) -> Server {
        let mut files = Files::new();
        if let Some(folders) = &init.workspace_folders {
            for f in folders {
                if let Some(p) = uri_to_path(&f.uri) {
                    files.add_root(p);
                }
            }
        }
        #[allow(deprecated)]
        if let Some(root) = init.root_uri.as_ref().and_then(uri_to_path) {
            if !files.roots().contains(&root) {
                files.add_root(root);
            }
        }
        // `initializationOptions`: `searchPath` (directories of the
        // import search path) and `declarations` (files or directories of
        // `.d.nl`, as `nlc --decl`).
        if let Some(opts) = &init.initialization_options {
            for dir in string_list(opts, "searchPath") {
                files.add_search(PathBuf::from(dir));
            }
            for p in string_list(opts, "declarations") {
                files.add_decl(PathBuf::from(p));
            }
        }
        Server { conn, files, docs: HashMap::new(), foreign: BTreeMap::new(), contributed: HashMap::new(), last_foreign: HashMap::new() }
    }

    fn main_loop(&mut self) -> Result<(), Box<dyn Error + Send + Sync>> {
        let receiver = self.conn.receiver.clone();
        for msg in &receiver {
            match msg {
                Message::Request(req) => {
                    if self.conn.handle_shutdown(&req)? {
                        return Ok(());
                    }
                    let resp = self.handle_request(req);
                    self.conn.sender.send(Message::Response(resp))?;
                }
                Message::Notification(n) if n.method == lsp_types::notification::Exit::METHOD => {
                    return Err("the client sent `exit` without `shutdown`".into());
                }
                Message::Notification(n) => self.handle_notification(n),
                Message::Response(_) => {}
            }
        }
        Err("the connection closed without `shutdown`".into())
    }

    fn handle_request(&mut self, req: Request) -> Response {
        let id = req.id.clone();
        let method = req.method.clone();
        match catch_unwind(AssertUnwindSafe(|| self.dispatch(req))) {
            Ok(Ok(value)) => Response::new_ok(id, value),
            Ok(Err((code, message))) => Response::new_err(id, code as i32, message),
            Err(panic) => {
                let what = panic_message(&panic);
                eprintln!("nl-lsp: internal error in {method}: {what}");
                Response::new_err(id, ErrorCode::InternalError as i32, format!("nl-lsp: internal error in {method}: {what}"))
            }
        }
    }

    fn dispatch(&mut self, req: Request) -> Result<serde_json::Value, RequestError> {
        use lsp_types::request::{Completion, GotoDefinition, HoverRequest};
        match req.method.as_str() {
            HoverRequest::METHOD => {
                let p: lsp_types::HoverParams = parse(req.params)?;
                let pos = p.text_document_position_params;
                let Some((doc, offset)) = self.at(&pos.text_document.uri, pos.position)? else { return Ok(serde_json::Value::Null) };
                to_value(hover::hover(&self.files, &doc, offset))
            }
            GotoDefinition::METHOD => {
                let p: lsp_types::GotoDefinitionParams = parse(req.params)?;
                let pos = p.text_document_position_params;
                let Some((doc, offset)) = self.at(&pos.text_document.uri, pos.position)? else { return Ok(serde_json::Value::Null) };
                to_value(definition::definition(&self.files, &doc, offset).map(GotoDefinitionResponse::Scalar))
            }
            Completion::METHOD => {
                let p: lsp_types::CompletionParams = parse(req.params)?;
                let pos = p.text_document_position;
                let Some((doc, offset)) = self.at(&pos.text_document.uri, pos.position)? else { return Ok(serde_json::Value::Null) };
                to_value(Some(CompletionResponse::Array(completion::complete(&self.files, &doc, offset))))
            }
            other => Err((ErrorCode::MethodNotFound, format!("nl-lsp does not handle {other}"))),
        }
    }

    /// The analysis of the document at `uri` and the byte offset of
    /// `position` in it; `None` when the document is not open.
    fn at(&mut self, uri: &Uri, position: Position) -> Result<Option<(Rc<Doc>, nl_syntax::TextSize)>, RequestError> {
        let Some(doc) = self.doc(uri.as_str()) else { return Ok(None) };
        let offset = doc.lines.offset(&doc.text, position).ok_or_else(|| (ErrorCode::InvalidParams, "position outside the document".to_string()))?;
        Ok(Some((doc, offset)))
    }

    /// The analysis of an open document, built on first use.
    fn doc(&mut self, uri: &str) -> Option<Rc<Doc>> {
        if let Some(d) = self.docs.get(uri) {
            return Some(d.clone());
        }
        let open = self.files.doc(uri)?;
        let doc = Rc::new(analysis::analyze(&self.files, uri, open.path.as_deref(), &open.text));
        self.docs.insert(uri.to_string(), doc.clone());
        Some(doc)
    }

    fn handle_notification(&mut self, n: Notification) {
        let method = n.method.clone();
        if let Err(panic) = catch_unwind(AssertUnwindSafe(|| self.notified(n))) {
            eprintln!("nl-lsp: internal error in {method}: {}", panic_message(&panic));
        }
    }

    fn notified(&mut self, n: Notification) {
        use lsp_types::notification::{DidChangeTextDocument, DidCloseTextDocument, DidOpenTextDocument, DidSaveTextDocument};
        match n.method.as_str() {
            DidOpenTextDocument::METHOD => {
                let Ok(p) = serde_json::from_value::<lsp_types::DidOpenTextDocumentParams>(n.params) else { return };
                let uri = p.text_document.uri;
                let path = uri_to_path(&uri);
                self.files.open(uri.as_str(), path, p.text_document.text);
                // The document owns its URI from now on.
                self.foreign.remove(uri.as_str());
                self.last_foreign.remove(uri.as_str());
                for set in self.contributed.values_mut() {
                    set.remove(uri.as_str());
                }
                self.publish_all();
            }
            DidChangeTextDocument::METHOD => {
                let Ok(p) = serde_json::from_value::<lsp_types::DidChangeTextDocumentParams>(n.params) else { return };
                let uri = p.text_document.uri;
                // Full synchronization: the last change is the whole text.
                let Some(change) = p.content_changes.into_iter().last() else { return };
                let path = uri_to_path(&uri);
                self.files.open(uri.as_str(), path, change.text);
                self.publish_all();
            }
            DidSaveTextDocument::METHOD => self.publish_all(),
            DidCloseTextDocument::METHOD => {
                let Ok(p) = serde_json::from_value::<lsp_types::DidCloseTextDocumentParams>(n.params) else { return };
                let uri = p.text_document.uri;
                self.files.close(uri.as_str());
                self.send_diagnostics(uri.as_str(), Vec::new());
                let touched = self.withdraw(uri.as_str(), &BTreeSet::new());
                self.republish(&touched);
                // The buffer is gone: what the other documents import or
                // declare may differ now.
                self.publish_all();
            }
            _ => {}
        }
    }

    /// Compiles every open document and publishes (module docs). Any of
    /// them may import the one that changed, and their analyses are
    /// rebuilt on their next use.
    fn publish_all(&mut self) {
        self.docs.clear();
        for uri in self.files.open_uris() {
            self.publish(&uri);
        }
    }

    /// Compiles one document: its own diagnostics go under its URI, what
    /// it reports about files that are not open is recorded as its
    /// contribution to them, and every file whose contributions changed
    /// is published again.
    fn publish(&mut self, uri: &str) {
        let Some(open) = self.files.doc(uri) else { return };
        let (path, text) = (open.path.clone(), open.text.clone());
        let compiled = catch_unwind(AssertUnwindSafe(|| diagnostics::compile(&self.files, uri, path.as_deref(), &text)));
        let mut by_file = match compiled {
            Ok(by_file) => by_file,
            Err(panic) => {
                let what = panic_message(&panic);
                eprintln!("nl-lsp: internal error compiling {uri}: {what}");
                HashMap::from([(uri.to_string(), vec![internal_error(&what)])])
            }
        };
        let own = by_file.remove(uri).unwrap_or_default();
        self.send_diagnostics(uri, own);
        let mut now = BTreeSet::new();
        for (file, diags) in by_file {
            if self.files.is_open(&file) {
                continue;
            }
            self.foreign.entry(file.clone()).or_default().insert(uri.to_string(), diags);
            now.insert(file);
        }
        let mut touched = self.withdraw(uri, &now);
        touched.extend(now.iter().cloned());
        self.contributed.insert(uri.to_string(), now);
        self.republish(&touched);
    }

    /// Removes the contributions of `owner` to the files it no longer
    /// reports about (those not in `keep`); the files touched.
    fn withdraw(&mut self, owner: &str, keep: &BTreeSet<String>) -> BTreeSet<String> {
        let mut touched = BTreeSet::new();
        for file in self.contributed.remove(owner).unwrap_or_default() {
            if keep.contains(&file) {
                continue;
            }
            if let Some(owners) = self.foreign.get_mut(&file) {
                owners.remove(owner);
                if owners.is_empty() {
                    self.foreign.remove(&file);
                }
            }
            touched.insert(file);
        }
        touched
    }

    /// Publishes the union of the contributions to each file, when it
    /// differs from what was last published there.
    fn republish(&mut self, files: &BTreeSet<String>) {
        for file in files {
            let all: Vec<Diagnostic> = self.foreign.get(file).map(|owners| owners.values().flatten().cloned().collect()).unwrap_or_default();
            let all = diagnostics::dedup(all);
            if self.last_foreign.get(file).map_or(all.is_empty(), |last| *last == all) {
                continue;
            }
            self.send_diagnostics(file, all.clone());
            if all.is_empty() {
                self.last_foreign.remove(file);
            } else {
                self.last_foreign.insert(file.clone(), all);
            }
        }
    }

    fn send_diagnostics(&self, uri: &str, diagnostics: Vec<Diagnostic>) {
        let Ok(uri) = Uri::from_str(uri) else { return };
        let params = PublishDiagnosticsParams { uri, diagnostics, version: None };
        let n = Notification::new(lsp_types::notification::PublishDiagnostics::METHOD.to_string(), params);
        let _ = self.conn.sender.send(Message::Notification(n));
    }
}

/// The one diagnostic of a document whose compile panicked.
fn internal_error(what: &str) -> Diagnostic {
    Diagnostic {
        range: Range::new(Position::new(0, 0), Position::new(0, 0)),
        severity: Some(DiagnosticSeverity::ERROR),
        code: Some(NumberOrString::String("internal-error".to_string())),
        source: Some("nl".to_string()),
        message: format!("internal error: the compiler panicked ({what})"),
        ..Diagnostic::default()
    }
}

fn parse<T: serde::de::DeserializeOwned>(v: serde_json::Value) -> Result<T, RequestError> {
    serde_json::from_value(v).map_err(|e| (ErrorCode::InvalidParams, format!("invalid parameters: {e}")))
}

fn to_value<T: serde::Serialize>(v: T) -> Result<serde_json::Value, RequestError> {
    serde_json::to_value(v).map_err(|e| (ErrorCode::InternalError, format!("cannot encode the result: {e}")))
}

fn string_list(opts: &serde_json::Value, key: &str) -> Vec<String> {
    opts.get(key).and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_owned)).collect()).unwrap_or_default()
}

fn panic_message(panic: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = panic.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = panic.downcast_ref::<String>() {
        s.clone()
    } else {
        "panic".to_string()
    }
}
