//! Hover: for a binding, a reference, a member
//! or a declared name, what declares it and its type as the analyzer
//! knows it, rendered as the analyzer renders types in its
//! messages (`TypeTable::show`), followed by the doc comment of the
//! declaration when it has one.

use lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind};
use nl_sema::analyzer::TypeInfo;
use nl_sema::types::{FnType, Member, NamedId, NamedKind, Type, TypeTable};
use nl_sema::{BindingId, DeclId, DeclKind, Prelude};
use nl_syntax::{SyntaxNode, TextSize, doc_comment};

use crate::analysis::{Doc, Symbol, export_target};
use crate::files::Files;

pub fn hover(files: &Files, doc: &Doc, offset: TextSize) -> Option<Hover> {
    let (sym, tok) = doc.symbol_at(offset)?;
    let (header, docs) = describe(files, doc, &sym)?;
    let mut value = format!("```nl\n{header}\n```");
    if let Some(d) = docs {
        value.push_str("\n\n");
        value.push_str(&d);
    }
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent { kind: MarkupKind::Markdown, value }),
        range: Some(doc.lines.range(&doc.text, tok.text_range())),
    })
}

/// A header line in NL syntax and the doc comment, for a symbol.
pub fn describe(files: &Files, doc: &Doc, sym: &Symbol) -> Option<(String, Option<String>)> {
    match sym {
        Symbol::Binding(id) => binding(doc, *id),
        Symbol::Declared(id) => declared(doc, *id),
        Symbol::Member { owner, name } => member(doc, *owner, name),
        Symbol::Export { import, name } => {
            // The export as the exporting file declares it.
            let (other, id) = export_target(files, doc, *import, name)?;
            binding(&other, id)
        }
        Symbol::Prelude(p) => prelude(doc, p),
    }
}

fn binding(doc: &Doc, id: BindingId) -> Option<(String, Option<String>)> {
    let b = doc.res.binding(id)?;
    let info = doc.info.as_ref();
    let ty = info.and_then(|i| i.binding_types.get(&id));
    let types = info.map(|i| &i.types);
    let show = |t: &Type| types.map_or_else(|| "any".to_string(), |tt| tt.show(t));
    let typed = |word: &str, name: &str| match ty {
        Some(t) => format!("{word} {name}: {}", show(t)),
        None => format!("{word} {name}"),
    };
    let name = b.name.as_str();
    let mut docs = doc.declaration_node(b.range).and_then(|n| doc_comment(&n));
    let header = match b.decl {
        DeclKind::Let => typed("let", name),
        DeclKind::Const => typed("const", name),
        DeclKind::Global => typed("global", name),
        DeclKind::Param => typed("param", name),
        DeclKind::RestParam => typed("param", &format!("*{name}")),
        DeclKind::KeyedParam => typed("param", &format!("**{name}")),
        DeclKind::SelfParam => typed("", name).trim_start().to_string(),
        DeclKind::Fn => match (ty, types) {
            (Some(Type::Fn(sig)), Some(types)) => render_fn(types, name, sig),
            _ => format!("fn {name}"),
        },
        DeclKind::LoopVar => typed("for", name),
        DeclKind::PatternVar => typed("let", name),
        DeclKind::CatchVar => typed("catch", name),
        DeclKind::Struct => format!("struct {name}"),
        DeclKind::Enum => format!("enum {name}"),
        DeclKind::Interface => format!("interface {name}"),
        DeclKind::TypeAlias => {
            let target = info.and_then(|i| i.named_of.get(&id)).and_then(|nid| info.and_then(|i| i.types.kind(*nid))).and_then(|k| match k {
                NamedKind::Alias(t) => Some(show(t)),
                _ => None,
            });
            match target {
                Some(t) => format!("type {name} = {t}"),
                None => format!("type {name}"),
            }
        }
        DeclKind::Module => format!("module {name}"),
        DeclKind::TypeParam => format!("type parameter {name}"),
        DeclKind::Import => {
            let what = match doc.res.declared_as(id) {
                Some(DeclKind::Fn) => "a function",
                Some(DeclKind::Const) => "a constant",
                Some(DeclKind::Struct) => "a struct",
                Some(DeclKind::Enum) => "an enum",
                Some(DeclKind::Interface) => "an interface",
                Some(DeclKind::TypeAlias) => "a type",
                Some(DeclKind::Module) => "a module",
                _ => "the export table",
            };
            if let Some((_, path)) = doc.import_of(id) {
                docs = Some(format!("{what} imported from '{path}'"));
            }
            format!("import {name}")
        }
    };
    Some((header, docs))
}

/// `fn name(a: Number, b: Number = …, *args: [T], **opts): Ret`.
pub fn render_fn(types: &TypeTable, name: &str, sig: &FnType) -> String {
    let mut s = String::new();
    if sig.is_async {
        s.push_str("async ");
    }
    s.push_str("fn ");
    s.push_str(name);
    s.push('(');
    let mut parts = Vec::new();
    for p in &sig.params {
        let mut t = if p.name.is_empty() { types.show(&p.ty) } else { format!("{}: {}", p.name, types.show(&p.ty)) };
        if p.optional {
            t.push_str(" = …");
        }
        parts.push(t);
    }
    if let Some(r) = &sig.rest {
        parts.push(format!("*args: {}", types.show(r)));
    }
    if let Some(k) = &sig.keyed {
        parts.push(match k {
            Some(t) => format!("**opts: {}", types.show(t)),
            None => "**opts".to_string(),
        });
    }
    s.push_str(&parts.join(", "));
    s.push(')');
    if sig.ret != Type::Nil {
        s.push_str(": ");
        s.push_str(&types.show(&sig.ret));
    }
    s
}

/// The first line of a declaration, as written in its file.
fn header_line(text: &str, node: &SyntaxNode) -> Option<String> {
    let start = usize::from(nl_sema::util::trimmed_range(node).start());
    let line = text.get(start..)?.lines().next()?;
    Some(line.trim().to_string())
}

fn declared(doc: &Doc, id: DeclId) -> Option<(String, Option<String>)> {
    let item = doc.decls.get(id)?;
    let text = doc.decl_texts.get(&item.file)?;
    let header = header_line(text, &item.node)?;
    Some((header, doc_comment(&item.node)))
}

fn member(doc: &Doc, owner: NamedId, name: &str) -> Option<(String, Option<String>)> {
    let info = doc.info.as_ref()?;
    let header = member_header(info, owner, name)?;
    let docs = doc
        .own_member_decl(owner, name)
        .and_then(|r| doc.declaration_node(r))
        .and_then(|n| doc_comment(&n))
        .or_else(|| doc.decl_member_node(owner, name).and_then(|(_, m)| doc_comment(&m)));
    Some((header, docs))
}

/// `Number`, `String` or `Bool` for the library table of a primitive.
fn prim_key(name: &str) -> Option<&'static str> {
    match name {
        "Number" => Some("Number"),
        "String" => Some("String"),
        "Bool" => Some("Bool"),
        _ => None,
    }
}

/// The header of a member of `owner` or one of its parents.
pub fn member_header(info: &TypeInfo, owner: NamedId, name: &str) -> Option<String> {
    let types = &info.types;
    let mut cur = Some(owner);
    for _ in 0..64 {
        let c = cur?;
        let cname = types.name(c).to_string();
        let full = format!("{cname}.{name}");
        if let Some(sig) = prim_key(&cname).and_then(|k| info.prim_methods.get(k)).and_then(|ms| ms.get(name)) {
            return Some(render_fn(types, &full, sig));
        }
        match types.kind(c)? {
            NamedKind::Struct { fields, methods, .. } => {
                if let Some(f) = fields.iter().find(|f| f.name == name) {
                    return Some(format!("{full}: {}", types.show(&f.ty)));
                }
                if let Some(m) = methods.get(name) {
                    return Some(render_fn(types, &full, m));
                }
            }
            NamedKind::Interface { methods, fields, .. } => {
                if let Some(f) = fields.iter().find(|f| f.name == name) {
                    return Some(format!("{full}: {}", types.show(&f.ty)));
                }
                if let Some(m) = methods.get(name) {
                    return Some(render_fn(types, &full, m));
                }
            }
            NamedKind::Enum { variants } => {
                if variants.iter().any(|(v, _)| v == name) {
                    return Some(format!("{full}: {cname}"));
                }
                match name {
                    "values" => return Some(format!("{full}: [{cname}]")),
                    "name" => return Some(format!("fn {full}(value: any): String?")),
                    _ => {}
                }
            }
            NamedKind::Module { members } | NamedKind::HostType { members, .. } | NamedKind::LuaClass { members, .. } => {
                if let NamedKind::LuaClass { fields, .. } = types.kind(c)? {
                    if let Some(f) = fields.iter().find(|f| f.name == name) {
                        return Some(format!("{full}: {}", types.show(&f.ty)));
                    }
                }
                match members.get(name) {
                    Some(Member::Function(f)) => return Some(render_fn(types, &full, f)),
                    Some(Member::Value(t)) => return Some(format!("{full}: {}", types.show(t))),
                    Some(Member::Const(t)) => return Some(format!("const {full}: {}", types.show(t))),
                    None => {}
                }
            }
            _ => {}
        }
        cur = types.parent(c);
    }
    None
}

fn prelude(doc: &Doc, p: &Prelude) -> Option<(String, Option<String>)> {
    if let Some(id) = doc.decls.lookup(p.name) {
        if let Some(d) = declared(doc, id) {
            return Some(d);
        }
    }
    let what = match p.class {
        nl_sema::PreludeClass::Value => "prelude value",
        nl_sema::PreludeClass::Type => "prelude type",
        nl_sema::PreludeClass::ValueAndType => "prelude library",
        nl_sema::PreludeClass::Struct => "prelude struct",
        nl_sema::PreludeClass::Interface => "prelude interface",
    };
    Some((format!("{what} {}", p.name), None))
}
