//! Go-to-definition: a reference goes to the
//! declaration the resolver bound it to; an imported name to its export
//! in the other file; a name of a declaration file to its
//! `declare` item, and a member of a declared module, struct or type to
//! the member's line in that `.d.nl`; a member of a struct,
//! enum or module of the file to its field, variant or method.

use lsp_types::{Location, Range};
use nl_sema::util::{child, has_token, trimmed_range};
use nl_sema::{BindingId, DeclId, DeclKind};
use nl_syntax::SyntaxKind::{AS_KW, NAME, OP_NAME};
use nl_syntax::{TextRange, TextSize};

use crate::analysis::{Doc, Symbol, decl_file_path, export_target, resolve_import};
use crate::files::Files;
use crate::position::{LineMap, path_to_uri};

pub fn definition(files: &Files, doc: &Doc, offset: TextSize) -> Option<Location> {
    let (sym, _) = doc.symbol_at(offset)?;
    locate(files, doc, &sym)
}

/// Where a symbol is declared.
pub fn locate(files: &Files, doc: &Doc, sym: &Symbol) -> Option<Location> {
    match sym {
        Symbol::Binding(id) => {
            let b = doc.res.binding(*id)?;
            if b.decl == DeclKind::Import {
                if let Some(loc) = import_target(files, doc, *id) {
                    return Some(loc);
                }
            }
            Some(Location::new(doc.uri()?, doc.lines.range(&doc.text, b.range)))
        }
        Symbol::Export { import, name } => {
            let (other, id) = export_target(files, doc, *import, name)?;
            let b = other.res.binding(id)?;
            Some(Location::new(other.uri()?, other.lines.range(&other.text, b.range)))
        }
        Symbol::Declared(id) => decl_location(files, doc, *id, None),
        Symbol::Member { owner, name } => {
            if let Some(r) = doc.own_member_decl(*owner, name) {
                return Some(Location::new(doc.uri()?, doc.lines.range(&doc.text, r)));
            }
            let (id, m) = doc.decl_member_node(*owner, name)?;
            let range = child(&m, NAME).or_else(|| child(&m, OP_NAME)).map(|n| n.text_range()).unwrap_or_else(|| trimmed_range(&m));
            decl_location(files, doc, id, Some(range))
        }
        Symbol::Prelude(p) => {
            let id = doc.decls.lookup(p.name)?;
            decl_location(files, doc, id, None)
        }
    }
}

/// A location in the declaration file of item `id`: the given range, or
/// the item's declaring name.
fn decl_location(files: &Files, doc: &Doc, id: DeclId, range: Option<TextRange>) -> Option<Location> {
    let item = doc.decls.get(id)?;
    // The one place a copy of an embedded declaration file is written.
    let path = decl_file_path(files, &item.file, true)?;
    let text = doc.decl_texts.get(&item.file)?;
    let lines = LineMap::new(text);
    Some(Location::new(path_to_uri(&path)?, lines.range(text, range.unwrap_or(item.range))))
}

/// The export an import binding takes its name from, in the exporting
/// file; the start of that file for `import from … as Name`.
fn import_target(files: &Files, doc: &Doc, id: BindingId) -> Option<Location> {
    let b = doc.res.binding(id)?;
    let (node, path) = doc.import_of(id)?;
    let (file, text, res) = resolve_import(files, doc.from_name().as_deref(), &path)?;
    let uri = path_to_uri(&file)?;
    if has_token(&node, AS_KW) {
        return Some(Location::new(uri, Range::default()));
    }
    let (bid, _) = res.export_bindings().find(|(_, e)| e.name == b.name)?;
    let target = res.binding(bid)?;
    let lines = LineMap::new(&text);
    Some(Location::new(uri, lines.range(&text, target.range)))
}
