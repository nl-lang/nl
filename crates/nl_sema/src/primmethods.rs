//! The methods and operators a file defines on the primitive types, as the
//! files that import it learn them.
//!
//! `fn String.+`, `fn Number.is_cool`, a `fn Table.name` and the functions
//! of `interface I for String` are not exports: the runtime installs them
//! in the type's library, once per Lua state, when the defining file runs,
//! and from then on they are there for every file. A file that imports the
//! defining one, directly or through other imports, runs after it, so its
//! analyzer takes them as known ([`crate::ImportProvider::primitive_methods`]):
//! without them `"a" + "b"` would be an error there (`bad-operand`), and
//! the same line in the defining file is not.
//!
//! What a file defines is read off its syntax tree ([`scan_file`]), so the
//! files of a chain of imports are parsed and nothing more:
//! [`primitive_methods_through`] follows the imports of each file, every
//! file once. Only a return type that is a primitive itself is kept; the
//! parameters and other types name things of the defining file that the
//! importer cannot see, so the importer checks no arguments.

use std::collections::HashSet;

use nl_syntax::{SyntaxKind, SyntaxNode};

use crate::util::*;

use SyntaxKind::*;

/// One method or operator defined on a primitive type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PrimMethod {
    /// `Number`, `String`, `Bool` or `Table`.
    pub owner: &'static str,
    /// The method's name, or the operator (`+`, `<>`, `[]`).
    pub name: String,
    /// The declared return type when it is `Number`, `String` or `Bool`.
    pub ret: Option<&'static str>,
}

/// What [`scan_file`] finds in one file.
#[derive(Clone, Debug, Default)]
pub struct FileScan {
    pub methods: Vec<PrimMethod>,
    /// The paths of its imports, as written.
    pub imports: Vec<String>,
}

fn primitive(name: &str) -> Option<&'static str> {
    ["Number", "String", "Bool", "Table"].into_iter().find(|p| *p == name)
}

/// The primitive methods a parsed file defines, and the paths it imports.
pub fn scan_file(root: &SyntaxNode) -> FileScan {
    let mut out = FileScan::default();
    for n in root.descendants() {
        match n.kind() {
            IMPORT_DECL => {
                if let Some(t) = token(&n, STRING) {
                    out.imports.push(crate::resolver::decode_plain_string(&t));
                }
            }
            FN_DECL if n.parent().is_none_or(|p| p.kind() != INTERFACE_IMPL) => {
                let Some(q) = child(&n, QUAL_NAME) else { continue };
                let segs = qual_segments(&q);
                let [seg] = segs.as_slice() else { continue };
                let Some(owner) = primitive(seg.text()) else { continue };
                out.methods.extend(method(&n, owner));
            }
            INTERFACE_IMPL => {
                let target = n.children().filter(|c| is_type(c.kind())).nth(1);
                let Some(owner) = target.filter(|t| t.kind() == NAMED_TYPE).and_then(|t| primitive(t.text().to_string().trim())) else { continue };
                if owner == "Table" {
                    continue;
                }
                for f in children(&n, FN_DECL) {
                    out.methods.extend(method(&f, owner));
                }
            }
            _ => {}
        }
    }
    out
}

fn method(f: &SyntaxNode, owner: &'static str) -> Option<PrimMethod> {
    let name = match child(f, NAME) {
        Some(n) => name_text(&n)?,
        None => child(f, OP_NAME)?.text().to_string().split_whitespace().collect(),
    };
    let ret = child(f, RETURN_TYPE).and_then(|r| type_child(&r)).filter(|t| t.kind() == NAMED_TYPE).and_then(|t| primitive(t.text().to_string().trim())).filter(|p| *p != "Table");
    Some(PrimMethod { owner, name, ret })
}

/// The primitive methods defined by the file that the import `path` of
/// `from` names and by every file it imports, transitively. `resolve`
/// finds a file as `import` does (its canonical name); `scan` gives
/// [`scan_file`] of a canonical name, `None` when it cannot be read or
/// parsed. Each file is scanned once, so a cycle ends.
pub fn primitive_methods_through(from: Option<&str>, path: &str, resolve: &dyn Fn(Option<&str>, &str) -> Option<String>, scan: &dyn Fn(&str) -> Option<FileScan>) -> Vec<PrimMethod> {
    let mut out: Vec<PrimMethod> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut queue: Vec<String> = resolve(from, path).into_iter().collect();
    if let Some(f) = from {
        seen.insert(f.to_string());
    }
    while let Some(name) = queue.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        let Some(found) = scan(&name) else { continue };
        for m in found.methods {
            if !out.iter().any(|o| o.owner == m.owner && o.name == m.name) {
                out.push(m);
            }
        }
        for p in &found.imports {
            if let Some(next) = resolve(Some(&name), p) {
                queue.push(next);
            }
        }
    }
    out
}
