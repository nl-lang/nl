//! `module` declarations: a table, created or reopened; the
//! body is a scope whose `fn`s are methods of the module (their hidden
//! `self` is the module) and whose other declarations are private.

use nl_sema::util::*;
use nl_sema::Ref;
use nl_syntax::{SyntaxKind, SyntaxNode};

use super::{Lowerer, Tail};
use crate::ir::*;

use SyntaxKind::*;

impl Lowerer<'_> {
    pub(crate) fn lower_module(&mut self, node: &SyntaxNode) {
        let Some(q) = child(node, QUAL_NAME) else { return };
        let Some((id, reopen)) = self.res.module(node.text_range()) else {
            self.internal(node, "module");
            return;
        };
        let segs: Vec<String> = qual_segments(&q).iter().map(|t| t.text().to_string()).collect();
        let full = segs.join(".");
        // A module crosses by name, as a value.
        if segs.len() == 1 {
            if !reopen {
                let value = Expr::intrinsic_call("module", vec![Expr::str(&full)]);
                let value = self.register_type(id, node, value);
                self.define(id, Some(value));
            }
        } else {
            // `module A.B`: the table `A.B`, created when missing.
            let root = match self.res.type_ref(q.text_range()).cloned() {
                Some(Ref::Binding(r)) => Expr::Local(self.local_of(r)),
                _ => {
                    self.internal(node, "nested module root");
                    return;
                }
            };
            let path = segs.iter().skip(1).map(|s| TableEntry::Item(Expr::str(s))).collect();
            let value = Expr::intrinsic_call("submodule", vec![root, Expr::Table(path), Expr::str(&full)]);
            let value = if reopen { value } else { self.register_type(id, node, value) };
            self.define(id, Some(value));
        }
        if let Some(b) = block(node) {
            let body = self.lower_block(&b, Tail::Discard);
            self.emit(StmtKind::Do(body));
        }
    }
}
