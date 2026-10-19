//! The types of the analyzer and the operations on them:
//! unions, negations, intersections, assignability, narrowing.
//!
//! Types are gradual: [`Type::Any`] opts out of checking in both
//! directions, and most positions a program leaves unannotated are `any`.
//! A type error is reported only when both sides are definite and the
//! value's type is not assignable to the declared one; the analyzer picks
//! the conservative answer wherever the language leaves a rule open.
//!
//! Nominal types (structs, interfaces, enums, host types, Lua classes,
//! aliases, modules and the opaque types of imports) are entries of a
//! [`TypeTable`], referred to by [`NamedId`]. The table is owned by the
//! analyzer; the lattice operations take it as a parameter so that struct
//! inheritance, aliases and interface conformance can be consulted.
//!
//! Type parameters are erased: a reference to one is
//! [`Type::Any`] at every use.

use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

/// Index of a nominal type in a [`TypeTable`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NamedId(pub u32);

/// A type.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Type {
    /// Opts out of checking.
    Any,
    /// Any value; must be narrowed before use.
    Unknown,
    /// No value (`raise`, a function that never returns).
    Never,
    Nil,
    Bool,
    Number,
    String,
    /// Any table: a plain table, a struct instance, a record.
    Table,
    /// Any function, without a signature.
    Function,
    Userdata,
    Thread,
    /// A struct, interface, enum, host type, Lua class, alias or opaque
    /// type of the [`TypeTable`].
    Named(NamedId),
    /// The value that stands for a named thing: the struct table
    /// (`Monster.create`), the enum table (`Direction.North`), a module or
    /// a library table (`Number.floor`).
    Static(NamedId),
    /// `{x: Number, y: Number}`: a table with at least these fields (sorted
    /// by name).
    Record(Rc<[(String, Type)]>),
    /// `[T]`.
    Array(Rc<Type>),
    /// `{[K]: V}`.
    Map(Rc<Type>, Rc<Type>),
    /// `Function(a: Number): String`.
    Fn(Rc<FnType>),
    /// `A | B`: at least two distinct members, none of them a union.
    Union(Rc<[Type]>),
    /// `A & B`: a value of every part, as TypeScript writes it. Only made by
    /// narrowing (a struct tested `is I`, an interface whose implementation
    /// may be in another file), never written. At least two
    /// distinct parts, none of them an intersection or a union.
    And(Rc<[Type]>),
    /// `~A`.
    Not(Rc<Type>),
    /// `(A, B)`: several return values. Only the type of a
    /// call; in a single-value position it is its first member.
    Tuple(Rc<[Type]>),
    /// `Task<T>`.
    Task(Rc<Type>),
    /// A range value.
    Range,
    /// A compiled regex.
    Regex,
    /// A worker handle made from a literal path; the id is
    /// the module that holds the worker file's exports, when known.
    Worker(Option<NamedId>),
}

/// A parameter of a function type.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Param {
    pub name: String,
    pub ty: Type,
    /// Has a default, or is typed `T?` in a declaration file: a call may
    /// leave it out.
    pub optional: bool,
}

/// The signature of a function.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FnType {
    pub params: Vec<Param>,
    /// The type of the table of `*args`, when declared.
    pub rest: Option<Type>,
    /// `**opts`: present, with the type of the table when declared.
    pub keyed: Option<Option<Type>>,
    pub ret: Type,
    /// Receives the receiver as `self`.
    pub is_method: bool,
    /// `async fn`: a call returns `Task<ret>`.
    pub is_async: bool,
    /// A named function, which a worker can run.
    pub named: bool,
    /// Declared in a declaration file: a Lua or host
    /// function, whose result is not checked at runtime.
    pub declared: bool,
}

impl FnType {
    /// A function about which nothing is known but that it is one.
    pub fn unknown() -> FnType {
        FnType { params: Vec::new(), rest: Some(Type::Any), keyed: Some(None), ret: Type::Any, is_method: false, is_async: false, named: false, declared: false }
    }

    /// True when some parameter takes a block: `Function`, a
    /// signature, or either made optional.
    pub fn takes_block(&self) -> bool {
        self.params.iter().any(|p| takes_block(&p.ty))
    }

    /// The parameters a call must supply: those without a default, before
    /// the rest parameter.
    pub fn required(&self) -> usize {
        self.params.iter().filter(|p| !p.optional).count()
    }
}

/// True for a parameter type that takes a block.
pub fn takes_block(ty: &Type) -> bool {
    match ty {
        Type::Function | Type::Fn(_) => true,
        Type::Union(m) => m.len() == 2 && m.contains(&Type::Nil) && m.iter().any(|t| matches!(t, Type::Function | Type::Fn(_))),
        _ => false,
    }
}

/// A field of a struct or a Lua class.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub ty: Type,
    pub has_default: bool,
}

/// What a member of a module, struct table or library is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Member {
    /// A function called as written (`Table.insert`, a module function
    /// that is not a method); a method when `is_method`.
    Function(Rc<FnType>),
    /// A value: a constant, a field of a library table, a nested module.
    Value(Type),
    /// A constant that cannot be assigned (`const` in a declaration file).
    Const(Type),
}

impl Member {
    pub fn ty(&self) -> Type {
        match self {
            Member::Function(f) => Type::Fn(f.clone()),
            Member::Value(t) | Member::Const(t) => t.clone(),
        }
    }
}

/// What a nominal type is.
#[derive(Clone, Debug)]
pub enum NamedKind {
    Struct {
        parent: Option<NamedId>,
        fields: Vec<Field>,
        methods: HashMap<String, Rc<FnType>>,
        /// Operators defined with `fn Name.op` (`+`, `[]`, `len`, …).
        operators: HashMap<String, Rc<FnType>>,
        /// Interfaces implemented for the struct.
        interfaces: Vec<NamedId>,
        type_params: Vec<String>,
        /// `To<T>` implementations: the targets.
        to: Vec<Type>,
        /// `From<S>` implementations: the sources.
        from: Vec<Type>,
    },
    Interface {
        methods: HashMap<String, Rc<FnType>>,
        fields: Vec<Field>,
        type_params: Vec<String>,
    },
    Enum {
        /// Variant names and the type of each value (`String` or
        /// `Number`).
        variants: Vec<(String, Type)>,
    },
    /// An NL module, a library table of the prelude (`Number` is `math`),
    /// a module of a declaration file, or the export table of
    /// `import from … as Name`.
    Module { members: HashMap<String, Member> },
    /// `declare type Name : Parent`: a host type, a kind of userdata.
    HostType { parent: Option<NamedId>, members: HashMap<String, Member> },
    /// `declare struct Name = :path`: a class written in Lua over tables,
    /// checked by its metatable.
    LuaClass { fields: Vec<Field>, members: HashMap<String, Member> },
    /// `type Name = T`.
    Alias(Type),
    /// A type the analyzer knows only by name (an import, a type parameter
    /// used as a value type, a type whose declaration it cannot see).
    Opaque,
}

/// One nominal type.
#[derive(Clone, Debug)]
pub struct Named {
    pub name: String,
    pub kind: NamedKind,
}

/// The nominal types of one analysis.
#[derive(Clone, Debug, Default)]
pub struct TypeTable {
    pub entries: Vec<Named>,
}

impl TypeTable {
    pub fn add(&mut self, name: &str, kind: NamedKind) -> NamedId {
        let id = NamedId(u32::try_from(self.entries.len()).unwrap_or(u32::MAX));
        self.entries.push(Named { name: name.to_string(), kind });
        id
    }

    pub fn get(&self, id: NamedId) -> Option<&Named> {
        self.entries.get(id.0 as usize)
    }

    pub fn get_mut(&mut self, id: NamedId) -> Option<&mut Named> {
        self.entries.get_mut(id.0 as usize)
    }

    pub fn name(&self, id: NamedId) -> &str {
        self.get(id).map_or("?", |n| n.name.as_str())
    }

    pub fn kind(&self, id: NamedId) -> Option<&NamedKind> {
        self.get(id).map(|n| &n.kind)
    }

    pub fn is_struct(&self, id: NamedId) -> bool {
        matches!(self.kind(id), Some(NamedKind::Struct { .. }))
    }

    pub fn is_enum(&self, id: NamedId) -> bool {
        matches!(self.kind(id), Some(NamedKind::Enum { .. }))
    }

    pub fn is_interface(&self, id: NamedId) -> bool {
        matches!(self.kind(id), Some(NamedKind::Interface { .. }))
    }

    /// The parent of a struct or host type.
    pub fn parent(&self, id: NamedId) -> Option<NamedId> {
        match self.kind(id)? {
            NamedKind::Struct { parent, .. } | NamedKind::HostType { parent, .. } => *parent,
            _ => None,
        }
    }

    /// True when `id` is `ancestor` or descends from it.
    pub fn descends(&self, mut id: NamedId, ancestor: NamedId) -> bool {
        for _ in 0..64 {
            if id == ancestor {
                return true;
            }
            match self.parent(id) {
                Some(p) => id = p,
                None => return false,
            }
        }
        false
    }

    /// The fields of a struct or Lua class, the parent's first.
    pub fn fields(&self, id: NamedId) -> Vec<Field> {
        let mut chain = Vec::new();
        let mut cur = Some(id);
        for _ in 0..64 {
            let Some(c) = cur else { break };
            chain.push(c);
            cur = self.parent(c);
        }
        let mut out = Vec::new();
        for c in chain.into_iter().rev() {
            if let Some(NamedKind::Struct { fields, .. } | NamedKind::LuaClass { fields, .. }) = self.kind(c) {
                out.extend(fields.iter().cloned());
            }
        }
        out
    }

    pub fn field(&self, id: NamedId, name: &str) -> Option<Field> {
        let mut cur = Some(id);
        for _ in 0..64 {
            let Some(c) = cur else { break };
            if let Some(NamedKind::Struct { fields, .. } | NamedKind::LuaClass { fields, .. }) = self.kind(c) {
                if let Some(f) = fields.iter().find(|f| f.name == name) {
                    return Some(f.clone());
                }
            }
            cur = self.parent(c);
        }
        None
    }

    /// A method of a struct, host type or interface, the struct's own
    /// before its parent's.
    pub fn method(&self, id: NamedId, name: &str) -> Option<Rc<FnType>> {
        let mut cur = Some(id);
        for _ in 0..64 {
            let Some(c) = cur else { break };
            match self.kind(c) {
                Some(NamedKind::Struct { methods, .. } | NamedKind::Interface { methods, .. }) => {
                    if let Some(m) = methods.get(name) {
                        return Some(m.clone());
                    }
                }
                Some(NamedKind::HostType { members, .. } | NamedKind::LuaClass { members, .. } | NamedKind::Module { members }) => {
                    if let Some(Member::Function(f)) = members.get(name) {
                        return Some(f.clone());
                    }
                }
                _ => {}
            }
            cur = self.parent(c);
        }
        None
    }

    /// An operator definition of a struct (`+`, `[]`, `len`, …).
    pub fn operator(&self, id: NamedId, op: &str) -> Option<Rc<FnType>> {
        let mut cur = Some(id);
        for _ in 0..64 {
            let Some(c) = cur else { break };
            if let Some(NamedKind::Struct { operators, .. }) = self.kind(c) {
                if let Some(m) = operators.get(op) {
                    return Some(m.clone());
                }
            }
            cur = self.parent(c);
        }
        None
    }

    /// True when the struct implements the interface, itself or through a
    /// parent.
    pub fn implements(&self, id: NamedId, iface: NamedId) -> bool {
        let mut cur = Some(id);
        for _ in 0..64 {
            let Some(c) = cur else { break };
            if let Some(NamedKind::Struct { interfaces, .. }) = self.kind(c) {
                if interfaces.contains(&iface) {
                    return true;
                }
            }
            cur = self.parent(c);
        }
        false
    }

    /// The expansion of an alias, through other aliases (bounded).
    pub fn expand(&self, ty: &Type) -> Type {
        let mut t = ty.clone();
        for _ in 0..16 {
            match &t {
                Type::Named(id) => match self.kind(*id) {
                    Some(NamedKind::Alias(inner)) => t = inner.clone(),
                    _ => return t,
                },
                _ => return t,
            }
        }
        t
    }

    // ---- the lattice -----------------------------------------------------------

    /// `from` may be stored where `to` is declared (gradual: `any` in either
    /// place accepts).
    pub fn assignable(&self, from: &Type, to: &Type) -> bool {
        self.assignable_depth(from, to, 0)
    }

    fn assignable_depth(&self, from: &Type, to: &Type, depth: u32) -> bool {
        if depth > 12 {
            return true;
        }
        let from = self.expand(from);
        let to = self.expand(to);
        use Type::*;
        match (&from, &to) {
            (Any, _) | (_, Any) | (Never, _) | (_, Unknown) => true,
            (Unknown, _) => false,
            (Tuple(m), _) => self.assignable_depth(m.first().unwrap_or(&Nil), &to, depth + 1),
            (_, Tuple(m)) => self.assignable_depth(&from, m.first().unwrap_or(&Nil), depth + 1),
            (Union(m), _) => m.iter().all(|t| self.assignable_depth(t, &to, depth + 1)),
            (_, Union(m)) => m.iter().any(|t| self.assignable_depth(&from, t, depth + 1)),
            // To every part; from any one (checked in this order, so that
            // `A & B` is assignable to itself).
            (_, And(m)) => m.iter().all(|t| self.assignable_depth(&from, t, depth + 1)),
            (And(m), _) => m.iter().any(|t| self.assignable_depth(t, &to, depth + 1)),
            (_, Not(inner)) => !self.overlaps(&from, inner),
            (Not(_), _) => false,
            (a, b) if a == b => true,
            // Tables.
            (Record(_) | Array(_) | Map(..), Table) => true,
            (Named(id), Table) => matches!(self.kind(*id), Some(NamedKind::Struct { .. } | NamedKind::LuaClass { .. } | NamedKind::Opaque)),
            (Static(_), Table) => true,
            // A field a value lacks is `nil` at runtime (records
            // are checked field by field), so a field whose type accepts
            // nil may be left out.
            (Record(a), Record(b)) => b.iter().all(|(k, bt)| match a.iter().find(|(ak, _)| ak == k) {
                Some((_, at)) => self.assignable_depth(at, bt, depth + 1),
                None => self.assignable_depth(&Nil, bt, depth + 1),
            }),
            (Record(a), Map(k, v)) => a.iter().all(|(_, at)| self.assignable_depth(&String, k, depth + 1) && self.assignable_depth(at, v, depth + 1)),
            (Array(a), Array(b)) => self.assignable_depth(a, b, depth + 1),
            (Array(a), Map(k, v)) => self.assignable_depth(&Number, k, depth + 1) && self.assignable_depth(a, v, depth + 1),
            (Map(ak, av), Map(bk, bv)) => self.assignable_depth(ak, bk, depth + 1) && self.assignable_depth(av, bv, depth + 1),
            (Table, Record(_) | Array(_) | Map(..)) => true,
            (Record(_), Array(_)) | (Array(_), Record(_)) => false,
            (Named(id), Record(fields)) => match self.kind(*id) {
                Some(NamedKind::Struct { .. } | NamedKind::LuaClass { .. }) => fields.iter().all(|(k, ft)| match self.field(*id, k) {
                    Some(f) => self.assignable_depth(&f.ty, ft, depth + 1),
                    None => self.assignable_depth(&Nil, ft, depth + 1),
                }),
                Some(NamedKind::Opaque) => true,
                _ => false,
            },
            (Named(id), Array(_) | Map(..)) => matches!(self.kind(*id), Some(NamedKind::Struct { .. } | NamedKind::Opaque)),
            // Functions.
            (Fn(_), Function) => true,
            (Function, Fn(_)) => true,
            (Fn(a), Fn(b)) => self.fn_assignable(a, b, depth),
            // Nominal types.
            (Named(a), Named(b)) => self.named_assignable(*a, *b),
            (Number | String | Bool, Named(b)) => match self.kind(*b) {
                Some(NamedKind::Opaque) => true,
                Some(NamedKind::Interface { .. }) => true,
                Some(NamedKind::Enum { variants }) => variants.iter().any(|(_, t)| *t == from),
                _ => false,
            },
            (Named(a), Number | String | Bool) => match self.kind(*a) {
                Some(NamedKind::Opaque) => true,
                Some(NamedKind::Enum { variants }) => variants.iter().all(|(_, t)| *t == to),
                _ => false,
            },
            (Named(a), Userdata) => matches!(self.kind(*a), Some(NamedKind::HostType { .. } | NamedKind::Opaque)),
            (Named(a), _) => matches!(self.kind(*a), Some(NamedKind::Opaque)),
            (_, Named(b)) => matches!(self.kind(*b), Some(NamedKind::Opaque)),
            (Task(a), Task(b)) => self.assignable_depth(a, b, depth + 1),
            (Static(_), Function) => false,
            (Worker(_), Worker(_)) => true,
            _ => false,
        }
    }

    fn fn_assignable(&self, a: &FnType, b: &FnType, depth: u32) -> bool {
        // Parameters are checked loosely (contravariance would reject
        // too many untyped functions); the return type is covariant.
        for (pa, pb) in a.params.iter().zip(b.params.iter()) {
            if !self.assignable_depth(&pb.ty, &pa.ty, depth + 1) && !self.assignable_depth(&pa.ty, &pb.ty, depth + 1) {
                return false;
            }
        }
        self.assignable_depth(&a.ret, &b.ret, depth + 1) || self.assignable_depth(&b.ret, &a.ret, depth + 1)
    }

    fn named_assignable(&self, a: NamedId, b: NamedId) -> bool {
        if a == b {
            return true;
        }
        match (self.kind(a), self.kind(b)) {
            (Some(NamedKind::Opaque), _) | (_, Some(NamedKind::Opaque)) => true,
            (Some(NamedKind::Struct { .. }), Some(NamedKind::Struct { .. })) => self.descends(a, b),
            (Some(NamedKind::HostType { .. }), Some(NamedKind::HostType { .. })) => self.descends(a, b),
            (Some(NamedKind::Struct { .. }), Some(NamedKind::Interface { .. })) => self.implements(a, b),
            // Conformance is nominal and may be declared in another file:
            // a struct the analyzer does not see implementing an interface
            // may still do so at runtime. Only interfaces and structs of
            // this analysis are decided here; the rest is accepted.
            (Some(NamedKind::Interface { .. }), Some(NamedKind::Interface { .. })) => false,
            _ => false,
        }
    }

    /// True when a value may be of both types.
    pub fn overlaps(&self, a: &Type, b: &Type) -> bool {
        self.overlaps_depth(a, b, 0)
    }

    /// Bounded, as [`Self::assignable`] is: an alias that refers to
    /// itself without a record in between (an error the lowering
    /// reports) must not recurse forever here.
    fn overlaps_depth(&self, a: &Type, b: &Type, depth: u32) -> bool {
        if depth > 12 {
            return true;
        }
        let a = self.expand(a);
        let b = self.expand(b);
        use Type::*;
        match (&a, &b) {
            (Any | Unknown, _) | (_, Any | Unknown) => true,
            (Never, _) | (_, Never) => false,
            (Union(m), _) => m.iter().any(|t| self.overlaps_depth(t, &b, depth + 1)),
            (_, Union(m)) => m.iter().any(|t| self.overlaps_depth(&a, t, depth + 1)),
            (And(m), _) => m.iter().all(|t| self.overlaps_depth(t, &b, depth + 1)),
            (_, And(m)) => m.iter().all(|t| self.overlaps_depth(&a, t, depth + 1)),
            (Not(inner), _) => !self.assignable(&b, inner),
            (_, Not(inner)) => !self.assignable(&a, inner),
            (Tuple(m), _) => self.overlaps_depth(m.first().unwrap_or(&Nil), &b, depth + 1),
            (_, Tuple(m)) => self.overlaps_depth(&a, m.first().unwrap_or(&Nil), depth + 1),
            _ => self.assignable(&a, &b) || self.assignable(&b, &a) || self.same_runtime_kind(&a, &b),
        }
    }

    /// Two types whose values share a Lua type and may be the same value:
    /// a record and a struct, two records, two interfaces.
    fn same_runtime_kind(&self, a: &Type, b: &Type) -> bool {
        use Type::*;
        let tableish = |t: &Type| matches!(t, Table | Record(_) | Array(_) | Map(..) | Static(_)) || matches!(t, Named(id) if !self.is_enum(*id));
        if tableish(a) && tableish(b) {
            // Two different structs never overlap unless related.
            if let (Named(x), Named(y)) = (a, b) {
                if self.is_struct(*x) && self.is_struct(*y) {
                    return self.descends(*x, *y) || self.descends(*y, *x);
                }
                if self.is_struct(*x) && self.is_interface(*y) || self.is_interface(*x) && self.is_struct(*y) {
                    // An implementation may be declared elsewhere.
                    return true;
                }
            }
            return true;
        }
        let primitive = |t: &Type| matches!(t, Number | String | Bool);
        if let (Named(x), p) | (p, Named(x)) = (a, b) {
            if self.is_interface(*x) && primitive(p) {
                return true;
            }
        }
        false
    }

    /// `a | b`, normalized: flattened, without duplicates, `any` absorbing
    /// everything, `never` absorbed.
    pub fn union(&self, a: &Type, b: &Type) -> Type {
        let mut members = Vec::new();
        self.push_members(a, &mut members);
        self.push_members(b, &mut members);
        Self::make_union(members)
    }

    /// The union of several types.
    pub fn union_all(&self, types: &[Type]) -> Type {
        let mut members = Vec::new();
        for t in types {
            self.push_members(t, &mut members);
        }
        Self::make_union(members)
    }

    fn push_members(&self, t: &Type, out: &mut Vec<Type>) {
        match t {
            Type::Union(m) => {
                for x in m.iter() {
                    self.push_members(x, out);
                }
            }
            Type::Never => {}
            _ => {
                if !out.contains(t) {
                    out.push(t.clone());
                }
            }
        }
    }

    fn make_union(mut members: Vec<Type>) -> Type {
        if members.contains(&Type::Any) {
            return Type::Any;
        }
        // `unknown | T` is unknown.
        if members.contains(&Type::Unknown) {
            return Type::Unknown;
        }
        // A tuple joins member-wise with other results.
        if members.iter().any(|t| matches!(t, Type::Tuple(_))) {
            let width = members.iter().map(|t| match t { Type::Tuple(m) => m.len(), _ => 1 }).max().unwrap_or(1);
            let mut cols: Vec<Vec<Type>> = (0..width).map(|_| Vec::new()).collect();
            for t in &members {
                let parts: Vec<Type> = match t {
                    Type::Tuple(m) => m.to_vec(),
                    other => vec![other.clone()],
                };
                for (i, col) in cols.iter_mut().enumerate() {
                    let p = parts.get(i).cloned().unwrap_or(Type::Nil);
                    if !col.contains(&p) {
                        col.push(p);
                    }
                }
            }
            let cols: Vec<Type> = cols.into_iter().map(Self::make_union).collect();
            return Type::Tuple(cols.into());
        }
        members.dedup();
        match members.len() {
            0 => Type::Never,
            1 => members.pop().unwrap_or(Type::Never),
            _ => Type::Union(members.into()),
        }
    }

    /// `~t`.
    pub fn negate(&self, t: &Type) -> Type {
        match t {
            Type::Not(inner) => (**inner).clone(),
            Type::Any | Type::Unknown => Type::Any,
            Type::Never => Type::Any,
            _ => Type::Not(Rc::new(t.clone())),
        }
    }

    /// `a & b`, normalized: flattened, without duplicates. Only for parts
    /// that are not unions (narrowing intersects member by member).
    pub fn intersect(&self, a: &Type, b: &Type) -> Type {
        let mut parts: Vec<Type> = Vec::new();
        for t in [a, b] {
            let ts: Vec<Type> = match t {
                Type::And(m) => m.to_vec(),
                other => vec![other.clone()],
            };
            for x in ts {
                if !parts.contains(&x) {
                    parts.push(x);
                }
            }
        }
        match parts.len() {
            1 => parts.pop().unwrap_or(Type::Never),
            _ => Type::And(parts.into()),
        }
    }

    /// The members of a union, or the type itself.
    pub fn members(&self, t: &Type) -> Vec<Type> {
        match self.expand(t) {
            Type::Union(m) => m.to_vec(),
            other => vec![other],
        }
    }

    /// `t` narrowed by a successful `is test`: the part of `t`
    /// that may be a `test`.
    pub fn narrow(&self, t: &Type, test: &Type) -> Type {
        let t = self.expand(t);
        match &t {
            Type::Any | Type::Unknown => return test.clone(),
            Type::Never => return Type::Never,
            _ => {}
        }
        let mut out = Vec::new();
        for m in self.members(&t) {
            for tm in self.members(test) {
                if self.assignable(&m, &tm) {
                    out.push(m.clone());
                } else if self.assignable(&tm, &m) {
                    out.push(tm.clone());
                } else if self.overlaps(&m, &tm) {
                    // A record narrowed by a struct, an interface by a
                    // struct: the more specific side.
                    match (&m, &tm) {
                        // A struct tested against an interface is both,
                        // `S & I`, as TypeScript narrows: the implementation
                        // is a runtime statement, perhaps in another file,
                        // and the struct's own members stay.
                        (Type::Named(s), Type::Named(i)) if self.is_struct(*s) && self.is_interface(*i) => out.push(self.intersect(&m, &tm)),
                        (_, Type::Named(_)) => out.push(tm.clone()),
                        _ => out.push(m.clone()),
                    }
                }
            }
        }
        self.union_all(&out)
    }

    /// `t` after a failed `is test`: the part of `t` that is not a `test`.
    pub fn exclude(&self, t: &Type, test: &Type) -> Type {
        let t = self.expand(t);
        match &t {
            Type::Any | Type::Unknown => return t.clone(),
            _ => {}
        }
        let mut out = Vec::new();
        for m in self.members(&t) {
            if !self.members(test).iter().any(|tm| self.assignable(&m, tm)) {
                out.push(m);
            }
        }
        self.union_all(&out)
    }

    /// The part of `t` that is truthy (`if x`): without `nil` and `false`.
    pub fn truthy(&self, t: &Type) -> Type {
        let t = self.expand(t);
        if matches!(t, Type::Any | Type::Unknown) {
            return t;
        }
        let out: Vec<Type> = self.members(&t).into_iter().filter(|m| *m != Type::Nil).collect();
        self.union_all(&out)
    }

    /// The part of `t` that is not `nil`: what a nil-safe step and `??`
    /// pass on. `false` stays.
    pub fn non_nil(&self, t: &Type) -> Type {
        let t = self.expand(t);
        if matches!(t, Type::Any | Type::Unknown) {
            return t;
        }
        let out: Vec<Type> = self.members(&t).into_iter().filter(|m| *m != Type::Nil).collect();
        self.union_all(&out)
    }

    /// The part of `t` that is falsy: `nil`, `Bool`.
    pub fn falsy(&self, t: &Type) -> Type {
        let t = self.expand(t);
        if matches!(t, Type::Any | Type::Unknown) {
            return t;
        }
        let out: Vec<Type> = self.members(&t).into_iter().filter(|m| matches!(m, Type::Nil | Type::Bool)).collect();
        self.union_all(&out)
    }

    /// True when a value of `t` can be `nil`.
    pub fn may_be_nil(&self, t: &Type) -> bool {
        self.may_be_nil_depth(t, 0)
    }

    fn may_be_nil_depth(&self, t: &Type, depth: u32) -> bool {
        if depth > 12 {
            return true;
        }
        match self.expand(t) {
            Type::Any | Type::Unknown | Type::Nil => true,
            Type::Union(m) => m.iter().any(|x| self.may_be_nil_depth(x, depth + 1)),
            Type::Not(inner) => !self.assignable(&Type::Nil, &inner),
            _ => false,
        }
    }

    /// True when `t` is a definite type: not `any`, not `unknown`, and not
    /// a type the analyzer knows only by name.
    pub fn is_definite(&self, t: &Type) -> bool {
        self.is_definite_depth(t, 0)
    }

    fn is_definite_depth(&self, t: &Type, depth: u32) -> bool {
        if depth > 12 {
            return false;
        }
        match self.expand(t) {
            Type::Any | Type::Unknown => false,
            Type::Named(id) => !matches!(self.kind(id), Some(NamedKind::Opaque)),
            Type::Union(m) | Type::And(m) => m.iter().all(|x| self.is_definite_depth(x, depth + 1)),
            Type::Tuple(m) => m.first().is_none_or(|x| self.is_definite_depth(x, depth + 1)),
            _ => true,
        }
    }

    /// The first value of a possibly multi-valued type.
    pub fn first(&self, t: &Type) -> Type {
        match t {
            Type::Tuple(m) => m.first().cloned().unwrap_or(Type::Nil),
            other => other.clone(),
        }
    }

    /// Value `i` (0-based) of a possibly multi-valued type; `nil` past the
    /// end, as Lua gives it.
    pub fn nth(&self, t: &Type, i: usize) -> Type {
        match t {
            Type::Tuple(m) => m.get(i).cloned().unwrap_or(Type::Nil),
            Type::Any => Type::Any,
            other if i == 0 => other.clone(),
            _ => Type::Nil,
        }
    }

    /// The element type of a `for v in x` loop over one variable, when
    /// the loop form is known from the type.
    pub fn element(&self, t: &Type) -> Option<Type> {
        match self.expand(t) {
            Type::Array(e) => Some((*e).clone()),
            Type::Map(_, v) => Some((*v).clone()),
            Type::String => Some(Type::String),
            Type::Range => Some(Type::Number),
            _ => None,
        }
    }

    /// The text of a type for messages.
    pub fn show(&self, t: &Type) -> String {
        Shown { table: self, ty: t }.to_string()
    }
}

struct Shown<'a> {
    table: &'a TypeTable,
    ty: &'a Type,
}

impl fmt::Display for Shown<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let t = self.table;
        let sub = |ty: &Type| Shown { table: t, ty }.to_string();
        match self.ty {
            Type::Any => write!(f, "any"),
            Type::Unknown => write!(f, "unknown"),
            Type::Never => write!(f, "never"),
            Type::Nil => write!(f, "nil"),
            Type::Bool => write!(f, "Bool"),
            Type::Number => write!(f, "Number"),
            Type::String => write!(f, "String"),
            Type::Table => write!(f, "Table"),
            Type::Function => write!(f, "Function"),
            Type::Userdata => write!(f, "Userdata"),
            Type::Thread => write!(f, "Thread"),
            Type::Named(id) => write!(f, "{}", t.name(*id)),
            Type::Static(id) => write!(f, "{}", t.name(*id)),
            Type::Record(fields) => {
                write!(f, "{{")?;
                for (i, (k, v)) in fields.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{k}: {}", sub(v))?;
                }
                write!(f, "}}")
            }
            Type::Array(e) => write!(f, "[{}]", sub(e)),
            Type::Map(k, v) => write!(f, "{{[{}]: {}}}", sub(k), sub(v)),
            Type::Fn(sig) => {
                write!(f, "Function(")?;
                for (i, p) in sig.params.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    if p.name.is_empty() {
                        write!(f, "{}", sub(&p.ty))?;
                    } else {
                        write!(f, "{}: {}", p.name, sub(&p.ty))?;
                    }
                }
                write!(f, ")")?;
                if sig.ret != Type::Nil {
                    write!(f, ": {}", sub(&sig.ret))?;
                }
                Ok(())
            }
            Type::Union(m) => {
                if m.len() == 2 && m.contains(&Type::Nil) {
                    if let Some(other) = m.iter().find(|x| **x != Type::Nil) {
                        let inner = sub(other);
                        return if matches!(other, Type::Union(_) | Type::And(_) | Type::Fn(_)) { write!(f, "({inner})?") } else { write!(f, "{inner}?") };
                    }
                }
                for (i, x) in m.iter().enumerate() {
                    if i > 0 {
                        write!(f, " | ")?;
                    }
                    if matches!(x, Type::And(_)) {
                        write!(f, "({})", sub(x))?;
                    } else {
                        write!(f, "{}", sub(x))?;
                    }
                }
                Ok(())
            }
            Type::And(m) => {
                for (i, x) in m.iter().enumerate() {
                    if i > 0 {
                        write!(f, " & ")?;
                    }
                    write!(f, "{}", sub(x))?;
                }
                Ok(())
            }
            Type::Not(inner) => write!(f, "~{}", sub(inner)),
            Type::Tuple(m) => {
                write!(f, "(")?;
                for (i, x) in m.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", sub(x))?;
                }
                write!(f, ")")
            }
            Type::Task(r) => write!(f, "Task<{}>", sub(r)),
            Type::Range => write!(f, "Range"),
            Type::Regex => write!(f, "Regex"),
            Type::Worker(_) => write!(f, "Worker"),
        }
    }
}

/// `T?`.
pub fn optional(table: &TypeTable, t: &Type) -> Type {
    table.union(t, &Type::Nil)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> (TypeTable, NamedId, NamedId, NamedId, NamedId) {
        let mut t = TypeTable::default();
        let entity = t.add("Entity", NamedKind::Struct { parent: None, fields: vec![Field { name: "id".into(), ty: Type::Number, has_default: false }], methods: HashMap::new(), operators: HashMap::new(), interfaces: Vec::new(), type_params: Vec::new(), to: Vec::new(), from: Vec::new() });
        let monster = t.add("Monster", NamedKind::Struct { parent: Some(entity), fields: Vec::new(), methods: HashMap::new(), operators: HashMap::new(), interfaces: Vec::new(), type_params: Vec::new(), to: Vec::new(), from: Vec::new() });
        let shape = t.add("Shape", NamedKind::Interface { methods: HashMap::new(), fields: Vec::new(), type_params: Vec::new() });
        let dir = t.add("Dir", NamedKind::Enum { variants: vec![("North".into(), Type::String)] });
        (t, entity, monster, shape, dir)
    }

    #[test]
    fn intersections() {
        let (t, entity, monster, shape, _) = table();
        let (e, m, s) = (Type::Named(entity), Type::Named(monster), Type::Named(shape));
        // A struct tested against an interface it is not seen to implement.
        let es = t.narrow(&e, &s);
        assert_eq!(es, Type::And(vec![e.clone(), s.clone()].into()));
        assert_eq!(t.show(&es), "Entity & Shape");
        assert_eq!(t.show(&optional(&t, &es)), "(Entity & Shape)?");
        assert!(t.assignable(&es, &e) && t.assignable(&es, &s) && t.assignable(&es, &es));
        assert!(!t.assignable(&e, &es) && !t.assignable(&m, &es) && !t.assignable(&es, &m));
        assert_eq!(t.narrow(&es, &s), es);
        assert_eq!(t.intersect(&es, &s), es);
        assert_eq!(t.narrow(&Type::Any, &s), s);
        assert_eq!(t.exclude(&e, &s), e);
        assert_eq!(t.exclude(&es, &s), Type::Never);
    }

    #[test]
    fn unions_normalize() {
        let (t, ..) = table();
        let u = t.union(&Type::Number, &Type::String);
        assert_eq!(t.show(&u), "Number | String");
        assert_eq!(t.union(&u, &Type::Number), u);
        assert_eq!(t.union(&Type::Number, &Type::Any), Type::Any);
        assert_eq!(t.union(&Type::Never, &Type::Number), Type::Number);
        let o = optional(&t, &Type::Number);
        assert_eq!(t.show(&o), "Number?");
        assert_eq!(t.union(&Type::Tuple(vec![Type::Number, Type::String].into()), &Type::Nil), Type::Tuple(vec![t.union(&Type::Number, &Type::Nil), t.union(&Type::String, &Type::Nil)].into()));
    }

    #[test]
    fn assignability() {
        let (t, entity, monster, shape, dir) = table();
        assert!(t.assignable(&Type::Number, &Type::Number));
        assert!(!t.assignable(&Type::String, &Type::Number));
        assert!(t.assignable(&Type::Any, &Type::Number));
        assert!(t.assignable(&Type::Number, &Type::Any));
        assert!(!t.assignable(&Type::Unknown, &Type::Number));
        assert!(t.assignable(&Type::Number, &Type::Unknown));
        assert!(t.assignable(&Type::Never, &Type::String));
        assert!(t.assignable(&Type::Number, &t.union(&Type::Number, &Type::String)));
        assert!(!t.assignable(&t.union(&Type::Number, &Type::String), &Type::Number));
        assert!(t.assignable(&Type::Named(monster), &Type::Named(entity)));
        assert!(!t.assignable(&Type::Named(entity), &Type::Named(monster)));
        assert!(!t.assignable(&Type::Named(monster), &Type::Named(shape)));
        assert!(t.assignable(&Type::Named(monster), &Type::Table));
        assert!(t.assignable(&Type::String, &Type::Named(dir)));
        assert!(!t.assignable(&Type::Number, &Type::Named(dir)));
        assert!(t.assignable(&Type::Nil, &optional(&t, &Type::Number)));
        assert!(!t.assignable(&Type::Nil, &Type::Number));
        // Negation.
        let not_num = t.negate(&Type::Number);
        assert!(t.assignable(&Type::String, &not_num));
        assert!(!t.assignable(&Type::Number, &not_num));
        assert!(!t.assignable(&t.union(&Type::Number, &Type::String), &not_num));
        assert_eq!(t.negate(&not_num), Type::Number);
        // Records and structs.
        let rec: Type = Type::Record(vec![("id".to_string(), Type::Number)].into());
        assert!(t.assignable(&Type::Named(entity), &rec));
        assert!(t.assignable(&Type::Record(vec![("id".to_string(), Type::Number), ("z".to_string(), Type::String)].into()), &rec));
        assert!(!t.assignable(&Type::Record(vec![("id".to_string(), Type::String)].into()), &rec));
        assert!(t.assignable(&rec, &Type::Map(Rc::new(Type::String), Rc::new(Type::Number))));
        assert!(t.assignable(&Type::Array(Rc::new(Type::Never)), &Type::Array(Rc::new(Type::Number))));
        // Functions.
        assert!(t.assignable(&Type::Fn(Rc::new(FnType::unknown())), &Type::Function));
    }

    #[test]
    fn narrowing() {
        let (t, entity, monster, _, _) = table();
        let ns = t.union(&Type::Number, &Type::String);
        assert_eq!(t.narrow(&ns, &Type::Number), Type::Number);
        assert_eq!(t.exclude(&ns, &Type::Number), Type::String);
        assert_eq!(t.narrow(&Type::Any, &Type::Number), Type::Number);
        assert_eq!(t.exclude(&Type::Any, &Type::Number), Type::Any);
        let opt = optional(&t, &Type::Named(entity));
        assert_eq!(t.exclude(&opt, &Type::Nil), Type::Named(entity));
        assert_eq!(t.truthy(&opt), Type::Named(entity));
        assert_eq!(t.narrow(&Type::Named(entity), &Type::Named(monster)), Type::Named(monster));
        assert_eq!(t.exclude(&Type::Named(entity), &Type::Named(monster)), Type::Named(entity));
        assert_eq!(t.narrow(&Type::Named(monster), &Type::Named(entity)), Type::Named(monster));
        assert_eq!(t.exclude(&Type::Named(monster), &Type::Named(entity)), Type::Never);
        assert_eq!(t.narrow(&Type::String, &Type::Number), Type::Never);
        assert!(t.may_be_nil(&opt));
        assert!(!t.may_be_nil(&Type::Named(entity)));
        assert_eq!(t.narrow(&Type::Unknown, &t.negate(&Type::String)), t.negate(&Type::String));
    }
}
