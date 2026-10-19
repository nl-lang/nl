//! `import` and the export table a file's main function
//! returns.
//!
//! Each `.nl` file is a chunk returning a table of its `pub` items. An
//! import becomes one call of `rt.import_from(from, path, names...)`
//! (runtime, `nl_70_module.lua`), which resolves `path` as seen from the
//! importing file `from` (the chunk name without its `@`, or nil for a
//! chunk that is not a file), compiles and runs the file once per state,
//! and returns its export table, or the named fields of it:
//!
//! ```lua
//! local square, answer = __rt.import_from("main.nl", "import/lib", "square", "answer")
//! local Lib = __rt.import_from("main.nl", "import/lib")
//! ```
//!
//! The resolver has already checked that the file exists and exports the
//! names (through the driver's `ImportProvider`), and given each name the
//! binding kind it has in the exporting file.

use nl_sema::util::*;
use nl_sema::{BindingId, DeclKind};
use nl_syntax::{SyntaxKind, SyntaxNode};

use super::Lowerer;
use crate::ir::*;

use SyntaxKind::*;

impl Lowerer<'_> {
    /// `import name from 'path'`, `import { names } from 'path'`,
    /// `import from 'path' as Name`.
    pub(crate) fn lower_import(&mut self, node: &SyntaxNode) {
        let Some(path_tok) = token(node, STRING) else {
            self.internal(node, "import without a path");
            return;
        };
        let path = nl_sema::decode_plain_string(&path_tok);
        let from = match self.opts.chunk_name.strip_prefix('@') {
            Some(name) => Expr::str(name),
            None => Expr::Nil,
        };
        let mut args = vec![from, Expr::str(&path)];

        // The bound names: the alias after `as`, or the imported names.
        let whole = has_token(node, AS_KW);
        let names: Vec<SyntaxNode> = match child(node, IMPORT_LIST) {
            Some(list) => children(&list, NAME).collect(),
            None => children(node, NAME).collect(),
        };
        let mut ids = Vec::new();
        for n in &names {
            let Some(id) = self.res.declaration(n.text_range()) else {
                self.internal(n, "unresolved import name");
                return;
            };
            if !whole {
                let text = self.res.binding(id).map_or_else(|| name_text(n).unwrap_or_default(), |b| b.name.clone());
                args.push(Expr::str(&text));
            }
            ids.push(id);
        }
        let call = Expr::intrinsic_call("import_from", args);
        if ids.is_empty() {
            // Not reached for a file the resolver accepted (every import
            // names what it binds); run the import for its effect.
            self.emit_effect(call);
            return;
        }

        // Hoisting declares every name of a block or none of them.
        if ids.iter().all(|id| self.is_predeclared(*id)) {
            let targets = ids.iter().map(|id| Place::Local(self.local_of(*id))).collect();
            self.emit(StmtKind::Assign { targets, values: vec![call] });
        } else if ids.iter().all(|id| !self.is_predeclared(*id)) {
            let locals = ids.iter().map(|id| self.local_of(*id)).collect();
            self.emit(StmtKind::Local { locals, values: vec![call] });
        } else {
            let temps: Vec<LocalId> = ids.iter().map(|_| self.new_local("import")).collect();
            self.emit(StmtKind::Local { locals: temps.clone(), values: vec![call] });
            for (id, t) in ids.iter().zip(temps) {
                self.define(*id, Some(Expr::Local(t)));
            }
        }
    }

    /// Ends the main function with `return { pub items }`. The items, their
    /// names and kinds are the resolver's (`Resolution::exports`), the same
    /// list the driver hands to the files that import this one.
    pub(crate) fn emit_exports(&mut self) {
        let exports: Vec<(BindingId, String, DeclKind)> = self.res.export_bindings().map(|(id, e)| (id, e.name.clone(), e.decl)).collect();
        let mut entries = Vec::new();
        for (id, name, decl) in exports {
            let value = if decl == DeclKind::TypeAlias {
                // A `type` alias has no runtime value of its own; the
                // export table holds the type value it stands for, so that
                // an importing file can check against it (imported
                // names keep their kinds). A type with no runtime
                // check exports `any`.
                let value = match self.aliases.get(&id).cloned() {
                    Some(ty) => self.type_value(&ty),
                    None => None,
                };
                value.unwrap_or_else(|| self.lua_type("Any"))
            } else {
                Expr::Local(self.local_of(id))
            };
            entries.push(TableEntry::Pair(Expr::str(&name), value));
        }
        self.emit(StmtKind::Return(vec![Expr::Table(entries)]));
    }
}
