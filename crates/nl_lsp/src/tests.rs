//! The server through an in-memory connection (`Connection::memory()`):
//! a client opens documents, sends requests and checks the answers and
//! the published diagnostics. No process is spawned.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::thread::JoinHandle;
use std::time::Duration;

use lsp_server::{Connection, Message, Notification, Request, RequestId, ResponseError};
use lsp_types::notification::Notification as _;
use lsp_types::request::Request as _;
use lsp_types::{notification, request, *};

use crate::analysis::is_word;
use crate::position::{LineMap, path_to_uri, uri_to_path};

type ServerResult = Result<(), Box<dyn Error + Send + Sync>>;

struct Client {
    conn: Connection,
    next: i32,
    pending: Vec<Notification>,
    thread: Option<JoinHandle<ServerResult>>,
}

const WAIT: Duration = Duration::from_secs(120);

impl Client {
    fn start(root: Option<&Path>) -> Client {
        let (server, client) = Connection::memory();
        let thread = std::thread::Builder::new().stack_size(crate::STACK_SIZE).spawn(move || crate::run(server)).unwrap();
        let mut c = Client { conn: client, next: 1, pending: Vec::new(), thread: Some(thread) };
        #[allow(deprecated)]
        let params = InitializeParams { root_uri: root.and_then(path_to_uri), ..InitializeParams::default() };
        let result = c.request::<request::Initialize>(params).unwrap();
        assert_eq!(result.capabilities.hover_provider, Some(HoverProviderCapability::Simple(true)));
        c.notify::<notification::Initialized>(InitializedParams {});
        c
    }

    fn notify<N: notification::Notification>(&self, params: N::Params) {
        self.conn.sender.send(Message::Notification(Notification::new(N::METHOD.to_string(), params))).unwrap();
    }

    fn raw_request(&mut self, method: &str, params: serde_json::Value) -> Result<serde_json::Value, ResponseError> {
        let id = RequestId::from(self.next);
        self.next += 1;
        self.conn.sender.send(Message::Request(Request { id: id.clone(), method: method.to_string(), params })).unwrap();
        loop {
            match self.conn.receiver.recv_timeout(WAIT).expect("the server answers") {
                Message::Response(r) if r.id == id => return r.response_result,
                Message::Response(r) => panic!("unexpected response {r:?}"),
                Message::Notification(n) => self.pending.push(n),
                Message::Request(r) => panic!("unexpected request {r:?}"),
            }
        }
    }

    fn request<R: request::Request>(&mut self, params: R::Params) -> Result<R::Result, ResponseError> {
        let value = self.raw_request(R::METHOD, serde_json::to_value(params).unwrap())?;
        Ok(serde_json::from_value(value).unwrap())
    }

    fn open(&mut self, path: &Path, text: &str) -> Uri {
        let uri = path_to_uri(path).unwrap();
        self.open_uri(uri.clone(), text);
        uri
    }

    fn open_uri(&mut self, uri: Uri, text: &str) {
        self.notify::<notification::DidOpenTextDocument>(DidOpenTextDocumentParams { text_document: TextDocumentItem::new(uri, "nl".to_string(), 1, text.to_string()) });
    }

    fn close(&self, uri: &Uri) {
        self.notify::<notification::DidCloseTextDocument>(DidCloseTextDocumentParams { text_document: TextDocumentIdentifier::new(uri.clone()) });
    }

    /// True when a `publishDiagnostics` for `uri` is waiting unread.
    fn pending_for(&self, uri: &Uri) -> bool {
        self.pending.iter().any(|n| n.method == notification::PublishDiagnostics::METHOD && serde_json::from_value::<PublishDiagnosticsParams>(n.params.clone()).is_ok_and(|p| &p.uri == uri))
    }

    fn change(&self, uri: &Uri, text: &str) {
        self.notify::<notification::DidChangeTextDocument>(DidChangeTextDocumentParams {
            text_document: VersionedTextDocumentIdentifier::new(uri.clone(), 2),
            content_changes: vec![TextDocumentContentChangeEvent { range: None, range_length: None, text: text.to_string() }],
        });
    }

    fn save(&self, uri: &Uri) {
        self.notify::<notification::DidSaveTextDocument>(DidSaveTextDocumentParams { text_document: TextDocumentIdentifier::new(uri.clone()), text: None });
    }

    /// The next `publishDiagnostics` for `uri`.
    fn diagnostics(&mut self, uri: &Uri) -> Vec<Diagnostic> {
        loop {
            let found = self.pending.iter().position(|n| {
                n.method == notification::PublishDiagnostics::METHOD && serde_json::from_value::<PublishDiagnosticsParams>(n.params.clone()).is_ok_and(|p| &p.uri == uri)
            });
            if let Some(i) = found {
                let n = self.pending.remove(i);
                return serde_json::from_value::<PublishDiagnosticsParams>(n.params).unwrap().diagnostics;
            }
            match self.conn.receiver.recv_timeout(WAIT).expect("diagnostics are published") {
                Message::Notification(n) => self.pending.push(n),
                other => panic!("unexpected message {other:?}"),
            }
        }
    }

    fn position_params(uri: &Uri, pos: Position) -> TextDocumentPositionParams {
        TextDocumentPositionParams { text_document: TextDocumentIdentifier::new(uri.clone()), position: pos }
    }

    fn hover(&mut self, uri: &Uri, pos: Position) -> Result<Option<Hover>, ResponseError> {
        self.request::<request::HoverRequest>(HoverParams { text_document_position_params: Self::position_params(uri, pos), work_done_progress_params: WorkDoneProgressParams::default() })
    }

    fn definition(&mut self, uri: &Uri, pos: Position) -> Result<Option<GotoDefinitionResponse>, ResponseError> {
        self.request::<request::GotoDefinition>(GotoDefinitionParams {
            text_document_position_params: Self::position_params(uri, pos),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
        })
    }

    fn complete(&mut self, uri: &Uri, pos: Position) -> Result<Option<CompletionResponse>, ResponseError> {
        self.request::<request::Completion>(CompletionParams {
            text_document_position: Self::position_params(uri, pos),
            work_done_progress_params: WorkDoneProgressParams::default(),
            partial_result_params: PartialResultParams::default(),
            context: None,
        })
    }

    fn shutdown(mut self) {
        self.request::<request::Shutdown>(()).unwrap();
        self.notify::<notification::Exit>(());
        self.thread.take().unwrap().join().unwrap().unwrap();
    }
}

/// The hover text, flattened.
fn hover_text(h: Option<Hover>) -> String {
    match h.expect("a hover").contents {
        HoverContents::Markup(m) => m.value,
        other => panic!("unexpected hover contents {other:?}"),
    }
}

fn location(d: Option<GotoDefinitionResponse>) -> Location {
    match d.expect("a definition") {
        GotoDefinitionResponse::Scalar(l) => l,
        other => panic!("unexpected definition {other:?}"),
    }
}

fn labels(c: Option<CompletionResponse>) -> Vec<(String, Option<String>)> {
    match c.expect("a completion list") {
        CompletionResponse::Array(items) => items.into_iter().map(|i| (i.label, i.detail)).collect(),
        other => panic!("unexpected completion {other:?}"),
    }
}

/// The position of the `nth` occurrence (from 0) of `needle` in `text`.
fn at(text: &str, needle: &str, nth: usize) -> Position {
    let mut offset = 0;
    let mut from = 0;
    for _ in 0..=nth {
        let i = text[from..].find(needle).unwrap_or_else(|| panic!("{needle:?} occurrence {nth} not in text"));
        offset = from + i;
        from = offset + needle.len();
    }
    LineMap::new(text).position(text, nl_syntax::TextSize::new(offset as u32))
}

/// The range of the `nth` occurrence of `needle`.
fn range_of(text: &str, needle: &str, nth: usize) -> Range {
    let start = at(text, needle, nth);
    Range { start, end: Position::new(start.line, start.character + needle.encode_utf16().count() as u32) }
}

/// The range of `word` inside the first occurrence of `context`.
fn word(text: &str, context: &str, word: &str) -> Range {
    let ctx = text.find(context).unwrap_or_else(|| panic!("{context:?} not in text"));
    let inner = context.find(word).unwrap_or_else(|| panic!("{word:?} not in {context:?}"));
    let offset = ctx + inner;
    let lines = LineMap::new(text);
    let start = lines.position(text, nl_syntax::TextSize::new(offset as u32));
    Range { start, end: Position::new(start.line, start.character + word.encode_utf16().count() as u32) }
}

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nl_lsp_test_{}_{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn diagnostics_on_open_change_save_and_through_imports() {
    let dir = temp_dir("diagnostics");
    std::fs::write(dir.join("lib.nl"), "pub fn helper\n  1\nend\n").unwrap();
    std::fs::write(dir.join("bad.nl"), "pub fn f\n  1\nend\nprint nope\n").unwrap();
    let mut c = Client::start(Some(&dir));

    let text = "import { helper } from 'lib'\nprint helper\nprint unknown_name\n";
    let main = c.open(&dir.join("main.nl"), text);
    let diags = c.diagnostics(&main);
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].severity, Some(DiagnosticSeverity::ERROR));
    assert_eq!(diags[0].code, Some(NumberOrString::String("undeclared-name".to_string())));
    assert_eq!(diags[0].range, range_of(text, "unknown_name", 0));

    c.change(&main, "import { helper } from 'lib'\nprint helper\n");
    assert!(c.diagnostics(&main).is_empty());

    // An error in a directly imported file is published under that file.
    let bad_uri = path_to_uri(&dir.join("bad.nl")).unwrap();
    let main2 = c.open(&dir.join("main2.nl"), "import { f } from 'bad'\nprint f\n");
    let own = c.diagnostics(&main2);
    assert!(own.iter().any(|d| d.code == Some(NumberOrString::String("import-failed".to_string()))), "{own:?}");
    let theirs = c.diagnostics(&bad_uri);
    assert_eq!(theirs.len(), 1, "{theirs:?}");
    assert_eq!(theirs[0].range.start, Position::new(3, 6));
    assert!(theirs[0].message.contains("nope"));

    // The unsaved buffer of bad.nl takes precedence over the disk.
    c.open(&dir.join("bad.nl"), "pub fn f\n  1\nend\n");
    assert!(c.diagnostics(&bad_uri).is_empty());
    c.change(&main2, "import { f } from 'bad'\nprint f\n");
    assert!(c.diagnostics(&main2).is_empty());
    assert!(c.diagnostics(&bad_uri).is_empty(), "the stale diagnostics of bad.nl are cleared");

    // A save compiles every open document again.
    c.save(&bad_uri);
    assert!(c.diagnostics(&main).is_empty());
    assert!(c.diagnostics(&main2).is_empty());
    assert!(c.diagnostics(&bad_uri).is_empty());

    // A declaration file opened in the editor is parsed as one: its
    // syntax errors and duplicate names are reported, and requests on it
    // answer without an error.
    let decl_text = "declare module A = :a\n  fn f(x: Number): Number\nend\ndeclare module A = :b\nend\ndeclare fn g(\n";
    let decl = c.open(&dir.join("api.d.nl"), decl_text);
    let diags = c.diagnostics(&decl);
    assert!(diags.iter().any(|d| d.code == Some(NumberOrString::String("syntax".to_string()))), "{diags:?}");
    assert!(c.hover(&decl, at(decl_text, "f(x", 0)).unwrap().is_none());
    assert!(c.definition(&decl, at(decl_text, "Number", 0)).unwrap().is_none());
    c.change(&decl, "declare module A = :a\nend\ndeclare module A = :b\nend\n");
    let diags = c.diagnostics(&decl);
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].code, Some(NumberOrString::String("decl-duplicate".to_string())));
    c.shutdown();
}

/// Diagnostics of a line with non-ASCII text have UTF-16 columns.
#[test]
fn diagnostic_columns_are_utf16() {
    let dir = temp_dir("utf16");
    let mut c = Client::start(Some(&dir));
    let text = "let s = \"é😀\"\nprint s, nope\n";
    let uri = c.open(&dir.join("u.nl"), text);
    let diags = c.diagnostics(&uri);
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].range, range_of(text, "nope", 0));
    let text = "print \"é😀\", nope\n";
    c.change(&uri, text);
    let diags = c.diagnostics(&uri);
    assert_eq!(diags[0].range, range_of(text, "nope", 0));
    assert_eq!(diags[0].range.start.character, 13);
    c.shutdown();
}

const ONE_FILE: &str = "## Adds one.
fn inc(n: Number): Number
  n + 1
end
let x = inc(2)
## A point.
struct Pt
  ## Across.
  x: Number
  y: Number
end
fn Pt.length(self): Number
  @x + @y
end
let p = Pt { x: 1, y: 2 }
print p.length
print p.x
enum Mode
  Easy
  Hard
end
let m = Mode.Easy
print x
fn Number.dbl(self)
  self * 2
end
print 3.dbl
print p:length()
";

#[test]
fn definitions_in_one_file() {
    let dir = temp_dir("definitions");
    let mut c = Client::start(Some(&dir));
    let uri = c.open(&dir.join("one.nl"), ONE_FILE);
    let def = |c: &mut Client, needle: &str, nth: usize| location(c.definition(&uri, at(ONE_FILE, needle, nth)).unwrap());

    let d = def(&mut c, "inc(2)", 0);
    assert_eq!(d.uri, uri);
    assert_eq!(d.range, word(ONE_FILE, "fn inc", "inc"));
    assert_eq!(def(&mut c, "x\nfn Number", 0).range, word(ONE_FILE, "let x", "x"));
    // A method through a member, a field through a member, a variant.
    assert_eq!(def(&mut c, "length\n", 0).range, word(ONE_FILE, "length(self)", "length"));
    assert_eq!(def(&mut c, "p.x", 0).range, word(ONE_FILE, "let p", "p"), "the receiver");
    let px = at(ONE_FILE, "p.x", 0);
    let field = location(c.definition(&uri, Position { character: px.character + 2, ..px }).unwrap());
    assert_eq!(field.range, word(ONE_FILE, "x: Number", "x"));
    assert_eq!(def(&mut c, "Easy\n  Hard", 0).range, word(ONE_FILE, "Easy\n  Hard", "Easy"));
    let easy = at(ONE_FILE, "Mode.Easy", 0);
    let variant = location(c.definition(&uri, Position { character: easy.character + 5, ..easy }).unwrap());
    assert_eq!(variant.range, word(ONE_FILE, "Easy\n  Hard", "Easy"));
    // `self` in a method is the hidden parameter: its range is the method.
    let s = location(c.definition(&uri, at(ONE_FILE, "self * 2", 0)).unwrap());
    assert_eq!(s.range.start, at(ONE_FILE, "fn Number.dbl", 0));
    // A primitive method through a member goes to its definition.
    let dbl = at(ONE_FILE, "3.dbl", 0);
    let d = location(c.definition(&uri, Position { character: dbl.character + 2, ..dbl }).unwrap());
    assert_eq!(d.range, word(ONE_FILE, "dbl(self)", "dbl"));
    // A method call `x:m()` goes to the method too.
    let call = at(ONE_FILE, "p:length()", 0);
    let d = location(c.definition(&uri, Position { character: call.character + 2, ..call }).unwrap());
    assert_eq!(d.range, word(ONE_FILE, "length(self)", "length"));
    let h = hover_text(c.hover(&uri, Position { character: call.character + 2, ..call }).unwrap());
    assert!(h.contains("fn Pt.length(): Number"), "{h}");
    // Nothing under a keyword or a literal.
    assert!(c.definition(&uri, at(ONE_FILE, "struct", 0)).unwrap().is_none());
    assert!(c.definition(&uri, at(ONE_FILE, "2)", 0)).unwrap().is_none());
    c.shutdown();
}

#[test]
fn hover_shows_kind_type_and_doc_comment() {
    let dir = temp_dir("hover");
    let mut c = Client::start(Some(&dir));
    let uri = c.open(&dir.join("one.nl"), ONE_FILE);
    let hov = |c: &mut Client, needle: &str, nth: usize| hover_text(c.hover(&uri, at(ONE_FILE, needle, nth)).unwrap());

    let h = hov(&mut c, "inc(2)", 0);
    assert!(h.contains("fn inc(n: Number): Number"), "{h}");
    assert!(h.contains("Adds one."), "{h}");
    let h = hov(&mut c, "x = inc", 0);
    assert_eq!(h, "```nl\nlet x: Number\n```");
    let h = hov(&mut c, "n + 1", 0);
    assert_eq!(h, "```nl\nparam n: Number\n```");
    let h = hov(&mut c, "Pt {", 0);
    assert!(h.contains("struct Pt") && h.contains("A point."), "{h}");
    let px = at(ONE_FILE, "p.x", 0);
    let h = hover_text(c.hover(&uri, Position { character: px.character + 2, ..px }).unwrap());
    assert!(h.contains("Pt.x: Number") && h.contains("Across."), "{h}");
    let h = hov(&mut c, "length\n", 0);
    assert!(h.contains("fn Pt.length(): Number"), "{h}");
    let h = hov(&mut c, "Mode.Easy", 0);
    assert!(h.contains("enum Mode"), "{h}");
    let h = hov(&mut c, "p.length", 0);
    assert_eq!(h, "```nl\nlet p: Pt\n```");
    // The hover range is the token.
    let r = c.hover(&uri, at(ONE_FILE, "inc(2)", 0)).unwrap().unwrap().range.unwrap();
    assert_eq!(r, range_of(ONE_FILE, "inc", 1));
    // A member after `?.` hovers as after `.`.
    let text = format!("{ONE_FILE}let q: Pt? = p\nprint q?.x\n");
    c.change(&uri, &text);
    let qx = at(&text, "q?.x", 0);
    let h = hover_text(c.hover(&uri, Position { character: qx.character + 3, ..qx }).unwrap());
    assert!(h.contains("Pt.x: Number") && h.contains("Across."), "{h}");
    c.shutdown();
}

const LAMBDAS: &str = "let scale = 3\nlet area = |w: Number, h: Number|: Number -> w * h * scale\nlet inc = x -> x + 1\nlet both = || -> area(1, 2) + inc(3)\nprint both\n";

/// The parameters of a short function are block parameters:
/// hover shows them as parameters, and a use goes to its declaration.
#[test]
fn short_function_parameters() {
    let dir = temp_dir("lambda");
    let mut c = Client::start(Some(&dir));
    let uri = c.open(&dir.join("one.nl"), LAMBDAS);
    assert!(c.diagnostics(&uri).is_empty());
    let def = |c: &mut Client, needle: &str| location(c.definition(&uri, at(LAMBDAS, needle, 0)).unwrap());
    assert_eq!(def(&mut c, "w * h").range, word(LAMBDAS, "|w:", "w"));
    assert_eq!(def(&mut c, "h * scale").range, word(LAMBDAS, "h: Number|", "h"));
    assert_eq!(def(&mut c, "scale\n").range, word(LAMBDAS, "let scale", "scale"));
    assert_eq!(def(&mut c, "x + 1").range, word(LAMBDAS, "x ->", "x"));
    assert_eq!(def(&mut c, "area(1").range, word(LAMBDAS, "let area", "area"));
    let h = hover_text(c.hover(&uri, at(LAMBDAS, "w * h", 0)).unwrap());
    assert_eq!(h, "```nl\nparam w: Number\n```");
    let h = hover_text(c.hover(&uri, at(LAMBDAS, "x + 1", 0)).unwrap());
    assert!(h.starts_with("```nl\nparam x"), "{h}");
    c.shutdown();
}

const LIB: &str = "## The helper.\npub fn helper(a: Number): Number\n  a\nend\npub const limit = 3\n";
const DECL: &str = "## The game API.\ndeclare module Foo = :foo\n  ## Largest of two.\n  fn return_largest(a: Number, b: Number): Number as 'ReturnLargest'\n  const limit: Number\n  module Sub\n    fn deep(): Number\n  end\nend\ndeclare fn local_player(): Number = :LocalPlayer\n";
const MAIN: &str = "import { helper, limit } from 'lib'\nimport from 'lib' as Lib\nprint helper(limit)\nprint Foo.return_largest(1, 2)\nprint Foo.limit\nprint local_player\nprint Number.floor(1.5)\nprint Lib.helper(1)\nprint Foo.Sub.deep()\nprint \"a\".upcase\n";

#[test]
fn definitions_across_files_and_declaration_files() {
    let dir = temp_dir("cross");
    std::fs::write(dir.join("lib.nl"), LIB).unwrap();
    std::fs::write(dir.join("game.d.nl"), DECL).unwrap();
    let mut c = Client::start(Some(&dir));
    let uri = c.open(&dir.join("main.nl"), MAIN);
    assert!(c.diagnostics(&uri).is_empty());
    let lib_uri = path_to_uri(&dir.join("lib.nl")).unwrap();
    let decl_uri = path_to_uri(&dir.join("game.d.nl")).unwrap();
    let def = |c: &mut Client, pos: Position| location(c.definition(&uri, pos).unwrap());
    let after = |p: Position, n: u32| Position { character: p.character + n, ..p };

    // An imported name goes to its export.
    let d = def(&mut c, at(MAIN, "helper(limit)", 0));
    assert_eq!((d.uri.clone(), d.range), (lib_uri.clone(), word(LIB, "pub fn helper", "helper")));
    let d = def(&mut c, at(MAIN, "limit)", 0));
    assert_eq!((d.uri.clone(), d.range), (lib_uri.clone(), word(LIB, "pub const limit", "limit")));
    // The name at the import itself goes there too.
    let d = def(&mut c, at(MAIN, "helper", 0));
    assert_eq!(d.uri, lib_uri);
    // The export table: the start of the file; a member of it: the export.
    let d = def(&mut c, at(MAIN, "Lib.helper", 0));
    assert_eq!((d.uri.clone(), d.range), (lib_uri.clone(), Range::default()));
    let d = def(&mut c, after(at(MAIN, "Lib.helper", 0), 4));
    assert_eq!((d.uri.clone(), d.range), (lib_uri.clone(), word(LIB, "pub fn helper", "helper")));
    let h = hover_text(c.hover(&uri, after(at(MAIN, "Lib.helper", 0), 4)).unwrap());
    assert!(h.contains("fn helper(a: Number): Number") && h.contains("The helper."), "{h}");
    // A declared module, its members and a nested module's member.
    let d = def(&mut c, at(MAIN, "Foo.return_largest", 0));
    assert_eq!((d.uri.clone(), d.range), (decl_uri.clone(), word(DECL, "declare module Foo", "Foo")));
    let d = def(&mut c, after(at(MAIN, "Foo.return_largest", 0), 4));
    assert_eq!((d.uri.clone(), d.range), (decl_uri.clone(), word(DECL, "fn return_largest", "return_largest")));
    let d = def(&mut c, after(at(MAIN, "Foo.limit", 0), 4));
    assert_eq!((d.uri.clone(), d.range), (decl_uri.clone(), word(DECL, "const limit", "limit")));
    let d = def(&mut c, after(at(MAIN, "Foo.Sub.deep", 0), 8));
    assert_eq!((d.uri.clone(), d.range), (decl_uri.clone(), word(DECL, "fn deep", "deep")));
    let d = def(&mut c, at(MAIN, "local_player", 0));
    assert_eq!((d.uri.clone(), d.range), (decl_uri.clone(), word(DECL, "declare fn local_player", "local_player")));
    // A prelude library's function goes to the built-in declaration file.
    let d = def(&mut c, after(at(MAIN, "Number.floor", 0), 7));
    let path = uri_to_path(&d.uri).unwrap();
    assert!(path.ends_with("decl/lua.d.nl"), "{path:?}");
    let lua = std::fs::read_to_string(&path).unwrap();
    assert_eq!(d.range, word(&lua, "fn floor(", "floor"));
    let d = def(&mut c, at(MAIN, "Number.floor", 0));
    assert_eq!((uri_to_path(&d.uri).unwrap(), d.range), (path, word(&lua, "declare module Number", "Number")));
    // A string method goes to NL's own declaration file.
    let d = def(&mut c, after(at(MAIN, "\"a\".upcase", 0), 4));
    let path = uri_to_path(&d.uri).unwrap();
    assert!(path.ends_with("decl/nl.d.nl"), "{path:?}");

    // Hover on declared names shows the declaration and its doc comment.
    let h = hover_text(c.hover(&uri, after(at(MAIN, "Foo.return_largest", 0), 4)).unwrap());
    assert!(h.contains("fn Foo.return_largest(a: Number, b: Number): Number") && h.contains("Largest of two."), "{h}");
    let h = hover_text(c.hover(&uri, at(MAIN, "Foo.limit", 0)).unwrap());
    assert!(h.contains("declare module Foo = :foo") && h.contains("The game API."), "{h}");
    let h = hover_text(c.hover(&uri, at(MAIN, "local_player", 0)).unwrap());
    assert!(h.contains("declare fn local_player(): Number = :LocalPlayer"), "{h}");
    let h = hover_text(c.hover(&uri, at(MAIN, "helper(limit)", 0)).unwrap());
    assert!(h.contains("import helper") && h.contains("a function imported from 'lib'"), "{h}");
    let h = hover_text(c.hover(&uri, at(MAIN, "Number.floor", 0)).unwrap());
    assert!(h.contains("declare module Number = :math"), "{h}");
    c.shutdown();
}

#[test]
fn completion_of_members_and_names_in_scope() {
    let dir = temp_dir("completion");
    std::fs::write(dir.join("game.d.nl"), DECL).unwrap();
    std::fs::write(dir.join("lib.nl"), LIB).unwrap();
    let mut c = Client::start(Some(&dir));
    // The declarations every scenario starts from; each scenario is the
    // file with one line being typed, as an editor sends it.
    let head = "import from 'lib' as Lib\nlet p = Pt { x: 1 }\n## A point.\nstruct Pt\n  x: Number\nend\nfn Pt.len2(self): Number\n  1\nend\nlet s = \"a\"\nenum Mode\n  Easy\nend\n";
    let uri = c.open(&dir.join("c.nl"), head);
    let complete_at_end = |c: &mut Client, line: &str| {
        let text = format!("{head}{line}\n");
        c.change(&uri, &text);
        let p = at(&text, line, 0);
        labels(c.complete(&uri, Position { character: p.character + line.encode_utf16().count() as u32, ..p }).unwrap())
    };
    let items = complete_at_end(&mut c, "print p.");
    assert!(items.contains(&("x".to_string(), Some("Number".to_string()))), "{items:?}");
    assert!(items.contains(&("len2".to_string(), Some("Function(): Number".to_string()))), "{items:?}");

    let items = complete_at_end(&mut c, "print Foo.");
    let names: Vec<&str> = items.iter().map(|(l, _)| l.as_str()).collect();
    assert_eq!(names, ["Sub", "limit", "return_largest"], "{items:?}");
    assert_eq!(items[2].1.as_deref(), Some("Function(a: Number, b: Number): Number"));

    // A partial name after the dot: the whole list, the client filters.
    let items = complete_at_end(&mut c, "print s.up");
    assert!(items.iter().any(|(l, _)| l == "upcase") && items.iter().any(|(l, _)| l == "length"), "{items:?}");

    let items = complete_at_end(&mut c, "print Mode.");
    assert!(items.contains(&("Easy".to_string(), Some("Mode".to_string()))) && items.iter().any(|(l, _)| l == "values"), "{items:?}");
    let items = complete_at_end(&mut c, "print Pt.");
    assert!(items.contains(&("len2".to_string(), Some("Function(): Number".to_string()))), "{items:?}");
    let items = complete_at_end(&mut c, "print Number.");
    assert!(items.iter().any(|(l, _)| l == "floor") && items.contains(&("pi".to_string(), Some("Number".to_string()))), "{items:?}");
    // After `:`, a method call: the members of the receiver.
    let items = complete_at_end(&mut c, "print p:");
    assert!(items.contains(&("len2".to_string(), Some("Function(): Number".to_string()))), "{items:?}");
    // After `?.` and `?.:`, as after `.` and `:`: the members of the
    // receiver without `nil`.
    let items = complete_at_end(&mut c, "let q: Pt? = p; print q?.");
    assert!(items.contains(&("x".to_string(), Some("Number".to_string()))), "{items:?}");
    assert!(items.contains(&("len2".to_string(), Some("Function(): Number".to_string()))), "{items:?}");
    let items = complete_at_end(&mut c, "let q: Pt? = p; print q?.:");
    assert!(items.contains(&("len2".to_string(), Some("Function(): Number".to_string()))), "{items:?}");
    // After `Lib.`, the exports of the file, with their types.
    let items = complete_at_end(&mut c, "print Lib.");
    assert_eq!(items, [("helper".to_string(), Some("Function(a: Number): Number".to_string())), ("limit".to_string(), Some("Number".to_string()))]);
    // A `:` that is no method call (an annotation): the names in scope.
    let items = complete_at_end(&mut c, "let q:");
    assert!(items.iter().any(|(l, _)| l == "Pt") && items.iter().any(|(l, _)| l == "Number"), "{items:?}");

    // Names in scope inside `main`: `inner`, `p`, `main`, `Pt`, the
    // declared `Foo` and the prelude; not `outer`, declared later.
    let text = format!("{head}fn main\n  let inner = 1\n  print \nend\nlet outer = 2\nprint 1\n");
    c.change(&uri, &text);
    let inside = at(&text, "  print \nend", 0);
    let items = labels(c.complete(&uri, Position { character: inside.character + 8, ..inside }).unwrap());
    let names: Vec<&str> = items.iter().map(|(l, _)| l.as_str()).collect();
    for expected in ["inner", "p", "main", "Pt", "Foo", "local_player", "print", "Number", "Exception"] {
        assert!(names.contains(&expected), "{expected} missing from {names:?}");
    }
    assert!(!names.contains(&"outer"), "{names:?}");
    assert!(items.contains(&("inner".to_string(), Some("Number".to_string()))), "{items:?}");
    // At file level after `let outer`, `outer` is in scope and `inner` is not.
    let items = labels(c.complete(&uri, at(&text, "print 1", 0)).unwrap());
    let names: Vec<&str> = items.iter().map(|(l, _)| l.as_str()).collect();
    assert!(names.contains(&"outer") && !names.contains(&"inner"), "{names:?}");
    c.shutdown();
}

/// Malformed input, requests at every position, and bad parameters: the
/// server answers everything and keeps going.
#[test]
fn malformed_input_and_requests_never_break_the_server() {
    let dir = temp_dir("malformed");
    let mut c = Client::start(Some(&dir));
    let text = "fn (((\nlet = \nstruct\n  x:\nend\nprint foo.\n@@@ ))) \"unterminated\nimport { a } from\nlet p = Pt {\nprint p.\nx:m(\n";
    let uri = c.open(&dir.join("bad.nl"), text);
    assert!(!c.diagnostics(&uri).is_empty());
    for (line, l) in text.lines().enumerate() {
        for character in 0..=l.encode_utf16().count() as u32 + 1 {
            let pos = Position::new(line as u32, character);
            assert!(c.hover(&uri, pos).is_ok(), "hover at {pos:?}");
            assert!(c.definition(&uri, pos).is_ok(), "definition at {pos:?}");
            assert!(c.complete(&uri, pos).is_ok(), "completion at {pos:?}");
        }
    }
    // Bad parameters are an error response; an unknown method too; a
    // document that is not open answers null. Then the server still
    // answers.
    assert!(c.raw_request(request::HoverRequest::METHOD, serde_json::json!({ "bogus": 1 })).is_err());
    assert!(c.raw_request("nl/unknown", serde_json::json!(null)).is_err());
    let other = path_to_uri(&dir.join("not-open.nl")).unwrap();
    assert_eq!(c.hover(&other, Position::new(0, 0)).unwrap(), None);
    assert_eq!(c.definition(&other, Position::new(0, 0)).unwrap(), None);
    assert_eq!(c.complete(&other, Position::new(0, 0)).unwrap(), None);
    assert!(c.hover(&uri, Position::new(0, 0)).is_ok());
    c.shutdown();
}

/// An `untitled:` document is compiled like a file in the working
/// directory and answers requests.
#[test]
fn untitled_documents() {
    let mut c = Client::start(None);
    let uri = Uri::from_str("untitled:Untitled-1").unwrap();
    let text = "let x = 1\nprint x, nope\n";
    c.open_uri(uri.clone(), text);
    let diags = c.diagnostics(&uri);
    assert_eq!(diags.len(), 1, "{diags:?}");
    assert_eq!(diags[0].range, range_of(text, "nope", 0));
    let h = hover_text(c.hover(&uri, at(text, "x, nope", 0)).unwrap());
    assert_eq!(h, "```nl\nlet x: Number\n```");
    let d = location(c.definition(&uri, at(text, "x, nope", 0)).unwrap());
    assert_eq!((d.uri, d.range), (uri.clone(), word(text, "let x", "x")));
    c.shutdown();
}

/// `exit` without `shutdown` is an error of the server (exit code 1).
#[test]
fn exit_without_shutdown_is_an_error() {
    let (server, client) = Connection::memory();
    let thread = std::thread::Builder::new().stack_size(crate::STACK_SIZE).spawn(move || crate::run(server)).unwrap();
    let mut c = Client { conn: client, next: 1, pending: Vec::new(), thread: None };
    let _ = c.request::<request::Initialize>(InitializeParams::default()).unwrap();
    c.notify::<notification::Initialized>(InitializedParams {});
    c.notify::<notification::Exit>(());
    assert!(thread.join().unwrap().is_err());
}

/// An open document owns its URI: another document's compile never
/// publishes there, and closing it never clears there. What open
/// documents report about files that are not open is the union of their
/// reports, and a close removes only that document's part.
#[test]
fn open_documents_own_their_uris() {
    let dir = temp_dir("ownership");
    std::fs::write(dir.join("dep.nl"), "pub fn d\n  1\nend\nprint nope\n").unwrap();
    let mut c = Client::start(Some(&dir));
    // An analyzer error and the lints that come with it (a resolver
    // error would stop the compile before the lints).
    let lib_text = "pub fn f\n  1\nend\nlet unused = 1\nlet n: Number = \"s\"\n";
    let lib = c.open(&dir.join("lib.nl"), lib_text);
    let own = c.diagnostics(&lib);
    let is_error = |d: &Diagnostic| d.severity == Some(DiagnosticSeverity::ERROR);
    assert!(own.iter().any(is_error) && own.iter().any(|d| !is_error(d)), "an error and a lint: {own:?}");
    let full = own.len();

    // main imports lib: lib's errors are forwarded to main's compile, but
    // lib's URI keeps lib's own results (every open document is compiled
    // again, so lib is published again, in full).
    let main = c.open(&dir.join("main.nl"), "import { f } from 'lib'\nprint f\n");
    assert_eq!(c.diagnostics(&lib).len(), full);
    let main_diags = c.diagnostics(&main);
    assert!(main_diags.iter().any(|d| d.code == Some(NumberOrString::String("import-failed".to_string()))), "{main_diags:?}");
    assert!(!c.pending_for(&lib));

    // A broken declaration file buffer in the directory: its syntax error
    // is published under its own URI and nowhere else (the compiler stops
    // at a broken declaration file, so lib reports nothing of its own for
    // the moment). Closing it compiles the others again.
    let api = c.open(&dir.join("api.d.nl"), "declare fn g(\n");
    let api_diags = c.diagnostics(&api);
    assert!(api_diags.iter().any(|d| d.code == Some(NumberOrString::String("syntax".to_string()))), "{api_diags:?}");
    let lib_diags = c.diagnostics(&lib);
    assert!(lib_diags.iter().all(|d| d.code != Some(NumberOrString::String("syntax".to_string()))), "{lib_diags:?}");
    let _ = c.diagnostics(&main);
    assert!(!c.pending_for(&lib));
    c.close(&api);
    assert!(c.diagnostics(&api).is_empty());
    assert_eq!(c.diagnostics(&lib).len(), full);
    let _ = c.diagnostics(&main);
    assert!(!c.pending_for(&lib));

    // Fixing lib in its buffer updates main (stale diagnostics), and
    // removing the export again does too.
    c.change(&lib, "pub fn f\n  1\nend\nlet unused = 1\n");
    assert!(c.diagnostics(&lib).iter().all(|d| !is_error(d)), "only lints stay");
    assert!(c.diagnostics(&main).is_empty());
    c.change(&lib, "fn f\n  1\nend\nprint f\n");
    assert!(c.diagnostics(&lib).iter().all(|d| !is_error(d)));
    let main_diags = c.diagnostics(&main);
    assert!(main_diags.iter().any(|d| d.code == Some(NumberOrString::String("not-exported".to_string()))), "{main_diags:?}");

    // Closing main does not clear lib: lib is compiled again (its buffer
    // may have imported main), never emptied.
    c.close(&main);
    assert!(c.diagnostics(&main).is_empty());
    assert!(c.diagnostics(&lib).iter().all(|d| !is_error(d)));
    assert!(!c.pending_for(&lib));
    c.change(&lib, "fn f\n  1\nend\nprint nope\n");
    assert!(c.diagnostics(&lib).iter().any(is_error), "no empty publication for lib came first");

    // A file that is not open: the union of what the open documents
    // report about it, until the last of them closes.
    // An unchanged union is not published again.
    let dep = path_to_uri(&dir.join("dep.nl")).unwrap();
    let a = c.open(&dir.join("a.nl"), "import { d } from 'dep'\nprint d\n");
    let _ = c.diagnostics(&a);
    assert_eq!(c.diagnostics(&dep).len(), 1);
    let b = c.open(&dir.join("b.nl"), "import { d } from 'dep'\nprint d\n");
    // The open compiles a again too.
    let _ = c.diagnostics(&a);
    let _ = c.diagnostics(&b);
    assert!(!c.pending_for(&dep), "one error, reported by two documents: unchanged");
    c.close(&a);
    assert!(c.diagnostics(&a).is_empty());
    let _ = c.diagnostics(&b);
    assert!(!c.pending_for(&dep), "b still reports it: unchanged");
    c.close(&b);
    assert!(c.diagnostics(&b).is_empty());
    assert!(c.diagnostics(&dep).is_empty(), "the last reporter closed");
    c.shutdown();
}

/// An open document is known by the URI the editor gave, whatever its
/// spelling: what another document's compile reports about it goes under
/// that URI, not under the server's own spelling of the path.
#[test]
fn open_documents_are_known_by_the_editors_uri() {
    let dir = temp_dir("urispelling");
    let mut c = Client::start(Some(&dir));
    let path = dir.join("lib.nl");
    let own = path_to_uri(&path).unwrap();
    // The same file, with a letter percent-encoded.
    let editors = Uri::from_str(&own.as_str().replace("lib.nl", "l%69b.nl")).unwrap();
    assert_eq!(uri_to_path(&editors).unwrap(), path);
    c.open_uri(editors.clone(), "pub fn f\n  1\nend\nprint nope\n");
    assert_eq!(c.diagnostics(&editors).len(), 1);
    let main = c.open(&dir.join("main.nl"), "import { f } from 'lib'\nprint f\n");
    let main_diags = c.diagnostics(&main);
    assert!(main_diags.iter().any(|d| d.code == Some(NumberOrString::String("import-failed".to_string()))), "{main_diags:?}");
    assert_eq!(c.diagnostics(&editors).len(), 1, "lib's own compile, under the editor's URI");
    // A request answers after every notification sent before it.
    let _ = c.hover(&main, Position::new(0, 0));
    assert!(!c.pending_for(&own), "nothing is published under the server's spelling");
    assert!(!c.pending_for(&editors));
    c.shutdown();
}

/// A declaration file that exists only as an open buffer is in scope for
/// the documents of its directory.
#[test]
fn open_declaration_buffers_are_in_scope() {
    let dir = temp_dir("declbuffer");
    let mut c = Client::start(Some(&dir));
    let api = c.open(&dir.join("api.d.nl"), "declare module Api = :api\n  ## Answers.\n  fn f(): Number\nend\n");
    assert!(c.diagnostics(&api).is_empty());
    let text = "print Api.f\n";
    let main = c.open(&dir.join("main.nl"), text);
    assert!(c.diagnostics(&main).is_empty());
    let p = at(text, "Api.f", 0);
    let h = hover_text(c.hover(&main, Position { character: p.character + 4, ..p }).unwrap());
    assert!(h.contains("fn Api.f(): Number") && h.contains("Answers."), "{h}");
    let d = location(c.definition(&main, Position { character: p.character + 4, ..p }).unwrap());
    assert_eq!(d.uri, api);
    c.shutdown();
}

fn nl_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(d) = dirs.pop() {
        for e in std::fs::read_dir(&d).unwrap().filter_map(Result::ok) {
            let p = e.path();
            if p.is_dir() {
                dirs.push(p);
            } else if p.extension().is_some_and(|x| x == "nl") {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// Every program of the repository: hover and definition at every word
/// token answer without an error, and every reference the resolver bound
/// to a binding of the file has a definition at that binding.
#[test]
fn every_program_hovers_and_defines() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = vec![repo.join("example.nl")];
    files.extend(nl_files(&repo.join("tests/run")));
    files.extend(nl_files(&repo.join("crates/nlc/tests/cases")));
    let mut c = Client::start(Some(&repo));
    let plain = crate::files::Files::new();
    let mut checked = 0;
    for path in files {
        let path = crate::position::normalize(&path);
        let text = std::fs::read_to_string(&path).unwrap();
        let uri = c.open(&path, &text);
        let _ = c.diagnostics(&uri);
        let doc = crate::analysis::analyze(&plain, uri.as_str(), Some(&path), &text);
        for tok in doc.root.descendants_with_tokens().filter_map(|e| e.into_token()).filter(is_word) {
            let pos = doc.lines.position(&text, tok.text_range().start());
            let hover = c.hover(&uri, pos);
            assert!(hover.is_ok(), "hover in {} at {pos:?}: {hover:?}", path.display());
            let def = c.definition(&uri, pos);
            assert!(def.is_ok(), "definition in {} at {pos:?}: {def:?}", path.display());
            let Some(parent) = tok.parent() else { continue };
            let bound = match parent.kind() {
                nl_syntax::SyntaxKind::NAME_REF | nl_syntax::SyntaxKind::SELF_EXPR => doc.res.reference(parent.text_range()),
                nl_syntax::SyntaxKind::LUA_PATH => doc.res.reference(tok.text_range()),
                _ => None,
            };
            let Some(nl_sema::Ref::Binding(id)) = bound else { continue };
            let b = doc.res.binding(*id).unwrap();
            if b.decl == nl_sema::DeclKind::Import {
                continue;
            }
            let loc = location(def.unwrap());
            assert_eq!(loc.uri, uri, "definition of `{}` in {} at {pos:?}", tok.text(), path.display());
            assert_eq!(loc.range, doc.lines.range(&text, b.range), "definition of `{}` in {} at {pos:?}", tok.text(), path.display());
            checked += 1;
        }
        // Every open document is compiled again on each open: close each
        // one once it is checked, as an editor would.
        c.close(&uri);
        assert!(c.diagnostics(&uri).is_empty());
    }
    assert!(checked > 500, "{checked} references checked");
    c.shutdown();
}
