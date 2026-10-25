//! Structs, enums, struct literals and methods declared
//! on types (`fn Owner.name`).
//!
//! At runtime a struct is a table built by `rt.struct`; it is the metatable
//! and method table of its instances. Field types and
//! defaults are passed as functions, so that they are evaluated lazily
//! (types: once, at the first construction, when the struct may refer to
//! itself; defaults: at each construction that omits the field).
//!
//! A literal of a struct the compiler sees whole (declared in this file,
//! its parents too) is built inline, as `rt.new` would build it
//! ([`Lowerer::inline_struct_literal`]); any other goes through `rt.new`,
//! which is told the literal's keys ([`Lowerer::struct_literal`]).

use std::rc::Rc;

use nl_sema::util::*;
use nl_sema::{BindingId, DeclKind, MethodOwner, Ref, SelfTarget};
use nl_syntax::{SyntaxKind, SyntaxNode};

use super::func::FnSpec;
use super::types::Inline;
use super::{FnState, Lowerer};
use crate::ir::*;

use SyntaxKind::*;

/// One field of a struct as `rt.struct` receives it, for literals built
/// inline.
#[derive(Clone, Debug)]
pub(crate) struct FieldLayout {
    pub name: String,
    /// The declared type, when the field is checked at runtime (a checked
    /// build, and a type with a runtime value).
    pub ty: Option<SyntaxNode>,
    pub default: Option<FieldDefault>,
}

/// A field's default value.
#[derive(Clone, Debug)]
pub(crate) enum FieldDefault {
    /// A constant (`= 1`, `= "unnamed"`), filled in place.
    Constant(Expr),
    /// Anything else, evaluated by the struct's own thunk
    /// (`rt.newdefault`), at each construction that omits the field.
    Thunk,
}

impl Lowerer<'_> {
    /// A function without parameters returning what `f` lowers, for lazy
    /// values.
    pub(crate) fn thunk(&mut self, f: impl FnOnce(&mut Self) -> Expr) -> Expr {
        let line = self.line;
        self.fns.push(FnState { ret_check: None, loops: Vec::new(), regions: Vec::new() });
        let (mut stmts, value) = self.capture(f);
        self.fns.pop();
        stmts.push(Stmt { line, kind: StmtKind::Return(vec![value]) });
        Expr::Function(Box::new(Function { params: Vec::new(), vararg: false, body: Block::new(stmts), line, name: None, method: false, transient: false }))
    }

    /// `struct Name [: Parent] fields end`.
    pub(crate) fn lower_struct(&mut self, node: &SyntaxNode) {
        let Some(name) = child(node, NAME) else { return };
        let Some(id) = self.res.declaration(name.text_range()) else {
            self.internal(node, "undeclared struct");
            return;
        };
        let text = name_text(&name).unwrap_or_default();
        let parent = match child(node, STRUCT_PARENT).and_then(|p| type_child(&p)) {
            Some(t) => self.type_value(&t).unwrap_or(Expr::Nil),
            None => Expr::Nil,
        };
        // The layout starts with the parent's fields, as `rt.struct` lists
        // them; a parent that is not a struct of this file whose layout is
        // known leaves the layout unknown.
        let mut layout: Option<Vec<FieldLayout>> = match child(node, STRUCT_PARENT).and_then(|p| type_child(&p)) {
            None => Some(Vec::new()),
            Some(t) => self.parent_layout(&t),
        };
        let mut fields = Vec::new();
        for f in children(node, FIELD_DECL) {
            let Some(fname) = child(&f, NAME).and_then(|n| name_text(&n)) else { continue };
            let mut entry = vec![TableEntry::Pair(Expr::str("name"), Expr::str(&fname))];
            let mut checked = None;
            if let Some(t) = type_child(&f) {
                if self.opts.checks {
                    // Type values need no statements, so the value can be
                    // computed here and placed in the thunk.
                    if let Some(v) = self.type_value(&t) {
                        let ty = self.thunk(|_| v);
                        entry.push(TableEntry::Pair(Expr::str("type"), ty));
                        checked = Some(t);
                    }
                }
            }
            let mut default = None;
            if let Some(d) = expr_child(&f) {
                let thunk = self.thunk(|l| l.expr1(&d));
                default = Some(match &thunk {
                    Expr::Function(func) => match func.body.stmts.as_slice() {
                        [Stmt { kind: StmtKind::Return(vals), .. }] => match vals.as_slice() {
                            [c] if c.is_constant() && !matches!(c, Expr::Intrinsic(_)) => FieldDefault::Constant(c.clone()),
                            _ => FieldDefault::Thunk,
                        },
                        _ => FieldDefault::Thunk,
                    },
                    _ => FieldDefault::Thunk,
                });
                entry.push(TableEntry::Pair(Expr::str("default"), thunk));
            }
            fields.push(TableEntry::Item(Expr::Table(entry)));
            // A field declared twice is refused by the resolver;
            // should one get here, the runtime's way stays.
            let twice = layout.as_ref().is_some_and(|l| l.iter().any(|x| x.name == fname));
            if twice {
                layout = None;
            }
            if let Some(l) = layout.as_mut() {
                l.push(FieldLayout { name: fname, ty: checked, default });
            }
        }
        self.struct_layouts.insert(id, layout.map(Rc::from));
        let value = Expr::intrinsic_call(
            "struct",
            vec![Expr::str(&text), parent, Expr::Table(fields), Expr::str(&self.opts.file_label)],
        );
        // A struct declared at file level or in a module
        // body is registered under its file and its path of modules
        // (spawn.rs).
        let value = self.register_type(id, node, value);
        self.define(id, Some(value));
    }

    /// The value a struct-literal path or an owner path names (`Point`,
    /// `Lib.Point`, `Self`), read without auto-call.
    pub(crate) fn type_path(&mut self, node: &SyntaxNode) -> Expr {
        match node.kind() {
            NAME_REF => self.name_raw(node),
            SELF_EXPR => {
                let t = self.res.self_target(node.text_range()).cloned();
                self.self_target_value(node, t)
            }
            FIELD_EXPR => {
                let name = field_name(node).map(|t| t.text().to_string()).unwrap_or_default();
                let base = match expr_child(node) {
                    Some(r) => self.type_path(&r),
                    None => self.internal(node, "type path"),
                };
                Expr::index(base, Expr::str(&name))
            }
            _ => self.expr1(node),
        }
    }

    /// The layout of the parent named by `ty`, when it is a struct of this
    /// file whose layout is known.
    fn parent_layout(&self, ty: &SyntaxNode) -> Option<Vec<FieldLayout>> {
        if ty.kind() != NAMED_TYPE {
            return None;
        }
        let q = child(ty, QUAL_NAME)?;
        if qual_segments(&q).len() != 1 {
            return None;
        }
        let Some(Ref::Binding(pid)) = self.res.type_ref(q.text_range()) else { return None };
        if self.res.binding(*pid).map(|b| b.decl) != Some(DeclKind::Struct) {
            return None;
        }
        self.struct_layouts.get(pid).cloned().flatten().map(|l| l.to_vec())
    }

    /// The struct a literal's path names when the literal can be built
    /// inline: a struct of this file with a known layout, by its name or as
    /// `Self`, outside `spawn do` blocks.
    fn inline_literal_target(&self, path: &SyntaxNode) -> Option<(BindingId, Rc<[FieldLayout]>)> {
        if self.spawn_depth > 0 {
            return None;
        }
        let id = match path.kind() {
            NAME_REF => match self.res.reference(path.text_range())? {
                Ref::Binding(id) => *id,
                _ => return None,
            },
            SELF_EXPR => match self.res.self_target(path.text_range())? {
                SelfTarget::Binding(id) => *id,
                _ => return None,
            },
            _ => return None,
        };
        if self.res.binding(id).map(|b| b.decl) != Some(DeclKind::Struct) {
            return None;
        }
        let layout = self.struct_layouts.get(&id).cloned().flatten()?;
        Some((id, layout))
    }

    /// `Name { field: value, … }` built inline: what
    /// `rt.new` does, in its order, for a struct whose fields are all
    /// known (see runtime/nl_20_struct.lua). `None` when a key is not a
    /// field, which `rt.new` reports.
    ///
    /// The values are evaluated in the order written, into the table that
    /// becomes the instance, as they were into the table `rt.new` took.
    /// Then, field by field in declaration order (the parents' first), an
    /// omitted or nil field gets its default (a constant in place, any
    /// other from `rt.newdefault`), and a checked field is tested inline,
    /// `rt.newfield` raising the TypeError when the test fails. The
    /// literal is `(rt.setmetatable(t, S))`, an instance exactly like
    /// `rt.new`'s.
    fn inline_struct_literal(&mut self, node: &SyntaxNode, id: BindingId, layout: &[FieldLayout]) -> Option<Expr> {
        let inits: Vec<(String, SyntaxNode, SyntaxNode)> = children(node, FIELD_INIT)
            .filter_map(|fi| {
                let k = name_token(&fi)?.text().to_string();
                let v = expr_child(&fi)?;
                Some((k, v, fi))
            })
            .collect();
        if inits.iter().any(|(k, _, _)| !layout.iter().any(|f| f.name == *k)) {
            return None;
        }
        let s = Expr::Local(self.local_of(id));
        let mut vals = Vec::new();
        for (_, v, _) in &inits {
            self.push_operand(&mut vals, |l| l.expr1(v));
        }
        let mut entries: Vec<TableEntry> = Vec::new();
        // The value each field has after the constructor, when known, and
        // whether the analyzer proved the written value's type.
        let mut given: Vec<(Option<Expr>, bool)> = Vec::new();
        for f in layout {
            let last = inits.iter().zip(&vals).rev().find(|((k, _, _), _)| *k == f.name);
            given.push(match last {
                Some(((_, _, fi), v)) => (Some(v.clone()), self.opts.hints.is_proven(fi.text_range())),
                None => (None, false),
            });
        }
        for ((k, _, _), v) in inits.iter().zip(vals) {
            entries.push(TableEntry::Pair(Expr::str(k), v));
        }
        // Constant defaults of omitted fields go in the constructor.
        for (f, (g, _)) in layout.iter().zip(&given) {
            if let (None, Some(FieldDefault::Constant(c))) = (g, &f.default) {
                entries.push(TableEntry::Pair(Expr::str(&f.name), c.clone()));
            }
        }
        let line = self.line;
        let mut steps: Vec<Stmt> = Vec::new();
        let obj = self.new_local("obj");
        let field = |name: &str| Expr::index(Expr::Local(obj), Expr::str(name));
        for (i, (f, (g, proven))) in layout.iter().zip(&given).enumerate() {
            let index = Expr::Number((i + 1) as f64);
            // The field's value when the compiler knows it: a constant
            // written or filled in, or nil.
            let mut known: Option<Expr> = match g {
                Some(v) if v.is_constant() => Some(v.clone()),
                Some(_) => None,
                None => Some(Expr::Nil),
            };
            let mut proven = *proven;
            if let Some(d) = &f.default {
                let value = match d {
                    FieldDefault::Constant(c) => c.clone(),
                    FieldDefault::Thunk => Expr::intrinsic_call("newdefault", vec![s.clone(), index.clone()]),
                };
                let set = Stmt { line, kind: StmtKind::Assign { targets: vec![Place::Index(Expr::Local(obj), Expr::str(&f.name))], values: vec![value] } };
                match (&known, d) {
                    // A written constant other than nil stays.
                    (Some(c), _) if !matches!(c, Expr::Nil) => {}
                    // Omitted, with a constant default: in the constructor.
                    (Some(_), FieldDefault::Constant(c)) if g.is_none() => known = Some(c.clone()),
                    // Nil: the default, whatever it is.
                    (Some(_), FieldDefault::Constant(c)) => {
                        steps.push(set);
                        known = Some(c.clone());
                    }
                    (Some(_), FieldDefault::Thunk) => {
                        steps.push(set);
                        known = None;
                    }
                    // A value that may be nil, and then the default.
                    (None, _) => {
                        let cond = Expr::binary(BinOp::Eq, field(&f.name), Expr::Nil);
                        steps.push(Stmt { line, kind: StmtKind::If { cond, then: Block::new(vec![set]), otherwise: None } });
                        // The proof was of the written value only.
                        proven = false;
                    }
                }
            }
            let Some(ty) = &f.ty else { continue };
            if proven {
                continue;
            }
            let test = self.inline_test(ty);
            // Known to pass: a constant of the type, or a literal of the
            // struct written for the field (never nil, so no default
            // replaced it).
            let statically = |l: &Self, t: &Inline| match &known {
                Some(k) => l.accepts_statically(t, k),
                None => g.as_ref().is_some_and(|w| l.accepts_statically(t, w)),
            };
            let v = known.clone().unwrap_or_else(|| field(&f.name));
            let call = Stmt { line, kind: StmtKind::Expr(Expr::intrinsic_call("newfield", vec![s.clone(), index, v.clone()])) };
            match test {
                Some(t) if statically(self, &t) => {}
                Some(t) => {
                    let cond = self.inline_fails(&t, &v);
                    steps.push(Stmt { line, kind: StmtKind::If { cond, then: Block::new(vec![call]), otherwise: None } });
                }
                None => steps.push(call),
            }
        }
        let table = Expr::Table(entries);
        let instance = if steps.is_empty() {
            table
        } else {
            self.emit(StmtKind::Local { locals: vec![obj], values: vec![table] });
            self.stmts.extend(steps);
            Expr::Local(obj)
        };
        let setmetatable = self.rt_cached("setmetatable");
        Some(Expr::Paren(Box::new(Expr::call(setmetatable, vec![instance, s]))))
    }

    /// `Name { field: value, … }`: built inline when the
    /// compiler knows the struct whole (see [`Self::inline_struct_literal`]);
    /// otherwise `rt.new` fills in defaults and checks the field types.
    ///
    /// `rt.new(S, {x = …, y = …}, "x,y")`: the third argument names the
    /// keys written, in their order. The table can hold no other key (a
    /// value may be nil), so the runtime decides once for the struct and
    /// these keys whether one of them is not a field, and otherwise builds
    /// the instance without its loops (runtime/nl_20_struct.lua,
    /// CONSTRUCTORS), which the JIT compiles into the calling trace.
    pub(crate) fn struct_literal(&mut self, node: &SyntaxNode) -> Expr {
        let Some(path) = node.children().find(|c| matches!(c.kind(), NAME_REF | FIELD_EXPR | SELF_EXPR)) else {
            return self.internal(node, "struct literal");
        };
        if let Some((id, layout)) = self.inline_literal_target(&path) {
            if let Some(e) = self.inline_struct_literal(node, id, &layout) {
                return e;
            }
        }
        let target = match self.alias_literal_target(&path) {
            Some(Ok(v)) => v,
            Some(Err(msg)) => {
                self.error(&path, "not-a-struct", msg);
                return Expr::Nil;
            }
            None => self.type_path(&path),
        };
        let mut vals = vec![target];
        let mut keys = Vec::new();
        for fi in children(node, FIELD_INIT) {
            let Some(k) = name_token(&fi) else { continue };
            let Some(v) = expr_child(&fi) else { continue };
            keys.push(k.text().to_string());
            self.push_operand(&mut vals, |l| l.expr1(&v));
        }
        let mut it = vals.into_iter();
        let s = it.next().unwrap_or(Expr::Nil);
        let shape = keys.join(",");
        let fields = keys.into_iter().zip(it).map(|(k, v)| TableEntry::Pair(Expr::str(&k), v)).collect();
        Expr::intrinsic_call("new", vec![s, Expr::Table(fields), Expr::str(&shape)])
    }

    /// `enum Name variants end`.
    pub(crate) fn lower_enum(&mut self, node: &SyntaxNode) {
        let Some(name) = child(node, NAME) else { return };
        let Some(id) = self.res.declaration(name.text_range()) else {
            self.internal(node, "undeclared enum");
            return;
        };
        let text = name_text(&name).unwrap_or_default();
        let mut variants = Vec::new();
        for v in children(node, VARIANT) {
            let Some(vname) = child(&v, NAME).and_then(|n| name_text(&n)) else { continue };
            let value = match expr_child(&v) {
                Some(e) => self.expr1(&e),
                None => Expr::str(&vname),
            };
            variants.push(TableEntry::Item(Expr::Table(vec![TableEntry::Item(Expr::str(&vname)), TableEntry::Item(value)])));
        }
        let value = Expr::intrinsic_call("enum", vec![Expr::str(&text), Expr::Table(variants)]);
        let value = self.register_type(id, node, value);
        self.define(id, Some(value));
    }

    /// `fn Owner.name … end`: a method on a struct or module, or on
    /// `Number`, `String` or `Bool`.
    pub(crate) fn lower_method_decl(&mut self, node: &SyntaxNode) {
        let word = child(node, NAME).and_then(|n| name_text(&n));
        // `len`, `coerce` and `iter` are operators too.
        if child(node, OP_NAME).is_some() || matches!(word.as_deref(), Some("len" | "coerce" | "iter")) {
            self.lower_operator_def(node);
            return;
        }
        let Some(owner) = self.res.method_owner(node.text_range()).cloned() else {
            self.internal(node, "method owner");
            return;
        };
        let method = child(node, NAME).and_then(|n| name_text(&n)).unwrap_or_default();
        let owner_text = child(node, QUAL_NAME).map(|q| qual_text(&q)).unwrap_or_default();
        let self_binding = self.res.method_self(node.text_range());
        let spec = FnSpec { name: format!("{owner_text}.{method}"), self_binding, discard_tail: false };
        let f = self.function_value(node, spec);
        match owner {
            MethodOwner::Binding { id, path } => {
                let mut o = Expr::Local(self.local_of(id));
                for p in &path {
                    o = Expr::index(o, Expr::str(p));
                }
                let mut marked = Expr::intrinsic_call("method", vec![f]);
                // A named function, under its owner's
                // registry name, when the owner has one and the method is
                // declared where a declaration can have one (spawn.rs).
                if let (Some(owner_name), Some(_)) = (self.registry_name(id), self.registry_scope(node)) {
                    let mut member = path.join(".");
                    if !member.is_empty() {
                        member.push('.');
                    }
                    member.push_str(&method);
                    let full = format!("{owner_name}.{member}");
                    marked = self.register_named((Some(id), member), &full, marked);
                }
                self.emit(StmtKind::Assign { targets: vec![Place::Index(o, Expr::str(&method))], values: vec![marked] });
            }
            MethodOwner::Primitive(p) => {
                // Registered by name only once `primmethod` has accepted it:
                // a rejected redefinition leaves no trace.
                let fl = self.new_local("method");
                let line = self.line;
                let define = Expr::intrinsic_call("primmethod", vec![Expr::str(p.name), Expr::str(&method), Expr::Local(fl), self.owner_value()]);
                let mut stmts = vec![
                    Stmt { line, kind: StmtKind::Local { locals: vec![fl], values: vec![f] } },
                    Stmt { line, kind: StmtKind::Expr(define) },
                ];
                if self.registry_scope(node).is_some() {
                    let full = format!("{}.{method}", p.name);
                    let register = self.register_named((None, full.clone()), &full, Expr::Local(fl));
                    stmts.push(Stmt { line, kind: StmtKind::Expr(register) });
                }
                self.emit(StmtKind::Do(Block::new(stmts)));
            }
            // `fn Util.name` on a declared module: `rt.declfn(util, "name",
            // f, owner, "Util")` stores a plain function in the module's
            // Lua table. `fn Player.name` on a declared host type or Lua
            // class: `rt.declmethod(mt, "name", f, owner, "Player", kind)`
            // stores a method where the values find their members, `mt`
            // being the metatable the host registered under the type's
            // name (`rt.gettype`), or the class at the struct's Lua path.
            MethodOwner::Declared(id) => {
                let kind = self.res.declared_kind(id);
                let target = match kind {
                    Some(nl_sema::decl::DeclItemKind::HostType) => Expr::intrinsic_call("gettype", vec![Expr::str(&owner_text)]),
                    _ => match self.res.declared_path(id).cloned() {
                        Some(root) => self.lua_root_value(&root),
                        None => {
                            self.internal(node, "declared owner without a Lua path");
                            return;
                        }
                    },
                };
                let owner = self.owner_value();
                let call = match kind {
                    Some(nl_sema::decl::DeclItemKind::Module) => Expr::intrinsic_call("declfn", vec![target, Expr::str(&method), f, owner, Expr::str(&owner_text)]),
                    _ => {
                        let how = if kind == Some(nl_sema::decl::DeclItemKind::HostType) { "host" } else { "class" };
                        Expr::intrinsic_call("declmethod", vec![target, Expr::str(&method), f, owner, Expr::str(&owner_text), Expr::str(how)])
                    }
                };
                self.emit(StmtKind::Expr(call));
            }
        }
    }
}
