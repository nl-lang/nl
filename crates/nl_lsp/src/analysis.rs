//! One document as the server understands it: its tree, the resolver's
//! [`Resolution`] and the analyzer's [`TypeInfo`] (`AnalyzeOptions::record`),
//! and the questions hover, definition and
//! completion ask of them: which symbol is under a position, and where a
//! declaration file or an imported file declares a name.
//!
//! The pipeline here is that of `nl_capi::driver::pipeline` up to the
//! analyzer, with two differences an editor wants: the resolver and the
//! analyzer run whatever errors the stages before them found (a file being
//! typed is rarely error-free), and imported files are resolved for their
//! exports only, never checked (diagnostics come from the real compile in
//! `diagnostics.rs`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use lsp_types::Uri;
use nl_sema::analyzer::TypeInfo;
use nl_sema::types::{NamedId, NamedKind, Type};
use nl_sema::util::{expr_child, field_name, method_name_of};
use nl_sema::{BindingId, DeclId, DeclKind, Declarations, ExportInfo, ImportProvider, Prelude, Ref, Resolution, SelfTarget};
use nl_syntax::{SyntaxKind, SyntaxNode, SyntaxToken, TextRange, TextSize};
use rowan::{NodeOrToken, TokenAtOffset};

use crate::files::Files;
use crate::position::{LineMap, path_to_uri};

use SyntaxKind::*;

/// An analyzed document.
pub struct Doc {
    pub uri: String,
    /// The document's file, for a `file:` URI.
    pub path: Option<PathBuf>,
    pub text: String,
    pub lines: LineMap,
    pub root: SyntaxNode,
    pub decls: Declarations,
    /// The text of each declaration file, by canonical name, for
    /// positions in them.
    pub decl_texts: HashMap<String, String>,
    pub res: Resolution,
    /// `None` when the analyzer did not run (a declaration file).
    pub info: Option<TypeInfo>,
}

/// True for a declaration file, which has its own parser and
/// no names to resolve; `name` is a URI or a path.
pub fn is_decl_file(name: &str) -> bool {
    name.ends_with(".d.nl")
}

/// Parses, resolves and analyzes `text` as the document `uri` (the file
/// `path`, when it is one). A declaration file is only parsed: nothing in
/// it refers to anything.
pub fn analyze(files: &Files, uri: &str, path: Option<&Path>, text: &str) -> Doc {
    let path = path.map(Path::to_path_buf);
    if is_decl_file(uri) {
        let root = nl_syntax::parse_decl_file(text).syntax();
        return Doc { uri: uri.to_string(), path, text: text.to_owned(), lines: LineMap::new(text), root, decls: Declarations::default(), decl_texts: HashMap::new(), res: Resolution::default(), info: None };
    }
    let parse = nl_syntax::parse(text);
    let root = parse.syntax();
    let dir = path.as_deref().and_then(Path::parent);
    let (decls, decl_texts) = load_declarations(files, dir);
    let from = path.as_ref().map(|p| p.to_string_lossy().into_owned());
    let imports = Exports { files, from };
    let res = nl_sema::resolve_with(&root, &imports, &decls);
    let opts = nl_sema::AnalyzeOptions { decls: Some(&decls), imports: Some(&imports), record: true, ..nl_sema::AnalyzeOptions::default() };
    let info = nl_sema::analyze(&root, &res, &opts).info;
    Doc { uri: uri.to_string(), path, text: text.to_owned(), lines: LineMap::new(text), root, decls, decl_texts, res, info }
}

/// The declaration files in scope: the built-in prelude and the loader's
/// (`load_declarations` of the pipeline, without its diagnostics).
pub fn load_declarations(files: &Files, doc_dir: Option<&Path>) -> (Declarations, HashMap<String, String>) {
    let mut texts: HashMap<String, String> = nl_sema::decl::PRELUDE_SOURCES.iter().map(|(n, s)| (n.to_string(), s.to_string())).collect();
    let mut trees = nl_sema::decl::prelude_files();
    for (name, bytes) in files.declarations(doc_dir) {
        let Ok(text) = nl_syntax::source_from_bytes(&bytes) else { continue };
        let parse = nl_syntax::parse_decl_file(text);
        if parse.has_errors() {
            continue;
        }
        texts.insert(name.clone(), text.to_owned());
        trees.push((name, parse.syntax()));
    }
    (Declarations::load(&trees).0, texts)
}

/// The exports of imported files, read through [`Files`] and resolved
/// without reading the files they import in turn (as the pipeline does
/// for a file two imports away).
struct Exports<'a> {
    files: &'a Files,
    from: Option<String>,
}

impl ImportProvider for Exports<'_> {
    fn exports(&self, path: &str) -> Result<Vec<ExportInfo>, String> {
        let (_, _, res) = resolve_import(self.files, self.from.as_deref(), path).ok_or_else(|| format!("cannot find the file '{path}' to import"))?;
        Ok(res.exports())
    }

    fn primitive_methods(&self, path: &str) -> Vec<nl_sema::PrimMethod> {
        let scan = |name: &str| {
            let text = self.files.read_text(Path::new(name))?;
            let parse = nl_syntax::parse(&text);
            (!parse.has_errors()).then(|| nl_sema::scan_file(&parse.syntax()))
        };
        nl_sema::primitive_methods_through(self.from.as_deref(), path, &|from, p| self.files.resolve(from, p), &scan)
    }
}

/// Finds, reads and resolves the file an `import` of `from` names: its
/// canonical path, its text and its resolution.
pub fn resolve_import(files: &Files, from: Option<&str>, path: &str) -> Option<(PathBuf, String, Resolution)> {
    let name = files.resolve(from, path)?;
    if Some(name.as_str()) == from {
        return None;
    }
    let text = files.read_text(Path::new(&name))?;
    let parse = nl_syntax::parse(&text);
    if parse.has_errors() {
        return None;
    }
    let res = nl_sema::resolve(&parse.syntax(), &nl_sema::NoImports);
    Some((PathBuf::from(name), text, res))
}

/// The analysis of the file that the import binding `import` of `doc`
/// names.
pub fn imported_doc(files: &Files, doc: &Doc, import: BindingId) -> Option<Doc> {
    let (_, path) = doc.import_of(import)?;
    let (file, text, _) = resolve_import(files, doc.from_name().as_deref(), &path)?;
    let uri = path_to_uri(&file)?;
    Some(analyze(files, uri.as_str(), Some(&file), &text))
}

/// The export `name` of the file that `import` (an `import from … as`
/// binding of `doc`) names: that file's analysis and the binding holding
/// the export.
pub fn export_target(files: &Files, doc: &Doc, import: BindingId, name: &str) -> Option<(Doc, BindingId)> {
    let other = imported_doc(files, doc, import)?;
    let (id, _) = other.res.export_bindings().find(|(_, e)| e.name == name)?;
    Some((other, id))
}

/// The token at a byte offset, preferring a word (a name, a keyword) when
/// the offset lies between two tokens.
pub fn token_at(root: &SyntaxNode, offset: TextSize) -> Option<SyntaxToken> {
    match root.token_at_offset(offset) {
        TokenAtOffset::None => None,
        TokenAtOffset::Single(t) => Some(t),
        TokenAtOffset::Between(l, r) => Some(if is_word(&l) || !is_word(&r) { l } else { r }),
    }
}

/// A token hover, definition and completion are about.
pub fn is_word(t: &SyntaxToken) -> bool {
    let k = t.kind();
    k.is_name() || k.is_keyword() || k == AT_NAME
}

/// What a position names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Symbol {
    /// A binding of the document, at its declaration or a reference.
    Binding(BindingId),
    /// A name of a declaration file.
    Declared(DeclId),
    /// A member of a nominal type: `x.name`, `Enum.Variant`, a field in
    /// its struct, a method at its definition.
    Member { owner: NamedId, name: String },
    /// `Lib.name` where `Lib` is bound by `import from … as Lib`: the
    /// export `name` of that file.
    Export { import: BindingId, name: String },
    /// A prelude name.
    Prelude(&'static Prelude),
}

impl Doc {
    pub fn uri(&self) -> Option<Uri> {
        Uri::from_str(&self.uri).ok()
    }

    /// The name imports resolve from: the file's path, for a file.
    pub fn from_name(&self) -> Option<String> {
        self.path.as_ref().map(|p| p.to_string_lossy().into_owned())
    }

    /// The symbol under `offset`, and the token that names it.
    pub fn symbol_at(&self, offset: TextSize) -> Option<(Symbol, SyntaxToken)> {
        let tok = token_at(&self.root, offset)?;
        if !is_word(&tok) {
            return None;
        }
        let parent = tok.parent()?;
        let sym = match parent.kind() {
            NAME_REF => self.ref_symbol(self.res.reference(parent.text_range())?),
            NAME => self.name_symbol(&parent),
            SELF_EXPR if tok.kind() == SELF_TYPE_KW => match self.res.self_target(parent.text_range())? {
                SelfTarget::Binding(id) => Some(Symbol::Binding(*id)),
                SelfTarget::Member { id, .. } => Some(Symbol::Binding(*id)),
                SelfTarget::Prelude(p) => Some(Symbol::Prelude(p)),
            },
            SELF_EXPR => self.ref_symbol(self.res.reference(parent.text_range())?),
            AT_EXPR => {
                // `@name` is a field of `self`.
                let Some(Ref::Binding(id)) = self.res.reference(parent.text_range()) else { return None };
                let info = self.info.as_ref()?;
                let name = tok.text().trim_start_matches('@').to_string();
                match info.types.expand(info.binding_types.get(id)?) {
                    Type::Named(owner) | Type::Static(owner) => Some(Symbol::Member { owner, name }),
                    _ => Some(Symbol::Binding(*id)),
                }
            }
            FIELD_EXPR | METHOD_CALL => {
                // The receiver's own name is handled through its node; the
                // member is the word after the dot or colon.
                let name = match parent.kind() {
                    FIELD_EXPR => field_name(&parent).map(|t| t.text().to_string()),
                    _ => method_name_of(&parent),
                }?;
                if tok.text() != name {
                    return None;
                }
                let recv = expr_child(&parent);
                if let Some(import) = recv.as_ref().and_then(|r| self.as_import(r)) {
                    return Some((Symbol::Export { import, name }, tok));
                }
                let owner = match self.info.as_ref()?.members.get(&parent.text_range()) {
                    Some((owner, _)) => *owner,
                    None => self.receiver_nid(&recv?)?,
                };
                Some(Symbol::Member { owner, name })
            }
            QUAL_NAME => self.qual_symbol(&parent, &tok),
            LUA_PATH => self.ref_symbol(self.res.reference(tok.text_range())?),
            _ => None,
        }?;
        Some((sym, tok))
    }

    fn ref_symbol(&self, r: &Ref) -> Option<Symbol> {
        match r {
            Ref::Binding(id) => Some(Symbol::Binding(*id)),
            Ref::Prelude(p) => Some(Symbol::Prelude(p)),
            Ref::Declared(id) => Some(Symbol::Declared(*id)),
            Ref::LuaGlobal(_) => None,
        }
    }

    /// A declaring `NAME`: a binding, or a member of one of the file's
    /// types (a field, a variant, a method, an interface member).
    fn name_symbol(&self, name: &SyntaxNode) -> Option<Symbol> {
        if let Some(id) = self.res.declaration(name.text_range()) {
            return Some(Symbol::Binding(id));
        }
        let info = self.info.as_ref()?;
        let range = name.text_range();
        info.member_decls.iter().find(|(_, r)| **r == range).map(|((owner, n), _)| Symbol::Member { owner: *owner, name: n.clone() })
    }

    /// A segment of a dotted type name, struct literal path or pattern
    /// path: the first resolves as the resolver said, the later ones are
    /// members of the ones before (`Mode.Easy`, `Outer.Inner`).
    fn qual_symbol(&self, qual: &SyntaxNode, tok: &SyntaxToken) -> Option<Symbol> {
        let segments = nl_sema::util::qual_segments(qual);
        let first = segments.first()?;
        let r = self.res.type_ref(qual.text_range()).or_else(|| self.res.reference(qual.text_range()))?;
        if first.text_range() == tok.text_range() {
            return self.ref_symbol(r);
        }
        let info = self.info.as_ref()?;
        let mut cur = self.ref_nid(r)?;
        for seg in segments.iter().skip(1) {
            let name = seg.text().to_string();
            if seg.text_range() == tok.text_range() {
                return Some(Symbol::Member { owner: cur, name });
            }
            cur = match info.types.kind(cur)? {
                NamedKind::Module { members } => match members.get(&name)? {
                    nl_sema::types::Member::Value(Type::Static(inner)) => *inner,
                    _ => return None,
                },
                _ => return None,
            };
        }
        None
    }

    /// The nominal type a reference to a type-level name stands for.
    fn ref_nid(&self, r: &Ref) -> Option<NamedId> {
        let info = self.info.as_ref()?;
        match r {
            Ref::Binding(id) => info.named_of.get(id).copied(),
            Ref::Declared(id) => self.nid_of_decl(*id),
            Ref::Prelude(p) => info.prelude_named.get(p.name).copied(),
            Ref::LuaGlobal(_) => None,
        }
    }

    /// The static type of a type-level name used as a value (`Mode` in
    /// `Mode.Easy`, `Pt` in `Pt.create`), which the analyzer leaves `any`
    /// for the file's own structs, enums and modules: the value that
    /// stands for the named thing.
    pub fn static_type_of_name(&self, recv: &SyntaxNode) -> Option<Type> {
        if recv.kind() != NAME_REF {
            return None;
        }
        self.ref_nid(self.res.reference(recv.text_range())?).map(Type::Static)
    }

    /// The type of a receiver expression: what the analyzer recorded for
    /// it, or the static type of a type-level name.
    pub fn receiver_type(&self, recv: &SyntaxNode) -> Option<Type> {
        let info = self.info.as_ref()?;
        match info.expr_types.get(&recv.text_range()) {
            Some(Type::Any) | None => self.static_type_of_name(recv),
            Some(t) => Some(t.clone()),
        }
    }

    /// The nominal type of a receiver, for member lookup.
    pub fn receiver_nid(&self, recv: &SyntaxNode) -> Option<NamedId> {
        let info = self.info.as_ref()?;
        match info.types.expand(&self.receiver_type(recv)?) {
            Type::Named(nid) | Type::Static(nid) => Some(nid),
            Type::Number => info.prelude_named.get("Number").copied(),
            Type::String => info.prelude_named.get("String").copied(),
            Type::Bool => info.prelude_named.get("Bool").copied(),
            Type::Regex => info.prelude_named.get("Regex").copied(),
            _ => None,
        }
    }

    /// The binding of `import from … as Name` when `recv` is a reference
    /// to such a name (the whole export table).
    pub fn as_import(&self, recv: &SyntaxNode) -> Option<BindingId> {
        if recv.kind() != NAME_REF {
            return None;
        }
        let Ref::Binding(id) = self.res.reference(recv.text_range())? else { return None };
        (self.res.declared_as(*id) == Some(DeclKind::Import)).then_some(*id)
    }

    /// The nominal type of a declared name.
    pub fn nid_of_decl(&self, id: DeclId) -> Option<NamedId> {
        let info = self.info.as_ref()?;
        let item = self.decls.get(id)?;
        let range = item.node.text_range();
        info.decl_nodes.iter().find(|(_, (d, r))| *d == id && *r == range).map(|(nid, _)| *nid)
    }

    /// The `DECL_*` node of a nominal type that comes from a declaration
    /// file, with the item it belongs to.
    pub fn decl_node(&self, nid: NamedId) -> Option<(DeclId, SyntaxNode)> {
        let info = self.info.as_ref()?;
        let (id, range) = info.decl_nodes.get(&nid)?;
        let item = self.decls.get(*id)?;
        if item.node.text_range() == *range {
            return Some((*id, item.node.clone()));
        }
        item.node.descendants().find(|n| n.text_range() == *range).map(|n| (*id, n))
    }

    /// The member node (`DECL_FN`, `DECL_FIELD`, `DECL_VALUE`, a nested
    /// declaration) that declares `name` on `owner` or one of its parents,
    /// with the item it belongs to.
    pub fn decl_member_node(&self, owner: NamedId, name: &str) -> Option<(DeclId, SyntaxNode)> {
        let info = self.info.as_ref()?;
        let mut cur = Some(owner);
        for _ in 0..64 {
            let c = cur?;
            if let Some((id, node)) = self.decl_node(c) {
                let found = node.children().find(|m| matches!(m.kind(), DECL_FN | DECL_FIELD | DECL_VALUE | DECL_MODULE | DECL_STRUCT | DECL_TYPE) && member_name(m).as_deref() == Some(name));
                if let Some(m) = found {
                    return Some((id, m));
                }
            }
            cur = info.types.parent(c);
        }
        None
    }

    /// Where this file declares `name` on `owner` or one of its parents:
    /// the range of the declaring name.
    pub fn own_member_decl(&self, owner: NamedId, name: &str) -> Option<TextRange> {
        let info = self.info.as_ref()?;
        let mut cur = Some(owner);
        for _ in 0..64 {
            let c = cur?;
            if let Some(r) = info.member_decls.get(&(c, name.to_string())) {
                return Some(*r);
            }
            cur = info.types.parent(c);
        }
        None
    }

    /// The declaring construct around the `NAME` at `range` (the node a
    /// doc comment attaches to).
    pub fn declaration_node(&self, range: TextRange) -> Option<SyntaxNode> {
        let elem = self.root.covering_element(range);
        let node = match elem {
            NodeOrToken::Node(n) => n,
            NodeOrToken::Token(t) => t.parent()?,
        };
        node.ancestors().find(|a| {
            matches!(
                a.kind(),
                FN_DECL | LET_DECL | CONST_DECL | GLOBAL_DECL | STRUCT_DECL | FIELD_DECL | INTERFACE_DECL | INTERFACE_IMPL | IFACE_METHOD | IFACE_FIELD | ENUM_DECL | VARIANT | TYPE_DECL | MODULE_DECL | IMPORT_DECL | PARAM | REST_PARAM | KEYED_PARAM | TYPE_PARAM | ON_EXPR
            )
        })
    }

    /// The `import` statement that binds the import `id`, and the path it
    /// names.
    pub fn import_of(&self, id: BindingId) -> Option<(SyntaxNode, String)> {
        let b = self.res.binding(id)?;
        if b.decl != DeclKind::Import {
            return None;
        }
        let node = self.declaration_node(b.range)?;
        if node.kind() != IMPORT_DECL {
            return None;
        }
        let path = nl_sema::util::token(&node, STRING).map(|t| nl_sema::decode_plain_string(&t))?;
        Some((node, path))
    }
}

/// The name a member of a declaration declares (`NAME`, or an `OP_NAME`).
pub fn member_name(m: &SyntaxNode) -> Option<String> {
    match nl_sema::util::child(m, NAME) {
        Some(n) => nl_sema::util::name_text(&n),
        None => nl_sema::util::child(m, OP_NAME).map(|o| o.text().to_string().split_whitespace().collect()),
    }
}

/// The path of a declaration file by its canonical name. The built-in
/// files of `decl/` are embedded in the compiler: they are found in a
/// workspace root or the working directory when the editor works in the
/// NL repository, and otherwise in a per-user cache, where `write` puts a
/// copy per version of the server, again when the copy there is not this
/// server's text (atomically, so that two servers cannot leave a
/// half-written file) so that go-to-definition has a file to open.
/// Without `write`, `None` when no current copy exists.
pub fn decl_file_path(files: &Files, name: &str, write: bool) -> Option<PathBuf> {
    let embedded = nl_sema::decl::PRELUDE_SOURCES.iter().find(|(n, _)| *n == name);
    let Some((_, source)) = embedded else { return Some(PathBuf::from(name)) };
    for root in files.roots().iter().chain(std::iter::once(&files.cwd().to_path_buf())) {
        let p = root.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    let p = cache_dir().join("nl-lsp").join(env!("CARGO_PKG_VERSION")).join(name);
    // A copy is used only while it is this server's text: builds of one
    // version can differ in their declarations.
    if std::fs::read_to_string(&p).is_ok_and(|text| text == *source) {
        return Some(p);
    }
    if !write {
        return None;
    }
    match write_atomically(&p, source) {
        Ok(()) => Some(p),
        Err(e) => {
            eprintln!("nl-lsp: cannot write {}: {e}", p.display());
            None
        }
    }
}

/// `$XDG_CACHE_HOME`, else the platform's cache directory, else the
/// temporary directory.
fn cache_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("XDG_CACHE_HOME").filter(|d| !d.is_empty()) {
        return PathBuf::from(d);
    }
    if cfg!(windows) {
        if let Some(d) = std::env::var_os("LOCALAPPDATA").filter(|d| !d.is_empty()) {
            return PathBuf::from(d);
        }
    }
    if let Some(home) = std::env::var_os("HOME").filter(|d| !d.is_empty()) {
        let home = PathBuf::from(home);
        return if cfg!(target_os = "macos") { home.join("Library/Caches") } else { home.join(".cache") };
    }
    std::env::temp_dir()
}

fn write_atomically(path: &Path, content: &str) -> std::io::Result<()> {
    let dir = path.parent().ok_or_else(|| std::io::Error::other("no parent directory"))?;
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!("{}.tmp-{}", path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default(), std::process::id()));
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}
