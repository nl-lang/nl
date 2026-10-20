//! Patterns: the types of the names they bind, narrowing in
//! `when` and `if let`, and `case` with exhaustiveness over enums
//! and unions of structs and unreachable clauses.

use std::rc::Rc;

use nl_syntax::{SyntaxKind, SyntaxNode, TextRange};

use super::{Analyzer, Env, Ty};
use crate::binding::*;
use crate::types::*;
use crate::util::*;

use SyntaxKind::*;

impl Analyzer<'_> {
    /// Declares the names a pattern binds from the type of the matched
    /// value, and returns the type the value has when the pattern matches.
    /// `must`: a must-match context (a name bound to a missing key is
    /// `nil`); in a may-match context a bound name is never `nil`.
    pub(crate) fn bind_pattern(&mut self, pat: &SyntaxNode, value: &Ty, must: bool) -> Type {
        match pat.kind() {
            BIND_PAT => {
                let Some(id) = child(pat, NAME).and_then(|n| self.res.declaration(n.text_range())) else { return value.t.clone() };
                let t = if must { value.t.clone() } else { self.types.exclude(&value.t, &Type::Nil) };
                let certain = value.certain && !self.assigned.contains(&id);
                let declared = if self.assigned.contains(&id) { Ty::any() } else { Ty::new(t.clone(), certain) };
                self.declare_type(id, declared);
                if self.assigned.contains(&id) {
                    self.set_flow(id, t.clone());
                }
                if must { value.t.clone() } else { t }
            }
            LITERAL_PAT => {
                let t = match tokens(pat).find(|t| matches!(t.kind(), NUMBER | STRING | TRUE_KW | FALSE_KW | NIL_KW)).map(|t| t.kind()) {
                    Some(NUMBER) => Type::Number,
                    Some(STRING) => Type::String,
                    Some(TRUE_KW | FALSE_KW) => Type::Bool,
                    Some(NIL_KW) => Type::Nil,
                    _ => Type::Any,
                };
                self.check_pattern_overlap(pat, value, &t);
                self.types.narrow(&value.t, &t)
            }
            RANGE_PAT => {
                self.check_pattern_overlap(pat, value, &Type::Number);
                self.types.narrow(&value.t, &Type::Number)
            }
            IS_PAT => {
                let t = type_child(pat).map(|t| self.type_of(&t)).unwrap_or(Type::Any);
                self.check_pattern_overlap(pat, value, &t);
                self.types.narrow(&value.t, &t)
            }
            TYPED_PAT => {
                let t = type_child(pat).map(|t| self.type_of(&t)).unwrap_or(Type::Any);
                self.check_pattern_overlap(pat, value, &t);
                let narrowed = self.types.narrow(&value.t, &t);
                match pattern_children(pat).next() {
                    Some(inner) => {
                        self.bind_pattern(&inner, &Ty::certain(narrowed.clone()), must);
                        narrowed
                    }
                    None => narrowed,
                }
            }
            AS_PAT => {
                let narrowed = match pattern_children(pat).next() {
                    Some(inner) => self.bind_pattern(&inner, value, must),
                    None => value.t.clone(),
                };
                if let Some(id) = child(pat, NAME).and_then(|n| self.res.declaration(n.text_range())) {
                    self.declare_type(id, Ty::new(narrowed.clone(), value.certain));
                }
                narrowed
            }
            BRACE_PAT => {
                self.check_pattern_overlap(pat, value, &Type::Table);
                let narrowed = self.types.narrow(&value.t, &Type::Table);
                for c in pat.children() {
                    match c.kind() {
                        PAT_FIELD => self.bind_pat_field(&c, &narrowed, value.certain, must),
                        k if is_pattern(k) => {
                            self.bind_pattern(&c, &Ty::any(), must);
                        }
                        _ => {}
                    }
                }
                narrowed
            }
            STRUCT_PAT => {
                let target = child(pat, QUAL_NAME).and_then(|q| self.struct_of_qual(&q));
                let st = target.map(Type::Named).unwrap_or(Type::Table);
                self.check_pattern_overlap(pat, value, &st);
                let narrowed = self.types.narrow(&value.t, &st);
                for c in pat.children() {
                    if c.kind() == PAT_FIELD {
                        self.bind_pat_field(&c, &narrowed, true, must);
                    }
                }
                narrowed
            }
            BRACKET_PAT => {
                self.check_pattern_overlap(pat, value, &Type::Table);
                let narrowed = self.types.narrow(&value.t, &Type::Table);
                let elem = match self.types.expand(&narrowed) {
                    Type::Array(e) => (*e).clone(),
                    _ => Type::Any,
                };
                let elem_ty = Ty::uncertain(elem.clone());
                for c in pat.children() {
                    match c.kind() {
                        REST_PAT => {
                            if let Some(id) = child(&c, NAME).and_then(|n| self.res.declaration(n.text_range())) {
                                self.declare_type(id, Ty::uncertain(Type::Array(Rc::new(elem.clone()))));
                            }
                        }
                        k if is_pattern(k) => {
                            self.bind_pattern(&c, &elem_ty, must);
                        }
                        _ => {}
                    }
                }
                narrowed
            }
            QUALIFIED_PAT => {
                let Some(q) = child(pat, QUAL_NAME) else { return value.t.clone() };
                if self.res.is_enum_variant(q.text_range()) {
                    match self.enum_of_variant(&q) {
                        Some(nid) => {
                            self.check_pattern_overlap(pat, value, &Type::Named(nid));
                            self.types.narrow(&value.t, &Type::Named(nid))
                        }
                        None => value.t.clone(),
                    }
                } else {
                    let t = self.qual_type(&q);
                    self.check_pattern_overlap(pat, value, &t);
                    self.types.narrow(&value.t, &t)
                }
            }
            _ => value.t.clone(),
        }
    }

    /// One keyed item of a brace or struct pattern.
    fn bind_pat_field(&mut self, field: &SyntaxNode, owner: &Type, certain: bool, must: bool) {
        let key = name_token(field).map(|t| t.text().to_string()).or_else(|| first_token(field).map(|t| match t.kind() {
            STRING => crate::resolver::decode_plain_string(&t),
            _ => t.text().to_string(),
        }));
        let field_ty = key.as_deref().map(|k| self.field_type_of(owner, k)).unwrap_or(Type::Any);
        let value = Ty::new(field_ty, certain && false);
        // `{name?}`, `{key?: p}`: an optional field matches when it is
        // `nil`, so what it binds may be `nil`, as in a must-match context.
        let must = must || has_token(field, QUESTION);
        if !has_token(field, COLON) {
            // `{name}`: binds the key.
            if let Some(id) = child(field, NAME).and_then(|n| self.res.declaration(n.text_range())) {
                let t = if must { optional(&self.types, &value.t) } else { self.types.exclude(&value.t, &Type::Nil) };
                let t = if value.t == Type::Any { Type::Any } else { t };
                self.declare_type(id, Ty::uncertain(t));
            }
            return;
        }
        if let Some(inner) = pattern_children(field).next() {
            let narrowed = self.bind_pattern(&inner, &value, must);
            // `{key: Mod.Type}` binds the key too.
            if inner.kind() == QUALIFIED_PAT {
                if let Some(q) = child(&inner, QUAL_NAME) {
                    if !self.res.is_enum_variant(q.text_range()) {
                        if let Some(id) = name_token(field).and_then(|k| self.res.declaration(k.text_range())) {
                            self.declare_type(id, Ty::certain(narrowed));
                        }
                    }
                }
            }
        } else if let Some(ty) = type_child(field) {
            // `{key: Type}` binds `key` and requires the type.
            let t = self.type_of(&ty);
            let t = if has_token(field, QUESTION) { optional(&self.types, &t) } else { t };
            if let Some(id) = name_token(field).and_then(|k| self.res.declaration(k.text_range())) {
                let fn_init = self.binding_sigs.contains_key(&id);
                let _ = fn_init;
                self.declare_type(id, Ty::certain(t));
            }
        }
    }

    /// The type of a field of a matched value: a struct's or a record's
    /// field, an array's element for a numeric key; `any` otherwise.
    fn field_type_of(&self, owner: &Type, key: &str) -> Type {
        match self.types.expand(owner) {
            Type::Named(nid) => self.types.field(nid, key).map(|f| f.ty).unwrap_or(Type::Any),
            Type::Record(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, t)| t.clone()).unwrap_or(Type::Any),
            Type::Array(e) if key.parse::<f64>().is_ok() => (*e).clone(),
            Type::Map(_, v) => (*v).clone(),
            _ => Type::Any,
        }
    }

    /// A pattern that can never match a value of a definite type is a
    /// warning (an unreachable clause, or a `let` that always raises).
    fn check_pattern_overlap(&mut self, pat: &SyntaxNode, value: &Ty, test: &Type) {
        if !self.types.is_definite(&value.t) || value.t == Type::Never {
            return;
        }
        if !self.types.overlaps(&value.t, test) {
            let got = self.show(&value.t);
            let want = self.show(test);
            self.warn(pat.text_range(), "pattern-never-matches", format!("this pattern needs {want}, and the value is {got}: it never matches"));
        }
    }

    /// The struct a `QUAL_NAME` of a struct pattern names.
    pub(crate) fn struct_of_qual(&mut self, q: &SyntaxNode) -> Option<NamedId> {
        let segs = qual_segments(q);
        if segs.first().is_some_and(|s| s.kind() == SELF_TYPE_KW) {
            return match self.res.self_target(q.text_range())? {
                SelfTarget::Binding(id) => self.named_of.get(id).copied().filter(|n| self.types.is_struct(*n)),
                _ => None,
            };
        }
        match self.res.type_ref(q.text_range())? {
            Ref::Binding(id) if segs.len() == 1 => {
                let nid = *self.named_of.get(id)?;
                match self.types.kind(nid)? {
                    NamedKind::Struct { .. } => Some(nid),
                    NamedKind::Alias(t) => match self.types.expand(t) {
                        Type::Named(s) if self.types.is_struct(s) => Some(s),
                        _ => None,
                    },
                    _ => None,
                }
            }
            Ref::Prelude(p) if segs.len() == 1 => self.prelude_type_id(p.name).filter(|n| self.types.is_struct(*n)),
            _ => None,
        }
    }

    /// The type a dotted name in a pattern names.
    fn qual_type(&mut self, q: &SyntaxNode) -> Type {
        let segs = qual_segments(q);
        match self.res.type_ref(q.text_range()) {
            Some(Ref::Binding(id)) if segs.len() == 1 => self.named_of.get(id).map(|n| Type::Named(*n)).unwrap_or(Type::Any),
            Some(Ref::Prelude(p)) if segs.len() == 1 => self.prelude_type(p),
            _ => Type::Any,
        }
    }

    /// The enum of a `QUAL_NAME` that names a variant (`Dir.North`).
    fn enum_of_variant(&self, q: &SyntaxNode) -> Option<NamedId> {
        let segs = qual_segments(q);
        if segs.len() != 2 {
            return None;
        }
        match self.res.type_ref(q.text_range())? {
            Ref::Binding(id) => self.named_of.get(id).copied().filter(|n| self.types.is_enum(*n)),
            _ => None,
        }
    }

    // ---- case ------------------------------------------------------------------------

    /// `case subject when … end`.
    pub(crate) fn case_expr(&mut self, node: &SyntaxNode, value_used: bool) -> Ty {
        let Some(subject) = expr_child(node) else { return Ty::any() };
        let subject_ty = self.expr1(&subject);
        let subject_binding = self.subject_binding(&subject);
        let saved = self.env.clone();
        let mut remaining = subject_ty.t.clone();
        let mut envs: Vec<Env> = Vec::new();
        let mut values: Vec<Type> = Vec::new();
        let mut has_else = false;
        let was = self.diverges;
        // What the unguarded clauses cover, for exhaustiveness.
        let mut covered: Vec<Cover> = Vec::new();
        for c in node.children() {
            match c.kind() {
                WHEN_CLAUSE => {
                    self.env = saved.clone();
                    let guard = child(&c, WHEN_GUARD);
                    let mut matched: Vec<Type> = Vec::new();
                    let mut excluded: Vec<Type> = Vec::new();
                    let mut excludes_all = true;
                    let mut unreachable = true;
                    let items: Vec<SyntaxNode> = c.children().filter(|i| is_pattern(i.kind()) || (is_expr(i.kind()) && i.kind() != WHEN_GUARD)).collect();
                    for item in &items {
                        let before = self.diags.len();
                        let (m, cover, excludes) = self.when_item(item, &subject_ty, &remaining);
                        if guard.is_none() {
                            if let Some(cv) = cover {
                                covered.push(cv);
                            }
                        }
                        match excludes {
                            Some(e) => excluded.push(e),
                            None => excludes_all = false,
                        }
                        let cannot_match = remaining == Type::Never || (self.types.is_definite(&remaining) && !self.types.overlaps(&remaining, &m));
                        if !cannot_match || !self.types.is_definite(&subject_ty.t) {
                            unreachable = false;
                        }
                        let _ = before;
                        matched.push(m);
                    }
                    if items.is_empty() {
                        unreachable = false;
                    }
                    let matched_ty = self.types.union_all(&matched);
                    if unreachable && self.types.is_definite(&subject_ty.t) && subject_ty.t != Type::Any {
                        let at = token(&c, WHEN_KW).map_or(c.text_range(), |t| t.text_range());
                        let shown = self.show(&remaining);
                        let msg = if remaining == Type::Never {
                            "this `when` is unreachable: the clauses before it cover every value of the subject".to_string()
                        } else {
                            format!("this `when` is unreachable: what remains of the subject here is {shown}, which it cannot match")
                        };
                        self.warn(at, "unreachable-when", msg);
                    }
                    if let Some(id) = subject_binding {
                        let narrowed = self.types.narrow(&remaining, &matched_ty);
                        if narrowed != Type::Never || !self.types.is_definite(&remaining) {
                            self.set_flow(id, narrowed);
                        }
                    }
                    if let Some(g) = &guard {
                        if let Some(e) = expr_child(g) {
                            self.expr1(&e);
                            let (t, _) = self.condition_envs(&e);
                            self.env = t;
                        }
                    }
                    self.diverges = false;
                    let v = block(&c).map(|b| self.scoped_block(&b, value_used)).unwrap_or_else(|| Ty::certain(Type::Nil));
                    if !self.diverges {
                        envs.push(self.env.clone());
                        values.push(v.t);
                    }
                    if guard.is_none() && excludes_all && self.types.is_definite(&remaining) {
                        let ex = self.types.union_all(&excluded);
                        remaining = self.types.exclude(&remaining, &ex);
                    }
                }
                ELSE_CLAUSE => {
                    has_else = true;
                    self.env = saved.clone();
                    if let Some(id) = subject_binding {
                        if self.types.is_definite(&remaining) && remaining != Type::Never {
                            self.set_flow(id, remaining.clone());
                        }
                    }
                    self.diverges = false;
                    let v = block(&c).map(|b| self.scoped_block(&b, value_used)).unwrap_or_else(|| Ty::certain(Type::Nil));
                    if !self.diverges {
                        envs.push(self.env.clone());
                        values.push(v.t);
                    }
                }
                _ => {}
            }
        }
        self.diverges = was;
        if !has_else {
            let exhaustive = self.check_exhaustive(node, &subject_ty.t, &covered);
            if !exhaustive {
                envs.push(saved.clone());
                values.push(Type::Nil);
            }
        }
        self.env = if envs.is_empty() { saved } else { self.merge_envs(&envs) };
        if envs.is_empty() {
            self.diverges = true;
            return Ty::certain(Type::Never);
        }
        Ty::uncertain(self.types.union_all(&values))
    }

    /// The local the subject names, for narrowing in the clauses.
    fn subject_binding(&self, subject: &SyntaxNode) -> Option<BindingId> {
        if subject.kind() != NAME_REF {
            return None;
        }
        match self.res.reference(subject.text_range())? {
            Ref::Binding(id) => {
                let b = self.binding(*id)?;
                if matches!(b.decl, DeclKind::Global | DeclKind::Fn | DeclKind::Import) || b.kind != BindingKind::Variable {
                    return None;
                }
                Some(*id)
            }
            _ => None,
        }
    }

    /// One item of a `when`: the type of the subject when it matches, what
    /// it covers for exhaustiveness, and the type it takes out of the
    /// subject for the clauses after it (a test of a whole type; a literal
    /// or a range takes nothing out).
    fn when_item(&mut self, item: &SyntaxNode, subject: &Ty, remaining: &Type) -> (Type, Option<Cover>, Option<Type>) {
        let current = Ty::new(remaining.clone(), subject.certain);
        if is_pattern(item.kind()) {
            let matched = self.bind_pattern(item, &current, false);
            let whole_type = match item.kind() {
                IS_PAT | BIND_PAT | QUALIFIED_PAT => true,
                TYPED_PAT => pattern_children(item).next().is_some_and(|p| p.kind() == BIND_PAT),
                STRUCT_PAT => children(item, PAT_FIELD).all(|f| !has_token(&f, COLON) || pattern_children(&f).next().is_some_and(|p| p.kind() == BIND_PAT)),
                _ => false,
            };
            let excludes = if whole_type { Some(matched.clone()) } else { None };
            let cover = match item.kind() {
                IS_PAT => Some(Cover::Type(type_child(item).map(|t| self.type_of(&t)).unwrap_or(Type::Any))),
                STRUCT_PAT => {
                    let binds_only = children(item, PAT_FIELD).all(|f| !has_token(&f, COLON) || pattern_children(&f).next().is_some_and(|p| p.kind() == BIND_PAT));
                    let target = child(item, QUAL_NAME).and_then(|q| self.struct_of_qual(&q));
                    match target {
                        Some(nid) if binds_only => Some(Cover::Type(Type::Named(nid))),
                        _ => None,
                    }
                }
                QUALIFIED_PAT => {
                    let q = child(item, QUAL_NAME);
                    match q {
                        Some(q) if self.res.is_enum_variant(q.text_range()) => qual_segments(&q).get(1).map(|v| Cover::Variant(v.text().to_string())),
                        Some(q) => Some(Cover::Type(self.qual_type(&q))),
                        None => None,
                    }
                }
                BIND_PAT => Some(Cover::All),
                _ => None,
            };
            return (matched, cover, excludes);
        }
        // An expression: compared with `==`; a range or regex by
        // containment or matching.
        self.check_when_type_name(item);
        let t = self.expr1(item);
        let cover = match item.kind() {
            FIELD_EXPR => {
                let recv = expr_child(item);
                let variant = field_name(item).map(|t| t.text().to_string());
                match (recv, variant) {
                    (Some(r), Some(v)) if r.kind() == NAME_REF => match self.res.reference(r.text_range()) {
                        Some(Ref::Binding(id)) if self.res.declared_as(*id) == Some(DeclKind::Enum) => Some(Cover::Variant(v)),
                        _ => None,
                    },
                    _ => None,
                }
            }
            _ => None,
        };
        let matched = match self.types.expand(&t.t) {
            Type::Range => self.types.narrow(remaining, &Type::Number),
            Type::Regex => self.types.narrow(remaining, &Type::String),
            Type::Named(nid) if self.types.is_enum(nid) => self.types.narrow(remaining, &Type::Named(nid)),
            Type::Number | Type::String | Type::Bool | Type::Nil => {
                if self.types.is_definite(remaining) && !self.types.overlaps(remaining, &t.t) && t.certain {
                    let got = self.show(remaining);
                    let want = self.show(&t.t);
                    self.warn(item.text_range(), "when-never-matches", format!("this value is {want}, and the subject here is {got}: they are never equal"));
                }
                self.types.narrow(remaining, &t.t)
            }
            _ => remaining.clone(),
        };
        (matched, cover, None)
    }

    /// `when Name` with the bare name of a struct, an enum, an interface or
    /// a library type compares the subject with the type's table, which
    /// only that table equals: no instance ever matches. The test of a type
    /// is `when is Name` (or `when Name {…}` for a struct), so the bare name
    /// is an error. A dotted name (`when M.Point`) is parsed as a pattern,
    /// a type test, and does not come here.
    fn check_when_type_name(&mut self, item: &SyntaxNode) {
        if item.kind() != NAME_REF {
            return;
        }
        let what = match self.res.reference(item.text_range()) {
            Some(Ref::Binding(id)) => match self.res.declared_as(*id) {
                Some(DeclKind::Struct) => "struct",
                Some(DeclKind::Enum) => "enum",
                Some(DeclKind::Interface) => "interface",
                _ => return,
            },
            Some(Ref::Prelude(p)) => match p.class {
                crate::prelude::PreludeClass::Struct => "struct",
                crate::prelude::PreludeClass::ValueAndType => "type",
                _ => return,
            },
            _ => return,
        };
        let name = name_text(item).unwrap_or_default();
        self.error(
            item.text_range(),
            "when-type-compare",
            format!("`when {name}` compares the subject with the {what} `{name}` itself, which no value of it equals; to test the type, write `when is {name}`"),
        );
    }

    /// Exhaustiveness: for an enum or a union of structs, a
    /// `case` without `else` must cover every variant. Returns whether
    /// it does.
    fn check_exhaustive(&mut self, node: &SyntaxNode, subject: &Type, covered: &[Cover]) -> bool {
        if covered.iter().any(|c| matches!(c, Cover::All)) {
            return true;
        }
        let at = token(node, CASE_KW).map_or(node.text_range(), |t| t.text_range());
        match self.types.expand(subject) {
            Type::Named(nid) if self.types.is_enum(nid) => {
                let variants: Vec<String> = match self.types.kind(nid) {
                    Some(NamedKind::Enum { variants }) => variants.iter().map(|(v, _)| v.clone()).collect(),
                    _ => Vec::new(),
                };
                if covered.iter().any(|c| matches!(c, Cover::Type(t) if self.types.assignable(&Type::Named(nid), t))) {
                    return true;
                }
                let missing: Vec<String> = variants.into_iter().filter(|v| !covered.iter().any(|c| matches!(c, Cover::Variant(x) if x == v))).collect();
                if missing.is_empty() {
                    return true;
                }
                let name = self.types.name(nid).to_string();
                self.warn(at, "non-exhaustive", format!("this `case` over `{name}` has no `else` and misses {}", missing.iter().map(|m| format!("`{name}.{m}`")).collect::<Vec<_>>().join(", ")));
                false
            }
            t => {
                let members = self.types.members(&t);
                let structs: Vec<NamedId> = members.iter().filter_map(|m| match m { Type::Named(n) if self.types.is_struct(*n) => Some(*n), _ => None }).collect();
                if structs.is_empty() || structs.len() != members.len() {
                    return false;
                }
                let missing: Vec<String> = structs.into_iter().filter(|s| !covered.iter().any(|c| matches!(c, Cover::Type(ct) if self.types.assignable(&Type::Named(*s), ct)))).map(|s| self.types.name(s).to_string()).collect();
                if missing.is_empty() {
                    return true;
                }
                self.warn(at, "non-exhaustive", format!("this `case` has no `else` and misses {}", missing.iter().map(|m| format!("`{m}`")).collect::<Vec<_>>().join(", ")));
                false
            }
        }
    }
}

/// What an unguarded `when` item covers.
enum Cover {
    All,
    Type(Type),
    Variant(String),
}

#[allow(dead_code)]
fn range_of(n: &SyntaxNode) -> TextRange {
    n.text_range()
}
