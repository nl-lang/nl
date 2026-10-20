//! Completion. After `.` on a receiver whose
//! type the analyzer knows: the members of that type, from the
//! declaration files for a declared module, struct or type
//! and a prelude library, and from the file for its own structs, enums
//! and modules. At an identifier: the bindings in scope, the names of the
//! declaration files and the prelude.
//!
//! A file being typed has no name after the dot yet, so the text is
//! analyzed with a placeholder name put at the cursor, which gives the
//! parser a well-formed member expression to type.

use std::collections::HashMap;

use lsp_types::{CompletionItem, CompletionItemKind};
use nl_sema::analyzer::TypeInfo;
use nl_sema::decl::DeclItemKind;
use nl_sema::types::{Member, NamedId, NamedKind, Type};
use nl_sema::util::{expr_child, qual_segments};
use nl_sema::{Binding, BindingId, DeclKind, PreludeClass, Ref};
use nl_syntax::SyntaxKind::{self, *};
use nl_syntax::TextSize;
use rowan::NodeOrToken;

use crate::analysis::{Doc, analyze, imported_doc, token_at};
use crate::files::Files;

pub fn complete(files: &Files, doc: &Doc, offset: TextSize) -> Vec<CompletionItem> {
    let cursor = usize::from(offset).min(doc.text.len());
    let bytes = doc.text.as_bytes();
    let mut start = cursor;
    while start > 0 && bytes.get(start - 1).is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_') {
        start -= 1;
    }
    // After `.` or `:` (a method call, `x:m()`): the members of the
    // receiver. A `.` or `:` that is not a member access (a Lua path, a
    // type annotation) falls back to the names in scope.
    if start > 0 && matches!(bytes.get(start - 1), Some(b'.' | b':')) {
        if let Some(items) = member_completion(files, doc, start, cursor) {
            return items;
        }
    }
    scope_completion(doc, offset)
}

/// The placeholder name put after a `.` nothing follows yet.
const PLACEHOLDER: &str = "zz";

fn member_completion(files: &Files, doc: &Doc, start: usize, cursor: usize) -> Option<Vec<CompletionItem>> {
    let text = if start == cursor {
        let (before, after) = (doc.text.get(..cursor).unwrap_or(""), doc.text.get(cursor..).unwrap_or(""));
        format!("{before}{PLACEHOLDER}{after}")
    } else {
        doc.text.clone()
    };
    let d2 = analyze(files, &doc.uri, doc.path.as_deref(), &text);
    let info = d2.info.as_ref()?;
    let tok = token_at(&d2.root, TextSize::new(u32::try_from(start).unwrap_or(u32::MAX)))?;
    let parent = tok.parent()?;
    let ty = match parent.kind() {
        FIELD_EXPR | METHOD_CALL => {
            let recv = expr_child(&parent)?;
            if let Some(import) = d2.as_import(&recv) {
                return Some(export_items(files, &d2, import));
            }
            d2.receiver_type(&recv)?
        }
        QUAL_NAME => {
            // The segments before this one, as a static value.
            let segments = qual_segments(&parent);
            if segments.first().is_some_and(|f| f.text_range() == tok.text_range()) {
                return None;
            }
            let r = d2.res.type_ref(parent.text_range()).or_else(|| d2.res.reference(parent.text_range()));
            let mut cur = match r {
                Some(Ref::Binding(id)) => info.named_of.get(id).copied(),
                Some(Ref::Declared(id)) => d2.nid_of_decl(*id),
                Some(Ref::Prelude(p)) => info.prelude_named.get(p.name).copied(),
                _ => None,
            };
            for seg in segments.iter().skip(1) {
                if seg.text_range() == tok.text_range() {
                    break;
                }
                cur = cur.and_then(|c| match info.types.kind(c) {
                    Some(NamedKind::Module { members }) => match members.get(seg.text()) {
                        Some(Member::Value(Type::Static(inner))) => Some(*inner),
                        _ => None,
                    },
                    _ => None,
                });
            }
            Type::Static(cur?)
        }
        _ => return None,
    };
    Some(members_of_type(info, &ty))
}

/// The exports of the file an `import from … as Name` binding names, with
/// the types that file's analysis gives them.
fn export_items(files: &Files, doc: &Doc, import: BindingId) -> Vec<CompletionItem> {
    let Some(other) = imported_doc(files, doc, import) else { return Vec::new() };
    let mut out = Vec::new();
    for (id, e) in other.res.export_bindings() {
        let kind = decl_kind_item(e.decl);
        let detail = other.info.as_ref().and_then(|i| i.binding_types.get(&id).map(|t| i.types.show(t)));
        out.push(item(&e.name, kind, detail));
    }
    out.sort_by(|a, b| a.label.cmp(&b.label));
    dedup(out)
}

fn decl_kind_item(decl: DeclKind) -> CompletionItemKind {
    match decl {
        DeclKind::Fn => CompletionItemKind::FUNCTION,
        DeclKind::Struct => CompletionItemKind::STRUCT,
        DeclKind::Enum => CompletionItemKind::ENUM,
        DeclKind::Interface => CompletionItemKind::INTERFACE,
        DeclKind::TypeAlias => CompletionItemKind::CLASS,
        DeclKind::Module | DeclKind::Import => CompletionItemKind::MODULE,
        DeclKind::Const => CompletionItemKind::CONSTANT,
        DeclKind::TypeParam => CompletionItemKind::TYPE_PARAMETER,
        _ => CompletionItemKind::VARIABLE,
    }
}

fn item(label: &str, kind: CompletionItemKind, detail: Option<String>) -> CompletionItem {
    CompletionItem { label: label.to_string(), kind: Some(kind), detail, ..CompletionItem::default() }
}

/// The members of a type, as completion items with their types as detail.
pub fn members_of_type(info: &TypeInfo, ty: &Type) -> Vec<CompletionItem> {
    let types = &info.types;
    let mut out: Vec<CompletionItem> = Vec::new();
    match types.expand(ty) {
        Type::Static(nid) => static_members(info, nid, &mut out),
        Type::Named(nid) => instance_members(info, nid, &mut out),
        // A table typed as one has the functions of `Table` as methods.
        Type::Number | Type::String | Type::Bool | Type::Table | Type::Map(..) | Type::Array(_) => {
            let key = match types.expand(ty) {
                Type::Number => "Number",
                Type::String => "String",
                Type::Bool => "Bool",
                _ => "Table",
            };
            if let Some(ms) = info.prim_methods.get(key) {
                for (name, sig) in ms {
                    out.push(item(name, CompletionItemKind::METHOD, Some(types.show(&Type::Fn(sig.clone())))));
                }
            }
            if let Some(&lib) = info.prelude_named.get(key) {
                static_members(info, lib, &mut out);
            }
        }
        Type::Regex => {
            if let Some(&lib) = info.prelude_named.get("Regex") {
                static_members(info, lib, &mut out);
            }
        }
        Type::Task(r) => {
            out.push(item("await", CompletionItemKind::METHOD, Some(types.show(&r))));
            out.push(item("on_done", CompletionItemKind::METHOD, Some(format!("Function(callback: Function): Task<{}>", types.show(&r)))));
            out.push(item("on_error", CompletionItemKind::METHOD, Some(format!("Function(callback: Function): Task<{}>", types.show(&r)))));
        }
        Type::Record(fields) => {
            for (k, v) in fields.iter() {
                out.push(item(k, CompletionItemKind::FIELD, Some(types.show(v))));
            }
        }
        Type::Union(members) | Type::And(members) => {
            for m in members.iter() {
                out.extend(members_of_type(info, m));
            }
        }
        _ => {}
    }
    dedup(out)
}

/// Keeps the first item of each label.
fn dedup(items: Vec<CompletionItem>) -> Vec<CompletionItem> {
    let mut seen = std::collections::HashSet::new();
    items.into_iter().filter(|i| seen.insert(i.label.clone())).collect()
}

fn module_members(info: &TypeInfo, members: &HashMap<String, Member>, out: &mut Vec<CompletionItem>) {
    let types = &info.types;
    let mut names: Vec<&String> = members.keys().collect();
    names.sort();
    for name in names {
        let Some(m) = members.get(name) else { continue };
        let kind = match m {
            Member::Function(f) if f.is_method => CompletionItemKind::METHOD,
            Member::Function(_) => CompletionItemKind::FUNCTION,
            Member::Value(Type::Static(_)) => CompletionItemKind::MODULE,
            Member::Value(_) => CompletionItemKind::FIELD,
            Member::Const(_) => CompletionItemKind::CONSTANT,
        };
        out.push(item(name, kind, Some(types.show(&m.ty()))));
    }
}

/// The members of the table that stands for a named thing: a struct's
/// methods, an enum's variants, a module's members.
fn static_members(info: &TypeInfo, nid: NamedId, out: &mut Vec<CompletionItem>) {
    let types = &info.types;
    let mut cur = Some(nid);
    for _ in 0..64 {
        let Some(c) = cur else { break };
        match types.kind(c) {
            Some(NamedKind::Struct { methods, .. }) => {
                let mut names: Vec<&String> = methods.keys().collect();
                names.sort();
                for n in names {
                    if let Some(m) = methods.get(n) {
                        out.push(item(n, CompletionItemKind::METHOD, Some(types.show(&Type::Fn(m.clone())))));
                    }
                }
            }
            Some(NamedKind::Enum { variants }) => {
                let name = types.name(c).to_string();
                for (v, _) in variants {
                    out.push(item(v, CompletionItemKind::ENUM_MEMBER, Some(name.clone())));
                }
                out.push(item("values", CompletionItemKind::FIELD, Some(format!("[{name}]"))));
                out.push(item("name", CompletionItemKind::FUNCTION, Some("Function(value: any): String?".to_string())));
            }
            Some(NamedKind::Module { members } | NamedKind::HostType { members, .. } | NamedKind::LuaClass { members, .. }) => module_members(info, members, out),
            _ => {}
        }
        cur = types.parent(c);
    }
}

/// The members of a value of a nominal type: fields and methods.
fn instance_members(info: &TypeInfo, nid: NamedId, out: &mut Vec<CompletionItem>) {
    let types = &info.types;
    for f in types.fields(nid) {
        out.push(item(&f.name, CompletionItemKind::FIELD, Some(types.show(&f.ty))));
    }
    let mut cur = Some(nid);
    for _ in 0..64 {
        let Some(c) = cur else { break };
        match types.kind(c) {
            Some(NamedKind::Struct { methods, .. } | NamedKind::Interface { methods, .. }) => {
                let mut names: Vec<&String> = methods.keys().collect();
                names.sort();
                for n in names {
                    if let Some(m) = methods.get(n) {
                        out.push(item(n, CompletionItemKind::METHOD, Some(types.show(&Type::Fn(m.clone())))));
                    }
                }
                if let Some(NamedKind::Interface { fields, .. }) = types.kind(c) {
                    for f in fields {
                        out.push(item(&f.name, CompletionItemKind::FIELD, Some(types.show(&f.ty))));
                    }
                }
            }
            Some(NamedKind::HostType { members, .. } | NamedKind::LuaClass { members, .. }) => module_members(info, members, out),
            _ => {}
        }
        cur = types.parent(c);
    }
}

/// Nodes that open a scope for the bindings declared in them.
fn is_scope(k: SyntaxKind) -> bool {
    matches!(
        k,
        BLOCK | SOURCE_FILE | FN_DECL | FN_EXPR | LAMBDA_EXPR | TRAILING_BLOCK | ON_EXPR | SPAWN_EXPR | FOR_STMT | WHEN_CLAUSE | CATCH_CLAUSE | IF_EXPR | UNLESS_EXPR | ELSEIF_CLAUSE | WHILE_STMT | UNTIL_STMT | LOOP_STMT | STRUCT_DECL | INTERFACE_DECL | INTERFACE_IMPL | ENUM_DECL | MODULE_DECL | TYPE_DECL | COND_CLAUSE | CASE_EXPR | DO_BLOCK | TRY_EXPR
    )
}

/// Whether a binding is visible at `offset`: declared in a scope around
/// it, and before it unless hoisted. An approximation
/// from the tree, since the resolver keeps no scope table.
fn visible(doc: &Doc, b: &Binding, offset: TextSize) -> bool {
    let hoisted = matches!(b.decl, DeclKind::Fn | DeclKind::Struct | DeclKind::Enum | DeclKind::Interface | DeclKind::TypeAlias | DeclKind::Module | DeclKind::Import);
    if b.decl == DeclKind::SelfParam {
        return b.range.contains_inclusive(offset);
    }
    let node = match doc.root.covering_element(b.range) {
        NodeOrToken::Node(n) => n,
        NodeOrToken::Token(t) => match t.parent() {
            Some(p) => p,
            None => return false,
        },
    };
    let scope = if hoisted { node.ancestors().find(|a| matches!(a.kind(), BLOCK | SOURCE_FILE)) } else { node.ancestors().find(|a| is_scope(a.kind())) };
    let Some(scope) = scope else { return false };
    scope.text_range().contains_inclusive(offset) && (hoisted || b.range.end() <= offset)
}

fn binding_kind(doc: &Doc, id: BindingId, b: &Binding) -> CompletionItemKind {
    decl_kind_item(doc.res.declared_as(id).unwrap_or(b.decl))
}

fn scope_completion(doc: &Doc, offset: TextSize) -> Vec<CompletionItem> {
    let info = doc.info.as_ref();
    let mut out: Vec<CompletionItem> = Vec::new();
    // The file's bindings: an inner one shadows an outer one of its name.
    let mut by_name: HashMap<String, CompletionItem> = HashMap::new();
    for (i, b) in doc.res.bindings.iter().enumerate() {
        if b.name.is_empty() || b.name.contains('.') || !visible(doc, b, offset) {
            continue;
        }
        let id = BindingId(u32::try_from(i).unwrap_or(u32::MAX));
        let detail = info.and_then(|i| i.binding_types.get(&id).map(|t| i.types.show(t)));
        by_name.insert(b.name.clone(), item(&b.name, binding_kind(doc, id, b), detail));
    }
    let mut names: Vec<String> = by_name.keys().cloned().collect();
    names.sort();
    for n in names {
        if let Some(i) = by_name.remove(&n) {
            out.push(i);
        }
    }
    // Declaration files.
    for d in &doc.decls.items {
        let kind = match d.kind {
            DeclItemKind::Module => CompletionItemKind::MODULE,
            DeclItemKind::Struct | DeclItemKind::HostType => CompletionItemKind::CLASS,
            DeclItemKind::Fn => CompletionItemKind::FUNCTION,
            DeclItemKind::Let => CompletionItemKind::VARIABLE,
            DeclItemKind::Const => CompletionItemKind::CONSTANT,
        };
        let detail = doc.decl_texts.get(&d.file).and_then(|t| t.get(usize::from(nl_sema::util::trimmed_range(&d.node).start())..)).and_then(|t| t.lines().next()).map(|l| l.trim().to_string());
        out.push(item(&d.name, kind, detail));
    }
    // The prelude.
    for p in nl_sema::prelude::all() {
        let kind = match p.class {
            PreludeClass::Value => match doc.decls.lookup(p.name).and_then(|id| doc.decls.get(id)).map(|d| d.kind) {
                Some(DeclItemKind::Fn) => CompletionItemKind::FUNCTION,
                _ => CompletionItemKind::MODULE,
            },
            PreludeClass::Type => CompletionItemKind::CLASS,
            PreludeClass::ValueAndType => CompletionItemKind::MODULE,
            PreludeClass::Struct => CompletionItemKind::STRUCT,
            PreludeClass::Interface => CompletionItemKind::INTERFACE,
        };
        out.push(item(p.name, kind, Some("prelude".to_string())));
    }
    dedup(out)
}
