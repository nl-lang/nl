//! Interfaces: declarations, implementations (`interface I for
//! T`), and the conversion interfaces `To` and `From`.
//!
//! A declaration becomes `rt.interface(name, methods, fields, file)`, with
//! the names of its methods and required fields; type parameters are erased.
//! An implementation becomes `rt.implement(I, T, methods, targ,
//! file)`: the runtime (`runtime/nl_22_iface.lua`) checks it against the
//! declaration, records it for `is` and `as`, and makes the methods ordinary
//! methods of `T`. `targ` is the type argument of `To<U>` and `From<S>`,
//! the only type arguments that exist at runtime, because `as` needs them.

use std::collections::HashSet;

use nl_sema::util::*;
use nl_sema::{BindingId, DeclKind, PreludeClass, Ref};
use nl_syntax::{SyntaxKind, SyntaxNode};

use super::Lowerer;
use super::func::FnSpec;
use crate::ir::*;

use SyntaxKind::*;

impl Lowerer<'_> {
    /// `interface Name [<T>] members end`.
    pub(crate) fn lower_interface(&mut self, node: &SyntaxNode) {
        let Some(name) = child(node, NAME) else { return };
        let Some(id) = self.res.declaration(name.text_range()) else {
            self.internal(node, "undeclared interface");
            return;
        };
        let text = name_text(&name).unwrap_or_default();
        let mut methods = Vec::new();
        let mut fields = Vec::new();
        let mut seen = HashSet::new();
        for m in node.children().filter(|c| matches!(c.kind(), IFACE_METHOD | IFACE_FIELD)) {
            let Some(n) = child(&m, NAME) else { continue };
            let mname = name_text(&n).unwrap_or_default();
            if !self.check_member_name(&n, &mname, &mut seen) {
                continue;
            }
            let list = if m.kind() == IFACE_METHOD { &mut methods } else { &mut fields };
            list.push(TableEntry::Item(Expr::str(&mname)));
        }
        let value = Expr::intrinsic_call(
            "interface",
            vec![Expr::str(&text), Expr::Table(methods), Expr::Table(fields), Expr::str(&self.opts.file_label)],
        );
        let value = self.register_type(id, node, value);
        self.define(id, Some(value));
    }

    /// `len` is reserved, and a member is declared once.
    fn check_member_name(&mut self, name: &SyntaxNode, text: &str, seen: &mut HashSet<String>) -> bool {
        if text == "len" {
            self.error(name, "len-member", "`len` is a reserved member name; a struct defines its length with `fn Name.len`");
            return false;
        }
        if !seen.insert(text.to_string()) {
            self.error(name, "duplicate-member", format!("`{text}` is declared twice in this interface"));
            return false;
        }
        true
    }

    /// `interface I [<args>] for T  fn … end … end`.
    pub(crate) fn lower_interface_impl(&mut self, node: &SyntaxNode) {
        let types: Vec<SyntaxNode> = node.children().filter(|c| is_type(c.kind())).collect();
        let (Some(iface_ty), Some(target_ty)) = (types.first(), types.get(1)) else {
            self.internal(node, "interface implementation");
            return;
        };
        let Some(conversion) = self.check_interface_ref(iface_ty) else { return };
        let Some(target_text) = self.check_impl_target(target_ty) else { return };
        let targ = match conversion {
            Some(which) => {
                let args: Vec<SyntaxNode> = child(iface_ty, TYPE_ARGS).map(|a| a.children().filter(|c| is_type(c.kind())).collect()).unwrap_or_default();
                let [arg] = args.as_slice() else {
                    self.error(iface_ty, "conversion-type-arg", format!("`{which}` takes one type argument: `{which}<Type>`"));
                    return;
                };
                match self.type_value(arg) {
                    Some(v) => v,
                    None => {
                        self.error(arg, "conversion-type-arg", format!("the type argument of `{which}` must be a type that exists at runtime, such as a struct or `String`"));
                        return;
                    }
                }
            }
            None => Expr::Nil,
        };
        let Some(iface) = self.type_value(iface_ty) else {
            self.internal(iface_ty, "interface value");
            return;
        };
        let Some(target) = self.type_value(target_ty) else {
            self.internal(target_ty, "implementation target");
            return;
        };
        // The methods of an implementation are named functions
        // when the type has an identity another state can find (a primitive,
        // or a struct with a registry name) and the implementation stands
        // where a declaration can have one; they are registered under the
        // target's registry name (spawn.rs).
        let named = match child(target_ty, QUAL_NAME).and_then(|q| self.res.type_ref(q.text_range()).cloned()) {
            _ if self.registry_scope(node).is_none() => None,
            Some(Ref::Prelude(_)) => Some((None, target_text.clone())),
            Some(Ref::Binding(id)) => self.registry_name(id).map(|n| (Some(id), n)),
            _ => None,
        };
        let mut seen = HashSet::new();
        let mut methods = Vec::new();
        for f in children(node, FN_DECL) {
            if child(&f, QUAL_NAME).is_some() || child(&f, OP_NAME).is_some() {
                self.error(&f, "impl-method-name", "a method of an implementation is written `fn name`, without an owner; operators are defined with `fn Type.op`");
                continue;
            }
            let Some(n) = child(&f, NAME) else { continue };
            let mname = name_text(&n).unwrap_or_default();
            if !self.check_member_name(&n, &mname, &mut seen) {
                continue;
            }
            self.line = self.line_of(&f);
            let spec = FnSpec { name: format!("{target_text}.{mname}"), self_binding: self.res.method_self(f.text_range()), discard_tail: false };
            let mut value = self.function_value(&f, spec);
            if let Some((owner, prefix)) = &named {
                let slot = match owner {
                    Some(id) => (Some(*id), mname.clone()),
                    None => (None, format!("{prefix}.{mname}")),
                };
                value = self.register_named(slot, &format!("{prefix}.{mname}"), value);
            }
            methods.push(TableEntry::Pair(Expr::str(&mname), value));
        }
        self.line = self.line_of(node);
        // The methods of an implementation for a primitive type are
        // primitive methods, owned by the chunk.
        let primitive = matches!(child(target_ty, QUAL_NAME).and_then(|q| self.res.type_ref(q.text_range()).cloned()), Some(Ref::Prelude(_)));
        let owner = if primitive { self.owner_value() } else { Expr::str(&self.opts.file_label) };
        let call = Expr::intrinsic_call("implement", vec![iface, target, Expr::Table(methods), targ, owner]);
        self.emit(StmtKind::Expr(call));
    }

    /// Checks that the type after `interface` names an interface. Returns
    /// `Some(Some("To"))` or `Some(Some("From"))` for the conversion
    /// interfaces of the prelude, `Some(None)` for another
    /// interface, and `None` after reporting an error.
    fn check_interface_ref(&mut self, ty: &SyntaxNode) -> Option<Option<&'static str>> {
        let q = child(ty, QUAL_NAME);
        let r = q.as_ref().and_then(|q| self.res.type_ref(q.text_range()).cloned());
        let single = q.as_ref().is_some_and(|q| qual_segments(q).len() == 1);
        match r {
            Some(Ref::Prelude(p)) if p.class == PreludeClass::Interface => {
                Some(Some(if p.name == "To" { "To" } else { "From" }))
            }
            Some(Ref::Binding(id)) if self.binding_decl(id) == Some(DeclKind::Interface) && single => Some(None),
            // An imported name or a path into a module: the runtime checks.
            Some(Ref::Binding(id)) if self.binding_decl(id) == Some(DeclKind::Import) || !single => Some(None),
            _ => {
                self.error(ty, "not-an-interface", "only an interface can be implemented with `interface … for …`");
                None
            }
        }
    }

    /// Checks the type after `for`: a struct, `Number`, `String` or `Bool`.
    /// Returns its name for messages.
    fn check_impl_target(&mut self, ty: &SyntaxNode) -> Option<String> {
        let Some(q) = child(ty, QUAL_NAME) else {
            self.error(ty, "impl-target", "interfaces are implemented for structs, `Number`, `String` and `Bool`");
            return None;
        };
        let text = qual_text(&q);
        if child(ty, TYPE_ARGS).is_some() {
            // Type parameters are erased, so an interface is
            // implemented for every `Box<T>` at once: `interface I for Box`.
            self.error(ty, "impl-type-args", format!("`{text}` takes no type arguments here: type parameters are erased, so write `interface … for {}` to implement it for every instance", text.split('<').next().unwrap_or(&text)));
            return None;
        }
        let single = qual_segments(&q).len() == 1;
        match self.res.type_ref(q.text_range()).cloned() {
            Some(Ref::Prelude(p)) if p.is_primitive() => Some(text),
            Some(Ref::Binding(id)) if self.binding_decl(id) == Some(DeclKind::Struct) && single => Some(text),
            // An imported name or a path into a module: the runtime checks.
            Some(Ref::Binding(id)) if self.binding_decl(id) == Some(DeclKind::Import) || !single => Some(text),
            _ => {
                self.error(ty, "impl-target", "interfaces are implemented for structs, `Number`, `String` and `Bool`");
                None
            }
        }
    }

    fn binding_decl(&self, id: BindingId) -> Option<DeclKind> {
        self.res.binding(id).map(|b| b.decl)
    }
}
