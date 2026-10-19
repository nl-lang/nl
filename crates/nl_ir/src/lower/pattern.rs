//! Patterns: must-match contexts (`let`, `const`, parameters,
//! block parameters, `for`) raise `MatchError` on a mismatch and bind a
//! missing key as `nil`; may-match contexts (`if let`, `when`) give a
//! condition, and a name bound to `nil` is a mismatch there.
//!
//! Each pattern becomes a sequence of tests and bindings, without a
//! decision tree:
//!
//! - must-match: `if not test then rt.matcherror(…) end` before each step;
//! - may-match: the bindings are declared first, then
//!   `local ok = false; while true do … if not test then break end … ok =
//!   true; break end`, and the condition is `ok`.

use nl_sema::util::*;
use nl_sema::Ref;
use nl_syntax::{SyntaxKind, SyntaxNode};

use super::Lowerer;
use super::func::simple_binding;
use crate::ir::*;

use SyntaxKind::*;

/// A typed pattern that fails in a must-match context raises `TypeError`,
/// as an annotated parameter does; this constant is the
/// one place that decides it.
const TYPED_MISMATCH_IS_TYPE_ERROR: bool = true;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Must,
    May,
}

/// The description of a part of a value, for messages: `field 'hp'` of
/// `ctx` (empty: the value being destructured).
fn part_of(part: String, ctx: &str) -> String {
    if ctx.is_empty() { part } else { format!("{part} of {ctx}") }
}

/// The description of the whole value.
fn whole(ctx: &str) -> &str {
    if ctx.is_empty() { "the destructured value" } else { ctx }
}

impl Lowerer<'_> {
    /// Binds a must-match pattern to `value`. `ctx` describes the value in
    /// messages (`parameter 1 of describe`); empty for the value of a
    /// `let`.
    ///
    /// The names the pattern binds are declared first; its tests and
    /// temporaries are scoped to a block of their own, so that they do not
    /// accumulate in the enclosing block (LuaJIT allows 200 locals).
    pub(crate) fn bind_must(&mut self, pat: &SyntaxNode, value: Expr, ctx: &str) {
        if let Some((name, ty)) = simple_binding(pat) {
            let Some(id) = self.res.declaration(name.text_range()) else {
                self.internal(pat, "pattern binding");
                return;
            };
            let l = self.define(id, Some(value));
            if let Some(t) = ty {
                let what = if ctx.is_empty() { format!("variable '{}'", name_text(&name).unwrap_or_default()) } else { ctx.to_string() };
                self.emit_check(Expr::Local(l), &t, &what);
            }
            return;
        }
        let mut ids = Vec::new();
        self.pattern_bindings(pat, &mut ids);
        let fresh: Vec<_> = ids.into_iter().filter(|id| !self.is_predeclared(*id)).collect();
        if !fresh.is_empty() {
            let locals = fresh.iter().map(|id| self.local_of(*id)).collect();
            self.emit(StmtKind::Local { locals, values: Vec::new() });
            for id in fresh {
                self.mark_predeclared(id);
            }
        }
        let body = self.sub_block(|l| {
            let v = l.stable(value);
            l.pattern(pat, v, Mode::Must, ctx);
        });
        self.emit(StmtKind::Do(body));
    }

    /// Tries a may-match pattern against `value`: emits the bindings and
    /// returns the condition under which they hold.
    pub(crate) fn bind_may(&mut self, pat: &SyntaxNode, value: Expr) -> Expr {
        if pat.kind() == BIND_PAT {
            let Some((name, _)) = simple_binding(pat) else { return self.internal(pat, "pattern") };
            let Some(id) = self.res.declaration(name.text_range()) else { return self.internal(pat, "pattern binding") };
            let l = self.define(id, Some(value));
            // Binding requires a non-nil value in a may-match context.
            return Expr::binary(BinOp::Ne, Expr::Local(l), Expr::Nil);
        }
        // The bindings live outside the test loop.
        let mut ids = Vec::new();
        self.pattern_bindings(pat, &mut ids);
        for id in ids {
            self.define(id, None);
            self.mark_predeclared(id);
        }
        let v = self.stable(value);
        let ok = self.fresh_temp(Expr::Bool(false), "matched");
        let mut body = self.sub_block(|l| {
            l.pattern(pat, v, Mode::May, "");
            l.emit(StmtKind::Assign { targets: vec![Place::Local(ok)], values: vec![Expr::Bool(true)] });
        });
        body.stmts.push(Stmt { line: self.line, kind: StmtKind::Break });
        self.emit(StmtKind::While { cond: Expr::Bool(true), body });
        Expr::Local(ok)
    }

    /// The test of a pattern item of `when` against the subject.
    pub(crate) fn case_pattern(&mut self, pat: &SyntaxNode, subject: Expr) -> Expr {
        self.bind_may(pat, subject)
    }

    /// A value that can be read several times: a local or a constant.
    fn stable(&mut self, v: Expr) -> Expr {
        match v {
            Expr::Local(_) => v,
            v if v.is_constant() => v,
            v => Expr::Local(self.fresh_temp(v, "value")),
        }
    }

    /// Fails the match unless `cond` holds.
    fn require(&mut self, cond: Expr, mode: Mode, what: &str) {
        let line = self.line;
        let fail = match mode {
            Mode::May => StmtKind::Break,
            Mode::Must => StmtKind::Expr(Expr::intrinsic_call("matcherror", vec![Expr::str(what)])),
        };
        self.emit(StmtKind::If { cond: Expr::lnot(cond), then: Block::new(vec![Stmt { line, kind: fail }]), otherwise: None });
    }

    /// Requires `v` to be of type `ty`. In a must-match context that is a
    /// runtime check.
    fn require_type(&mut self, v: &Expr, ty: &SyntaxNode, mode: Mode, what: &str) {
        let Some(t) = self.type_value(ty) else { return };
        if mode == Mode::Must && TYPED_MISMATCH_IS_TYPE_ERROR {
            if self.opts.checks {
                let test = self.inline_test(ty);
                self.emit_check_value(v.clone(), t, test, what);
            }
            return;
        }
        self.require(Expr::intrinsic_call("isa", vec![v.clone(), t]), mode, what);
    }

    /// Binds a name declared by the pattern.
    fn bind_name(&mut self, decl_range: nl_syntax::TextRange, v: Expr, mode: Mode, node: &SyntaxNode) {
        let Some(id) = self.res.declaration(decl_range) else {
            self.internal(node, "pattern binding");
            return;
        };
        let l = self.define(id, Some(v));
        if mode == Mode::May {
            // A name bound to a missing key is a mismatch.
            self.require(Expr::binary(BinOp::Ne, Expr::Local(l), Expr::Nil), mode, "a value");
        }
    }

    /// Emits the tests and bindings of `pat` against the stable value `v`,
    /// which `ctx` describes (see [`Self::bind_must`]).
    fn pattern(&mut self, pat: &SyntaxNode, v: Expr, mode: Mode, ctx: &str) {
        match pat.kind() {
            BIND_PAT => {
                if let Some(name) = child(pat, NAME) {
                    self.bind_name(name.text_range(), v, mode, pat);
                }
            }
            TYPED_PAT => {
                if let Some(t) = type_child(pat) {
                    let name = pattern_children(pat).next().filter(|i| i.kind() == BIND_PAT).and_then(|i| child(&i, NAME)).and_then(|n| name_text(&n));
                    let what = match name {
                        Some(n) if ctx.is_empty() => format!("variable '{n}'"),
                        Some(n) => format!("variable '{n}' ({ctx})"),
                        None => whole(ctx).to_string(),
                    };
                    self.require_type(&v, &t, mode, &what);
                }
                if let Some(inner) = pattern_children(pat).next() {
                    self.pattern(&inner, v, mode, ctx);
                }
            }
            AS_PAT => {
                if let Some(inner) = pattern_children(pat).next() {
                    self.pattern(&inner, v.clone(), mode, ctx);
                }
                if let Some(name) = child(pat, NAME) {
                    self.bind_name(name.text_range(), v, mode, pat);
                }
            }
            LITERAL_PAT => {
                let lit = self.literal_pattern_value(pat);
                self.require(Expr::binary(BinOp::Eq, v, lit), mode, "a literal pattern");
            }
            RANGE_PAT => {
                let ends: Vec<SyntaxNode> = children(pat, LITERAL_PAT).collect();
                let (Some(a), Some(b)) = (ends.first(), ends.get(1)) else {
                    self.internal(pat, "range pattern");
                    return;
                };
                let excl = has_token(pat, DOT3);
                let (a, b) = (self.literal_pattern_value(a), self.literal_pattern_value(b));
                self.require(Expr::intrinsic_call("inrange", vec![v, a, b, Expr::Bool(excl)]), mode, "a range pattern");
            }
            IS_PAT => {
                if let Some(tv) = type_child(pat).and_then(|t| self.type_value(&t)) {
                    self.require(Expr::intrinsic_call("isa", vec![v, tv]), mode, "an `is` pattern");
                }
            }
            BRACE_PAT => {
                let keyed = children(pat, PAT_FIELD).next().is_some();
                if keyed {
                    self.require(Expr::intrinsic_call("istable", vec![v.clone()]), mode, "a table");
                    for f in children(pat, PAT_FIELD) {
                        self.pattern_field(&f, &v, mode, ctx);
                    }
                } else {
                    let items: Vec<SyntaxNode> = pattern_children(pat).collect();
                    self.positional(&items, None, &v, mode, ctx);
                }
            }
            BRACKET_PAT => {
                let items: Vec<SyntaxNode> = pattern_children(pat).collect();
                let rest = child(pat, REST_PAT);
                self.positional(&items, rest.as_ref().map(|r| (r, child(r, NAME))), &v, mode, ctx);
            }
            STRUCT_PAT => {
                let ty = child(pat, QUAL_NAME).and_then(|q| self.qual_type_value(pat, &q));
                if let Some(t) = ty {
                    self.require(Expr::intrinsic_call("isa", vec![v.clone(), t]), mode, "a struct pattern");
                }
                for f in children(pat, PAT_FIELD) {
                    self.pattern_field(&f, &v, mode, ctx);
                }
            }
            QUALIFIED_PAT => {
                if let Some(q) = child(pat, QUAL_NAME) {
                    self.qualified(pat, &q, v, mode);
                }
            }
            _ => {
                self.internal(pat, "pattern");
            }
        }
    }

    /// `[P1, P2]`, `[P1, *rest]` and positional `{L1, L2}`: exact length
    /// unless there is a rest.
    fn positional(&mut self, items: &[SyntaxNode], rest: Option<(&SyntaxNode, Option<SyntaxNode>)>, v: &Expr, mode: Mode, ctx: &str) {
        self.require(Expr::intrinsic_call("istable", vec![v.clone()]), mode, "a table");
        let n = items.len() as f64;
        let len = Expr::Unary(UnOp::Len, Box::new(v.clone()));
        let cond = if rest.is_some() { Expr::binary(BinOp::Ge, len, Expr::Number(n)) } else { Expr::binary(BinOp::Eq, len, Expr::Number(n)) };
        self.require(cond, mode, "a table of the right length");
        for (i, item) in items.iter().enumerate() {
            let sub = Expr::index(v.clone(), Expr::Number((i + 1) as f64));
            let sub = self.stable(sub);
            self.pattern(item, sub, mode, &part_of(format!("item {}", i + 1), ctx));
        }
        if let Some((node, Some(name))) = rest {
            let tail = Expr::intrinsic_call("rest", vec![v.clone(), Expr::Number(n + 1.0)]);
            self.bind_name_plain(name.text_range(), tail, node);
        }
    }

    /// Binds without the non-nil requirement (a rest table is never nil).
    fn bind_name_plain(&mut self, decl_range: nl_syntax::TextRange, v: Expr, node: &SyntaxNode) {
        match self.res.declaration(decl_range) {
            Some(id) => {
                self.define(id, Some(v));
            }
            None => {
                self.internal(node, "pattern binding");
            }
        }
    }

    /// One keyed item: `{name}`, `{key: P}`, `{key: Type}`.
    fn pattern_field(&mut self, f: &SyntaxNode, v: &Expr, mode: Mode, ctx: &str) {
        if !has_token(f, COLON) {
            // `{name}`: short for `{name: name}`.
            if let Some(name) = child(f, NAME) {
                let key = name_text(&name).unwrap_or_default();
                let sub = Expr::index(v.clone(), Expr::str(&key));
                self.bind_name(name.text_range(), sub, mode, f);
            }
            return;
        }
        let Some(key_tok) = first_token(f) else { return };
        let key = match key_tok.kind() {
            NUMBER => Expr::Number(nl_syntax::number_value(key_tok.text()).unwrap_or(0.0)),
            STRING => Expr::Str(nl_syntax::decode_string(&[key_tok.text()]).into_iter().next().unwrap_or_default()),
            _ => Expr::str(key_tok.text()),
        };
        let sub = Expr::index(v.clone(), key);
        let sub = self.stable(sub);
        let field = part_of(format!("field '{}'", key_label(&key_tok)), ctx);
        if let Some(p) = pattern_children(f).next() {
            self.pattern(&p, sub.clone(), mode, &field);
            // `{key: Mod.Type}` also binds the key, unless the dotted name is
            // an enum variant (the resolver decides).
            if let Some(q) = child(&p, QUAL_NAME).filter(|_| p.kind() == QUALIFIED_PAT) {
                if !self.res.is_enum_variant(q.text_range()) && self.res.declaration(key_tok.text_range()).is_some() {
                    self.bind_name(key_tok.text_range(), sub, mode, f);
                }
            }
        } else if let Some(t) = type_child(f) {
            if t.kind() == NAMED_TYPE && has_token(&t, NIL_KW) {
                // `{key: nil}` requires the key to be `nil` (the type and
                // the literal agree) and binds nothing: a name bound to
                // `nil` would be a mismatch in a may-match context, as
                // Rust's `{a: None}` binds nothing.
                self.require_type(&sub, &t, mode, &field);
                return;
            }
            // `{key: Type}` binds `key` and requires the type. A key
            // that is a number or a string binds nothing.
            self.require_type(&sub, &t, mode, &field);
            if self.res.declaration(key_tok.text_range()).is_some() {
                self.bind_name(key_tok.text_range(), sub, mode, f);
            }
        }
    }

    /// A dotted name in a pattern: an enum variant is compared with
    /// `==`; a type is tested with `is`. Which one it is, the resolver
    /// decided.
    fn qualified(&mut self, pat: &SyntaxNode, q: &SyntaxNode, v: Expr, mode: Mode) {
        let segs: Vec<String> = qual_segments(q).iter().map(|t| t.text().to_string()).collect();
        let Some(base) = self.qual_base(pat, q) else { return };
        let mut value = base;
        for s in segs.iter().skip(1) {
            value = Expr::index(value, Expr::str(s));
        }
        if self.res.is_enum_variant(q.text_range()) {
            self.require(Expr::binary(BinOp::Eq, v, value), mode, "an enum value");
        } else {
            self.require(Expr::intrinsic_call("isa", vec![v, value]), mode, "a type");
        }
    }

    /// The value the first segment of a pattern's dotted name refers to.
    fn qual_base(&mut self, pat: &SyntaxNode, q: &SyntaxNode) -> Option<Expr> {
        match self.res.type_ref(q.text_range()).cloned() {
            Some(Ref::Binding(id)) => Some(Expr::Local(self.local_of(id))),
            Some(Ref::Prelude(p)) => Some(self.prelude_value(p)),
            _ => {
                if let Some(t) = self.res.self_target(q.text_range()).cloned() {
                    return Some(self.self_target_value(pat, Some(t)));
                }
                self.internal(pat, "unresolved name in a pattern");
                None
            }
        }
    }

    /// The struct a struct pattern names.
    fn qual_type_value(&mut self, pat: &SyntaxNode, q: &SyntaxNode) -> Option<Expr> {
        let segs: Vec<String> = qual_segments(q).iter().map(|t| t.text().to_string()).collect();
        let mut base = self.qual_base(pat, q)?;
        for s in segs.iter().skip(1) {
            base = Expr::index(base, Expr::str(s));
        }
        Some(base)
    }

    /// The value of a `LITERAL_PAT`: `[-] NUMBER`, a string, `true`,
    /// `false`, `nil`.
    fn literal_pattern_value(&mut self, lit: &SyntaxNode) -> Expr {
        let negative = has_token(lit, MINUS);
        match tokens(lit).find(|t| t.kind() != MINUS) {
            Some(t) => match t.kind() {
                NUMBER => {
                    let n = nl_syntax::number_value(t.text()).unwrap_or(0.0);
                    Expr::Number(if negative { -n } else { n })
                }
                STRING => Expr::Str(nl_syntax::decode_string(&[t.text()]).into_iter().next().unwrap_or_default()),
                TRUE_KW => Expr::Bool(true),
                FALSE_KW => Expr::Bool(false),
                NIL_KW => Expr::Nil,
                _ => self.internal(lit, "literal pattern"),
            },
            None => self.internal(lit, "literal pattern"),
        }
    }
}

/// A pattern key as messages show it: the name, number or decoded string.
fn key_label(tok: &nl_syntax::SyntaxToken) -> String {
    match tok.kind() {
        STRING => String::from_utf8_lossy(&nl_syntax::decode_string(&[tok.text()]).into_iter().next().unwrap_or_default()).into_owned(),
        _ => tok.text().to_string(),
    }
}
