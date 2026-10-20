//! Expressions: literals, names, members, calls, operators, `is` and
//! `as`, `spawn`.

use std::rc::Rc;

use nl_syntax::{SyntaxKind, SyntaxNode, TextRange};

use super::{Analyzer, Ty};
use crate::binding::*;
use crate::hints::*;
use crate::types::*;
use crate::util::*;

use SyntaxKind::*;

/// What `x.member` resolved to.
pub(crate) enum Lookup {
    /// A field or a plain value: read as data.
    Field(Type, Option<String>),
    /// A method: called with the receiver as `self`.
    Method(Rc<FnType>, Option<String>),
    /// A function held as data: called as written.
    Function(Rc<FnType>, Option<String>),
    /// A library function on a primitive receiver: a method whose first
    /// parameter is the receiver.
    LibraryMethod(Rc<FnType>),
    /// A function of the `Table` library on a receiver typed as a table
    /// (the library's or a `fn Table.name` of this file), whose first
    /// parameter is the receiver. Unlike a primitive's library, nothing
    /// finds it at runtime: the call is resolved here, in every mode
    /// ([`MemberHint::TableFn`]).
    TableFn(Rc<FnType>),
    /// A method of a worker handle: a call returns a task.
    WorkerCall,
    Unknown,
}

/// The type a method's receiver is checked as. For a function of `Table`
/// it is a table, which is all the resolution relies on: which kind of
/// table is not checked (a list function on a map goes by its list part,
/// as Lua's table functions do), nor a record against `[T]`. A receiver
/// that may be `nil` (`T?`) is checked as `T`: the member is read from it
/// before the call, which fails on `nil`.
fn receiver_ty(types: &TypeTable, recv: &Ty, table_fn: bool) -> Ty {
    if table_fn { Ty::uncertain(Type::Table) } else { Ty::new(types.non_nil(&recv.t), recv.certain) }
}

/// A receiver as a diagnostic writes it: its text when it is short and
/// on one line.
fn receiver_text(node: &SyntaxNode) -> Option<String> {
    let t = node.text().to_string();
    (t.len() <= 24 && !t.contains(char::is_whitespace)).then_some(t)
}

/// How to read the key `name` of a receiver: "as t["name"]" with the
/// receiver's text, or, when the receiver is too long to repeat, "with
/// brackets, `["name"]`" (a name made up for it would be wrong advice).
fn bracket_key(node: &SyntaxNode, name: &str) -> String {
    match receiver_text(node) {
        Some(x) => format!("as {x}[\"{name}\"]"),
        None => format!("with brackets, `[\"{name}\"]`"),
    }
}

/// One argument of a call, typed.
pub(crate) struct ArgTy {
    pub ty: Ty,
    pub range: TextRange,
    pub spread: bool,
    pub keyed: Option<String>,
    pub keyed_spread: bool,
    /// A bare function-kind name (`register callback`): called, not
    /// passed.
    pub fn_kind_name: Option<String>,
    /// A `fn … end` literal, which cannot be sent to a worker.
    pub is_fn_literal: bool,
}

impl Analyzer<'_> {
    /// The type of an expression whose value is used.
    pub(crate) fn expr(&mut self, node: &SyntaxNode) -> Ty {
        let mut t = self.expr_inner(node);
        // Nil-safe chains: the step after `?.` sees the value
        // it tests without `nil`; the chain is `nil` when a test fails. An
        // assignment target is guarded as a whole statement, not a value.
        if is_safe_receiver(node) {
            t = Ty::new(self.types.non_nil(&self.types.first(&t.t)), t.certain);
        }
        if is_safe_chain_top(node) && !is_single_target(node) {
            t = Ty::new(optional(&self.types, &self.types.first(&t.t)), t.certain);
        }
        self.record_expr(node.text_range(), &t.t);
        t
    }

    fn expr_inner(&mut self, node: &SyntaxNode) -> Ty {
        match node.kind() {
            LITERAL => self.literal(node),
            INTERP_STRING => self.interp_string(node),
            NAME_REF => self.name_value(node),
            SELF_EXPR => self.self_expr(node),
            AT_EXPR => self.at_expr(node),
            PAREN_EXPR => match expr_child(node) {
                Some(inner) => {
                    let t = self.expr(&inner);
                    Ty::new(self.types.first(&t.t), t.certain)
                }
                None => Ty::any(),
            },
            TABLE_EXPR => self.table(node),
            LIST_EXPR => self.list(node),
            STRUCT_LIT => self.struct_literal(node),
            FN_EXPR | LAMBDA_EXPR => self.fn_expr(node),
            LUA_PATH => self.lua_path(node),
            PREFIX_EXPR => self.prefix(node),
            BIN_EXPR => self.binary(node),
            IS_EXPR => self.is_expr(node),
            AS_EXPR => self.as_expr(node),
            PIPE_EXPR => self.pipe(node),
            CALL_EXPR | COMMAND_CALL | METHOD_CALL => self.call(node),
            INDEX_EXPR => self.index_expr(node),
            FIELD_EXPR => self.field_value(node),
            BANG_EXPR => self.bang(node),
            SPAWN_EXPR => self.spawn(node),
            ON_EXPR => self.on_expr(node),
            IF_EXPR | UNLESS_EXPR | CASE_EXPR | COND_EXPR | TRY_EXPR => self.construct_value(node),
            _ => {
                for c in expr_children(node) {
                    self.expr(&c);
                }
                Ty::any()
            }
        }
    }

    /// An expression in a single-value position.
    pub(crate) fn expr1(&mut self, node: &SyntaxNode) -> Ty {
        let t = self.expr(node);
        Ty::new(self.types.first(&t.t), t.certain)
    }

    // ---- literals ---------------------------------------------------------------

    fn literal(&mut self, node: &SyntaxNode) -> Ty {
        match first_token(node).map(|t| t.kind()) {
            Some(NUMBER) => Ty::certain(Type::Number),
            Some(STRING) => Ty::certain(Type::String),
            Some(TRUE_KW | FALSE_KW) => Ty::certain(Type::Bool),
            Some(NIL_KW) => Ty::certain(Type::Nil),
            _ => Ty::any(),
        }
    }

    fn interp_string(&mut self, node: &SyntaxNode) -> Ty {
        for e in expr_children(node) {
            let t = self.expr1(&e);
            if t.certain && t.t == Type::String && self.opts.static_resolution {
                self.hints.interp_string.insert(e.text_range());
            }
        }
        Ty::certain(Type::String)
    }

    // ---- names ------------------------------------------------------------------

    /// The value a name holds, without calling it (`f!`, a callee).
    pub(crate) fn name_raw(&mut self, node: &SyntaxNode) -> Ty {
        match self.res.reference(node.text_range()).cloned() {
            Some(Ref::Binding(id)) => {
                self.used.insert(id);
                if let Some(sig) = self.binding_fn_type(id) {
                    return Ty::certain(Type::Fn(sig));
                }
                if let Some(b) = self.binding(id) {
                    if b.decl == DeclKind::Global {
                        let name = b.name.clone();
                        self.lint(node.text_range(), "global-use", format!("`{name}` is a global; globals are shared with every Lua script and mod"));
                    }
                }
                let t = self.type_of_binding(id);
                if self.current_fn_is_spawn_block() {
                    if let Some(b) = self.binding(id) {
                        if b.file_level && matches!(b.decl, DeclKind::Let | DeclKind::Global) {
                            let name = b.name.clone();
                            self.warn(node.text_range(), "spawn-file-variable", format!("`{name}` is a file-level variable: inside a `spawn do` block it is the worker's own variable, not this state's"));
                        }
                    }
                }
                t
            }
            Some(Ref::Prelude(p)) => self.prelude_value(p),
            Some(Ref::Declared(id)) => match self.env.get(&Self::declared_flow_id(id)) {
                // Narrowed by a test along the path (`stmt.rs`,
                // `local_binding`).
                Some(t) => Ty::uncertain(t.clone()),
                None => self.declared_value(id),
            },
            Some(Ref::LuaGlobal(_)) | None => Ty::any(),
        }
    }

    /// A name whose value is used: a function-kind name is called.
    fn name_value(&mut self, node: &SyntaxNode) -> Ty {
        if let Some(Ref::Binding(id)) = self.res.reference(node.text_range()) {
            if self.binding(*id).is_some_and(|b| b.kind == BindingKind::Function) {
                self.used.insert(*id);
                let sig = self.binding_fn_type(*id).unwrap_or_else(|| Rc::new(FnType::unknown()));
                self.check_call(&sig, &self.binding(*id).map(|b| b.name.clone()).unwrap_or_default(), &[], None, node.text_range(), true);
                return self.call_of(&sig, self.checked_fn_name(node));
            }
        }
        if let Some(Ref::Declared(id)) = self.res.reference(node.text_range()).cloned() {
            if self.decls.binding_kind(id) == BindingKind::Function {
                let v = self.declared_value(id);
                if let Type::Fn(sig) = &v.t {
                    let name = self.decls.get(id).map(|i| i.name.clone()).unwrap_or_default();
                    self.check_call(sig, &name, &[], None, node.text_range(), true);
                    return self.call_result(sig);
                }
            }
        }
        self.name_raw(node)
    }

    fn current_fn_is_spawn_block(&self) -> bool {
        self.fns.last().is_some_and(|f| f.spawn_block)
    }

    fn self_expr(&mut self, node: &SyntaxNode) -> Ty {
        if has_token(node, SELF_TYPE_KW) {
            return match self.res.self_target(node.text_range()).cloned() {
                Some(SelfTarget::Binding(id)) => self.named_of.get(&id).map(|n| Ty::certain(Type::Static(*n))).unwrap_or_else(Ty::any),
                Some(SelfTarget::Member { id, path }) => {
                    let text = self.binding(id).map(|b| b.name.clone()).unwrap_or_default();
                    let full = std::iter::once(text).chain(path).collect::<Vec<_>>().join(".");
                    let hidden = self.res.bindings.iter().enumerate().find(|(_, b)| b.decl == DeclKind::Module && b.name == full).map(|(i, _)| BindingId(u32::try_from(i).unwrap_or(u32::MAX)));
                    hidden.and_then(|h| self.named_of.get(&h).copied()).map(|n| Ty::certain(Type::Static(n))).unwrap_or_else(Ty::any)
                }
                Some(SelfTarget::Prelude(p)) => self.prelude_value(p),
                None => Ty::any(),
            };
        }
        match self.self_type() {
            Some(t) => Ty::certain(t),
            None => Ty::any(),
        }
    }

    /// `@name`: a plain field read of `self`.
    fn at_expr(&mut self, node: &SyntaxNode) -> Ty {
        let name = token(node, AT_NAME).map(|t| t.text().trim_start_matches('@').to_string()).unwrap_or_default();
        let Some(st) = self.self_type() else { return Ty::any() };
        match self.types.expand(&st) {
            Type::Named(nid) => match self.types.field(nid, &name) {
                Some(f) => Ty::uncertain(f.ty),
                None => Ty::any(),
            },
            _ => Ty::any(),
        }
    }

    // ---- tables ---------------------------------------------------------------

    /// `[a, b]`: a `[T]`, `T` being the union of the items.
    fn list(&mut self, node: &SyntaxNode) -> Ty {
        let mut elems = Vec::new();
        let mut certain = true;
        let mut spread = false;
        for c in node.children() {
            match c.kind() {
                SPREAD_ARG => {
                    spread = true;
                    if let Some(e) = expr_child(&c) {
                        let t = self.expr1(&e);
                        match self.types.expand(&t.t) {
                            Type::Array(el) => elems.push((*el).clone()),
                            _ => elems.push(Type::Any),
                        }
                        certain &= t.certain;
                    }
                }
                k if is_expr(k) => {
                    let t = self.expr(&c);
                    certain &= t.certain;
                    for m in self.types.members(&self.types.first(&t.t)) {
                        if m != Type::Nil {
                            elems.push(m);
                        }
                    }
                }
                _ => {}
            }
        }
        let _ = spread;
        let elem = self.types.union_all(&elems);
        Ty::new(Type::Array(Rc::new(elem)), certain)
    }

    /// `{k: v}`: a record when every key is a word or a string; otherwise
    /// a table.
    fn table(&mut self, node: &SyntaxNode) -> Ty {
        let mut fields: Vec<(String, Type)> = Vec::new();
        let mut record = true;
        let mut certain = true;
        for c in node.children() {
            match c.kind() {
                TABLE_PAIR => {
                    let key = child(&c, TABLE_KEY).and_then(|k| first_token(&k));
                    let key = match key {
                        Some(t) if t.kind() == STRING => Some(crate::resolver::decode_plain_string(&t)),
                        Some(t) if t.kind() == NUMBER => None,
                        Some(t) => Some(t.text().to_string()),
                        None => None,
                    };
                    let v = match expr_child(&c) {
                        Some(e) => self.expr1(&e),
                        None => Ty::any(),
                    };
                    certain &= v.certain;
                    match key {
                        Some(k) => {
                            fields.retain(|(fk, _)| *fk != k);
                            fields.push((k, v.t));
                        }
                        None => record = false,
                    }
                }
                TABLE_ITEM => {
                    if let Some(e) = expr_child(&c) {
                        self.expr(&e);
                    }
                    record = false;
                }
                _ => {}
            }
        }
        if !record {
            return Ty::new(Type::Table, certain);
        }
        fields.sort_by(|a, b| a.0.cmp(&b.0));
        Ty::new(Type::Record(fields.into()), certain)
    }

    /// `Name { field: value }`: the fields must be declared,
    /// and a value of a definite type must fit the field's type.
    fn struct_literal(&mut self, node: &SyntaxNode) -> Ty {
        let Some(path) = node.children().find(|c| matches!(c.kind(), NAME_REF | FIELD_EXPR | SELF_EXPR)) else { return Ty::any() };
        let target = self.struct_literal_target(&path);
        let fields: Vec<SyntaxNode> = children(node, FIELD_INIT).collect();
        let nid = match target {
            Some(nid) => nid,
            None => {
                for fi in &fields {
                    if let Some(v) = expr_child(fi) {
                        self.expr1(&v);
                    }
                }
                return Ty::any();
            }
        };
        let name = self.types.name(nid).to_string();
        for fi in &fields {
            let Some(k) = name_token(fi) else { continue };
            let key = k.text().to_string();
            let v = match expr_child(fi) {
                Some(e) => self.expr1(&e),
                None => Ty::any(),
            };
            match self.types.field(nid, &key) {
                Some(f) => {
                    let what = format!("field '{key}' of {name}");
                    let proven = self.check_assignable(&v, &f.ty, fi.text_range(), &what);
                    // A literal the lowering builds inline then skips the
                    // field's check (nl_ir, lower/structs.rs).
                    if proven && self.opts.static_resolution {
                        self.hints.proven.insert(fi.text_range());
                    }
                }
                None => {
                    self.error(k.text_range(), "unknown-field", format!("`{name}` has no field '{key}'"));
                }
            }
        }
        if self.types.is_struct(nid) {
            Ty::certain(Type::Named(nid))
        } else {
            Ty::any()
        }
    }

    /// The struct a struct literal constructs, when the analyzer knows it.
    fn struct_literal_target(&mut self, path: &SyntaxNode) -> Option<NamedId> {
        match path.kind() {
            NAME_REF => match self.res.reference(path.text_range()).cloned()? {
                Ref::Binding(id) => {
                    self.used.insert(id);
                    let nid = *self.named_of.get(&id)?;
                    match self.types.kind(nid)? {
                        NamedKind::Struct { .. } => Some(nid),
                        NamedKind::Alias(t) => match self.types.expand(t) {
                            Type::Named(s) if self.types.is_struct(s) => Some(s),
                            _ => None,
                        },
                        _ => None,
                    }
                }
                Ref::Prelude(p) => self.prelude_type_id(p.name).filter(|n| self.types.is_struct(*n)),
                _ => None,
            },
            SELF_EXPR => match self.self_expr(path).t {
                Type::Static(nid) if self.types.is_struct(nid) => Some(nid),
                _ => None,
            },
            // `Lib.Point { … }` through an import table: not known here.
            _ => {
                if let Some(recv) = expr_child(path) {
                    self.expr(&recv);
                }
                None
            }
        }
    }

    // ---- members ----------------------------------------------------------------

    /// `x.member` whose value is used: `.len` is the length;
    /// a method is auto-called.
    fn field_value(&mut self, node: &SyntaxNode) -> Ty {
        let Some(recv_node) = expr_child(node) else { return Ty::any() };
        let name = field_name(node).map(|t| t.text().to_string()).unwrap_or_default();
        let recv = self.expr1(&recv_node);
        self.note_table_read(node, &recv);
        if name == "len" {
            return self.len_of(&recv, node);
        }
        self.check_enum_member(&recv_node, &recv, &name, node.text_range());
        let lookup = self.lookup_member(&recv, &name, node.text_range());
        let lookup = self.table_member_of_name(&recv_node, &name, lookup);
        let lookup = self.primitive_library_member(&recv_node, &name).unwrap_or(lookup);
        // The read of a compound assignment's target (`c.count += 1`):
        // the store refuses a function of `Table` (`table_method_assign`),
        // so the read resolves to nothing and reports nothing.
        let place = self.place_read == Some(node.text_range());
        let lookup = match lookup {
            Lookup::TableFn(_) if place => Lookup::Unknown,
            Lookup::Unknown if !place => {
                self.unresolved_table_member(&recv_node, &recv, &name, node.text_range());
                Lookup::Unknown
            }
            other => other,
        };
        self.record_member_hint(node, &recv, &lookup, &name);
        let is_table_fn = matches!(lookup, Lookup::TableFn(_));
        match lookup {
            Lookup::Field(t, _) => Ty::uncertain(t),
            Lookup::Method(sig, _) => {
                self.check_call(&sig, &format!("{}.{name}", self.show(&recv.t)), &[], None, node.text_range(), true);
                self.call_result(&sig)
            }
            Lookup::LibraryMethod(sig) | Lookup::TableFn(sig) => {
                let args = [ArgTy { ty: receiver_ty(&self.types, &recv, is_table_fn), range: recv_node.text_range(), spread: false, keyed: None, keyed_spread: false, fn_kind_name: None, is_fn_literal: false }];
                self.check_call(&sig, &name, &args, None, node.text_range(), false);
                self.call_result(&sig)
            }
            Lookup::Function(sig, _) => Ty::uncertain(Type::Fn(sig)),
            Lookup::WorkerCall => Ty::uncertain(Type::Task(Rc::new(Type::Any))),
            Lookup::Unknown => Ty::any(),
        }
    }

    /// `x.len`.
    fn len_of(&mut self, recv: &Ty, node: &SyntaxNode) -> Ty {
        match self.types.expand(&recv.t) {
            Type::String => {
                if recv.certain && self.opts.static_resolution {
                    self.hints.len_string.insert(node.text_range());
                }
                Ty::certain(Type::Number)
            }
            Type::Named(nid) if self.types.is_struct(nid) => match self.types.operator(nid, "len") {
                Some(sig) => Ty::uncertain(sig.ret.clone()),
                None => Ty::uncertain(Type::Number),
            },
            Type::Nil | Type::Number | Type::Bool => {
                if recv.certain {
                    let shown = self.show(&recv.t);
                    self.error(node.text_range(), "len-of-non-length", format!("a {shown} has no length"));
                }
                Ty::uncertain(Type::Number)
            }
            _ => Ty::uncertain(Type::Number),
        }
    }

    /// Records how a member is resolved when the receiver's type decides
    /// it, and the Lua key of a declared member.
    fn record_member_hint(&mut self, node: &SyntaxNode, recv: &Ty, lookup: &Lookup, name: &str) {
        // A function of `Table`: the runtime has no way to find it, so it
        // is resolved whatever the mode, and whether or not the receiver's
        // type is certain (the type comes from a declaration, a literal or
        // a declared function's result, as for the declared members below).
        if let Lookup::TableFn(_) = lookup {
            self.hints.members_declared.insert(node.text_range(), MemberHint::TableFn { key: name.to_string() });
            return;
        }
        let key = match lookup {
            Lookup::Field(_, k) | Lookup::Method(_, k) | Lookup::Function(_, k) => k.clone(),
            _ => None,
        };
        // A member of a declared module, Lua class or host type, or of a
        // prelude library: resolved by its
        // declaration, in every mode. It is emitted under its Lua key.
        let declared_type = match self.types.expand(&recv.t) {
            Type::Static(nid) => matches!(self.types.kind(nid), Some(NamedKind::Module { .. } | NamedKind::LuaClass { .. } | NamedKind::HostType { .. })) && !self.types.is_struct(nid) && self.is_declared_module(nid),
            Type::Named(nid) => matches!(self.types.kind(nid), Some(NamedKind::LuaClass { .. } | NamedKind::HostType { .. })),
            // `T?` and unions of declared types, whose members
            // `lookup_member` found under one key.
            Type::Union(parts) => {
                let mut non_nil = parts.iter().filter(|p| **p != Type::Nil).peekable();
                non_nil.peek().is_some() && non_nil.all(|p| self.is_declared_instance(p))
            }
            _ => false,
        };
        let declared_key = if declared_type { Some(key.clone().unwrap_or_else(|| name.to_string())) } else { key.filter(|k| k != name) };
        let certain_struct = recv.certain && matches!(self.types.expand(&recv.t), Type::Named(nid) if self.types.is_struct(nid));
        let hint = match lookup {
            Lookup::Field(..) if declared_key.is_some() => Some(MemberHint::Field { key: declared_key.clone().unwrap_or_default() }),
            Lookup::Method(..) if declared_key.is_some() => Some(MemberHint::Method { key: declared_key.clone().unwrap_or_default() }),
            Lookup::Function(..) if declared_key.is_some() => Some(MemberHint::Function { key: declared_key.clone().unwrap_or_default() }),
            Lookup::Field(t, _) if certain_struct && self.opts.static_resolution => {
                // A field holding a function is data; the
                // runtime would call it when it is a marked method, so a
                // field of function type is left to the runtime test.
                if matches!(self.types.expand(t), Type::Function | Type::Fn(_) | Type::Any) { None } else { Some(MemberHint::Field { key: name.to_string() }) }
            }
            Lookup::Method(..) if certain_struct && self.opts.static_resolution => Some(MemberHint::Method { key: name.to_string() }),
            _ => None,
        };
        if let Some(h) = hint {
            if declared_key.is_some() {
                self.hints.members_declared.insert(node.text_range(), h);
            } else {
                self.hints.members.insert(node.text_range(), h);
            }
        }
    }

    /// A module of a declaration file, at the top level or nested in
    /// another, or of the prelude's libraries (not an NL module of this
    /// file, whose functions are methods with the module as `self`).
    fn is_declared_module(&self, nid: NamedId) -> bool {
        self.decl_types.contains(&nid)
    }

    /// What `recv.name` is.
    pub(crate) fn lookup_member(&mut self, recv: &Ty, name: &str, at: TextRange) -> Lookup {
        let t = self.types.expand(&recv.t);
        // For tools: which nominal type the member is looked up on, whether
        // or not it is found there.
        if self.recording() {
            let owner = match &t {
                Type::Named(nid) | Type::Static(nid) => Some(*nid),
                Type::Number => self.prelude_type_id("Number"),
                Type::String => self.prelude_type_id("String"),
                Type::Bool => self.prelude_type_id("Bool"),
                Type::Regex => self.prelude_type_id("Regex"),
                Type::Table | Type::Map(..) | Type::Array(_) => self.prelude_type_id("Table"),
                _ => None,
            };
            if let Some(owner) = owner {
                self.record_member(at, owner, name);
            }
        }
        match &t {
            // A map's other members are its keys, when they are strings.
            Type::Map(k, v) => match self.lookup_table_member(name) {
                Lookup::Unknown if self.types.assignable(&Type::String, k) => Lookup::Field((**v).clone(), None),
                other => other,
            },
            Type::Table | Type::Array(_) => self.lookup_table_member(name),
            Type::Any | Type::Unknown | Type::Function | Type::Fn(_) | Type::Userdata | Type::Thread | Type::Range => Lookup::Unknown,
            Type::Nil => {
                if recv.certain {
                    self.error(at, "index-nil", format!("`{name}` is read from a value that is `nil` here"));
                }
                Lookup::Unknown
            }
            Type::Named(nid) => self.lookup_named_member(*nid, name),
            Type::Static(nid) => self.lookup_static_member(*nid, name),
            // A record's members are its fields, also the ones it does not
            // list (`let hooks = {}`, then `hooks.reject = f`): it is not
            // a collection, and gets no functions of `Table`.
            Type::Record(fields) => match fields.iter().find(|(k, _)| k == name) {
                Some((_, ft)) => Lookup::Field(ft.clone(), None),
                None => Lookup::Unknown,
            },
            Type::Number | Type::String | Type::Bool => self.lookup_primitive_member(&t, name),
            Type::Task(r) => self.lookup_task_member(r, name),
            Type::Regex => match self.prelude_type_id("Regex") {
                Some(nid) => self.lookup_instance_member(nid, name),
                None => Lookup::Unknown,
            },
            Type::Worker(exports) => match name {
                "emit" => Lookup::Method(Rc::new(FnType { params: vec![Param { name: "event".into(), ty: Type::String, optional: false }], rest: Some(Type::Any), keyed: None, ret: Type::Nil, is_method: true, is_async: false, named: false, declared: false }), None),
                "stop" => Lookup::Method(Rc::new(FnType { params: Vec::new(), rest: None, keyed: None, ret: Type::Nil, is_method: true, is_async: false, named: false, declared: false }), None),
                _ => {
                    if let Some(m) = exports {
                        let known = matches!(self.types.kind(*m), Some(NamedKind::Module { members }) if members.contains_key(name));
                        if !known {
                            self.error(at, "worker-unknown-fn", format!("the worker file exports no `pub fn {name}`"));
                        }
                    }
                    Lookup::WorkerCall
                }
            },
            Type::Union(members) => {
                // `T?`: the member of `T`, under its declared key. The read
                // fails at runtime when the value is `nil`, which is not
                // known here (`index-nil` is for a certain `nil`).
                let non_nil: Vec<Type> = members.iter().filter(|m| **m != Type::Nil).cloned().collect();
                if non_nil.len() == 1 && non_nil.len() < members.len() {
                    let t = non_nil.into_iter().next().unwrap_or(Type::Any);
                    return self.lookup_member(&Ty::uncertain(t), name, at);
                }
                if let Some(agreed) = self.union_declared_member(&non_nil, name, at) {
                    return agreed;
                }
                // Every member must know the name the same way; otherwise
                // the runtime decides.
                let mut found: Option<Type> = None;
                let mut table_fn: Option<Rc<FnType>> = None;
                for (i, m) in members.iter().enumerate() {
                    match self.lookup_member(&Ty::new(m.clone(), false), name, at) {
                        // The same function of `Table` for every member.
                        Lookup::TableFn(sig) if i == 0 || table_fn.is_some() => table_fn = Some(sig),
                        _ if table_fn.is_some() => return Lookup::Unknown,
                        Lookup::Field(t, None) => found = Some(match found { Some(f) => self.types.union(&f, &t), None => t }),
                        Lookup::Method(sig, None) | Lookup::LibraryMethod(sig) => {
                            let r = sig.ret.clone();
                            found = Some(match found { Some(f) => self.types.union(&f, &r), None => r });
                        }
                        _ => return Lookup::Unknown,
                    }
                }
                if let Some(sig) = table_fn {
                    return Lookup::TableFn(sig);
                }
                match found {
                    Some(t) => Lookup::Field(t, None),
                    None => Lookup::Unknown,
                }
            }
            // `S & I`: the first part that knows the name (the struct's
            // fields and methods, then the interface's declared members).
            Type::And(parts) => {
                for p in parts.iter() {
                    let found = self.lookup_member(&Ty::new(p.clone(), recv.certain), name, at);
                    if !matches!(found, Lookup::Unknown) {
                        return found;
                    }
                }
                Lookup::Unknown
            }
            Type::Not(_) | Type::Never | Type::Tuple(_) => Lookup::Unknown,
        }
    }

    /// A member of a union of declared types (host types, Lua classes, with
    /// `nil` taken out) that every one of them declares as the same kind of
    /// member under the same Lua key: that member, as it is on the first,
    /// with the union of the fields' types. `None` when they disagree, or
    /// when a part is not a declared type.
    fn union_declared_member(&mut self, parts: &[Type], name: &str, at: TextRange) -> Option<Lookup> {
        if parts.len() < 2 || !parts.iter().all(|p| self.is_declared_instance(p)) {
            return None;
        }
        let mut agreed: Option<Lookup> = None;
        for p in parts {
            let found = self.lookup_member(&Ty::uncertain(p.clone()), name, at);
            agreed = Some(match (agreed, found) {
                (None, f @ (Lookup::Method(_, Some(_)) | Lookup::Function(_, Some(_)) | Lookup::Field(_, Some(_)))) => f,
                (Some(Lookup::Method(sig, Some(k))), Lookup::Method(_, Some(k2))) if k == k2 => Lookup::Method(sig, Some(k)),
                (Some(Lookup::Function(sig, Some(k))), Lookup::Function(_, Some(k2))) if k == k2 => Lookup::Function(sig, Some(k)),
                (Some(Lookup::Field(t, Some(k))), Lookup::Field(t2, Some(k2))) if k == k2 => Lookup::Field(self.types.union(&t, &t2), Some(k)),
                _ => return None,
            });
        }
        agreed
    }

    /// An instance of a declared host type or Lua class.
    fn is_declared_instance(&self, t: &Type) -> bool {
        matches!(self.types.expand(t), Type::Named(nid) if matches!(self.types.kind(nid), Some(NamedKind::LuaClass { .. } | NamedKind::HostType { .. })))
    }

    /// `Enum.name` on an enum declared in this file, by its name or through
    /// a module (`M.Inner.name`): its variants, `values` and `name` are all
    /// it has, so another name is an error, as in Rust and
    /// TypeScript, not a `nil` at runtime. An imported enum is opaque here:
    /// its variants are not known.
    fn check_enum_member(&mut self, recv_node: &SyntaxNode, recv: &Ty, name: &str, at: TextRange) {
        let nid = match &recv.t {
            Type::Static(nid) => Some(*nid),
            _ => self.static_named(recv_node),
        };
        let Some(nid) = nid else { return };
        if self.types.is_enum(nid) && matches!(self.lookup_static_member(nid, name), Lookup::Unknown) {
            let owner = self.types.name(nid).to_string();
            self.error(at, "unknown-variant", format!("the enum `{owner}` has no variant '{name}'"));
        }
    }

    /// The nominal type a receiver names in this file: a type-level name
    /// (`Color`), or a member of a module that names one (`M.Inner`).
    fn static_named(&self, node: &SyntaxNode) -> Option<NamedId> {
        match node.kind() {
            NAME_REF => match self.res.reference(node.text_range())? {
                Ref::Binding(id) => self.named_of.get(id).copied(),
                _ => None,
            },
            FIELD_EXPR => {
                let owner = self.static_named(&expr_child(node)?)?;
                let name = field_name(node)?;
                match self.types.kind(owner)? {
                    NamedKind::Module { members } => match members.get(name.text())? {
                        Member::Value(Type::Static(nid)) => Some(*nid),
                        _ => None,
                    },
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// A member of an instance of a nominal type.
    fn lookup_named_member(&mut self, nid: NamedId, name: &str) -> Lookup {
        match self.types.kind(nid) {
            Some(NamedKind::Struct { .. }) => {
                if let Some(f) = self.types.field(nid, name) {
                    return Lookup::Field(f.ty, None);
                }
                if let Some(m) = self.types.method(nid, name) {
                    return Lookup::Method(m, None);
                }
                Lookup::Unknown
            }
            Some(NamedKind::Interface { fields, .. }) => {
                if let Some(f) = fields.iter().find(|f| f.name == name) {
                    return Lookup::Field(f.ty.clone(), None);
                }
                if let Some(m) = self.types.method(nid, name) {
                    return Lookup::Method(m, None);
                }
                Lookup::Unknown
            }
            Some(NamedKind::LuaClass { .. } | NamedKind::HostType { .. }) => self.lookup_instance_member(nid, name),
            Some(NamedKind::Enum { .. }) => Lookup::Unknown,
            Some(NamedKind::Alias(t)) => {
                let t = t.clone();
                self.lookup_member(&Ty::uncertain(t), name, TextRange::empty(0.into()))
            }
            _ => Lookup::Unknown,
        }
    }

    /// A member of an instance of a declared Lua class or host type: an
    /// `fn` with `self` is a method, one without is data.
    fn lookup_instance_member(&mut self, nid: NamedId, name: &str) -> Lookup {
        let key = self.decl_key(nid, name);
        let mut cur = Some(nid);
        for _ in 0..64 {
            let Some(c) = cur else { break };
            let member = match self.types.kind(c) {
                Some(NamedKind::HostType { members, .. } | NamedKind::LuaClass { members, .. } | NamedKind::Module { members }) => members.get(name).cloned(),
                _ => None,
            };
            if let Some(m) = member {
                let key = key.clone().or_else(|| self.decl_key(c, name));
                return match m {
                    Member::Function(f) if f.is_method => Lookup::Method(f, key),
                    Member::Function(f) => Lookup::Function(f, key),
                    Member::Value(t) | Member::Const(t) => Lookup::Field(t, key),
                };
            }
            cur = self.types.parent(c);
        }
        Lookup::Unknown
    }

    /// A member of the table that stands for a named thing.
    fn lookup_static_member(&mut self, nid: NamedId, name: &str) -> Lookup {
        match self.types.kind(nid).cloned() {
            Some(NamedKind::Struct { .. }) => match self.types.method(nid, name) {
                Some(m) => Lookup::Method(m, None),
                None => Lookup::Unknown,
            },
            Some(NamedKind::Enum { variants }) => {
                if let Some((_, _)) = variants.iter().find(|(v, _)| v == name) {
                    return Lookup::Field(Type::Named(nid), None);
                }
                match name {
                    "values" => Lookup::Field(Type::Array(Rc::new(Type::Named(nid))), None),
                    "name" => Lookup::Function(Rc::new(FnType { params: vec![Param { name: "value".into(), ty: Type::Any, optional: false }], rest: None, keyed: None, ret: optional(&self.types, &Type::String), is_method: false, is_async: false, named: false, declared: false }), None),
                    _ => Lookup::Unknown,
                }
            }
            Some(NamedKind::Module { members }) => match members.get(name) {
                Some(Member::Function(f)) if f.is_method => Lookup::Method(f.clone(), self.decl_key(nid, name)),
                Some(Member::Function(f)) => Lookup::Function(f.clone(), self.decl_key(nid, name)),
                Some(Member::Value(t) | Member::Const(t)) => Lookup::Field(t.clone(), self.decl_key(nid, name)),
                None => Lookup::Unknown,
            },
            Some(NamedKind::LuaClass { members, .. } | NamedKind::HostType { members, .. }) => match members.get(name) {
                Some(Member::Function(f)) if !f.is_method => Lookup::Function(f.clone(), self.decl_key(nid, name)),
                Some(Member::Function(f)) => Lookup::Field(Type::Fn(f.clone()), self.decl_key(nid, name)),
                Some(Member::Value(t) | Member::Const(t)) => Lookup::Field(t.clone(), self.decl_key(nid, name)),
                None => Lookup::Unknown,
            },
            _ => Lookup::Unknown,
        }
    }

    /// `String.shout` with the library of a primitive type named directly
    /// (`Resolution::plain_primitive_member`): a method defined on the type is
    /// a plain function there, whose first parameter is `self`. `None` for
    /// any other member, which is looked up as usual.
    fn primitive_library_member(&self, recv: &SyntaxNode, name: &str) -> Option<Lookup> {
        let p = self.res.plain_primitive_member(recv, name)?;
        let sig = self.prim_methods.get(p.name)?.get(name)?;
        let ty = match p.name {
            "Number" => Type::Number,
            "String" => Type::String,
            "Table" => Type::Table,
            _ => Type::Bool,
        };
        let mut params = vec![Param { name: "self".into(), ty, optional: false }];
        params.extend(sig.params.iter().cloned());
        Some(Lookup::Function(Rc::new(FnType { params, is_method: false, ..(**sig).clone() }), None))
    }

    /// A member of a number, string or boolean: a method defined in this
    /// file, else a function of the type's library.
    fn lookup_primitive_member(&mut self, t: &Type, name: &str) -> Lookup {
        let key = match t {
            Type::Number => "Number",
            Type::String => "String",
            _ => "Bool",
        };
        if let Some(m) = self.prim_methods.get(key).and_then(|m| m.get(name)) {
            return Lookup::Method(m.clone(), None);
        }
        let Some(lib) = self.prelude_type_id(key) else { return Lookup::Unknown };
        match self.types.kind(lib) {
            Some(NamedKind::Module { members }) => match members.get(name) {
                Some(Member::Function(f)) if f.is_method => Lookup::Method(f.clone(), None),
                Some(Member::Function(f)) => Lookup::LibraryMethod(f.clone()),
                Some(Member::Value(t) | Member::Const(t)) => Lookup::Field(t.clone(), None),
                None => Lookup::Unknown,
            },
            _ => Lookup::Unknown,
        }
    }

    /// `Table`, `[T]`, `{[K]: V}`, a record, or a union of these: a
    /// parameter of a `Table` function that takes a table first.
    pub(crate) fn is_table_type(&self, t: &Type) -> bool {
        match self.types.expand(t) {
            Type::Table | Type::Map(..) | Type::Array(_) | Type::Record(_) => true,
            Type::Union(m) => m.iter().all(|x| self.is_table_type(x)),
            _ => false,
        }
    }

    /// A member of a table: a `fn Table.name` of this file, else a function
    /// of the `Table` library whose first parameter takes a table (not
    /// `range`, `fill`, `from_iter`, `pack`, which make one). Either is
    /// called with the receiver as its first argument.
    pub(crate) fn lookup_table_member(&mut self, name: &str) -> Lookup {
        if let Some(sig) = self.prim_methods.get("Table").and_then(|m| m.get(name)) {
            let mut params = vec![Param { name: "self".into(), ty: Type::Table, optional: false }];
            params.extend(sig.params.iter().cloned());
            return Lookup::TableFn(Rc::new(FnType { params, is_method: false, ..(**sig).clone() }));
        }
        let Some(lib) = self.prelude_type_id("Table") else { return Lookup::Unknown };
        let f = match self.types.kind(lib) {
            Some(NamedKind::Module { members }) => match members.get(name) {
                Some(Member::Function(f)) => f.clone(),
                _ => return Lookup::Unknown,
            },
            _ => return Lookup::Unknown,
        };
        match f.params.first() {
            Some(p) if !f.is_method && self.is_table_type(&p.ty) => Lookup::TableFn(f),
            _ => Lookup::Unknown,
        }
    }

    /// What the declared type of a name receiver says beyond its flow type:
    /// a variable declared a table type keeps the functions of `Table`
    /// for the names its value's type does not know (`let t: Table = {a:
    /// 1}` has the record type `{a: Number}` here, and `t.keys` is still
    /// `Table.keys`); a `**opts` table holds the options a caller passed by
    /// name, so its members are its keys (`opts.max` is the option).
    fn table_member_of_name(&mut self, recv: &SyntaxNode, name: &str, lookup: Lookup) -> Lookup {
        let id = match self.res.reference(recv.text_range()) {
            Some(Ref::Binding(id)) if recv.kind() == NAME_REF => *id,
            _ => return lookup,
        };
        match lookup {
            Lookup::TableFn(_) if self.keyed_params.contains(&id) => Lookup::Unknown,
            Lookup::Unknown => {
                let declared = self.binding_types.get(&id).map(|t| self.types.expand(&t.t));
                if matches!(declared, Some(Type::Table | Type::Map(..) | Type::Array(_))) && !self.keyed_params.contains(&id) {
                    self.lookup_table_member(name)
                } else {
                    Lookup::Unknown
                }
            }
            other => other,
        }
    }

    /// `x.map(f)` on a value of no known type, where `map` is a function of
    /// `Table`: the call is a member call at runtime, and a plain table
    /// has no members but its own keys. A name that a string, number or
    /// boolean also has as a method (`reverse`, `index`) is left alone,
    /// since the value may be one.
    fn untyped_table_method(&mut self, name: &str, at: TextRange) {
        if !matches!(self.lookup_table_member(name), Lookup::TableFn(_)) {
            return;
        }
        for t in [Type::String, Type::Number, Type::Bool] {
            if !matches!(self.lookup_primitive_member(&t, name), Lookup::Unknown) {
                return;
            }
        }
        self.lint(
            at,
            "table-method-untyped",
            format!("`{name}` is a function of the Table library; it is only a method of a value typed `[T]`, `{{[K]: V}}` or `Table` (write `Table.{name}(x, …)` or `x |> Table.{name}(…)`)"),
        );
    }

    /// The parts of a receiver's type that are tables whose members are
    /// the functions of `Table` (`[T]`, `{[K]: V}`, `Table`), and its other
    /// parts, `nil` left out. A name receiver whose flow type has no such
    /// part is the table type it is declared with, as in
    /// [`Self::table_member_of_name`]; a `**opts` table has none, since its
    /// members are the options' keys.
    fn table_parts(&self, recv_node: &SyntaxNode, recv: &Ty) -> (Vec<Type>, Vec<Type>) {
        let mut tables = Vec::new();
        let mut others = Vec::new();
        for m in self.types.members(&recv.t) {
            match self.types.expand(&m) {
                Type::Nil => {}
                t @ (Type::Table | Type::Map(..) | Type::Array(_)) => tables.push(t),
                t => others.push(t),
            }
        }
        if let (NAME_REF, Some(Ref::Binding(id))) = (recv_node.kind(), self.res.reference(recv_node.text_range())) {
            if self.keyed_params.contains(id) {
                return (Vec::new(), Vec::new());
            }
            if tables.is_empty() {
                if let Some(d) = self.binding_types.get(id).map(|t| self.types.expand(&t.t)).filter(|d| matches!(d, Type::Table | Type::Map(..) | Type::Array(_))) {
                    return (vec![d], Vec::new());
                }
            }
        }
        (tables, others)
    }

    /// A member of a table-typed receiver that nothing resolved. A table
    /// has no members of its own at runtime, only keys, so such a read is
    /// never a working method; two cases are certain mistakes and refused:
    ///
    /// - The name is a function of `Table`, but the receiver may also be
    ///   something that has no such function, or has another one (`v:
    ///   [Number] | String`, `v.reverse`): no one call fits both, and
    ///   there is no runtime dispatch, so the value must be narrowed with
    ///   `is` (`ambiguous-member`).
    /// - The receiver is a list, which has no string keys, and no function
    ///   of `Table` (the library's, this file's `fn Table.name` or an
    ///   imported file's) has the name (`unknown-table-member`). `n` too:
    ///   a rest table keeps its count outside it, and what `Table.pack`
    ///   makes, which has a key `n` for Lua code, is a `Table`, not a list.
    ///
    /// On a `Table` or a map, another name is a key, read as one.
    fn unresolved_table_member(&mut self, recv_node: &SyntaxNode, recv: &Ty, name: &str, at: TextRange) {
        let (tables, others) = self.table_parts(recv_node, recv);
        let Some(table) = tables.first().cloned() else { return };
        if matches!(self.lookup_table_member(name), Lookup::TableFn(_)) {
            let Some(other) = others.first().cloned() else { return };
            if others.iter().any(|o| matches!(o, Type::Any | Type::Unknown)) {
                return;
            }
            let (t, o) = (self.show(&table), self.show(&other));
            self.error(at, "ambiguous-member", format!("`{name}` is a function of the Table library on a `{t}` but not on a `{o}`, and this value may be either: narrow it with `is` first, or call `Table.{name}` on the table"));
        } else if others.is_empty() && tables.iter().all(|t| matches!(t, Type::Array(_))) {
            let key = bracket_key(recv_node, name);
            let in_library = self.prelude_type_id("Table").is_some_and(|lib| matches!(self.types.kind(lib), Some(NamedKind::Module { members }) if members.contains_key(name)));
            if in_library {
                self.error(at, "unknown-table-member", format!("`{name}` is a function of the Table library that does not take a table first, so it is not a method of one: call `Table.{name}(…)`"));
            } else {
                self.error(at, "unknown-table-member", format!("`{name}` is not a function of the Table library; a table has no methods of its own: define `fn Table.{name}` in this file or a file it imports, or read the key {key}"));
            }
        }
    }

    /// `x.name = v`, `x.name += v`, `x.name ??= v`: on a receiver typed
    /// `[T]`, `{[K]: V}` or `Table`, a member that names a function of
    /// `Table` is that function when it is read (`x.count` is
    /// `Table.count(x)`), so storing a key under that name would make the
    /// two directions disagree; the store is refused. A `**opts` table
    /// keeps its keys, and a record its fields (neither resolves to a
    /// function of `Table`).
    pub(crate) fn table_method_assign(&mut self, recv_node: &SyntaxNode, recv: &Ty, name: &str, at: TextRange) {
        let (tables, others) = self.table_parts(recv_node, recv);
        if tables.is_empty() {
            return;
        }
        let lookup = self.lookup_member(recv, name, at);
        if let Lookup::TableFn(_) = self.table_member_of_name(recv_node, name, lookup) {
            let shown = self.table_receiver_text(recv_node, &[tables, others].concat());
            let advice = match receiver_text(recv_node) {
                Some(x) => format!("write {x}[\"{name}\"]"),
                None => format!("index the key with brackets, `[\"{name}\"]`,"),
            };
            self.error(at, "table-method-assign", format!("`{name}` is a function of the Table library on {shown}; {advice} to use it as a key"));
        }
    }

    /// A table receiver's type as a diagnostic names it: "a value typed
    /// `T`", except that `[never]`, the type of an empty list literal,
    /// says nothing to the reader: a name declared with a type shows that
    /// type, and another receiver is "a list".
    fn table_receiver_text(&self, recv_node: &SyntaxNode, parts: &[Type]) -> String {
        let empty_list = |t: &Type| matches!(self.types.expand(t), Type::Array(e) if matches!(*e, Type::Never));
        if !parts.is_empty() && parts.iter().all(empty_list) {
            if let (NAME_REF, Some(Ref::Binding(id))) = (recv_node.kind(), self.res.reference(recv_node.text_range())) {
                if self.binding(*id).is_some_and(|b| b.annotation.is_some()) {
                    if let Some(d) = self.binding_types.get(id) {
                        return format!("a value typed `{}`", self.show(&d.t));
                    }
                }
            }
            return "a list".to_string();
        }
        format!("a value typed `{}`", self.show(&self.types.union_all(parts)))
    }

    fn lookup_task_member(&mut self, result: &Type, name: &str) -> Lookup {
        let method = |params: Vec<Param>, ret: Type| Lookup::Method(Rc::new(FnType { params, rest: None, keyed: None, ret, is_method: true, is_async: false, named: false, declared: false }), None);
        match name {
            "await" => method(Vec::new(), result.clone()),
            "on_done" | "on_error" => method(vec![Param { name: "callback".into(), ty: Type::Function, optional: false }], Type::Task(Rc::new(result.clone()))),
            _ => Lookup::Unknown,
        }
    }

    /// `x[k]`.
    fn index_expr(&mut self, node: &SyntaxNode) -> Ty {
        let parts: Vec<SyntaxNode> = expr_children(node).collect();
        let (Some(o), Some(k)) = (parts.first(), parts.get(1)) else { return Ty::any() };
        let recv = self.expr1(o);
        let key = self.expr1(k);
        self.note_table_read(node, &recv);
        self.index_of(node, &recv, &key)
    }

    /// Records a read whose receiver is certain to be a Lua table
    /// ([`crate::Hints::table_reads`]).
    fn note_table_read(&mut self, node: &SyntaxNode, recv: &Ty) {
        if recv.certain && self.opts.static_resolution && matches!(self.types.expand(&recv.t), Type::Table | Type::Array(_) | Type::Map(..) | Type::Record(_)) {
            self.hints.table_reads.insert(node.text_range());
        }
    }

    /// The value of `recv[key]`.
    fn index_of(&mut self, node: &SyntaxNode, recv: &Ty, key: &Ty) -> Ty {
        match self.types.expand(&recv.t) {
            // A range index clamps and gives `""` when nothing is selected;
            // one position out of range is `nil`.
            Type::String if key.t == Type::Range => Ty::uncertain(Type::String),
            Type::String => Ty::uncertain(optional(&self.types, &Type::String)),
            Type::Array(e) => Ty::uncertain((*e).clone()),
            Type::Map(_, v) => Ty::uncertain((*v).clone()),
            Type::Nil if recv.certain => {
                self.error(node.text_range(), "index-nil", "indexing a value that is `nil` here");
                Ty::any()
            }
            Type::Number | Type::Bool if recv.certain && key.t != Type::String => {
                let shown = self.show(&recv.t);
                self.error(node.text_range(), "index-primitive", format!("a {shown} cannot be indexed"));
                Ty::any()
            }
            // Each member, as uncertain, so that none is reported for what
            // another may be at runtime. A `nil` member raises there (a VM
            // error) and adds no value; it is not reported,
            // as a member read of an optional is not.
            Type::Union(members) => {
                let mut out = Vec::new();
                for m in members.iter().filter(|m| **m != Type::Nil) {
                    out.push(self.index_of(node, &Ty::uncertain(m.clone()), key).t);
                }
                Ty::uncertain(self.types.union_all(&out))
            }
            _ => Ty::any(),
        }
    }

    /// `f!`, `x.m!`: the value itself.
    fn bang(&mut self, node: &SyntaxNode) -> Ty {
        let Some(inner) = expr_child(node) else { return Ty::any() };
        match inner.kind() {
            NAME_REF => {
                if let Some(Ref::Binding(id)) = self.res.reference(inner.text_range()) {
                    if self.binding(*id).is_some_and(|b| b.kind == BindingKind::Variable) {
                        let name = self.binding(*id).map(|b| b.name.clone()).unwrap_or_default();
                        self.lint(node.text_range(), "redundant-bang", format!("`{name}` is a variable, which is never called automatically; the `!` does nothing"));
                    }
                }
                self.name_raw(&inner)
            }
            FIELD_EXPR => {
                let Some(recv_node) = expr_child(&inner) else { return Ty::any() };
                let name = field_name(&inner).map(|t| t.text().to_string()).unwrap_or_default();
                let recv = self.expr1(&recv_node);
                self.note_table_read(&inner, &recv);
                self.check_enum_member(&recv_node, &recv, &name, inner.text_range());
                let lookup = self.lookup_member(&recv, &name, inner.text_range());
                let lookup = self.table_member_of_name(&recv_node, &name, lookup);
                if let Lookup::Unknown = lookup {
                    self.unresolved_table_member(&recv_node, &recv, &name, inner.text_range());
                }
                self.record_member_hint(&inner, &recv, &lookup, &name);
                // A method comes bound to the receiver, so its value takes
                // the arguments without `self`; on a string, number or
                // boolean it is the library's function, which takes the
                // receiver first.
                let primitive = matches!(self.types.expand(&recv.t), Type::Number | Type::String | Type::Bool);
                match lookup {
                    Lookup::Field(t, _) => Ty::uncertain(t),
                    Lookup::Method(sig, _) if !primitive => Ty::uncertain(Type::Fn(Rc::new(FnType { is_method: false, ..(*sig).clone() }))),
                    Lookup::Method(sig, _) | Lookup::Function(sig, _) | Lookup::LibraryMethod(sig) | Lookup::TableFn(sig) => Ty::uncertain(Type::Fn(sig)),
                    _ => Ty::any(),
                }
            }
            // `@m!` is `self.m!`: a method of the struct comes bound, so it
            // takes the arguments without `self`.
            AT_EXPR => {
                let name = token(&inner, AT_NAME).map(|t| t.text().trim_start_matches('@').to_string()).unwrap_or_default();
                let method = match self.self_type().map(|t| self.types.expand(&t)) {
                    Some(Type::Named(nid)) if self.types.is_struct(nid) && self.types.field(nid, &name).is_none() => self.types.method(nid, &name),
                    _ => None,
                };
                match method {
                    Some(sig) => {
                        if self.opts.static_resolution {
                            self.hints.members.insert(inner.text_range(), MemberHint::Method { key: name });
                        }
                        Ty::uncertain(Type::Fn(Rc::new(FnType { is_method: false, ..(*sig).clone() })))
                    }
                    None => self.at_expr(&inner),
                }
            }
            _ => self.expr(&inner),
        }
    }

    // ---- calls -----------------------------------------------------------------

    /// `CALL_EXPR`, `COMMAND_CALL` or `METHOD_CALL`.
    pub(crate) fn call(&mut self, node: &SyntaxNode) -> Ty {
        match node.kind() {
            METHOD_CALL => self.method_call(node, Vec::new()),
            _ => {
                let Some(callee) = expr_child(node) else { return Ty::any() };
                let list = child(node, ARG_LIST).or_else(|| child(node, COMMAND_ARGS));
                let block = child(node, TRAILING_BLOCK);
                self.call_target(&callee, Vec::new(), list.as_ref(), block.as_ref())
            }
        }
    }

    /// The typed arguments of an argument list.
    fn args_of(&mut self, list: Option<&SyntaxNode>) -> Vec<ArgTy> {
        let mut out = Vec::new();
        let Some(list) = list else { return out };
        for c in list.children() {
            match c.kind() {
                SPREAD_ARG => {
                    if let Some(e) = expr_child(&c) {
                        let ty = self.expr1(&e);
                        out.push(ArgTy { ty, range: c.text_range(), spread: true, keyed: None, keyed_spread: false, fn_kind_name: None, is_fn_literal: false });
                    }
                }
                KEYED_ARG => {
                    let key = first_token(&c).map(|t| t.text().to_string()).unwrap_or_default();
                    if let Some(e) = expr_child(&c) {
                        self.autocall_argument(&e);
                        let ty = self.expr1(&e);
                        out.push(ArgTy { ty, range: c.text_range(), spread: false, keyed: Some(key), keyed_spread: false, fn_kind_name: None, is_fn_literal: false });
                    }
                }
                KEYED_SPREAD_ARG => {
                    if let Some(e) = expr_child(&c) {
                        let ty = self.expr1(&e);
                        out.push(ArgTy { ty, range: c.text_range(), spread: false, keyed: None, keyed_spread: true, fn_kind_name: None, is_fn_literal: false });
                    }
                }
                k if is_expr(k) => {
                    let fn_kind_name = self.autocall_argument(&c);
                    let ty = self.expr(&c);
                    out.push(ArgTy { ty, range: c.text_range(), spread: false, keyed: None, keyed_spread: false, fn_kind_name, is_fn_literal: matches!(c.kind(), FN_EXPR | LAMBDA_EXPR) });
                }
                _ => {}
            }
        }
        out
    }

    /// An argument that is a bare function-kind name (a `fn`, a name
    /// annotated `Function`, `let f = fn … end`, a declared function):
    /// the name is called, with no arguments, and its result is passed. The
    /// name is returned, for the `fn-kind-argument` warning of a parameter
    /// that takes a function. When the function is known to take a
    /// required parameter, the call cannot be what was meant (the parameter
    /// gets `nil`), so it is the lint `autocall-argument`; a function of no
    /// required parameter is called on purpose (`print now`), and one whose
    /// parameters are unknown is left alone. `check_call` drops the lint
    /// where it warns `fn-kind-argument` instead.
    fn autocall_argument(&mut self, arg: &SyntaxNode) -> Option<String> {
        if arg.kind() != NAME_REF {
            return None;
        }
        let (name, sig) = match self.res.reference(arg.text_range())? {
            Ref::Binding(id) => {
                let b = self.binding(*id).filter(|b| b.kind == BindingKind::Function)?;
                let name = b.name.clone();
                let sig = self.binding_fn_type(*id).or_else(|| match self.binding_types.get(id).map(|t| self.types.expand(&t.t)) {
                    Some(Type::Fn(f)) => Some(f),
                    _ => None,
                });
                (name, sig)
            }
            Ref::Declared(id) if self.decls.binding_kind(*id) == BindingKind::Function => {
                let name = self.decls.get(*id)?.name.clone();
                let sig = match self.declared_value(*id).t {
                    Type::Fn(f) => Some(f),
                    _ => None,
                };
                (name, sig)
            }
            _ => return None,
        };
        let required = sig.as_ref().and_then(|f| f.params.iter().find(|p| !p.optional && (matches!(p.ty, Type::Any | Type::Unknown) || !self.types.may_be_nil(&p.ty))));
        if let Some(p) = required {
            let param = if p.name.is_empty() { String::new() } else { format!(" '{}'", p.name) };
            self.lint(
                arg.text_range(),
                "autocall-argument",
                format!("`{name}` is called here, with no arguments, because a bare function name is a call, and its parameter{param} gets `nil`; write `{name}!` to pass the function itself, or `{name}()` to call it"),
            );
        }
        Some(name)
    }

    /// A call of `callee` with the piped values `pre`, the written
    /// arguments and the trailing block.
    pub(crate) fn call_target(&mut self, callee: &SyntaxNode, pre: Vec<ArgTy>, list: Option<&SyntaxNode>, block: Option<&SyntaxNode>) -> Ty {
        let mut args = pre;
        args.extend(self.args_of(list));
        let block_ty = block.map(|b| (self.block_fn(b), b.text_range()));
        // `f?.(x)`: the call happens when the function is not `nil`.
        let safe_call = is_safe_receiver(callee);
        match callee.kind() {
            NAME_REF => {
                let name = name_text(callee).unwrap_or_default();
                let f = self.name_raw(callee);
                let f = if safe_call { Ty::new(self.types.non_nil(&f.t), f.certain) } else { f };
                let checked = self.checked_fn_name(callee);
                self.call_value(&f, &name, &args, block_ty, callee.text_range(), checked)
            }
            FIELD_EXPR if field_name(callee).is_some_and(|t| t.text() != "len") => {
                let Some(recv_node) = expr_child(callee) else { return Ty::any() };
                let name = field_name(callee).map(|t| t.text().to_string()).unwrap_or_default();
                if let Some(handle) = self.worker_new(&recv_node, &name, &args, callee) {
                    return handle;
                }
                let recv = self.expr1(&recv_node);
                self.check_enum_member(&recv_node, &recv, &name, callee.text_range());
                let lookup = self.lookup_member(&recv, &name, callee.text_range());
                let lookup = self.table_member_of_name(&recv_node, &name, lookup);
                let lookup = self.primitive_library_member(&recv_node, &name).unwrap_or(lookup);
                self.record_member_hint(callee, &recv, &lookup, &name);
                let desc = format!("{}.{name}", self.show(&recv.t));
                let is_table_fn = matches!(lookup, Lookup::TableFn(_));
                match lookup {
                    Lookup::Method(sig, _) => {
                        self.check_call(&sig, &desc, &args, block_ty, callee.text_range(), false);
                        self.call_result(&sig)
                    }
                    Lookup::LibraryMethod(sig) | Lookup::TableFn(sig) => {
                        let mut all = vec![ArgTy { ty: receiver_ty(&self.types, &recv, is_table_fn), range: recv_node.text_range(), spread: false, keyed: None, keyed_spread: false, fn_kind_name: None, is_fn_literal: false }];
                        all.extend(args);
                        self.check_call(&sig, &desc, &all, block_ty, callee.text_range(), false);
                        self.call_result(&sig)
                    }
                    Lookup::Unknown if matches!(self.types.expand(&recv.t), Type::Any | Type::Unknown) => {
                        self.untyped_table_method(&name, callee.text_range());
                        Ty::any()
                    }
                    Lookup::Function(sig, _) => {
                        self.check_call(&sig, &desc, &args, block_ty, callee.text_range(), false);
                        self.call_result(&sig)
                    }
                    Lookup::Field(t, _) => {
                        let t = if safe_call { self.types.non_nil(&t) } else { t };
                        let f = Ty::uncertain(t);
                        self.call_value(&f, &desc, &args, block_ty, callee.text_range(), false)
                    }
                    Lookup::WorkerCall => {
                        for a in &args {
                            self.check_sendable(a);
                        }
                        Ty::uncertain(Type::Task(Rc::new(Type::Any)))
                    }
                    Lookup::Unknown => {
                        self.unresolved_table_member(&recv_node, &recv, &name, callee.text_range());
                        Ty::any()
                    }
                }
            }
            METHOD_CALL if child(callee, ARG_LIST).is_none() && child(callee, TRAILING_BLOCK).is_none() => self.method_call(callee, args),
            LUA_PATH => {
                self.lua_path(callee);
                Ty::any()
            }
            AT_EXPR => {
                let f = self.at_expr(callee);
                let f = if safe_call { Ty::new(self.types.non_nil(&f.t), f.certain) } else { f };
                let name = token(callee, AT_NAME).map(|t| t.text().to_string()).unwrap_or_default();
                self.call_value(&f, &name, &args, block_ty, callee.text_range(), false)
            }
            _ => {
                let f = self.expr1(callee);
                self.call_value(&f, "the callee", &args, block_ty, callee.text_range(), false)
            }
        }
    }

    /// `Worker.new 'path'` with a literal path: the analyzer
    /// reads the worker file as `import` does, so that calls through the
    /// handle are checked against its `pub fn`s; a file that exports
    /// `emit` or `stop` is an error. `None` when this is not such a call.
    fn worker_new(&mut self, recv: &SyntaxNode, name: &str, args: &[ArgTy], callee: &SyntaxNode) -> Option<Ty> {
        if name != "new" || recv.kind() != NAME_REF || name_text(recv).as_deref() != Some("Worker") {
            return None;
        }
        if !matches!(self.res.reference(recv.text_range()), Some(Ref::Prelude(p)) if p.name == "Worker") {
            return None;
        }
        let path = match args.first() {
            Some(a) if args.len() == 1 && !a.spread && a.keyed.is_none() && !a.keyed_spread => {
                // The literal, from the argument node.
                let list = callee.parent().and_then(|c| child(&c, ARG_LIST).or_else(|| child(&c, COMMAND_ARGS)));
                list.and_then(|l| expr_children(&l).next()).filter(|e| e.kind() == LITERAL).and_then(|e| first_token(&e)).filter(|t| t.kind() == STRING).map(|t| crate::resolver::decode_plain_string(&t))
            }
            _ => None,
        };
        let Some(path) = path else {
            return Some(Ty::uncertain(Type::Worker(None)));
        };
        let Some(imports) = self.opts.imports else {
            return Some(Ty::uncertain(Type::Worker(None)));
        };
        match imports.exports(&path) {
            Ok(exports) => {
                let mut members = std::collections::HashMap::new();
                for e in &exports {
                    if e.decl == DeclKind::Fn {
                        if e.name == "emit" || e.name == "stop" {
                            self.error(callee.text_range(), "worker-reserved-export", format!("the worker file '{path}' exports `{}`, which is the name of a handle method; a worker file may not export `emit` or `stop`", e.name));
                        }
                        members.insert(e.name.clone(), Member::Function(Rc::new(FnType::unknown())));
                    }
                }
                let nid = self.types.add(&format!("worker '{path}'"), NamedKind::Module { members });
                Some(Ty::uncertain(Type::Worker(Some(nid))))
            }
            Err(msg) => {
                self.error(callee.text_range(), "worker-file", msg);
                Some(Ty::uncertain(Type::Worker(None)))
            }
        }
    }

    /// A call of a value: a function with a signature is checked, any
    /// other callable gives `any`.
    /// The result of calling a value of a function type is uncertain
    /// unless `checked` (the callee is an NL function, see
    /// [`Self::checked_fn_name`]): only the runtime-checked part of a type
    /// is relied on, and of a signature that is "a
    /// function", which may be a Lua function returning anything.
    fn call_value(&mut self, f: &Ty, desc: &str, args: &[ArgTy], block: Option<(Ty, TextRange)>, at: TextRange, checked: bool) -> Ty {
        match self.types.expand(&f.t) {
            Type::Fn(sig) => {
                self.check_call(&sig, desc, args, block, at, false);
                self.call_of(&sig, checked)
            }
            Type::Named(nid) if self.types.is_struct(nid) => match self.types.operator(nid, "()") {
                Some(sig) => {
                    self.check_call(&sig, desc, args, block, at, false);
                    self.call_result(&sig)
                }
                None => Ty::any(),
            },
            Type::Nil if f.certain => {
                self.error(at, "call-nil", format!("{desc} is `nil` here and cannot be called"));
                Ty::any()
            }
            Type::Number | Type::String | Type::Bool if f.certain => {
                let shown = self.show(&f.t);
                self.error(at, "call-primitive", format!("{desc} is a {shown} and cannot be called"));
                Ty::any()
            }
            _ => Ty::any(),
        }
    }

    /// True when `name` (a `NAME_REF`) is an NL function whose `return` is
    /// checked against its signature: a `fn` declaration, or
    /// a `let f = fn … end` that nothing assigns again. Not a `global`,
    /// which Lua code can replace, nor a variable of a function type.
    pub(crate) fn checked_fn_name(&self, name: &SyntaxNode) -> bool {
        match self.res.reference(name.text_range()) {
            Some(Ref::Binding(id)) => {
                self.binding_sigs.contains_key(id) && !self.assigned.contains(id) && self.binding(*id).is_some_and(|b| b.decl != DeclKind::Global)
            }
            _ => false,
        }
    }

    /// [`Self::call_result`], made uncertain unless `checked`.
    pub(crate) fn call_of(&self, sig: &FnType, checked: bool) -> Ty {
        let r = self.call_result(sig);
        if checked { r } else { Ty::uncertain(r.t) }
    }

    /// The result of a call: the return type, a `Task` for an `async fn`.
    /// The result of an NL function is checked at its return,
    /// so it is certain; a declared Lua function's is trusted for what it
    /// emits, not for dropping a check.
    pub(crate) fn call_result(&self, sig: &FnType) -> Ty {
        let certain = !sig.declared && self.types.is_definite(&sig.ret) && sig.ret != Type::Any;
        if sig.is_async {
            return Ty::new(Type::Task(Rc::new(sig.ret.clone())), certain);
        }
        Ty::new(sig.ret.clone(), certain)
    }

    /// Checks the arguments of a call against a signature. `bare`: the call is a bare function-kind name, which
    /// takes no arguments.
    pub(crate) fn check_call(&mut self, sig: &FnType, desc: &str, args: &[ArgTy], block: Option<(Ty, TextRange)>, at: TextRange, bare: bool) {
        let _ = bare;
        let spread = args.iter().any(|a| a.spread);
        let positional: Vec<&ArgTy> = args.iter().filter(|a| !a.spread && a.keyed.is_none() && !a.keyed_spread).collect();
        let keyed: Vec<&ArgTy> = args.iter().filter(|a| a.keyed.is_some() || a.keyed_spread).collect();
        let has_keyed = !keyed.is_empty();
        // The block is the final positional argument.
        let mut all: Vec<(Ty, TextRange, Option<String>)> = positional.iter().map(|a| (a.ty.clone(), a.range, a.fn_kind_name.clone())).collect();
        if let Some((bt, br)) = &block {
            all.push((bt.clone(), *br, None));
        }
        if let Some((_, br)) = &block {
            // The resolver checks the file's own functions by the block
            // rule; a declared Lua function is checked here, and
            // leniently: a parameter that can hold a function (`Function`,
            // a signature, a union with one, `any`) takes the block.
            let accepts = sig.params.iter().any(|p| self.accepts_function(&p.ty)) || sig.rest.is_some();
            if !accepts {
                self.error(*br, "block-without-function-param", format!("a block can only be passed to a function that declares a `Function` parameter, and {desc} has none"));
            }
        }
        if !spread {
            for (i, (ty, range, fn_name)) in all.iter().enumerate() {
                let Some(p) = sig.params.get(i) else { break };
                let what = if p.name.is_empty() { format!("argument {} of {desc}", i + 1) } else { format!("parameter '{}' of {desc}", p.name) };
                if let Some(n) = fn_name {
                    if takes_block(&p.ty) {
                        let at = *range;
                        self.diags.retain(|d| !(d.range == at && d.code == "autocall-argument"));
                        self.warn(*range, "fn-kind-argument", format!("`{n}` is a function-kind name, so it is called here and its result is passed; write `{n}!` to pass the function itself"));
                        continue;
                    }
                }
                self.check_assignable(ty, &p.ty, *range, &what);
            }
            // A missing argument is `nil`, which a required parameter of a
            // type that excludes `nil` rejects at runtime.
            if !bare {
                for p in sig.params.iter().skip(all.len()) {
                    if p.optional || p.ty == Type::Any || self.types.may_be_nil(&p.ty) || !self.types.is_definite(&p.ty) {
                        continue;
                    }
                    let shown = self.show(&p.ty);
                    self.error(at, "missing-argument", format!("parameter '{}' of {desc} expects {shown}, and no argument is given for it", p.name));
                    break;
                }
            }
        }
        if has_keyed {
            match &sig.keyed {
                None => {
                    // Keyed arguments that land in a parameter raise
                    // TypeError; beyond the parameters they are dropped.
                    // Both are reported.
                    let slot = all.len();
                    let in_params = slot < sig.params.len() || sig.rest.is_some();
                    let range = keyed.first().map(|a| a.range).unwrap_or(at);
                    if in_params {
                        self.error(range, "keyed-args-rejected", format!("{desc} has no `**opts` parameter, so keyed arguments are refused"));
                    } else {
                        self.warn(range, "keyed-args-dropped", format!("{desc} has no `**opts` parameter; these keyed arguments are beyond its parameters and are dropped"));
                    }
                }
                Some(Some(t)) => {
                    if let Type::Record(fields) = self.types.expand(t) {
                        for a in &keyed {
                            let Some(k) = &a.keyed else { continue };
                            match fields.iter().find(|(fk, _)| fk == k) {
                                Some((_, ft)) => {
                                    let what = format!("keyed argument '{k}' of {desc}");
                                    self.check_assignable(&a.ty, ft, a.range, &what);
                                }
                                None => self.error(a.range, "unknown-keyed-arg", format!("{desc} takes no keyed argument '{k}'")),
                            }
                        }
                    }
                }
                Some(None) => {}
            }
        }
    }

    /// A parameter type that can hold a function.
    fn accepts_function(&self, t: &Type) -> bool {
        match self.types.expand(t) {
            Type::Any | Type::Function | Type::Fn(_) => true,
            Type::Union(m) => m.iter().any(|x| self.accepts_function(x)),
            _ => false,
        }
    }

    /// `x:m(args)`: Lua's method call. On a string receiver
    /// the method is NL's; otherwise nothing is known.
    fn method_call(&mut self, node: &SyntaxNode, pre: Vec<ArgTy>) -> Ty {
        let Some(recv) = expr_child(node) else { return Ty::any() };
        let r = self.expr1(&recv);
        let mut args = pre;
        args.extend(self.args_of(child(node, ARG_LIST).or_else(|| child(node, COMMAND_ARGS)).as_ref()));
        if let Some(b) = child(node, TRAILING_BLOCK) {
            self.block_fn(&b);
        }
        if r.t == Type::String {
            let name = crate::util::method_name_of(node).unwrap_or_default();
            if let Lookup::LibraryMethod(sig) | Lookup::Method(sig, _) = self.lookup_member(&r, &name, node.text_range()) {
                return self.call_result(&sig);
            }
        }
        Ty::any()
    }

    /// `a |> f(b)`.
    fn pipe(&mut self, node: &SyntaxNode) -> Ty {
        let parts: Vec<SyntaxNode> = expr_children(node).collect();
        let (Some(lhs), Some(target)) = (parts.first(), parts.get(1)) else { return Ty::any() };
        let value = self.expr1(lhs);
        let pre = vec![ArgTy { ty: value, range: lhs.text_range(), spread: false, keyed: None, keyed_spread: false, fn_kind_name: None, is_fn_literal: false }];
        match target.kind() {
            CALL_EXPR => {
                let Some(callee) = expr_child(target) else { return Ty::any() };
                let block = child(target, TRAILING_BLOCK);
                self.call_target(&callee, pre, child(target, ARG_LIST).as_ref(), block.as_ref())
            }
            METHOD_CALL => self.method_call(target, pre),
            _ => self.call_target(target, pre, None, None),
        }
    }

    /// A Lua path: its expressions are checked; its value is
    /// unknown.
    fn lua_path(&mut self, node: &SyntaxNode) -> Ty {
        for step in node.children() {
            for e in step.descendants().filter(|d| is_expr(d.kind()) && d.parent().is_some_and(|p| !is_expr(p.kind()) || p.kind() == LUA_PATH)) {
                if e.ancestors().skip(1).take_while(|a| a != &step).any(|a| is_expr(a.kind())) {
                    continue;
                }
                self.expr(&e);
            }
            if let Some(b) = child(&step, TRAILING_BLOCK) {
                self.block_fn(&b);
            }
        }
        Ty::any()
    }

    // ---- operators -----------------------------------------------------------------

    fn prefix(&mut self, node: &SyntaxNode) -> Ty {
        let op = first_token(node).map(|t| t.kind());
        let Some(operand) = expr_child(node) else { return Ty::any() };
        let v = self.expr1(&operand);
        match op {
            Some(BANG | NOT_KW) => Ty::certain(Type::Bool),
            Some(MINUS) => match self.types.expand(&v.t) {
                Type::Number => Ty::new(Type::Number, v.certain),
                Type::Named(nid) if self.types.is_struct(nid) => self.types.operator(nid, "-").map(|s| Ty::uncertain(s.ret.clone())).unwrap_or_else(Ty::any),
                // A Lua class or host type with a declared `-`.
                Type::Named(nid) => self.decl_operator(nid, "-").map(|s| Ty::uncertain(s.ret.clone())).unwrap_or_else(Ty::any),
                Type::Nil | Type::Bool if v.certain => {
                    let shown = self.show(&v.t);
                    self.error(node.text_range(), "bad-operand", format!("unary `-` on a {shown}"));
                    Ty::any()
                }
                _ => Ty::any(),
            },
            Some(TILDE) => match self.types.expand(&v.t) {
                Type::Number => {
                    if self.opts.static_resolution && self.direct_op(TILDE, &v, &v) {
                        self.hints.direct_ops.insert(node.text_range());
                    }
                    Ty::new(Type::Number, v.certain)
                }
                Type::Named(nid) if self.types.is_struct(nid) => self.types.operator(nid, "~").map(|s| Ty::uncertain(s.ret.clone())).unwrap_or_else(Ty::any),
                _ => Ty::any(),
            },
            _ => Ty::any(),
        }
    }

    fn binary(&mut self, node: &SyntaxNode) -> Ty {
        let Some(op) = bin_op(node).map(|t| t.kind()) else { return Ty::any() };
        let parts: Vec<SyntaxNode> = expr_children(node).collect();
        let (Some(a), Some(b)) = (parts.first(), parts.get(1)) else { return Ty::any() };
        if matches!(op, AND_KW | AMP2 | OR_KW | PIPE2) {
            return self.logical(op, a, b);
        }
        if op == QUESTION2 {
            // `a ?? b`: the part of `a` that is not `nil`, or
            // `b`, which is evaluated only then.
            let x = self.expr1(a);
            let saved = self.env.clone();
            let y = self.expr1(b);
            self.env = saved;
            return Ty::new(self.types.union(&self.types.non_nil(&x.t), &y.t), x.certain && y.certain);
        }
        let x = self.expr1(a);
        let y = self.expr1(b);
        let r = self.binary_op(node.text_range(), op, &x, &y);
        if self.opts.static_resolution && self.direct_op(op, &x, &y) {
            self.hints.direct_ops.insert(node.text_range());
        }
        r
    }

    /// True when direct code gives the runtime path's result: two numbers,
    /// or two strings for comparisons and concatenation. For floor
    /// division and the bitwise operators the direct code is a call of
    /// `rt.nfloor` or `rt.nband` and its siblings (nl_ir's `binary_op`),
    /// the functions the runtime helper runs for two numbers. Unary `~`
    /// passes its operand twice (`rt.nbnot`, nl_ir's `prefix`).
    pub(crate) fn direct_op(&self, op: SyntaxKind, x: &Ty, y: &Ty) -> bool {
        if !(x.certain && y.certain) {
            return false;
        }
        let both = |t: Type| x.t == t && y.t == t;
        match op {
            LT | LT_EQ | GT | GT_EQ | EQ2 | NEQ | CONCAT | CONCAT_EQ => both(Type::Number) || both(Type::String),
            SLASH2 | SLASH2_EQ | PIPE | PIPE_EQ | AMP | AMP_EQ | CARET | CARET_EQ | SHL | SHL_EQ | SHR | SHR_EQ | USHR | USHR_EQ | TILDE => {
                both(Type::Number)
            }
            _ => false,
        }
    }

    /// The result of a binary operator on two typed operands: the left
    /// operand decides.
    pub(crate) fn binary_op(&mut self, at: TextRange, op: SyntaxKind, x: &Ty, y: &Ty) -> Ty {
        let xt = self.types.expand(&x.t);
        let yt = self.types.expand(&y.t);
        let arithmetic = matches!(op, PLUS | MINUS | STAR | SLASH | PERCENT | STAR2 | SLASH2 | PLUS_EQ | MINUS_EQ | STAR_EQ | SLASH_EQ | PERCENT_EQ | STAR2_EQ | SLASH2_EQ);
        let bitwise = matches!(op, PIPE | AMP | CARET | SHL | SHR | USHR | PIPE_EQ | AMP_EQ | CARET_EQ | SHL_EQ | SHR_EQ | USHR_EQ);
        let compare = matches!(op, LT | LT_EQ | GT | GT_EQ);
        let equality = matches!(op, EQ2 | NEQ);
        let concat = matches!(op, CONCAT | CONCAT_EQ);
        let range = matches!(op, DOT2 | DOT3);
        if equality {
            return Ty::certain(Type::Bool);
        }
        if range {
            for (t, c) in [(&xt, x.certain), (&yt, y.certain)] {
                if c && !self.types.overlaps(t, &Type::Number) {
                    let shown = self.show(t);
                    self.error(at, "bad-operand", format!("a range takes numbers, got {shown}"));
                }
            }
            return Ty::certain(Type::Range);
        }
        // A `nil` operand is an error whatever the operator (no type
        // defines operators for nil).
        if x.certain && xt == Type::Nil || y.certain && yt == Type::Nil && !matches!(xt, Type::Named(_) | Type::Any) {
            let op_text = op.fixed_text().unwrap_or("?");
            self.error(at, "bad-operand", format!("`{op_text}` with a `nil` operand"));
            return Ty::any();
        }
        match &xt {
            Type::Number => {
                if arithmetic || bitwise {
                    if yt == Type::Number {
                        return Ty::new(Type::Number, x.certain && y.certain);
                    }
                    if y.certain && matches!(yt, Type::String | Type::Bool) && self.prim_methods.get("Number").is_none_or(|m| !m.contains_key(op.fixed_text().unwrap_or("").trim_end_matches('='))) {
                        let shown = self.show(&yt);
                        let op_text = op.fixed_text().unwrap_or("?");
                        self.error(at, "bad-operand", format!("`Number {op_text} {shown}`: NL does not coerce strings in arithmetic, and `Number` defines no `{op_text}` here"));
                        return Ty::any();
                    }
                    return Ty::any();
                }
                if compare {
                    return Ty::certain(Type::Bool);
                }
                if concat {
                    if matches!(yt, Type::Number | Type::String) {
                        return Ty::new(Type::String, x.certain && y.certain);
                    }
                    return Ty::uncertain(Type::String);
                }
                Ty::any()
            }
            Type::String => {
                if concat {
                    if matches!(yt, Type::Number | Type::String) {
                        return Ty::new(Type::String, x.certain && y.certain);
                    }
                    return Ty::uncertain(Type::String);
                }
                if compare {
                    return Ty::certain(Type::Bool);
                }
                if arithmetic || bitwise {
                    let name = op.fixed_text().unwrap_or("").trim_end_matches('=').to_string();
                    if let Some(sig) = self.prim_methods.get("String").and_then(|m| m.get(&name)) {
                        return Ty::uncertain(sig.ret.clone());
                    }
                    if x.certain && y.certain && matches!(yt, Type::Number | Type::String | Type::Bool) {
                        let op_text = op.fixed_text().unwrap_or("?");
                        self.error(at, "bad-operand", format!("`String {op_text} …`: NL does not coerce strings in arithmetic, and `String` defines no `{op_text}` here"));
                    }
                    return Ty::any();
                }
                Ty::any()
            }
            Type::Named(nid) if self.types.is_struct(*nid) => {
                let name = op.fixed_text().unwrap_or("").trim_end_matches('=').to_string();
                let name = if name.is_empty() { String::new() } else { name };
                if compare {
                    return Ty::certain(Type::Bool);
                }
                match self.types.operator(*nid, &name) {
                    Some(sig) => Ty::uncertain(sig.ret.clone()),
                    None => Ty::any(),
                }
            }
            // A Lua class or host type of a declaration file: the declared
            // operator gives the result type; the metamethod runs.
            Type::Named(nid) if !compare && (arithmetic || bitwise || concat) => {
                let name = op.fixed_text().unwrap_or("").trim_end_matches('=');
                match self.decl_operator(*nid, name) {
                    Some(sig) => Ty::uncertain(sig.ret.clone()),
                    None => Ty::any(),
                }
            }
            _ => {
                if compare {
                    return Ty::certain(Type::Bool);
                }
                Ty::any()
            }
        }
    }

    /// `a and b`, `a or b`, with narrowing of `b` by `a`.
    fn logical(&mut self, op: SyntaxKind, a: &SyntaxNode, b: &SyntaxNode) -> Ty {
        let is_and = matches!(op, AND_KW | AMP2);
        let x = self.expr1(a);
        let (then_env, else_env) = self.condition_envs(a);
        let saved = self.env.clone();
        self.env = if is_and { then_env } else { else_env };
        let y = self.expr1(b);
        self.env = saved;
        let t = if is_and { self.types.union(&self.types.falsy(&x.t), &y.t) } else { self.types.union(&self.types.truthy(&x.t), &y.t) };
        Ty::new(t, x.certain && y.certain)
    }

    /// `x is T`.
    fn is_expr(&mut self, node: &SyntaxNode) -> Ty {
        if let Some(e) = expr_child(node) {
            self.expr1(&e);
        }
        if let Some(t) = type_child(node) {
            self.type_of(&t);
        }
        Ty::certain(Type::Bool)
    }

    /// `x as T`: the result is a `T`.
    fn as_expr(&mut self, node: &SyntaxNode) -> Ty {
        let Some(e) = expr_child(node) else { return Ty::any() };
        let v = self.expr1(&e);
        let Some(tn) = type_child(node) else { return Ty::any() };
        let target = self.type_of(&tn);
        if self.opts.static_resolution && v.certain {
            // `rt.as` returns the value itself when it is a `T` already
            // (the first step of a conversion), so that is the one conversion a type
            // decides; a `To`/`From` may be declared in another file.
            if self.types.is_definite(&v.t) && self.types.is_definite(&target) && self.types.assignable(&v.t, &target) {
                self.hints.conversions.insert(node.text_range(), AsHint::Identity);
            }
        }
        if target == Type::Any {
            return v;
        }
        Ty::certain(target)
    }

    // ---- functions and blocks --------------------------------------------------------

    /// `fn … end` in expression position.
    fn fn_expr(&mut self, node: &SyntaxNode) -> Ty {
        let sig = match self.fn_sigs.get(&node.text_range()) {
            Some(s) => s.clone(),
            None => {
                let s = Rc::new(self.signature_of(node, false, false, None));
                self.fn_sigs.insert(node.text_range(), s.clone());
                s
            }
        };
        self.function_body(node, &sig, None, false, false);
        Ty::certain(Type::Fn(self.fn_type(node)))
    }

    /// A trailing block: an anonymous function.
    pub(crate) fn block_fn(&mut self, node: &SyntaxNode) -> Ty {
        let sig = match self.fn_sigs.get(&node.text_range()) {
            Some(s) => s.clone(),
            None => {
                let s = Rc::new(self.signature_of(node, false, false, None));
                self.fn_sigs.insert(node.text_range(), s.clone());
                s
            }
        };
        self.function_body(node, &sig, None, false, false);
        Ty::certain(Type::Fn(self.fn_type(node)))
    }

    /// `spawn f(args)` and `spawn do … end`: a `Task<R>`.
    fn spawn(&mut self, node: &SyntaxNode) -> Ty {
        if has_token(node, DO_KW) {
            let sig = Rc::new(FnType { params: Vec::new(), rest: None, keyed: None, ret: Type::Any, is_method: false, is_async: false, named: true, declared: false });
            self.fn_sigs.insert(node.text_range(), sig.clone());
            self.function_body(node, &sig, None, true, false);
            let ret = self.inferred_ret.get(&node.text_range()).cloned().unwrap_or(Type::Any);
            self.check_spawn_loads_this_file(node);
            return Ty::uncertain(Type::Task(Rc::new(self.types.first(&ret))));
        }
        let Some(call) = node.children().find(|c| matches!(c.kind(), CALL_EXPR | COMMAND_CALL)) else { return Ty::any() };
        let Some(callee) = expr_child(&call) else { return Ty::any() };
        let list = child(&call, ARG_LIST).or_else(|| child(&call, COMMAND_ARGS));
        // Keyed arguments are a compile error.
        if let Some(l) = &list {
            for a in l.children() {
                if matches!(a.kind(), KEYED_ARG | KEYED_SPREAD_ARG) {
                    self.error(a.text_range(), "spawn-keyed", "the arguments of `spawn` are positional: keyed arguments cannot be sent");
                }
            }
        }
        let args = self.args_of(list.as_ref());
        for a in &args {
            self.check_sendable(a);
        }
        let result = match callee.kind() {
            NAME_REF => {
                let f = self.name_raw(&callee);
                let name = name_text(&callee).unwrap_or_default();
                if let Some(Ref::Binding(id)) = self.res.reference(callee.text_range()) {
                    if self.binding(*id).is_some_and(|b| b.named) {
                        self.check_spawn_loads_this_file(node);
                    }
                }
                match self.types.expand(&f.t) {
                    Type::Fn(sig) => {
                        self.check_call(&sig, &name, &args, None, callee.text_range(), false);
                        let r = self.call_result(&sig);
                        Ty::uncertain(match r.t {
                            Type::Task(inner) => (*inner).clone(),
                            other => self.types.first(&other),
                        })
                    }
                    _ => Ty::any(),
                }
            }
            FIELD_EXPR => {
                let Some(recv_node) = expr_child(&callee) else { return Ty::any() };
                let name = field_name(&callee).map(|t| t.text().to_string()).unwrap_or_default();
                let recv = self.expr1(&recv_node);
                self.check_enum_member(&recv_node, &recv, &name, callee.text_range());
                let lookup = self.lookup_member(&recv, &name, callee.text_range());
                let lookup = self.table_member_of_name(&recv_node, &name, lookup);
                let lookup = self.primitive_library_member(&recv_node, &name).unwrap_or(lookup);
                match lookup {
                    Lookup::Method(sig, _) | Lookup::Function(sig, _) => {
                        let desc = format!("{}.{name}", self.show(&recv.t));
                        self.check_call(&sig, &desc, &args, None, callee.text_range(), false);
                        if !sig.named {
                            self.error(callee.text_range(), "spawn-callee", format!("{desc} is not a named function, so a worker cannot look it up"));
                        }
                        let r = self.call_result(&sig);
                        Ty::uncertain(match r.t {
                            Type::Task(inner) => (*inner).clone(),
                            other => self.types.first(&other),
                        })
                    }
                    Lookup::TableFn(_) => {
                        self.error(callee.text_range(), "spawn-callee", format!("`{name}` is a function of the Table library, not a named function, so a worker cannot look it up"));
                        Ty::any()
                    }
                    Lookup::Unknown => {
                        self.unresolved_table_member(&recv_node, &recv, &name, callee.text_range());
                        Ty::any()
                    }
                    _ => Ty::any(),
                }
            }
            _ => {
                self.expr(&callee);
                Ty::any()
            }
        };
        Ty::uncertain(Type::Task(Rc::new(result.t)))
    }

    /// An argument that cannot cross to a worker.
    fn check_sendable(&mut self, a: &ArgTy) {
        if a.is_fn_literal {
            self.error(a.range, "unsendable", "an anonymous function cannot be sent to a worker: only named functions cross");
            return;
        }
        if !a.ty.certain {
            return;
        }
        let bad = match self.types.expand(&a.ty.t) {
            Type::Task(_) => Some("a task"),
            Type::Thread => Some("a coroutine"),
            Type::Fn(sig) if !sig.named => Some("an anonymous function"),
            _ => None,
        };
        if let Some(what) = bad {
            self.error(a.range, "unsendable", format!("{what} cannot be sent to a worker"));
        }
    }

    /// A `spawn` that makes workers load this file, which
    /// has top-level statements that a worker would run again.
    fn check_spawn_loads_this_file(&mut self, node: &SyntaxNode) {
        if let Some(at) = self.top_level_effects {
            let _ = at;
            self.warn(token(node, SPAWN_KW).map_or(node.text_range(), |t| t.text_range()), "spawn-loads-file", "this `spawn` makes a worker load this file, whose top level has statements that would run again there; keep a worker file free of top-level effects, or guard them with `unless Worker.parent`");
        }
    }

    /// `on Event do … end`: a handler; its value is the
    /// handler name.
    fn on_expr(&mut self, node: &SyntaxNode) -> Ty {
        let sig = match self.fn_sigs.get(&node.text_range()) {
            Some(s) => s.clone(),
            None => {
                let s = Rc::new(self.signature_of(node, false, false, None));
                self.fn_sigs.insert(node.text_range(), s.clone());
                s
            }
        };
        self.function_body(node, &sig, None, false, true);
        Ty::certain(Type::String)
    }

    // ---- checks ------------------------------------------------------------------

    /// Reports a value of a definite type that cannot be stored where
    /// `to` is declared. Returns whether the check is proven: the value is
    /// certain, both types are definite (a type known only by name accepts
    /// anything here but is checked at runtime), and the value fits.
    pub(crate) fn check_assignable(&mut self, value: &Ty, to: &Type, at: TextRange, what: &str) -> bool {
        if *to == Type::Any {
            return true;
        }
        if self.types.assignable(&value.t, to) {
            return value.certain && self.types.is_definite(&value.t) && self.types.is_definite(to);
        }
        if !self.types.is_definite(&value.t) && value.t != Type::Unknown {
            return false;
        }
        let got = self.show(&value.t);
        let want = self.show(to);
        if value.t == Type::Unknown {
            self.error(at, "unknown-not-narrowed", format!("{what} expects {want}, got a value of type `unknown`, which must be narrowed with `is` before use"));
        } else {
            self.error(at, "type-mismatch", format!("{what} expects {want}, got {got}"));
        }
        false
    }
}
