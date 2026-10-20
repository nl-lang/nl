//! Functions: declarations, anonymous functions and
//! blocks; parameters with defaults, annotations and patterns; `*args`;
//! tail values and `return`.

use nl_sema::util::*;
use nl_sema::BindingId;
use nl_syntax::{SyntaxKind, SyntaxNode};

use super::{FnState, Lowerer, Tail};
use crate::ir::*;

use SyntaxKind::*;

/// The work done at function entry for one parameter.
struct ParamEntry {
    local: LocalId,
    default: Option<SyntaxNode>,
    ty: Option<SyntaxNode>,
    /// A destructuring pattern bound from the parameter.
    pattern: Option<SyntaxNode>,
    /// The parameter's name; for a pattern, how messages describe it
    /// (`parameter 1 of describe`).
    name: String,
}

/// How to lower one function node.
pub(crate) struct FnSpec {
    /// The name used in messages (`area`, `Player.damage`).
    pub name: String,
    /// The hidden `self` parameter of a method.
    pub self_binding: Option<BindingId>,
    /// The tail value is discarded (an `on` handler).
    pub discard_tail: bool,
}

impl Lowerer<'_> {
    /// `fn name … end` in statement position. Called by the hoisting at
    /// the start of the block.
    pub(crate) fn lower_fn_decl(&mut self, node: &SyntaxNode) {
        let Some(name) = child(node, NAME) else {
            self.internal(node, "fn without a name");
            return;
        };
        let Some(id) = self.res.declaration(name.text_range()) else {
            self.internal(node, "undeclared fn");
            return;
        };
        let text = name_text(&name).unwrap_or_default();
        let module = self.res.binding(id).and_then(|b| b.module_owner);
        let self_binding = self.res.method_self(node.text_range());
        let spec = FnSpec { name: text.clone(), self_binding, discard_tail: false };
        let mut f = self.function_value(node, spec);
        if module.is_some() {
            // A function in a module body is a method of the module.
            f = Expr::intrinsic_call("method", vec![f]);
        }
        if self.res.binding(id).is_some_and(|b| b.named) {
            // Registered under its file and its path of
            // modules, when it has one unique in the file (spawn.rs).
            if let Some((scope, owner)) = self.registry_scope(node) {
                let full = super::spawn::qualified(&scope, &text);
                f = self.register_named((owner, text.clone()), &full, f);
            }
        }
        let l = self.define(id, Some(f));
        if let Some(m) = module {
            let ml = self.local_of(m);
            self.emit(StmtKind::Assign {
                targets: vec![Place::Index(Expr::Local(ml), Expr::str(&text))],
                values: vec![Expr::Local(l)],
            });
        }
        self.store_global_item(node);
    }

    /// `fn(…) … end` as a value, and a short function `|…| -> …`, which is
    /// the same function: its parameters are a
    /// `BLOCK_PARAMS` and its body a `BLOCK`, as `function` reads them.
    pub(crate) fn fn_expr(&mut self, node: &SyntaxNode) -> Expr {
        let spec = FnSpec { name: "anonymous function".to_string(), self_binding: None, discard_tail: false };
        self.function_value(node, spec)
    }

    /// A trailing block `do |…| … end`.
    pub(crate) fn block_fn(&mut self, node: &SyntaxNode) -> Expr {
        let spec = FnSpec { name: "block".to_string(), self_binding: None, discard_tail: false };
        self.function_value(node, spec)
    }

    /// A function node as a value: the function, made async when the node
    /// says `async`.
    pub(crate) fn function_value(&mut self, node: &SyntaxNode, spec: FnSpec) -> Expr {
        let f = Expr::Function(Box::new(self.function(node, spec)));
        if has_token(node, ASYNC_KW) { self.wrap_async(node, f) } else { f }
    }

    /// Lowers a function-like node (`FN_DECL`, `FN_EXPR`, `LAMBDA_EXPR`,
    /// `TRAILING_BLOCK`, `ON_EXPR`, a `spawn` block) into a function.
    pub(crate) fn function(&mut self, node: &SyntaxNode, spec: FnSpec) -> Function {
        let line = self.line_of(node);
        let saved_stmts = std::mem::take(&mut self.stmts);
        let saved_line = self.line;
        self.line = line;
        let ret_type = child(node, RETURN_TYPE).and_then(|r| type_child(&r));
        let nil_return = ret_type.as_ref().is_some_and(|t| t.kind() == NAMED_TYPE && has_token(t, NIL_KW));
        // `: nil` is checked too: `return 5` there is a TypeError;
        // its tail value is discarded, not returned.
        let ret_check = match &ret_type {
            Some(t) if self.opts.checks => Some((t.clone(), format!("return value of {}", spec.name))),
            _ => None,
        };
        let check_fall_off = ret_check.is_some() && !nil_return;
        self.fns.push(FnState { ret_check, loops: Vec::new(), regions: Vec::new(), name: spec.name.clone() });

        let mut params = Vec::new();
        if let Some(sb) = spec.self_binding {
            params.push(self.local_of(sb));
        }
        let mut vararg = false;
        // Work done at entry, in parameter order: defaults, checks,
        // destructuring.
        let mut entry: Vec<ParamEntry> = Vec::new();
        let mut rest: Option<(LocalId, Option<SyntaxNode>, String)> = None;
        let mut rest_id: Option<BindingId> = None;
        let mut keyed: Option<(LocalId, Option<SyntaxNode>, String)> = None;
        if let Some(list) = child(node, PARAM_LIST).or_else(|| child(node, BLOCK_PARAMS)) {
            for p in list.children() {
                match p.kind() {
                    PARAM => {
                        let Some(pat) = pattern_children(&p).next() else { continue };
                        let default = expr_child(&p);
                        match simple_binding(&pat) {
                            Some((name_node, ty)) => {
                                let Some(id) = self.res.declaration(name_node.text_range()) else { continue };
                                let l = self.local_of(id);
                                self.mark_predeclared(id);
                                params.push(l);
                                let pname = name_text(&name_node).unwrap_or_default();
                                entry.push(ParamEntry { local: l, default, ty, pattern: None, name: pname });
                            }
                            None => {
                                let t = self.new_local("arg");
                                params.push(t);
                                let what = format!("parameter {} of {}", entry.len() + 1, spec.name);
                                entry.push(ParamEntry { local: t, default, ty: None, pattern: Some(pat), name: what });
                            }
                        }
                    }
                    REST_PARAM => {
                        vararg = true;
                        if let Some(name) = child(&p, NAME) {
                            if let Some(id) = self.res.declaration(name.text_range()) {
                                let l = self.local_of(id);
                                rest = Some((l, type_child(&p), name_text(&name).unwrap_or_default()));
                                rest_id = Some(id);
                            }
                        }
                    }
                    KEYED_PARAM => {
                        if let Some(name) = child(&p, NAME) {
                            if let Some(id) = self.res.declaration(name.text_range()) {
                                let l = self.local_of(id);
                                keyed = Some((l, type_child(&p), name_text(&name).unwrap_or_default()));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        // The rest table, then the keyed arguments: the
        // caller passes them as one marked table after the positional
        // arguments, so they are the last item of the rest table, or the
        // last parameter that received a value. Found before defaults and
        // checks run, since the slot they landed in is not theirs.
        // The rest table carries its count, `{n = select("#", ...), ...}`,
        // so that a nil among the arguments is kept by `f(*args)`,
        // `args.len` and a spread anywhere the table goes (rt.spread). When
        // the table goes nowhere (the function only reads its items, its
        // length and spreads it), the count is a hidden local instead,
        // which spares the table its hash part, as costly as the table.
        if let Some((l, _, _)) = &rest {
            let select = self.rt_cached("select");
            let n = Expr::call(select, vec![Expr::str("#"), Expr::Vararg]);
            let local_count = keyed.is_none() && rest_id.is_some_and(|id| !self.res.reassigned(id) && rest_stays_local(node, self.res, id));
            match rest_id.filter(|_| local_count) {
                Some(id) => {
                    let c = self.new_local("n");
                    self.emit(StmtKind::Local { locals: vec![*l, c], values: vec![Expr::Table(vec![TableEntry::Item(Expr::Vararg)]), n] });
                    self.rest_counts.insert(id, c);
                }
                None => self.emit(StmtKind::Local { locals: vec![*l], values: vec![Expr::Table(vec![TableEntry::Pair(Expr::str("n"), n), TableEntry::Item(Expr::Vararg)])] }),
            }
        }
        let written: Vec<LocalId> = entry.iter().map(|e| e.local).collect();
        match &keyed {
            Some((opts, _, _)) => {
                let opts = *opts;
                // Without `*rest` the function still takes the arguments
                // beyond its parameters, since the keyed table may be there.
                let first = match &rest {
                    Some((r, _, _)) => Expr::intrinsic_call("kwpop", vec![Expr::Local(*r)]),
                    None => {
                        vararg = true;
                        Expr::intrinsic_call("kwlast", vec![Expr::Vararg])
                    }
                };
                self.emit(StmtKind::Local { locals: vec![opts], values: vec![first] });
                // From the last parameter back to the first: `if opts == nil
                // and rt.iskw(p) then opts = p; p = nil end`.
                for p in written.iter().rev() {
                    let cond = Expr::binary(
                        BinOp::And,
                        Expr::binary(BinOp::Eq, Expr::Local(opts), Expr::Nil),
                        Expr::intrinsic_call("iskw", vec![Expr::Local(*p)]),
                    );
                    let then = self.sub_block(|s| {
                        s.emit(StmtKind::Assign { targets: vec![Place::Local(opts)], values: vec![Expr::Local(*p)] });
                        s.emit(StmtKind::Assign { targets: vec![Place::Local(*p)], values: vec![Expr::Nil] });
                    });
                    self.emit(StmtKind::If { cond, then, otherwise: None });
                }
                self.emit(StmtKind::Assign { targets: vec![Place::Local(opts)], values: vec![Expr::intrinsic_call("kwtake", vec![Expr::Local(opts)])] });
            }
            None if self.opts.checks => {
                let rest_local = rest.as_ref().map(|(r, _, _)| *r);
                self.refuse_keyed(&spec.name, &mut entry, rest_local);
            }
            None => {}
        }
        for ParamEntry { local: l, default, ty, pattern: pat, name: pname } in entry {
            if let Some(d) = default {
                let body = self.sub_block(|s| {
                    let v = s.expr1(&d);
                    s.emit(StmtKind::Assign { targets: vec![Place::Local(l)], values: vec![v] });
                });
                self.emit(StmtKind::If { cond: Expr::binary(BinOp::Eq, Expr::Local(l), Expr::Nil), then: body, otherwise: None });
            }
            if let Some(t) = ty {
                let ctx = format!("parameter '{pname}' of {}", spec.name);
                self.emit_check(Expr::Local(l), &t, &ctx);
            }
            if let Some(pat) = pat {
                self.bind_must(&pat, Expr::Local(l), &pname);
            }
        }
        for (l, ty, rname) in rest.into_iter().chain(keyed) {
            if let Some(t) = ty {
                // `*args: T` and `**opts: T` type the table that holds the
                // arguments, as TypeScript's `...args: T[]` does,
                // so `[Number]` says what the items are and `any` opts out.
                let ctx = format!("parameter '{rname}' of {}", spec.name);
                self.emit_check(Expr::Local(l), &t, &ctx);
            }
        }

        let tail = if nil_return || spec.discard_tail { Tail::Discard } else { Tail::Return };
        if let Some(b) = block(node) {
            self.lower_block_inline(&b, tail);
        }
        // A function that ends without returning returns nothing, which the
        // declared type is asked about like any other result.
        if check_fall_off && can_fall_off(&self.stmts) {
            self.emit_return(Vec::new());
        }
        self.fns.pop();
        let body = Block::new(std::mem::replace(&mut self.stmts, saved_stmts));
        self.line = saved_line;
        Function { params, vararg, body, line, name: Some(spec.name), method: spec.self_binding.is_some(), transient: false }
    }

    /// The entry of a function without `**opts` in a checked build: it
    /// refuses keyed arguments that reach a parameter or its rest
    /// table, before any default or check runs.
    ///
    /// A call passes its keyed arguments as one marked table after the
    /// positional ones (runtime/nl_40_ops.lua, `KW`), so a keyed table
    /// that lands in a parameter is in the last one that is not `nil`:
    /// that is the only parameter tested (`rt.getmetatable(p) == rt.KW`),
    /// found by comparing the later ones with `nil`. A call with every
    /// argument costs one `getmetatable` whatever the number of
    /// parameters, and a call that leaves the last ones out one more
    /// comparison per missing argument. A marked table passed in the
    /// middle of positional arguments (`f(keyed(t), 1)`, which no call
    /// with `k: v` makes) is a positional table here.
    ///
    /// `rt.nokw(name, params…)` runs only when a test inline says it may
    /// raise: the last parameter that is not `nil` holds a marked table,
    /// or a parameter of the leading run fails its inline check. The run
    /// is the parameters from the first that have no default and no
    /// pattern, and either no runtime check or one with an inline test
    /// ([`super::types::Inline`]); their checks are made in the same
    /// branch, after `rt.nokw`, which is where they came before, and not
    /// again afterwards. A parameter of the run whose test rejects a
    /// marked table (`rt.type(x) ~= "number"`) is not tested for the mark.
    /// So `fn f(a, b)` is
    ///
    /// ```lua
    /// if b ~= nil and rt.getmetatable(b) == rt.KW or b == nil and rt.getmetatable(a) == rt.KW then
    ///   rt.nokw("f", a, b)
    /// end
    /// ```
    ///
    /// and `fn f(x: Number, t: Table)`, where `x` holds no marked table
    /// once its test passes, and a `nil` in it fails the test, is
    ///
    /// ```lua
    /// if rt.type(x) ~= "number" or rt.type(t) ~= "table" or rt.getmetatable(t) == rt.KW then
    ///   rt.nokw("f", x, t); rt.check(x, rt.T.Number, …); rt.check(t, rt.T.Table, …)
    /// end
    /// ```
    ///
    /// With a rest table the run is empty and `rt.nokwrest` follows.
    fn refuse_keyed(&mut self, name: &str, entry: &mut [ParamEntry], rest: Option<LocalId>) {
        let (getmetatable, kw) = if entry.is_empty() { (Expr::Nil, Expr::Nil) } else { (self.rt_cached("getmetatable"), self.rt_cached("KW")) };
        let marked = |l: LocalId| Expr::binary(BinOp::Eq, Expr::call(getmetatable.clone(), vec![Expr::Local(l)]), kw.clone());
        let mut run = 0;
        let mut conds = Vec::new();
        let mut checks = Vec::new();
        // The parameters whose inline test rejects a marked table, and
        // whether it also rejects `nil`.
        let mut rejects_mark: Vec<Option<bool>> = vec![None; entry.len()];
        if rest.is_none() {
            for (i, e) in entry.iter().enumerate() {
                if e.default.is_some() || e.pattern.is_some() {
                    break;
                }
                let v = Expr::Local(e.local);
                let typed = match &e.ty {
                    Some(ty) => self.type_value(ty).map(|t| (ty.clone(), t)),
                    None => None,
                };
                if let Some((ty, t)) = typed {
                    let Some(test) = self.inline_test(&ty) else { break };
                    conds.push(self.inline_fails(&test, &v));
                    if let Some(r) = rejects_mark.get_mut(i) {
                        *r = test.excludes_keyed().then(|| test.excludes_nil());
                    }
                    let what = format!("parameter '{}' of {name}", e.name);
                    checks.push(Expr::intrinsic_call("check", vec![v, t, Expr::str(&what)]));
                }
                run += 1;
            }
        }
        // The mark of the last parameter that is not `nil`, from the first
        // parameter on: `p ~= nil and marked(p) or p == nil and <before>`.
        // A parameter whose test rejects the mark adds only `p == nil and
        // <before>`, and nothing when the test rejects `nil` too: either
        // it holds a value, which is not marked, or its test fails.
        let mut last: Option<Expr> = None;
        for (e, rejects) in entry.iter().zip(&rejects_mark) {
            let p = Expr::Local(e.local);
            last = match (last, *rejects) {
                (_, Some(true)) => None,
                (before, Some(false)) => before.map(|b| Expr::binary(BinOp::And, Expr::binary(BinOp::Eq, p, Expr::Nil), b)),
                (None, None) => Some(marked(e.local)),
                (Some(b), None) => Some(Expr::binary(
                    BinOp::Or,
                    Expr::binary(BinOp::And, Expr::binary(BinOp::Ne, p.clone(), Expr::Nil), marked(e.local)),
                    Expr::binary(BinOp::And, Expr::binary(BinOp::Eq, p, Expr::Nil), b),
                )),
            };
        }
        conds.extend(last);
        let mut conds = conds.into_iter();
        if let Some(first) = conds.next() {
            let cond = conds.fold(first, |acc, c| Expr::binary(BinOp::Or, acc, c));
            let mut args = vec![Expr::str(name)];
            args.extend(entry.iter().map(|e| Expr::Local(e.local)));
            let line = self.line;
            let mut then = vec![Stmt { line, kind: StmtKind::Expr(Expr::intrinsic_call("nokw", args)) }];
            then.extend(checks.into_iter().map(|c| Stmt { line, kind: StmtKind::Expr(c) }));
            self.emit(StmtKind::If { cond, then: Block::new(then), otherwise: None });
        }
        for e in entry.iter_mut().take(run) {
            e.ty = None;
        }
        if let Some(r) = rest {
            let mut args = vec![Expr::str(name), Expr::Local(r)];
            if let Some(c) = self.rest_counts.iter().find(|(id, _)| self.bindings.get(id) == Some(&r)).map(|(_, c)| *c) {
                args.push(Expr::Local(c));
            }
            self.emit(StmtKind::Expr(Expr::intrinsic_call("nokwrest", args)));
        }
    }

    /// `return values`.
    pub(crate) fn lower_return(&mut self, node: &SyntaxNode) {
        let values: Vec<SyntaxNode> = child(node, EXPR_LIST).map(|l| expr_children(&l).collect()).unwrap_or_default();
        let vals = self.operands(&values);
        let n = vals.len();
        let vals = vals.into_iter().enumerate().map(|(i, v)| if i + 1 < n { super::expr::single(v) } else { v }).collect();
        self.lower_return_values(node, vals);
    }

    /// Returns lowered values from the current function, or, inside a
    /// protected region, through its status protocol.
    pub(crate) fn lower_return_values(&mut self, node: &SyntaxNode, vals: Vec<Expr>) {
        if self.in_protected_region() {
            self.return_from_region(node, vals);
            return;
        }
        self.emit_return_at(vals, Some(node.text_range()));
    }
}

/// True unless the statements certainly end by returning (conservative).
pub(crate) fn can_fall_off(stmts: &[Stmt]) -> bool {
    match stmts.last().map(|s| &s.kind) {
        Some(StmtKind::Return(_)) => false,
        Some(StmtKind::Do(b)) => can_fall_off(&b.stmts),
        Some(StmtKind::Hoist { rest, .. }) => can_fall_off(&rest.stmts),
        Some(StmtKind::If { then, otherwise: Some(o), .. }) => can_fall_off(&then.stmts) || can_fall_off(&o.stmts),
        _ => true,
    }
}

/// True when every use of the rest parameter `id` in the function `node`
/// reads its items (`args[i]`, `for x in args`), its length (`args.len`)
/// or spreads it (`f(*args)`, `[*args]`): the table goes nowhere that
/// needs its count in `n`.
fn rest_stays_local(node: &SyntaxNode, res: &nl_sema::Resolution, id: BindingId) -> bool {
    node.descendants().filter(|n| n.kind() == NAME_REF && res.reference(n.text_range()) == Some(&nl_sema::Ref::Binding(id))).all(|r| {
        let Some(p) = r.parent() else { return false };
        let first = expr_child(&p).is_some_and(|e| e == r);
        match p.kind() {
            FIELD_EXPR => first && field_name(&p).is_some_and(|t| t.text() == "len"),
            INDEX_EXPR => first,
            // A spread in a call; in a list it goes through rt.listcat.
            SPREAD_ARG => p.parent().is_some_and(|a| matches!(a.kind(), ARG_LIST | COMMAND_ARGS)),
            FOR_STMT => true,
            _ => false,
        }
    })
}

/// A parameter or `let` pattern that is a plain name, possibly annotated:
/// the `NAME` node and the type.
pub(crate) fn simple_binding(pat: &SyntaxNode) -> Option<(SyntaxNode, Option<SyntaxNode>)> {
    match pat.kind() {
        BIND_PAT => child(pat, NAME).map(|n| (n, None)),
        TYPED_PAT => {
            let inner = pattern_children(pat).next()?;
            if inner.kind() != BIND_PAT {
                return None;
            }
            child(&inner, NAME).map(|n| (n, type_child(pat)))
        }
        _ => None,
    }
}
