//! The expression dispatcher, and the expressions that need no module of
//! their own: literals, strings, names, `self`, `@name`, tables, indexing,
//! members (`.len` included) and `!`.

use nl_sema::util::*;
use nl_sema::{Access, BindingKind, DeclKind, MemberHint, Prelude, Ref, SelfTarget};
use nl_syntax::{SyntaxKind, SyntaxNode};

use super::{Lowerer, Tail};
use crate::ir::*;

use SyntaxKind::*;

impl Lowerer<'_> {
    /// Lowers an expression whose value is used. The result may be a call
    /// that yields several values; positions that need one value truncate.
    pub(crate) fn expr(&mut self, node: &SyntaxNode) -> Expr {
        // Nil-safe chains (nilsafe.rs): the value a guard tested, or a
        // chain that ends here.
        if let Some(v) = self.preset(node) {
            return v;
        }
        if let Some(v) = self.safe_chain_value(node) {
            return v;
        }
        match node.kind() {
            LITERAL => self.literal(node),
            INTERP_STRING => self.interp_string(node),
            NAME_REF => self.name_value(node),
            SELF_EXPR => self.self_expr(node),
            AT_EXPR => self.at_expr(node),
            PAREN_EXPR => match expr_child(node) {
                Some(inner) => Expr::Paren(Box::new(self.expr(&inner))),
                None => self.internal(node, "empty parentheses"),
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
            SPAWN_EXPR => self.lower_spawn(node),
            ON_EXPR => self.lower_on(node),
            IF_EXPR | UNLESS_EXPR | CASE_EXPR | COND_EXPR | TRY_EXPR => {
                let t = self.new_local("v");
                self.emit(StmtKind::Local { locals: vec![t], values: Vec::new() });
                self.construct(node, Tail::Assign(t));
                Expr::Local(t)
            }
            _ => self.internal(node, "expression"),
        }
    }

    /// Lowers an expression to exactly one value.
    pub(crate) fn expr1(&mut self, node: &SyntaxNode) -> Expr {
        let e = self.expr(node);
        if e.is_multi() { Expr::Paren(Box::new(e)) } else { e }
    }

    /// An expression whose value is discarded: a statement that is not
    /// last. Names and members are called, a Lua path is
    /// called.
    pub(crate) fn discard_expr(&mut self, node: &SyntaxNode) {
        if self.safe_chain_discard(node) {
            return;
        }
        match node.kind() {
            NAME_REF => {
                let e = self.call_name(node, Vec::new());
                self.emit_effect(e);
            }
            FIELD_EXPR if !is_len(node) => {
                let e = self.member_call_of(node, Vec::new(), &[], None);
                self.emit_effect(e);
            }
            LUA_PATH => {
                let e = self.lua_path_statement(node);
                self.emit_effect(e);
            }
            AT_EXPR => {
                let target = self.at_expr(node);
                self.emit_effect(Expr::call(target, Vec::new()));
            }
            PAREN_EXPR => {
                if let Some(inner) = expr_child(node) {
                    self.discard_expr(&inner);
                }
            }
            k if super::is_block_construct(k) => self.construct(node, Tail::Discard),
            _ => {
                let e = self.expr(node);
                self.emit_effect(e);
            }
        }
    }

    // ---- literals and strings ---------------------------------------------

    fn literal(&mut self, node: &SyntaxNode) -> Expr {
        let Some(tok) = first_token(node) else { return self.internal(node, "empty literal") };
        match tok.kind() {
            NUMBER => Expr::Number(nl_syntax::number_value(tok.text()).unwrap_or(0.0)),
            STRING => {
                let parts = nl_syntax::decode_string(&[tok.text()]);
                Expr::Str(parts.into_iter().next().unwrap_or_default())
            }
            TRUE_KW => Expr::Bool(true),
            FALSE_KW => Expr::Bool(false),
            NIL_KW => Expr::Nil,
            _ => self.internal(node, "literal"),
        }
    }

    /// `"text #{expr} text"`: pieces joined with `..`, each value converted
    /// with NL's `tostring`.
    fn interp_string(&mut self, node: &SyntaxNode) -> Expr {
        let mut texts = Vec::new();
        let mut exprs = Vec::new();
        for c in node.children_with_tokens() {
            match c {
                rowan::NodeOrToken::Token(t) if t.kind().is_string_part() => texts.push(t.text().to_string()),
                rowan::NodeOrToken::Node(n) if is_expr(n.kind()) => exprs.push(n),
                _ => {}
            }
        }
        let refs: Vec<&str> = texts.iter().map(String::as_str).collect();
        let pieces = nl_syntax::decode_string(&refs);
        let mut values = Vec::new();
        for e in &exprs {
            self.push_operand(&mut values, |l| {
                let v = l.expr1(e);
                // A value the analyzer knows to be a string needs no
                // conversion.
                if l.opts.hints.is_string_interp(e.text_range()) { v } else { Expr::intrinsic_call("tostring", vec![v]) }
            });
        }
        let mut parts: Vec<Expr> = Vec::new();
        for (i, piece) in pieces.into_iter().enumerate() {
            if !piece.is_empty() {
                parts.push(Expr::Str(piece));
            }
            if let Some(v) = values.get(i) {
                parts.push(v.clone());
            }
        }
        if parts.is_empty() {
            return Expr::str("");
        }
        concat_all(parts)
    }

    // ---- names -------------------------------------------------------------

    /// The value of a prelude name.
    pub(crate) fn prelude_value(&self, p: &Prelude) -> Expr {
        match p.access {
            Access::LuaGlobal(name) => Expr::Global(name.to_string()),
            Access::Runtime(name) => Expr::Intrinsic(name),
            Access::TypeOnly(name) => Expr::index(Expr::Intrinsic("T"), Expr::str(name)),
        }
    }

    /// The value a resolved name holds, without calling it.
    pub(crate) fn ref_value(&mut self, node: &SyntaxNode, r: Option<Ref>) -> Expr {
        match r {
            Some(Ref::Binding(id)) => {
                let Some(b) = self.res.binding(id) else { return self.internal(node, "binding") };
                if b.decl == DeclKind::Global {
                    Expr::Global(b.name.clone())
                } else {
                    Expr::Local(self.local_of(id))
                }
            }
            Some(Ref::Prelude(p)) => self.prelude_value(p),
            Some(Ref::LuaGlobal(name)) => Expr::Global(name),
            Some(Ref::Declared(id)) => match self.res.declared_path(id) {
                Some(root) => self.lua_root_value(root),
                None => self.internal(node, "declared name without a Lua path"),
            },
            None => self.internal(node, "unresolved name"),
        }
    }

    /// The value a declared name stands for: its Lua path, written out at
    /// each use.
    pub(crate) fn lua_root_value(&mut self, root: &nl_sema::LuaRoot) -> Expr {
        let arg = |a: &nl_sema::LuaArg| match a {
            nl_sema::LuaArg::Str(s) => Expr::Str(s.clone()),
            nl_sema::LuaArg::Number(n) => Expr::Number(*n),
            nl_sema::LuaArg::Bool(b) => Expr::Bool(*b),
            nl_sema::LuaArg::Nil => Expr::Nil,
        };
        let mut v = Expr::Global(root.global.clone());
        for step in &root.steps {
            v = match step {
                nl_sema::LuaStep::Field(name) => Expr::index(v, Expr::str(name)),
                nl_sema::LuaStep::Index(a) => Expr::index(v, arg(a)),
                nl_sema::LuaStep::Call(args) => Expr::Paren(Box::new(Expr::call(v, args.iter().map(arg).collect()))),
            };
        }
        v
    }

    /// The value of a `NAME_REF` without calling it (`f!`).
    pub(crate) fn name_raw(&mut self, node: &SyntaxNode) -> Expr {
        let r = self.res.reference(node.text_range()).cloned();
        self.ref_value(node, r)
    }

    /// A `NAME_REF` whose value is used: a function-kind name is called.
    fn name_value(&mut self, node: &SyntaxNode) -> Expr {
        let r = self.res.reference(node.text_range()).cloned();
        if let Some(Ref::Binding(id)) = &r {
            if self.res.binding(*id).is_some_and(|b| b.kind == BindingKind::Function) {
                return self.call_name(node, Vec::new());
            }
        }
        // A `declare fn` is function-kind too.
        if let Some(Ref::Declared(id)) = &r {
            if self.res.declared_binding_kind(*id) == Some(BindingKind::Function) {
                return self.call_name(node, Vec::new());
            }
        }
        self.ref_value(node, r)
    }

    fn self_expr(&mut self, node: &SyntaxNode) -> Expr {
        if has_token(node, SELF_TYPE_KW) {
            let t = self.res.self_target(node.text_range()).cloned();
            return self.self_target_value(node, t);
        }
        self.name_raw(node)
    }

    /// The value `Self` stands for.
    pub(crate) fn self_target_value(&mut self, node: &SyntaxNode, t: Option<SelfTarget>) -> Expr {
        match t {
            Some(SelfTarget::Binding(id)) => Expr::Local(self.local_of(id)),
            Some(SelfTarget::Member { id, path }) => {
                let mut v = Expr::Local(self.local_of(id));
                for p in &path {
                    v = Expr::index(v, Expr::str(p));
                }
                v
            }
            // `Bool` has no value as a prelude name; `Self` in `fn Bool.x`
            // is the table of Bool's methods, as it is `math` for `Number`
            // and NL's string library for `String`.
            Some(SelfTarget::Prelude(p)) if p.name == "Bool" => Expr::Intrinsic("Bool"),
            Some(SelfTarget::Prelude(p)) => self.prelude_value(p),
            None => self.internal(node, "Self"),
        }
    }

    /// `@name`: `self.name`, a plain field access.
    pub(crate) fn at_expr(&mut self, node: &SyntaxNode) -> Expr {
        let name = token(node, AT_NAME).map(|t| t.text().trim_start_matches('@').to_string()).unwrap_or_default();
        let this = self.name_raw(node);
        Expr::index(this, Expr::str(&name))
    }

    // ---- tables ----------------------------------------------------------------

    /// `[a, b, *rest]`: a table constructor. A spread as the
    /// last item expands in place; one elsewhere makes the list from its
    /// parts with `rt.listcat`.
    fn list(&mut self, node: &SyntaxNode) -> Expr {
        let items: Vec<SyntaxNode> = node.children().filter(|c| c.kind() == SPREAD_ARG || is_expr(c.kind())).collect();
        let mut values = Vec::new();
        for c in &items {
            if c.kind() == SPREAD_ARG {
                match expr_child(c) {
                    Some(e) => self.push_operand(&mut values, |l| l.expr1(&e)),
                    None => values.push(Expr::Nil),
                }
            } else {
                self.push_operand(&mut values, |l| l.expr(c));
            }
        }
        let spread_inside = items.iter().enumerate().any(|(i, c)| c.kind() == SPREAD_ARG && i + 1 < items.len());
        let mut entries = Vec::new();
        if !spread_inside {
            let n = items.len();
            for (i, (c, v)) in items.iter().zip(values).enumerate() {
                let v = if c.kind() == SPREAD_ARG {
                    Expr::intrinsic_call("spread", vec![v])
                } else if i + 1 < n && v.is_multi() {
                    Expr::Paren(Box::new(v))
                } else {
                    v
                };
                entries.push(TableEntry::Item(v));
            }
            return Expr::Table(entries);
        }
        // Parts: each run of plain items as a table, each spread as itself.
        let mut parts = Vec::new();
        let mut run = Vec::new();
        for (c, v) in items.iter().zip(values) {
            if c.kind() == SPREAD_ARG {
                if !run.is_empty() {
                    parts.push(Expr::Table(std::mem::take(&mut run)));
                }
                parts.push(v);
            } else {
                let v = if v.is_multi() { Expr::Paren(Box::new(v)) } else { v };
                run.push(TableEntry::Item(v));
            }
        }
        if !run.is_empty() {
            parts.push(Expr::Table(run));
        }
        Expr::intrinsic_call("listcat", parts)
    }

    fn table(&mut self, node: &SyntaxNode) -> Expr {
        // Keys and values are lowered in order as one operand sequence.
        enum Slot {
            Item,
            Pair,
        }
        let mut slots = Vec::new();
        let mut values = Vec::new();
        for c in node.children() {
            match c.kind() {
                TABLE_ITEM => {
                    if let Some(e) = expr_child(&c) {
                        slots.push(Slot::Item);
                        self.push_operand(&mut values, |l| l.expr(&e));
                    }
                }
                TABLE_PAIR => {
                    let key = child(&c, TABLE_KEY);
                    let value = expr_child(&c);
                    let (Some(key), Some(value)) = (key, value) else { continue };
                    slots.push(Slot::Pair);
                    self.push_operand(&mut values, |l| l.table_key(&key));
                    self.push_operand(&mut values, |l| l.expr1(&value));
                }
                _ => {}
            }
        }
        let mut entries = Vec::new();
        let mut it = values.into_iter();
        let count = slots.len();
        for (i, s) in slots.into_iter().enumerate() {
            match s {
                Slot::Item => {
                    let v = it.next().unwrap_or(Expr::Nil);
                    // Only the last positional item may expand.
                    let v = if i + 1 < count && v.is_multi() { Expr::Paren(Box::new(v)) } else { v };
                    entries.push(TableEntry::Item(v));
                }
                Slot::Pair => {
                    let k = it.next().unwrap_or(Expr::Nil);
                    let v = it.next().unwrap_or(Expr::Nil);
                    entries.push(TableEntry::Pair(k, v));
                }
            }
        }
        Expr::Table(entries)
    }

    /// A table key: a word, a number, a string, or `[expr]`.
    fn table_key(&mut self, key: &SyntaxNode) -> Expr {
        if let Some(e) = expr_child(key) {
            return self.expr1(&e);
        }
        match first_token(key) {
            Some(t) if t.kind() == NUMBER => Expr::Number(nl_syntax::number_value(t.text()).unwrap_or(0.0)),
            Some(t) if t.kind() == STRING => {
                Expr::Str(nl_syntax::decode_string(&[t.text()]).into_iter().next().unwrap_or_default())
            }
            Some(t) => Expr::str(t.text()),
            None => self.internal(key, "table key"),
        }
    }

    // ---- indexing and members ----------------------------------------------

    /// `x[k]` read: at level 0 through the runtime, so that a string
    /// receiver gets NL's character indexing;
    /// at level 1 a plain index, which the fork's VM gives the same
    /// meaning.
    fn index_expr(&mut self, node: &SyntaxNode) -> Expr {
        let parts: Vec<SyntaxNode> = expr_children(node).collect();
        let ops = self.operands(&parts);
        let mut it = ops.into_iter();
        let (Some(o), Some(k)) = (it.next(), it.next()) else { return self.internal(node, "index") };
        // A receiver certain to be a table: `rt.index` would only index it.
        if self.opts.hints.is_table_read(node.text_range()) {
            return Expr::index(single(o), single(k));
        }
        self.index_read(single(o), single(k))
    }

    /// A raw `x[k]` read under NL's rules (the index of a string, the
    /// `[]` of a struct): the level decides who applies them.
    pub(crate) fn index_read(&mut self, o: Expr, k: Expr) -> Expr {
        if self.opts.level >= 1 { Expr::index(o, k) } else { Expr::intrinsic_call("index", vec![o, k]) }
    }

    /// `x.member` whose value is used: `.len` is the length;
    /// any other member is auto-called when it is a method.
    fn field_value(&mut self, node: &SyntaxNode) -> Expr {
        let Some(recv) = expr_child(node) else { return self.internal(node, "member access") };
        let name = field_name(node).map(|t| t.text().to_string()).unwrap_or_default();
        // `args.len` of a rest parameter is the count of arguments it
        // received, nils included: the hidden local, or for a table that
        // escapes rt.restlen.
        if name == "len" {
            if let Some((_, n)) = self.counted_rest(&recv) {
                return n;
            }
            if self.rest_param(&recv).is_some() {
                let r = self.expr1(&recv);
                return Expr::intrinsic_call("restlen", vec![r]);
            }
        }
        let r = self.expr1(&recv);
        if name == "len" {
            // `#s` for a string; at level 1 `#` for every
            // value, since the fork honors `__len` on tables.
            if self.opts.level >= 1 || self.opts.hints.is_string_len(node.text_range()) {
                return Expr::Unary(UnOp::Len, Box::new(r));
            }
            if self.opts.hints.is_table_read(node.text_range()) {
                return self.table_len(r);
            }
            return Expr::intrinsic_call("len", vec![r]);
        }
        // A member of a primitive type's library named directly is read,
        // never called with the library as `self`: `String.shout` is the
        // function, as `String.upcase` is.
        if self.res.plain_primitive_member(&recv, &name).is_some() {
            let key = self.opts.hints.member(node.text_range()).map_or(name, |h| h.key().to_string());
            return Expr::index(r, Expr::str(&key));
        }
        // A member the analyzer resolved from the receiver's declared
        // type, or a declared member with a Lua key.
        match self.opts.hints.member(node.text_range()).cloned() {
            Some(MemberHint::Field { key } | MemberHint::Function { key }) => Expr::index(r, Expr::str(&key)),
            Some(MemberHint::Method { key }) => Expr::MethodCall(Box::new(r), key, Vec::new()),
            Some(MemberHint::TableFn { key }) => Expr::call(table_fn(&key), vec![r]),
            None if self.opts.level == 0 && self.opts.hints.is_table_read(node.text_range()) => self.table_field(r, &name),
            None => Expr::AutoGet { receiver: Box::new(r), key: Box::new(Expr::str(&name)) },
        }
    }

    /// The rest parameter (`*args`) named by `node`, when no assignment
    /// replaces it.
    pub(crate) fn rest_param(&self, node: &SyntaxNode) -> Option<nl_sema::BindingId> {
        if node.kind() != NAME_REF {
            return None;
        }
        let Some(Ref::Binding(id)) = self.res.reference(node.text_range()).cloned() else { return None };
        let b = self.res.binding(id)?;
        (b.decl == DeclKind::RestParam && !self.res.reassigned(id)).then_some(id)
    }

    /// The local of a rest parameter named by `node` whose count is a
    /// hidden local (func.rs), and that local.
    pub(crate) fn counted_rest(&mut self, node: &SyntaxNode) -> Option<(LocalId, Expr)> {
        let id = self.rest_param(node)?;
        let c = *self.rest_counts.get(&id)?;
        Some((self.local_of(id), Expr::Local(c)))
    }

    /// `t.len` at level 0 when `t` is certain to be a table
    /// (`Hints::table_reads`): `#t`, unless it has a metatable, which may
    /// have a `__len` that Lua 5.1's `#` does not call on a table; then
    /// `rt.len`. `rt.getmetatable` is the raw one, as in `rt.len`.
    fn table_len(&mut self, r: Expr) -> Expr {
        let t = self.stable(r);
        let mt = Expr::call(self.rt_cached("getmetatable"), vec![t.clone()]);
        let plain = Expr::binary(BinOp::And, Expr::binary(BinOp::Eq, mt, Expr::Nil), Expr::Unary(UnOp::Len, Box::new(t.clone())));
        Expr::binary(BinOp::Or, plain, Expr::intrinsic_call("len", vec![t]))
    }

    /// `t.key` whose value is used, at level 0, when `t` is certain to be
    /// a table and the key is no member its type knows: the key is read
    /// directly, and only a function found there goes on to `rt.mget`,
    /// which calls it when it is a method (a struct instance passed as a
    /// `Table`), as an untyped receiver's member would be:
    ///
    /// ```lua
    /// local v = t.key; if rt.type(v) == "function" then v = rt.mget(t, "key") end
    /// ```
    fn table_field(&mut self, r: Expr, name: &str) -> Expr {
        let t = self.stable(r);
        let v = self.fresh_temp(Expr::index(t.clone(), Expr::str(name)), "v");
        let cond = Expr::binary(BinOp::Eq, Expr::call(self.rt_cached("type"), vec![Expr::Local(v)]), Expr::str("function"));
        let then = self.sub_block(|l| {
            let get = Expr::AutoGet { receiver: Box::new(t), key: Box::new(Expr::str(name)) };
            l.emit(StmtKind::Assign { targets: vec![Place::Local(v)], values: vec![get] });
        });
        self.emit(StmtKind::If { cond, then, otherwise: None });
        Expr::Local(v)
    }

    /// `f!`, `x.m!`, `@m!`: the value itself, never called. A method read
    /// with `x.m!` is bound to `x` (`rt.bound`): `M.f!` of a module passes
    /// the module as `self`, as `M.f(args)` does. A field, a plain
    /// function and a library's function are read as they are.
    fn bang(&mut self, node: &SyntaxNode) -> Expr {
        let Some(inner) = expr_child(node) else { return self.internal(node, "`!`") };
        match inner.kind() {
            NAME_REF => self.name_raw(&inner),
            FIELD_EXPR => {
                let Some(recv) = expr_child(&inner) else { return self.internal(node, "member access") };
                let hint = self.opts.hints.member(inner.text_range()).cloned();
                let name = match &hint {
                    Some(h) => h.key().to_string(),
                    None => field_name(&inner).map(|t| t.text().to_string()).unwrap_or_default(),
                };
                let plain_library = self.res.plain_primitive_member(&recv, &name).is_some();
                let r = self.expr1(&recv);
                match hint {
                    // `xs.map!` on a table is the library's function, as
                    // `s.upcase!` is String's; the receiver is still evaluated.
                    Some(MemberHint::TableFn { key }) => {
                        if !r.is_constant() && !matches!(r, Expr::Local(_)) {
                            self.fresh_temp(r, "s");
                        }
                        table_fn(&key)
                    }
                    Some(MemberHint::Field { .. } | MemberHint::Function { .. }) => self.index_read(r, Expr::str(&name)),
                    // `String.shout!`, `Self.half!` in `fn Number.x`: the
                    // library's function, whose first parameter is `self`.
                    _ if plain_library => self.index_read(r, Expr::str(&name)),
                    // A method the receiver's declared type has (a host
                    // method under its Lua key, unmarked) is bound whatever
                    // the runtime would say of it.
                    Some(MemberHint::Method { .. }) => Expr::intrinsic_call("bound", vec![r, Expr::str(&name), Expr::Bool(true)]),
                    _ => Expr::intrinsic_call("bound", vec![r, Expr::str(&name)]),
                }
            }
            // `@m!` as `self.m!`: a method the analyzer found on the
            // struct is bound; otherwise the runtime decides, which binds a
            // method and leaves a field's value as it is.
            AT_EXPR => {
                let name = token(&inner, AT_NAME).map(|t| t.text().trim_start_matches('@').to_string()).unwrap_or_default();
                let this = self.name_raw(&inner);
                match self.opts.hints.member(inner.text_range()) {
                    Some(MemberHint::Method { .. }) => Expr::intrinsic_call("bound", vec![this, Expr::str(&name), Expr::Bool(true)]),
                    _ => Expr::intrinsic_call("bound", vec![this, Expr::str(&name)]),
                }
            }
            _ => self.expr(&inner),
        }
    }
}

/// How many pieces one concatenation joins at most (see [`concat_all`]).
const CONCAT_GROUP: usize = 32;

/// The concatenation of `parts` (at least one), left to right. A chain of
/// `..` is right-associative in Lua and one `CAT` instruction joins it in
/// linear time, but it needs one register per piece and one parser level
/// per operator; so the pieces are joined in groups of [`CONCAT_GROUP`],
/// and the groups again, which keeps both logarithmic in the number of
/// pieces.
pub(crate) fn concat_all(mut parts: Vec<Expr>) -> Expr {
    while parts.len() > CONCAT_GROUP {
        let mut grouped = Vec::with_capacity(parts.len() / CONCAT_GROUP + 1);
        let mut it = parts.into_iter().peekable();
        while it.peek().is_some() {
            let group: Vec<Expr> = it.by_ref().take(CONCAT_GROUP).collect();
            grouped.push(concat_chain(group));
        }
        parts = grouped;
    }
    concat_chain(parts)
}

/// `a .. (b .. (c .. d))`: printed without parentheses.
fn concat_chain(parts: Vec<Expr>) -> Expr {
    let mut it = parts.into_iter().rev();
    let Some(mut acc) = it.next() else { return Expr::str("") };
    for p in it {
        acc = Expr::binary(BinOp::Concat, p, acc);
    }
    acc
}

/// The function `key` of NL's `Table` library (`rt.Table`), which a
/// [`MemberHint::TableFn`] calls with the receiver first.
pub(crate) fn table_fn(key: &str) -> Expr {
    Expr::index(Expr::Intrinsic("Table"), Expr::str(key))
}

/// True for a `FIELD_EXPR` naming `len`.
pub(crate) fn is_len(node: &SyntaxNode) -> bool {
    node.kind() == FIELD_EXPR && field_name(node).is_some_and(|t| t.text() == "len")
}

/// Truncates a multi-value expression to one value.
pub(crate) fn single(e: Expr) -> Expr {
    if e.is_multi() { Expr::Paren(Box::new(e)) } else { e }
}
