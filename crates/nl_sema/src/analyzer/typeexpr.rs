//! Type expressions to [`Type`], for the file being
//! analyzed and for declaration files.

use std::collections::HashMap;
use std::rc::Rc;

use nl_syntax::{SyntaxKind, SyntaxNode};

use super::Analyzer;
use crate::binding::*;
use crate::decl::DeclId;
use crate::types::*;
use crate::util::*;

use SyntaxKind::*;

/// Where the names of a type in a declaration file resolve.
#[derive(Clone, Default)]
pub(crate) struct DeclCtx {
    /// What `Self` is: the struct or type whose member is declared.
    pub self_named: Option<NamedId>,
    /// Type parameters in scope (erased: `any`).
    pub type_params: Vec<String>,
}

impl Analyzer<'_> {
    /// The type a type expression of the file names.
    pub(crate) fn type_of(&mut self, ty: &SyntaxNode) -> Type {
        self.type_of_with_self(ty, None)
    }

    /// The same, with `Self` being `self_ty` where the resolver recorded
    /// no target (a method's return type collected before its body).
    pub(crate) fn type_of_with_self(&mut self, ty: &SyntaxNode, self_ty: Option<Type>) -> Type {
        match ty.kind() {
            NAMED_TYPE => self.named_type(ty, self_ty),
            FN_TYPE => Type::Fn(Rc::new(self.fn_type_of(ty, self_ty))),
            UNION_TYPE => {
                let members: Vec<Type> = ty.children().filter(|c| is_type(c.kind())).map(|c| self.type_of_with_self(&c, self_ty.clone())).collect();
                self.types.union_all(&members)
            }
            NOT_TYPE => {
                let inner = type_child(ty).map(|t| self.type_of_with_self(&t, self_ty)).unwrap_or(Type::Any);
                self.types.negate(&inner)
            }
            OPTIONAL_TYPE => {
                let inner = type_child(ty).map(|t| self.type_of_with_self(&t, self_ty)).unwrap_or(Type::Any);
                optional(&self.types, &inner)
            }
            PAREN_TYPE => type_child(ty).map(|t| self.type_of_with_self(&t, self_ty)).unwrap_or(Type::Any),
            TUPLE_TYPE => {
                let members: Vec<Type> = ty.children().filter(|c| is_type(c.kind())).map(|c| self.type_of_with_self(&c, self_ty.clone())).collect();
                Type::Tuple(members.into())
            }
            RECORD_TYPE => {
                let mut fields: Vec<(String, Type)> = Vec::new();
                for f in children(ty, RECORD_FIELD) {
                    let key = match first_token(&f) {
                        Some(t) if t.kind() == STRING => crate::resolver::decode_plain_string(&t),
                        Some(t) => t.text().to_string(),
                        None => continue,
                    };
                    let v = type_child(&f).map(|t| self.type_of_with_self(&t, self_ty.clone())).unwrap_or(Type::Any);
                    fields.push((key, v));
                }
                fields.sort_by(|a, b| a.0.cmp(&b.0));
                Type::Record(fields.into())
            }
            MAP_TYPE => {
                let parts: Vec<Type> = ty.children().filter(|c| is_type(c.kind())).map(|c| self.type_of_with_self(&c, self_ty.clone())).collect();
                let k = parts.first().cloned().unwrap_or(Type::Any);
                let v = parts.get(1).cloned().unwrap_or(Type::Any);
                Type::Map(Rc::new(k), Rc::new(v))
            }
            ARRAY_TYPE => {
                let e = type_child(ty).map(|t| self.type_of_with_self(&t, self_ty)).unwrap_or(Type::Any);
                Type::Array(Rc::new(e))
            }
            _ => Type::Any,
        }
    }

    fn fn_type_of(&mut self, ty: &SyntaxNode, self_ty: Option<Type>) -> FnType {
        let mut params = Vec::new();
        let mut rest = None;
        let mut keyed = None;
        if let Some(list) = child(ty, FN_TYPE_PARAMS) {
            for p in children(&list, FN_TYPE_PARAM) {
                let t = type_child(&p).map(|t| self.type_of_with_self(&t, self_ty.clone())).unwrap_or(Type::Any);
                let name = tokens(&p).find(|t| t.kind() == LNAME).map(|t| t.text().to_string()).unwrap_or_default();
                if has_token(&p, STAR2) {
                    keyed = Some(Some(t));
                } else if has_token(&p, STAR) {
                    rest = Some(t);
                } else {
                    let optional = self.types.may_be_nil(&t) && t != Type::Any;
                    params.push(Param { name, ty: t, optional });
                }
            }
        }
        let ret = child(ty, RETURN_TYPE).and_then(|r| type_child(&r)).map(|t| self.type_of_with_self(&t, self_ty)).unwrap_or(Type::Any);
        FnType { params, rest, keyed, ret, is_method: false, is_async: false, named: false, declared: false }
    }

    fn named_type(&mut self, ty: &SyntaxNode, self_ty: Option<Type>) -> Type {
        if has_token(ty, NIL_KW) {
            return Type::Nil;
        }
        if has_token(ty, SELF_TYPE_KW) {
            return self.self_type_at(ty.text_range(), self_ty);
        }
        let Some(q) = child(ty, QUAL_NAME) else {
            // `any`, `unknown`, `never`.
            return match first_token(ty).map(|t| t.text().to_string()).as_deref() {
                Some("unknown") => Type::Unknown,
                Some("never") => Type::Never,
                _ => Type::Any,
            };
        };
        let segs = qual_segments(&q);
        if segs.first().is_some_and(|s| s.kind() == SELF_TYPE_KW) {
            return self.self_type_at(q.text_range(), self_ty);
        }
        let args: Vec<Type> = child(ty, TYPE_ARGS).map(|a| a.children().filter(|c| is_type(c.kind())).map(|c| self.type_of_with_self(&c, self_ty.clone())).collect()).unwrap_or_default();
        match self.res.type_ref(q.text_range()).cloned() {
            Some(Ref::Binding(id)) => {
                if segs.len() > 1 {
                    // `Lib.Type` through an import table or a module: the
                    // analyzer does not see its declaration.
                    return Type::Any;
                }
                match self.res.binding(id).map(|b| b.decl) {
                    Some(DeclKind::TypeParam) => Type::Any,
                    _ => match self.named_of.get(&id) {
                        Some(nid) => Type::Named(*nid),
                        None => Type::Any,
                    },
                }
            }
            Some(Ref::Prelude(p)) => {
                if segs.len() > 1 {
                    return Type::Any;
                }
                match p.name {
                    "Task" => Type::Task(Rc::new(args.first().cloned().unwrap_or(Type::Any))),
                    _ => self.prelude_type(p),
                }
            }
            Some(Ref::Declared(id)) => self.decl_named.get(&id).map(|n| Type::Named(*n)).unwrap_or(Type::Any),
            _ => Type::Any,
        }
    }

    fn self_type_at(&self, range: nl_syntax::TextRange, self_ty: Option<Type>) -> Type {
        if let Some(t) = self_ty {
            return t;
        }
        match self.res.self_target(range) {
            Some(SelfTarget::Binding(id)) => match self.named_of.get(id) {
                Some(nid) if self.types.is_struct(*nid) => Type::Named(*nid),
                Some(nid) => Type::Static(*nid),
                None => Type::Any,
            },
            Some(SelfTarget::Prelude(p)) => match p.name {
                "Number" => Type::Number,
                "String" => Type::String,
                "Bool" => Type::Bool,
                _ => Type::Any,
            },
            _ => self.self_type().unwrap_or(Type::Any),
        }
    }

    // ---- declaration files --------------------------------------------------------

    /// A type written in a declaration file: names are the prelude's
    /// types and the declared structs and types.
    pub(crate) fn decl_type_of(&mut self, ty: &SyntaxNode, ctx: &DeclCtx) -> Type {
        match ty.kind() {
            NAMED_TYPE => {
                if has_token(ty, NIL_KW) {
                    return Type::Nil;
                }
                if has_token(ty, SELF_TYPE_KW) {
                    return ctx.self_named.map(Type::Named).unwrap_or(Type::Any);
                }
                let Some(q) = child(ty, QUAL_NAME) else {
                    return match first_token(ty).map(|t| t.text().to_string()).as_deref() {
                        Some("unknown") => Type::Unknown,
                        Some("never") => Type::Never,
                        _ => Type::Any,
                    };
                };
                let segs = qual_segments(&q);
                let Some(first) = segs.first() else { return Type::Any };
                if first.kind() == SELF_TYPE_KW {
                    return ctx.self_named.map(Type::Named).unwrap_or(Type::Any);
                }
                let name = first.text().to_string();
                if segs.len() > 1 {
                    // `Module.Type`: a nested declared type.
                    return self.decl_nested_type(&segs.iter().map(|s| s.text().to_string()).collect::<Vec<_>>());
                }
                if ctx.type_params.contains(&name) {
                    return Type::Any;
                }
                let args: Vec<Type> = child(ty, TYPE_ARGS).map(|a| a.children().filter(|c| is_type(c.kind())).map(|c| self.decl_type_of(&c, ctx)).collect()).unwrap_or_default();
                if let Some(p) = crate::prelude::lookup(&name).filter(|p| p.is_type()) {
                    return match p.name {
                        "Task" => Type::Task(Rc::new(args.first().cloned().unwrap_or(Type::Any))),
                        _ => self.prelude_type(p),
                    };
                }
                match self.decls.lookup(&name) {
                    Some(id) if self.decls.is_type(id) => self.decl_named.get(&id).map(|n| Type::Named(*n)).unwrap_or(Type::Any),
                    _ => Type::Any,
                }
            }
            FN_TYPE => {
                let mut params = Vec::new();
                let mut rest = None;
                let mut keyed = None;
                if let Some(list) = child(ty, FN_TYPE_PARAMS) {
                    for p in children(&list, FN_TYPE_PARAM) {
                        let t = type_child(&p).map(|t| self.decl_type_of(&t, ctx)).unwrap_or(Type::Any);
                        let name = tokens(&p).find(|t| t.kind() == LNAME).map(|t| t.text().to_string()).unwrap_or_default();
                        if has_token(&p, STAR2) {
                            keyed = Some(Some(t));
                        } else if has_token(&p, STAR) {
                            rest = Some(t);
                        } else {
                            let optional = self.types.may_be_nil(&t) && t != Type::Any;
                            params.push(Param { name, ty: t, optional });
                        }
                    }
                }
                let ret = child(ty, RETURN_TYPE).and_then(|r| type_child(&r)).map(|t| self.decl_type_of(&t, ctx)).unwrap_or(Type::Any);
                Type::Fn(Rc::new(FnType { params, rest, keyed, ret, is_method: false, is_async: false, named: false, declared: false }))
            }
            UNION_TYPE => {
                let members: Vec<Type> = ty.children().filter(|c| is_type(c.kind())).map(|c| self.decl_type_of(&c, ctx)).collect();
                self.types.union_all(&members)
            }
            NOT_TYPE => {
                let inner = type_child(ty).map(|t| self.decl_type_of(&t, ctx)).unwrap_or(Type::Any);
                self.types.negate(&inner)
            }
            OPTIONAL_TYPE => {
                let inner = type_child(ty).map(|t| self.decl_type_of(&t, ctx)).unwrap_or(Type::Any);
                optional(&self.types, &inner)
            }
            PAREN_TYPE => type_child(ty).map(|t| self.decl_type_of(&t, ctx)).unwrap_or(Type::Any),
            TUPLE_TYPE => {
                let members: Vec<Type> = ty.children().filter(|c| is_type(c.kind())).map(|c| self.decl_type_of(&c, ctx)).collect();
                Type::Tuple(members.into())
            }
            RECORD_TYPE => {
                let mut fields: Vec<(String, Type)> = Vec::new();
                for f in children(ty, RECORD_FIELD) {
                    let key = match first_token(&f) {
                        Some(t) if t.kind() == STRING => crate::resolver::decode_plain_string(&t),
                        Some(t) => t.text().to_string(),
                        None => continue,
                    };
                    let v = type_child(&f).map(|t| self.decl_type_of(&t, ctx)).unwrap_or(Type::Any);
                    fields.push((key, v));
                }
                fields.sort_by(|a, b| a.0.cmp(&b.0));
                Type::Record(fields.into())
            }
            MAP_TYPE => {
                let parts: Vec<Type> = ty.children().filter(|c| is_type(c.kind())).map(|c| self.decl_type_of(&c, ctx)).collect();
                Type::Map(Rc::new(parts.first().cloned().unwrap_or(Type::Any)), Rc::new(parts.get(1).cloned().unwrap_or(Type::Any)))
            }
            ARRAY_TYPE => Type::Array(Rc::new(type_child(ty).map(|t| self.decl_type_of(&t, ctx)).unwrap_or(Type::Any))),
            _ => Type::Any,
        }
    }

    /// `Module.Type` in a declaration: the nested declared type.
    fn decl_nested_type(&self, segs: &[String]) -> Type {
        let Some(first) = segs.first() else { return Type::Any };
        let Some(mut cur) = self.decls.lookup(first).and_then(|id| self.decl_named.get(&id).copied()) else { return Type::Any };
        for s in segs.iter().skip(1) {
            let next = match self.types.kind(cur) {
                Some(NamedKind::Module { members }) => match members.get(s) {
                    Some(Member::Value(Type::Static(n))) => Some(*n),
                    _ => None,
                },
                _ => None,
            };
            match next {
                Some(n) => cur = n,
                None => return Type::Any,
            }
        }
        if matches!(self.types.kind(cur), Some(NamedKind::Module { .. })) {
            return Type::Any;
        }
        Type::Named(cur)
    }

    /// The signature of a `DECL_FN` (a `declare fn` or an `fn` member).
    pub(crate) fn decl_fn_sig(&mut self, node: &SyntaxNode, ctx: &DeclCtx) -> FnType {
        let mut ctx = ctx.clone();
        if let Some(tp) = child(node, TYPE_PARAMS) {
            ctx.type_params.extend(children(&tp, TYPE_PARAM).filter_map(|p| child(&p, NAME)).filter_map(|n| name_text(&n)));
        }
        let mut params = Vec::new();
        let mut rest = None;
        let mut keyed = None;
        let mut is_method = false;
        if let Some(list) = child(node, PARAM_LIST) {
            for p in list.children() {
                match p.kind() {
                    SELF_PARAM => is_method = true,
                    PARAM => {
                        let (name, t) = match pattern_children(&p).next() {
                            Some(pat) if pat.kind() == TYPED_PAT => {
                                let name = pattern_children(&pat).next().and_then(|b| child(&b, NAME)).and_then(|n| name_text(&n)).unwrap_or_default();
                                let t = type_child(&pat).map(|t| self.decl_type_of(&t, &ctx)).unwrap_or(Type::Any);
                                (name, t)
                            }
                            Some(pat) => (child(&pat, NAME).and_then(|n| name_text(&n)).unwrap_or_default(), Type::Any),
                            None => (String::new(), Type::Any),
                        };
                        // An optional parameter is typed `T?`.
                        let optional = self.types.may_be_nil(&t) && t != Type::Any;
                        params.push(Param { name, ty: t, optional });
                    }
                    REST_PARAM => rest = Some(type_child(&p).map(|t| self.decl_type_of(&t, &ctx)).unwrap_or(Type::Any)),
                    KEYED_PARAM => keyed = Some(type_child(&p).map(|t| self.decl_type_of(&t, &ctx))),
                    _ => {}
                }
            }
        }
        // An omitted return type is `nil`.
        let ret = child(node, RETURN_TYPE).and_then(|r| type_child(&r)).map(|t| self.decl_type_of(&t, &ctx)).unwrap_or(Type::Nil);
        FnType { params, rest, keyed, ret, is_method, is_async: false, named: false, declared: true }
    }

    /// Fills the members of a declared module, struct or type from its
    /// node, and records the Lua key of each.
    pub(crate) fn fill_decl_members(&mut self, node: &SyntaxNode, nid: NamedId) {
        let is_type = matches!(node.kind(), DECL_STRUCT | DECL_TYPE);
        let ctx = DeclCtx { self_named: is_type.then_some(nid), type_params: Vec::new() };
        // A host type's parent.
        if node.kind() == DECL_TYPE {
            if let Some(parent) = child(node, STRUCT_PARENT).and_then(|p| type_child(&p)) {
                if let Type::Named(p) = self.decl_type_of(&parent, &ctx) {
                    if let Some(n) = self.types.get_mut(nid) {
                        if let NamedKind::HostType { parent, .. } = &mut n.kind {
                            *parent = Some(p);
                        }
                    }
                }
            }
        }
        if node.kind() == DECL_FN {
            // A top-level `declare fn`.
            let name = child(node, NAME).and_then(|n| name_text(&n)).unwrap_or_default();
            let sig = self.decl_fn_sig(node, &ctx);
            self.decl_fn_sigs.insert(name, Rc::new(sig));
            return;
        }
        if node.kind() == DECL_VALUE {
            let name = child(node, NAME).and_then(|n| name_text(&n)).unwrap_or_default();
            let t = type_child(node).map(|t| self.decl_type_of(&t, &ctx)).unwrap_or(Type::Any);
            self.decl_value_types.insert(name, t);
            return;
        }
        let mut fields = Vec::new();
        for m in node.children() {
            match m.kind() {
                DECL_FIELD => {
                    let Some(name) = child(&m, NAME).and_then(|n| name_text(&n)) else { continue };
                    let t = type_child(&m).map(|t| self.decl_type_of(&t, &ctx)).unwrap_or(Type::Any);
                    let key = lua_key(&m).unwrap_or_else(|| name.clone());
                    self.decl_keys.insert((nid, name.clone()), key);
                    if is_type && node.kind() == DECL_STRUCT {
                        fields.push(Field { name: name.clone(), ty: t.clone(), has_default: true });
                    }
                    self.add_member(nid, &name, Member::Value(t));
                }
                DECL_VALUE => {
                    let Some(name) = child(&m, NAME).and_then(|n| name_text(&n)) else { continue };
                    let t = type_child(&m).map(|t| self.decl_type_of(&t, &ctx)).unwrap_or(Type::Any);
                    let key = lua_key(&m).unwrap_or_else(|| name.clone());
                    self.decl_keys.insert((nid, name.clone()), key);
                    self.add_member(nid, &name, Member::Const(t));
                }
                DECL_FN => {
                    let name = match child(&m, NAME) {
                        Some(n) => name_text(&n).unwrap_or_default(),
                        None => child(&m, OP_NAME).map(|o| o.text().to_string().split_whitespace().collect::<String>()).unwrap_or_default(),
                    };
                    let sig = self.decl_fn_sig(&m, &ctx);
                    let key = lua_key(&m).unwrap_or_else(|| name.clone());
                    self.decl_keys.insert((nid, name.clone()), key);
                    if child(&m, OP_NAME).is_some() {
                        // Operators of a Lua class: the metamethods' types.
                        self.decl_operators.entry(nid).or_default().insert(name, Rc::new(sig));
                    } else {
                        self.add_member(nid, &name, Member::Function(Rc::new(sig)));
                    }
                }
                DECL_MODULE | DECL_STRUCT | DECL_TYPE => {
                    if let Some(&inner) = self.decl_nested.get(&m.text_range()) {
                        let name = child(&m, NAME).and_then(|n| name_text(&n)).unwrap_or_default();
                        self.decl_keys.insert((nid, name.clone()), name.clone());
                        self.fill_decl_members(&m, inner);
                    }
                }
                _ => {}
            }
        }
        if !fields.is_empty() {
            if let Some(n) = self.types.get_mut(nid) {
                if let NamedKind::LuaClass { fields: fs, .. } = &mut n.kind {
                    *fs = fields;
                }
            }
        }
    }

    /// The Lua key of a declared member, when it differs from its name.
    pub(crate) fn decl_key(&self, owner: NamedId, name: &str) -> Option<String> {
        self.decl_keys.get(&(owner, name.to_string())).cloned()
    }

    /// The type of a declared `let`/`const` by name.
    pub(crate) fn decl_value_type(&self, name: &str) -> Option<Type> {
        self.decl_value_types.get(name).cloned()
    }

    /// The nominal type a declared name stands for, as a value.
    pub(crate) fn declared_value(&self, id: DeclId) -> Ty {
        let Some(item) = self.decls.get(id) else { return Ty::any() };
        match item.kind {
            crate::decl::DeclItemKind::Module => self.decl_named.get(&id).map(|n| Ty::certain(Type::Static(*n))).unwrap_or_else(Ty::any),
            crate::decl::DeclItemKind::Struct | crate::decl::DeclItemKind::HostType => self.decl_named.get(&id).map(|n| Ty::certain(Type::Static(*n))).unwrap_or_else(Ty::any),
            crate::decl::DeclItemKind::Fn => self.decl_fn_type(&item.name).map(|f| Ty::uncertain(Type::Fn(f))).unwrap_or_else(Ty::any),
            crate::decl::DeclItemKind::Let | crate::decl::DeclItemKind::Const => self.decl_value_type(&item.name).map(Ty::uncertain).unwrap_or_else(Ty::any),
        }
    }
}

/// `as 'luaName'` of a declared member.
fn lua_key(member: &SyntaxNode) -> Option<String> {
    let k = child(member, LUA_KEY)?;
    let s = token(&k, STRING)?;
    Some(crate::resolver::decode_plain_string(&s))
}

/// Imported type of a struct's field or method lookups through a
/// declared module member path.
pub(crate) type DeclKeys = HashMap<(NamedId, String), String>;

pub(crate) use super::Ty;
