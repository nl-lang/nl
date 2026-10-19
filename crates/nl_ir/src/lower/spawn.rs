//! `spawn`. On stock LuaJIT, the intrinsic is a stand-in that
//! runs the job in the same state on deep copies of its
//! arguments. A `spawn do` block becomes a hidden named
//! function whose parameters are the captured locals.
//!
//! Both forms become one call of `rt.spawn` (runtime/nl_64_worker.lua):
//!
//! ```lua
//! __rt.spawn(nil, f, args…)                 -- spawn f(args)
//! __rt.spawn(false, f, M, args…)            -- spawn f(args), f a function of module M called by its bare name
//! __rt.spawn("m", x, args…)                 -- spawn x.m(args), member-call rule
//! __rt.spawn({"file", "<spawn do at L:C>", "n", "list"}, function(n, list) … end, n, list)
//! ```
//!
//! In the block form, every binding the block reads that is a local of an
//! enclosing function (a parameter, a `let`, `self`, a function or struct
//! declared inside a function) is a parameter of the hidden function, and
//! its value at the `spawn` is the argument; the runtime copies it like
//! any argument. File-level declarations, named functions and `global`s
//! are read directly: in a worker they are that worker's own.
//! The hidden function is written where the `spawn` stands; lift.rs then
//! moves it into the main function, where it is made and registered under
//! its name when the file loads, so that a worker that loads
//! the file finds it by name.
//!
//! This module also emits the name registry:
//! [`Lowerer::register_named`] wraps a value in `rt.register(file, name,
//! v)`, for the named functions and the structs that are not declared
//! inside a function.
//!
//! A registered name identifies one declaration in its file.
//! What is declared at file level is registered under
//! its name (`f`, `Point`); what is declared in a module body, under the
//! path of the enclosing module bodies (`M.f`, `M.N.Point`, also for a
//! module nested in another one's body, which is private to it); a method,
//! under its owner's registry name (`M.Point.sum`, `String.shout`). A
//! declaration in any other scope (inside a function, or in a nested block
//! such as `if` or `do`, at file level or in a module body) has no name
//! unique in the file and is not registered, so it cannot be sent
//! (SendError). Nor is a declaration whose name is already registered for
//! a different place: a module nested in a module body (`module A …
//! module B` against `module A.B`), or a type, enum, interface or module
//! declared again under the same name, which is another declaration (a
//! private `struct S` in each body of a reopened module `A`: the first is
//! `A.S`, the second has no name and cannot be sent). A later declaration
//! of the same function or method (a reopened module, a method defined
//! again) replaces the earlier one, in the registry as in its table.

use std::collections::BTreeMap;

use nl_sema::util::*;
use nl_sema::{BindingId, DeclKind, Ref, SelfTarget};
use nl_syntax::{SyntaxKind, SyntaxNode};

use super::Lowerer;
use super::call::{Arg, arg_nodes};
use super::expr::is_len;
use super::func::FnSpec;
use crate::ir::*;

use SyntaxKind::*;

impl Lowerer<'_> {
    pub(crate) fn lower_spawn(&mut self, node: &SyntaxNode) -> Expr {
        if has_token(node, DO_KW) {
            return self.spawn_block(node);
        }
        let Some(call) = node.children().find(|c| matches!(c.kind(), CALL_EXPR | COMMAND_CALL)) else {
            return self.internal(node, "`spawn` without a call");
        };
        let Some(callee) = expr_child(&call) else { return self.internal(&call, "`spawn` call without a callee") };
        let args = arg_nodes(child(&call, ARG_LIST).or_else(|| child(&call, COMMAND_ARGS)));
        if let Some(Arg::Keyed(_, e) | Arg::KeyedSpread(e)) = args.iter().find(|a| matches!(a, Arg::Keyed(..) | Arg::KeyedSpread(_))) {
            self.error(e, "spawn-keyed", "`spawn` sends positional arguments only: keyed arguments (`key: value`) cannot travel to a worker");
            return Expr::Nil;
        }
        let mut vals = Vec::new();
        match callee.kind() {
            FIELD_EXPR if !is_len(&callee) => {
                let Some(recv) = expr_child(&callee) else { return self.internal(&callee, "`spawn` member callee") };
                let name = field_name(&callee).map(|t| t.text().to_string()).unwrap_or_default();
                vals.push(Expr::str(&name));
                self.push_operand(&mut vals, |l| l.expr1(&recv));
            }
            NAME_REF => {
                // A module function called by its bare name inside its
                // module gets the module as `self`; the runtime
                // labels it `self`, so that the written arguments are
                // numbered as in `spawn M.f(args)`.
                let module = match self.res.reference(callee.text_range()).cloned() {
                    Some(Ref::Binding(id)) => self.res.binding(id).and_then(|b| b.module_owner),
                    _ => None,
                };
                vals.push(if module.is_some() { Expr::Bool(false) } else { Expr::Nil });
                vals.push(self.name_raw(&callee));
                if let Some(m) = module {
                    vals.push(Expr::Local(self.local_of(m)));
                }
            }
            AT_EXPR => {
                vals.push(Expr::Nil);
                vals.push(self.at_expr(&callee));
            }
            _ => {
                vals.push(Expr::Nil);
                self.push_operand(&mut vals, |l| l.expr1(&callee));
            }
        }
        self.push_args(&mut vals, &args, None, 0);
        Expr::intrinsic_call("spawn", vals)
    }

    /// `spawn do … end`.
    fn spawn_block(&mut self, node: &SyntaxNode) -> Expr {
        let captures = self.spawn_captures(node);
        // The captured values, read here.
        let mut values = Vec::new();
        for (id, _) in &captures {
            values.push(Expr::Local(self.local_of(*id)));
        }
        // In the block, each captured binding is a parameter.
        let mut params = Vec::new();
        let mut saved = Vec::new();
        for (id, name) in &captures {
            let p = self.new_local(name);
            params.push(p);
            saved.push((*id, self.bindings.insert(*id, p)));
        }
        let pos = self.lines.line_col(self.source, trimmed_range(node).start());
        let hidden = format!("<spawn do at {}:{}>", pos.line, pos.col);
        let spec = FnSpec { name: hidden.clone(), self_binding: None, discard_tail: false };
        // The block runs in a worker as a function of its own: the type
        // pools of the enclosing function are not there (types.rs).
        let barrier = std::mem::replace(&mut self.pool_barrier, self.pools.len());
        self.spawn_depth += 1;
        let mut f = self.function(node, spec);
        self.spawn_depth -= 1;
        self.pool_barrier = barrier;
        for (id, old) in saved.into_iter().rev() {
            match old {
                Some(l) => {
                    self.bindings.insert(id, l);
                }
                None => {
                    self.bindings.remove(&id);
                }
            }
        }
        f.params = params;
        let mut info = vec![TableEntry::Item(Expr::str(&self.opts.file_label)), TableEntry::Item(Expr::str(&hidden))];
        info.extend(captures.iter().map(|(_, name)| TableEntry::Item(Expr::str(name))));
        let mut args = vec![Expr::Table(info), Expr::Function(Box::new(f))];
        args.extend(values);
        Expr::intrinsic_call("spawn", args)
    }

    /// The bindings a `spawn do` block captures, in declaration order, with
    /// their names.
    fn spawn_captures(&self, node: &SyntaxNode) -> Vec<(BindingId, String)> {
        let inside = node.text_range();
        let mut found: BTreeMap<BindingId, String> = BTreeMap::new();
        let mut consider = |id: BindingId| {
            let Some(b) = self.res.binding(id) else { return };
            let captured = !inside.contains_range(b.range)
                && !b.file_level
                && !b.named
                && !matches!(b.decl, DeclKind::Global | DeclKind::TypeAlias | DeclKind::TypeParam)
                && !(declares_type(b.decl) && b.function == 0);
            if captured {
                found.entry(id).or_insert_with(|| b.name.clone());
            }
        };
        for el in node.descendants_with_tokens() {
            let range = el.text_range();
            if let Some(Ref::Binding(id)) = self.res.reference(range) {
                consider(*id);
            }
            if let rowan::NodeOrToken::Node(n) = &el {
                if n.kind() == QUAL_NAME {
                    if let Some(Ref::Binding(id)) = self.res.type_ref(range) {
                        consider(*id);
                    }
                }
                if let Some(id) = self.res.self_target(range).and_then(SelfTarget::binding) {
                    consider(id);
                }
            }
        }
        found.into_iter().collect()
    }

    /// `rt.register(file, name, value)`: records a named function or a
    /// struct under this file's canonical name and yields
    /// `value`. `slot` is the place the declaration fills; a name already
    /// registered for another place is not registered again, and `value`
    /// is returned as it is.
    pub(crate) fn register_named(&mut self, slot: RegistrySlot, name: &str, value: Expr) -> Expr {
        match self.registered.get(name) {
            Some(s) if *s != slot => return value,
            Some(_) => {}
            None => {
                self.registered.insert(name.to_string(), slot);
            }
        }
        Expr::intrinsic_call("register", vec![Expr::str(&self.opts.file_label), Expr::str(name), value])
    }

    /// `value`, registered under the registry name of the type-level
    /// declaration `id` at `node` (a struct, enum, interface or module)
    /// when it has one. The place is the declaration
    /// itself: a second declaration of the name (a private type declared
    /// again in a reopened module) is another type, and is not registered.
    pub(crate) fn register_type(&mut self, id: BindingId, node: &SyntaxNode, value: Expr) -> Expr {
        match (self.registry_name(id), self.registry_scope(node)) {
            (Some(full), Some(_)) => self.register_named((Some(id), String::new()), &full, value),
            _ => value,
        }
    }

    /// The registry name of a struct or module (see the module docs): the
    /// identity another state finds it by.
    pub(crate) fn registry_name(&self, id: BindingId) -> Option<String> {
        self.registry_names.get(&id).cloned()
    }

    /// Where the declarations directly in the block around `decl` are
    /// registered: the dotted path of the enclosing module bodies (empty
    /// at file level) and the innermost module; `None` in any other scope.
    pub(crate) fn registry_scope(&self, decl: &SyntaxNode) -> Option<(String, Option<BindingId>)> {
        let mut segs: Vec<String> = Vec::new();
        let mut innermost = None;
        let mut node = decl.clone();
        loop {
            let block = node.parent()?;
            if block.kind() != BLOCK {
                return None;
            }
            let owner = block.parent()?;
            match owner.kind() {
                SOURCE_FILE => break,
                MODULE_DECL => {
                    segs.push(qual_text(&child(&owner, QUAL_NAME)?));
                    if innermost.is_none() {
                        innermost = Some(self.res.module(owner.text_range())?.0);
                    }
                    node = owner;
                }
                _ => return None,
            }
        }
        segs.reverse();
        Some((segs.join("."), innermost))
    }

    /// Records the registry names of the file's structs, enums, interfaces
    /// and modules, before lowering, since a method or an implementation
    /// may come first.
    pub(crate) fn collect_registry_names(&mut self, root: &SyntaxNode) {
        for d in root.descendants() {
            let found = match d.kind() {
                // Enums and interfaces cross by name too, as values:
                // registered like structs.
                STRUCT_DECL | ENUM_DECL | INTERFACE_DECL => {
                    child(&d, NAME).and_then(|n| Some((self.res.declaration(n.text_range())?, name_text(&n)?)))
                }
                MODULE_DECL => match (self.res.module(d.text_range()), child(&d, QUAL_NAME)) {
                    (Some((id, false)), Some(q)) => Some((id, qual_text(&q))),
                    _ => None,
                },
                _ => None,
            };
            let Some((id, name)) = found else { continue };
            if let Some((scope, _)) = self.registry_scope(&d) {
                self.registry_names.insert(id, qualified(&scope, &name));
            }
        }
    }
}

/// The place a registered declaration fills: the struct or module whose
/// member it is (`None` for the file), and its name there. Two
/// declarations of one place are a redefinition, not a clash. A type-level
/// declaration is its own place (`Some(id)` and an empty name,
/// [`Lowerer::register_type`]).
pub(crate) type RegistrySlot = (Option<BindingId>, String);

/// `scope.name`, or `name` at file level.
pub(crate) fn qualified(scope: &str, name: &str) -> String {
    if scope.is_empty() { name.to_string() } else { format!("{scope}.{name}") }
}

/// Declarations of type-level values.
fn declares_type(decl: DeclKind) -> bool {
    matches!(decl, DeclKind::Struct | DeclKind::Enum | DeclKind::Interface | DeclKind::Module)
}
