//! Lints that need the whole file: unused bindings, and the
//! top-level statements a worker would repeat. The lints that are
//! found along the walk (a global read, a redundant `!`, a function-kind
//! name passed without `!`, shadowing in the resolver) live where they
//! are found.

use nl_syntax::{SyntaxKind, SyntaxNode};

use super::Analyzer;
use crate::binding::*;
use crate::util::*;

use SyntaxKind::*;

impl Analyzer<'_> {
    pub(crate) fn lints(&mut self, root: &SyntaxNode) {
        self.unused_bindings(root);
    }

    /// A `let`, `const`, loop or pattern variable that nothing reads.
    /// Parameters are the shape of a function and a `catch` variable is
    /// often there for its type alone: neither is reported; a name
    /// starting with `_` is meant to be unused.
    fn unused_bindings(&mut self, root: &SyntaxNode) {
        let exported: std::collections::HashSet<BindingId> = self.res.export_bindings().map(|(id, _)| id).collect();
        let mut referenced: std::collections::HashSet<BindingId> = self.used.clone();
        for n in root.descendants() {
            match n.kind() {
                NAME_REF | SELF_EXPR | AT_EXPR => {
                    if let Some(Ref::Binding(id)) = self.res.reference(n.text_range()) {
                        referenced.insert(*id);
                    }
                }
                LUA_PATH => {
                    if let Some(tok) = crate::resolver::lua_path_root(&n) {
                        if let Some(Ref::Binding(id)) = self.res.reference(tok.text_range()) {
                            referenced.insert(*id);
                        }
                    }
                }
                QUAL_NAME => {
                    if let Some(Ref::Binding(id)) = self.res.type_ref(n.text_range()) {
                        referenced.insert(*id);
                    }
                }
                _ => {}
            }
        }
        for (i, b) in self.res.bindings.iter().enumerate() {
            let id = BindingId(u32::try_from(i).unwrap_or(u32::MAX));
            if referenced.contains(&id) || exported.contains(&id) || b.name.starts_with('_') {
                continue;
            }
            let what = match b.decl {
                DeclKind::Let => "variable",
                DeclKind::Const => "constant",
                DeclKind::LoopVar => "loop variable",
                DeclKind::PatternVar => "pattern variable",
                _ => continue,
            };
            // A module `const` is a member read as `M.name`.
            if b.decl == DeclKind::Const && self.res.module_const_of(id) {
                continue;
            }
            let name = b.name.clone();
            let range = b.range;
            if self.pass == 2 {
                self.diags.push(SemaDiagnostic { range, code: "unused-binding", message: format!("the {what} `{name}` is never used; prefix it with `_` if that is intended"), error: false, lint: true });
            }
        }
    }

    /// Finds the top-level statements a worker would repeat when it loads
    /// this file: expression statements, assignments and
    /// control flow at file level, except handlers and a statement
    /// guarded by `unless Worker.parent`.
    pub(crate) fn find_top_level_effects(&mut self, root: &SyntaxNode) {
        let Some(b) = block(root) else { return };
        for s in statements(&b) {
            let effect = match s.kind() {
                EXPR_STMT => {
                    let guarded = modifier(&s).is_some_and(|m| first_token(&m).is_some_and(|t| t.kind() == UNLESS_KW) && expr_child(&m).is_some_and(|e| is_worker_parent(&e)));
                    let handler = expr_child(&s).is_some_and(|e| e.kind() == ON_EXPR);
                    !guarded && !handler
                }
                ASSIGN_STMT | COMPOUND_ASSIGN_STMT | MULTI_ASSIGN_STMT | WHILE_STMT | UNTIL_STMT | LOOP_STMT | FOR_STMT => true,
                _ => false,
            };
            if effect {
                self.top_level_effects = Some(s.text_range());
                return;
            }
        }
    }
}

/// `Worker.parent`.
fn is_worker_parent(e: &SyntaxNode) -> bool {
    e.kind() == FIELD_EXPR
        && field_name(e).is_some_and(|t| t.text() == "parent")
        && expr_child(e).is_some_and(|r| r.kind() == NAME_REF && name_text(&r).as_deref() == Some("Worker"))
}
