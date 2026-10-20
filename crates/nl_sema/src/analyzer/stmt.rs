//! Statements and blocks: declarations, assignment,
//! functions and returns, control flow and loops, `try`,
//! with the flow-sensitive narrowing of `is` and `nil` tests.

use std::collections::HashSet;
use std::rc::Rc;

use nl_syntax::{SyntaxKind, SyntaxNode, TextRange};

use super::{Analyzer, Env, FnCtx, Ty};
use crate::binding::*;
use crate::hints::*;
use crate::types::*;
use crate::util::*;

use SyntaxKind::*;

impl Analyzer<'_> {
    // ---- blocks ------------------------------------------------------------------

    /// Analyzes a block; returns the type of its value: the tail
    /// expression's when `tail_used`, else `nil` (a block whose last
    /// statement is not an expression yields nothing).
    pub(crate) fn block(&mut self, block: &SyntaxNode, tail_used: bool) -> Ty {
        let stmts: Vec<SyntaxNode> = statements(block).collect();
        let n = stmts.len();
        let mut value = Ty::certain(Type::Nil);
        for (i, s) in stmts.iter().enumerate() {
            let last = i + 1 == n;
            let v = self.stmt(s, last && tail_used);
            if last {
                value = v;
            }
        }
        if self.diverges {
            return Ty::certain(Type::Never);
        }
        value
    }

    /// A block in a scope of its own along the current path: the flow
    /// types it sets for bindings declared outside it stay, those of its
    /// own bindings do not matter after it.
    pub(crate) fn scoped_block(&mut self, block: &SyntaxNode, tail_used: bool) -> Ty {
        self.block(block, tail_used)
    }

    /// One statement; `value_used`: it is the tail of a block whose value
    /// is used.
    fn stmt(&mut self, s: &SyntaxNode, value_used: bool) -> Ty {
        if let Some(m) = modifier(s) {
            return self.modified_stmt(s, &m, value_used);
        }
        self.stmt_inner(s, value_used)
    }

    /// `stmt if cond`, `stmt while cond`. A tail `x if cond` has the value
    /// of `x`, or `nil` when the condition fails, as `if cond x end`.
    fn modified_stmt(&mut self, s: &SyntaxNode, m: &SyntaxNode, value_used: bool) -> Ty {
        let kw = first_token(m).map(|t| t.kind());
        let negated = matches!(kw, Some(UNLESS_KW | UNTIL_KW));
        let looping = matches!(kw, Some(WHILE_KW | UNTIL_KW));
        let Some(cond) = expr_child(m) else {
            return self.stmt_inner(s, false);
        };
        if looping {
            self.widen_assigned_in(s);
        }
        self.expr1(&cond);
        let (then_env, else_env) = self.condition_envs(&cond);
        let (run_env, skip_env) = if negated { (else_env, then_env) } else { (then_env, else_env) };
        let saved = std::mem::replace(&mut self.env, run_env);
        let was = self.diverges;
        self.diverges = false;
        if looping {
            self.current_fn().loop_depth += 1;
        }
        let v = self.stmt_inner(s, value_used && !looping);
        if looping {
            self.current_fn().loop_depth -= 1;
        }
        let after = std::mem::replace(&mut self.env, saved);
        let diverged = self.diverges;
        self.diverges = was;
        self.env = if diverged { skip_env } else { self.merge_envs(&[after, skip_env]) };
        if value_used && !looping && !diverged {
            return Ty::new(optional(&self.types, &v.t), v.certain);
        }
        Ty::certain(Type::Nil)
    }

    fn stmt_inner(&mut self, s: &SyntaxNode, value_used: bool) -> Ty {
        match s.kind() {
            EXPR_STMT => self.expr_stmt(s, value_used),
            LET_DECL => {
                self.let_decl(s, DeclKind::Let);
                Ty::certain(Type::Nil)
            }
            CONST_DECL => {
                self.let_decl(s, DeclKind::Const);
                Ty::certain(Type::Nil)
            }
            GLOBAL_DECL => {
                self.global_decl(s);
                Ty::certain(Type::Nil)
            }
            FN_DECL => {
                self.fn_decl(s);
                Ty::certain(Type::Nil)
            }
            STRUCT_DECL => {
                self.struct_decl(s);
                Ty::certain(Type::Nil)
            }
            INTERFACE_IMPL => {
                self.interface_impl(s);
                Ty::certain(Type::Nil)
            }
            ENUM_DECL | INTERFACE_DECL | TYPE_DECL | IMPORT_DECL => Ty::certain(Type::Nil),
            MODULE_DECL => {
                if let Some(b) = block(s) {
                    self.scoped_block(&b, false);
                }
                Ty::certain(Type::Nil)
            }
            ASSIGN_STMT | COMPOUND_ASSIGN_STMT | MULTI_ASSIGN_STMT => {
                self.assign(s);
                Ty::certain(Type::Nil)
            }
            RETURN_STMT => {
                self.return_stmt(s);
                Ty::certain(Type::Never)
            }
            BREAK_STMT | CONTINUE_STMT => {
                self.diverges = true;
                Ty::certain(Type::Never)
            }
            DO_BLOCK => match block(s) {
                Some(b) => self.scoped_block(&b, value_used),
                None => Ty::certain(Type::Nil),
            },
            WHILE_STMT | UNTIL_STMT | LOOP_STMT => {
                self.while_stmt(s);
                Ty::certain(Type::Nil)
            }
            FOR_STMT => {
                self.for_stmt(s);
                Ty::certain(Type::Nil)
            }
            _ => Ty::certain(Type::Nil),
        }
    }

    /// An expression statement: its value is used (the tail) or discarded
    /// (a name or member is called).
    fn expr_stmt(&mut self, s: &SyntaxNode, value_used: bool) -> Ty {
        if let Some(list) = child(s, EXPR_LIST) {
            // `a, b` as the tail of a function: several return values.
            let types: Vec<Type> = expr_children(&list).map(|e| self.expr1(&e).t).collect();
            return Ty::uncertain(Type::Tuple(types.into()));
        }
        let Some(e) = expr_child(s) else { return Ty::certain(Type::Nil) };
        let discarded = self.res.value_discarded(s.text_range()) || !value_used;
        if discarded {
            let t = self.discard_expr(&e);
            if t.t == Type::Never {
                self.diverges = true;
            }
            return Ty::certain(Type::Nil);
        }
        let t = self.expr(&e);
        if self.types.first(&t.t) == Type::Never {
            self.diverges = true;
        }
        t
    }

    /// An expression whose value is discarded: a bare name or
    /// member is called.
    fn discard_expr(&mut self, e: &SyntaxNode) -> Ty {
        match e.kind() {
            NAME_REF => {
                let f = self.name_raw(e);
                let name = name_text(e).unwrap_or_default();
                match self.types.expand(&f.t) {
                    Type::Fn(sig) => {
                        self.check_call(&sig, &name, &[], None, e.text_range(), true);
                        self.call_of(&sig, self.checked_fn_name(e))
                    }
                    _ => Ty::any(),
                }
            }
            FIELD_EXPR if field_name(e).is_some_and(|t| t.text() != "len") => self.call_target(e, Vec::new(), None, None),
            PAREN_EXPR => match expr_child(e) {
                Some(inner) => self.discard_expr(&inner),
                None => Ty::any(),
            },
            IF_EXPR | UNLESS_EXPR | CASE_EXPR | COND_EXPR | TRY_EXPR => {
                self.construct(e, false);
                Ty::certain(Type::Nil)
            }
            AT_EXPR => {
                let f = self.at_expr_value(e);
                let _ = f;
                Ty::any()
            }
            _ => self.expr(e),
        }
    }

    fn at_expr_value(&mut self, e: &SyntaxNode) -> Ty {
        self.expr(e)
    }

    // ---- declarations -----------------------------------------------------------------

    /// `let`/`const patterns = values`.
    fn let_decl(&mut self, node: &SyntaxNode, decl: DeclKind) {
        let values: Vec<SyntaxNode> = child(node, EXPR_LIST).map(|l| expr_children(&l).collect()).unwrap_or_default();
        let pats: Vec<SyntaxNode> = pattern_children(node).collect();
        let has_values = child(node, EXPR_LIST).is_some();
        let value_types = self.value_list(&values, pats.len());
        if has_values && pats.len() > 1 {
            let last_multi = values.last().is_some_and(|v| matches!(v.kind(), CALL_EXPR | COMMAND_CALL | METHOD_CALL | PIPE_EXPR | LUA_PATH | SPAWN_EXPR | PAREN_EXPR | FIELD_EXPR | NAME_REF | AT_EXPR | BANG_EXPR | IF_EXPR | CASE_EXPR | COND_EXPR | TRY_EXPR | UNLESS_EXPR));
            if values.len() > pats.len() || (values.len() < pats.len() && !last_multi) {
                let what = if decl == DeclKind::Let { "let" } else { "const" };
                self.warn(node.text_range(), "value-count", format!("this `{what}` declares {} names but is given {} values; the missing ones are `nil`", pats.len(), values.len()));
            }
        }
        for (i, pat) in pats.iter().enumerate() {
            let value = if has_values { value_types.get(i).cloned().unwrap_or_else(|| Ty::certain(Type::Nil)) } else { Ty::uncertain(Type::Nil) };
            self.bind_let(pat, &value, has_values);
        }
    }

    /// The types of the values of a declaration or assignment, spread to
    /// `wanted` positions: every value but the last gives one, the last
    /// may give several.
    pub(crate) fn value_list(&mut self, values: &[SyntaxNode], wanted: usize) -> Vec<Ty> {
        let mut out = Vec::new();
        let n = values.len();
        for (i, v) in values.iter().enumerate() {
            let t = self.expr(v);
            if i + 1 == n {
                let count = wanted.saturating_sub(i).max(1);
                for k in 0..count {
                    let nt = self.types.nth(&t.t, k);
                    let certain = t.certain && (k == 0 || matches!(t.t, Type::Tuple(_)));
                    out.push(Ty::new(nt, certain));
                }
            } else {
                out.push(Ty::new(self.types.first(&t.t), t.certain));
            }
        }
        out
    }

    /// Declares the names of a `let` pattern with the value's type.
    fn bind_let(&mut self, pat: &SyntaxNode, value: &Ty, has_value: bool) {
        match pat.kind() {
            BIND_PAT => {
                let Some(id) = child(pat, NAME).and_then(|n| self.res.declaration(n.text_range())) else { return };
                if self.binding_sigs.contains_key(&id) {
                    // `let f = fn … end`: function-kind, typed `Function`.
                    let sig = self.binding_fn_type(id).unwrap_or_else(|| Rc::new(FnType::unknown()));
                    self.declare_type(id, Ty::certain(Type::Fn(sig)));
                    return;
                }
                if self.assigned.contains(&id) || !has_value {
                    self.declare_type(id, Ty::any());
                    if has_value {
                        self.set_flow(id, value.t.clone());
                    } else {
                        self.set_flow(id, Type::Nil);
                    }
                } else {
                    self.declare_type(id, value.clone());
                }
            }
            TYPED_PAT => {
                let ty = type_child(pat).map(|t| self.type_of(&t)).unwrap_or(Type::Any);
                let inner = pattern_children(pat).next();
                match inner {
                    Some(i) if i.kind() == BIND_PAT => {
                        let Some(id) = child(&i, NAME).and_then(|n| self.res.declaration(n.text_range())) else { return };
                        let name = self.binding(id).map(|b| b.name.clone()).unwrap_or_default();
                        if self.binding_sigs.contains_key(&id) {
                            let sig = self.binding_fn_type(id).unwrap_or_else(|| Rc::new(FnType::unknown()));
                            self.declare_type(id, Ty::certain(Type::Fn(sig)));
                        } else {
                            self.declare_type(id, Ty::certain(ty.clone()));
                        }
                        if has_value {
                            let proven = self.check_assignable(value, &ty, pat.text_range(), &format!("variable '{name}'"));
                            if proven && self.opts.static_resolution {
                                if let Some(t) = type_child(pat) {
                                    self.hints.proven.insert(t.text_range());
                                }
                            }
                            // Along the flow the variable holds the value,
                            // unless the annotation opts out of checking.
                            if self.flow_narrows(&ty) && self.types.assignable(&value.t, &ty) && value.t != Type::Any {
                                self.set_flow(id, value.t.clone());
                            }
                        }
                    }
                    Some(i) => {
                        let narrowed = self.types.narrow(&value.t, &ty);
                        self.bind_pattern(&i, &Ty::certain(narrowed), true);
                    }
                    None => {}
                }
            }
            _ => {
                self.bind_pattern(pat, value, true);
            }
        }
    }

    fn global_decl(&mut self, node: &SyntaxNode) {
        let value = expr_child(node).map(|e| self.expr1(&e));
        let Some(id) = child(node, NAME).and_then(|n| self.res.declaration(n.text_range())) else { return };
        match type_child(node) {
            Some(t) => {
                let ty = self.type_of(&t);
                if let Some(v) = &value {
                    let name = self.binding(id).map(|b| b.name.clone()).unwrap_or_default();
                    self.check_assignable(v, &ty, node.text_range(), &format!("global '{name}'"));
                }
                // Uncertain: a Lua script can assign the global anything,
                // so its declared type is used for errors, never for
                // static resolution.
                self.declare_type(id, Ty::uncertain(ty));
            }
            None => self.declare_type(id, Ty::any()),
        }
    }

    /// `fn name … end` and `fn Owner.name … end`.
    fn fn_decl(&mut self, node: &SyntaxNode) {
        let sig = match self.fn_sigs.get(&node.text_range()) {
            Some(s) => s.clone(),
            None => {
                let s = Rc::new(self.signature_of(node, false, false, None));
                self.fn_sigs.insert(node.text_range(), s.clone());
                s
            }
        };
        let self_ty = match child(node, QUAL_NAME) {
            Some(owner) => self.method_owner_type(node, &owner),
            None => {
                let id = child(node, NAME).and_then(|n| self.res.declaration(n.text_range()));
                id.and_then(|id| self.module_of_binding.get(&id).copied()).map(Type::Static)
            }
        };
        self.function_body(node, &sig, self_ty, false, false);
    }

    fn struct_decl(&mut self, node: &SyntaxNode) {
        let nid = child(node, NAME).and_then(|n| self.res.declaration(n.text_range())).and_then(|id| self.named_of.get(&id).copied());
        for f in children(node, FIELD_DECL) {
            let Some(e) = expr_child(&f) else { continue };
            let v = self.expr1(&e);
            let name = child(&f, NAME).and_then(|n| name_text(&n)).unwrap_or_default();
            if let Some(nid) = nid {
                if let Some(field) = self.types.field(nid, &name) {
                    let owner = self.types.name(nid).to_string();
                    self.check_assignable(&v, &field.ty, e.text_range(), &format!("the default of field '{name}' of {owner}"));
                }
            }
        }
    }

    fn interface_impl(&mut self, node: &SyntaxNode) {
        let types: Vec<SyntaxNode> = node.children().filter(|c| is_type(c.kind())).collect();
        let target = types.get(1).map(|t| self.type_of(t));
        for f in children(node, FN_DECL) {
            let sig = match self.fn_sigs.get(&f.text_range()) {
                Some(s) => s.clone(),
                None => Rc::new(FnType::unknown()),
            };
            self.function_body(&f, &sig, target.clone(), false, false);
        }
    }

    // ---- functions -------------------------------------------------------------------

    /// The body of a function-like node: parameters, then the block; the
    /// return types are collected and the inferred one recorded.
    pub(crate) fn function_body(&mut self, node: &SyntaxNode, sig: &Rc<FnType>, self_ty: Option<Type>, spawn_block: bool, handler: bool) {
        let declared_ret = child(node, RETURN_TYPE).and_then(|r| type_child(&r)).map(|t| self.type_of_with_self(&t, self_ty.clone()));
        let nil_ret = declared_ret.as_ref().is_some_and(|t| *t == Type::Nil);
        let method_self = self.res.method_self(node.text_range());
        let own_self = if method_self.is_some() { self_ty.clone() } else { None };
        self.fns.push(FnCtx { node: Some(node.text_range()), declared_ret: declared_ret.clone(), nil_ret, returns: Vec::new(), self_type: own_self, spawn_block, loop_depth: 0 });
        let saved_env = self.env.clone();
        let saved_div = self.diverges;
        self.diverges = false;
        if let (Some(sb), Some(st)) = (method_self, self_ty.clone()) {
            self.declare_type(sb, Ty::certain(st));
        }
        // Parameters.
        let list = child(node, PARAM_LIST).or_else(|| child(node, BLOCK_PARAMS));
        if let Some(list) = &list {
            let mut i = 0;
            for p in list.children() {
                match p.kind() {
                    PARAM => {
                        let param = sig.params.get(i).cloned();
                        i += 1;
                        let default = expr_child(&p).map(|d| self.expr1(&d));
                        let Some(pat) = pattern_children(&p).next() else { continue };
                        let declared = param.map(|p| p.ty).unwrap_or(Type::Any);
                        if let Some(d) = &default {
                            if declared != Type::Any {
                                self.check_assignable(d, &declared, p.text_range(), "the default of this parameter");
                            }
                        }
                        let value = if declared == Type::Any {
                            match &default {
                                Some(d) if !self.assigned_anywhere(&pat) => Ty::uncertain(self.types.union(&d.t, &Type::Any)),
                                _ => Ty::any(),
                            }
                        } else {
                            Ty::certain(declared)
                        };
                        self.bind_param(&pat, &value);
                    }
                    REST_PARAM => {
                        if let Some(id) = child(&p, NAME).and_then(|n| self.res.declaration(n.text_range())) {
                            let t = match &sig.rest {
                                Some(t) if *t != Type::Any => Ty::certain(t.clone()),
                                _ => Ty::certain(Type::Array(Rc::new(Type::Any))),
                            };
                            self.declare_type(id, t);
                        }
                    }
                    KEYED_PARAM => {
                        if let Some(id) = child(&p, NAME).and_then(|n| self.res.declaration(n.text_range())) {
                            let t = match &sig.keyed {
                                Some(Some(t)) if *t != Type::Any => Ty::certain(t.clone()),
                                _ => Ty::certain(Type::Table),
                            };
                            self.declare_type(id, t);
                            self.keyed_params.insert(id);
                        }
                    }
                    _ => {}
                }
            }
        }
        // Body.
        let tail_used = !nil_ret && !handler;
        let tail = match block(node) {
            Some(b) => self.block(&b, tail_used),
            None => Ty::certain(Type::Nil),
        };
        let diverged = self.diverges;
        if !diverged && tail_used {
            let t = tail.clone();
            let last = block(node).and_then(|b| statements(&b).last());
            let site = last.as_ref().map(|s| s.text_range());
            let single = last.as_ref().and_then(|s| if s.kind() == EXPR_STMT { expr_child(s) } else { None }).is_some_and(|e| self.single_valued(&e));
            self.record_return(&t, site, true, single);
        } else if !diverged && !tail_used && declared_ret.as_ref().is_some_and(|t| *t != Type::Nil) {
            // A `: nil` function returns nothing; a handler's tail is
            // discarded.
        }
        let ctx = self.fns.pop();
        self.env = saved_env;
        self.diverges = saved_div;
        if let Some(ctx) = ctx {
            let ret = match &ctx.declared_ret {
                Some(t) => t.clone(),
                None => {
                    // No value returned: the body falls off its end
                    // (`nil`), or never ends normally (`never`: it always
                    // raises).
                    let mut types = ctx.returns.clone();
                    if types.is_empty() {
                        types.push(if diverged { Type::Never } else { Type::Nil });
                    }
                    self.types.union_all(&types)
                }
            };
            self.inferred_ret.insert(node.text_range(), ret);
        }
    }

    /// True when a parameter's name is assigned in the body.
    fn assigned_anywhere(&self, pat: &SyntaxNode) -> bool {
        match pat.kind() {
            BIND_PAT => child(pat, NAME).and_then(|n| self.res.declaration(n.text_range())).is_some_and(|id| self.assigned.contains(&id)),
            _ => true,
        }
    }

    /// Declares a parameter pattern with the parameter's declared type.
    fn bind_param(&mut self, pat: &SyntaxNode, value: &Ty) {
        match pat.kind() {
            BIND_PAT => {
                let Some(id) = child(pat, NAME).and_then(|n| self.res.declaration(n.text_range())) else { return };
                let t = if self.assigned.contains(&id) && value.t == Type::Any { Ty::any() } else { value.clone() };
                self.declare_type(id, t);
            }
            TYPED_PAT => {
                let ty = type_child(pat).map(|t| self.type_of(&t)).unwrap_or(Type::Any);
                if let Some(inner) = pattern_children(pat).next() {
                    let v = Ty::certain(ty);
                    if inner.kind() == BIND_PAT {
                        self.bind_param(&inner, &v);
                    } else {
                        self.bind_pattern(&inner, &v, true);
                    }
                }
            }
            _ => {
                self.bind_pattern(pat, value, true);
            }
        }
    }

    /// True for an expression that yields exactly one value: not a call,
    /// which may return any number (the runtime check also fixes the
    /// count), and not a name that may be called.
    fn single_valued(&self, e: &SyntaxNode) -> bool {
        match e.kind() {
            LITERAL | INTERP_STRING | PAREN_EXPR | TABLE_EXPR | LIST_EXPR | STRUCT_LIT | FN_EXPR | LAMBDA_EXPR | PREFIX_EXPR | BIN_EXPR | IS_EXPR | AS_EXPR | SELF_EXPR => true,
            NAME_REF => matches!(self.res.reference(e.text_range()), Some(Ref::Binding(id)) if self.binding(*id).is_some_and(|b| b.kind == BindingKind::Variable)),
            _ => false,
        }
    }

    /// Records a returned value against the declared return type (a
    /// warning: the runtime check reports the actual value)
    /// and for inference. `site`: the node the lowering checks; `single`:
    /// the value is one value, so dropping the check changes no count.
    fn record_return(&mut self, value: &Ty, site: Option<TextRange>, tail: bool, single: bool) {
        let declared = self.current_fn().declared_ret.clone();
        let nil_ret = self.current_fn().nil_ret;
        match declared {
            Some(ret) if !nil_ret => {
                let v = self.types.first(&value.t);
                let fits = match (&ret, &value.t) {
                    (Type::Tuple(want), got) => {
                        let mut ok = true;
                        for (i, w) in want.iter().enumerate() {
                            let g = self.types.nth(got, i);
                            if !self.types.assignable(&g, w) && self.types.is_definite(&g) {
                                ok = false;
                            }
                        }
                        ok
                    }
                    _ => self.types.assignable(&v, &ret) || !self.types.is_definite(&v),
                };
                if !fits && v != Type::Never {
                    let got = self.show(&value.t);
                    let want = self.show(&ret);
                    let where_ = if tail { "the tail value" } else { "the returned value" };
                    if let Some(s) = site {
                        self.warn(s, "return-type", format!("this function is declared to return {want}, but {where_} here is {got}; the runtime check will raise TypeError"));
                    }
                } else if fits && single && value.certain && self.types.is_definite(&value.t) && self.types.is_definite(&ret) && !matches!(ret, Type::Tuple(_)) && self.opts.static_resolution {
                    if let Some(s) = site {
                        self.hints.proven.insert(s);
                    }
                }
            }
            // `: nil`: a `return v` is checked at runtime.
            Some(_) if !tail && self.types.first(&value.t) != Type::Nil && self.types.is_definite(&value.t) => {
                if let Some(s) = site {
                    self.warn(s, "return-type", "this function is declared `: nil` and returns a value; the runtime check will raise TypeError");
                }
            }
            Some(_) => {}
            None => {}
        }
        self.current_fn().returns.push(value.t.clone());
    }

    fn return_stmt(&mut self, node: &SyntaxNode) {
        let values: Vec<SyntaxNode> = child(node, EXPR_LIST).map(|l| expr_children(&l).collect()).unwrap_or_default();
        let value = match values.len() {
            0 => Ty::certain(Type::Nil),
            1 => values.first().map(|v| self.expr(v)).unwrap_or_else(Ty::any),
            _ => {
                let mut types = Vec::new();
                let mut certain = true;
                let n = values.len();
                for (i, v) in values.iter().enumerate() {
                    let t = self.expr(v);
                    certain &= t.certain;
                    if i + 1 == n {
                        match &t.t {
                            Type::Tuple(m) => types.extend(m.iter().cloned()),
                            other => types.push(other.clone()),
                        }
                    } else {
                        types.push(self.types.first(&t.t));
                    }
                }
                Ty::new(Type::Tuple(types.into()), certain)
            }
        };
        if self.fns.len() <= 1 {
            // Reported by the resolver.
            return;
        }
        let single = values.len() == 1 && values.first().is_some_and(|v| self.single_valued(v));
        self.record_return(&value, Some(node.text_range()), false, single);
        self.diverges = true;
    }

    // ---- assignment -------------------------------------------------------------------

    fn assign(&mut self, node: &SyntaxNode) {
        let targets: Vec<SyntaxNode> = match child(node, TARGET_LIST) {
            Some(list) => expr_children(&list).collect(),
            None => expr_children(node).take(1).collect(),
        };
        let values: Vec<SyntaxNode> = match child(node, EXPR_LIST) {
            Some(list) => expr_children(&list).collect(),
            None => expr_children(node).skip(1).collect(),
        };
        if node.kind() == COMPOUND_ASSIGN_STMT {
            let op = tokens(node).find(|t| t.kind().is_compound_assign()).map(|t| t.kind());
            let (Some(target), Some(value)) = (targets.first(), values.first()) else { return };
            let current = self.target_type(target);
            let v = self.expr1(value);
            let result = match op {
                Some(PIPE2_EQ) => Ty::new(self.types.union(&self.types.truthy(&current.t), &v.t), current.certain && v.certain),
                Some(AMP2_EQ) => Ty::new(self.types.union(&self.types.falsy(&current.t), &v.t), current.certain && v.certain),
                Some(QUESTION2_EQ) => Ty::new(self.types.union(&self.types.non_nil(&current.t), &v.t), current.certain && v.certain),
                Some(op) => {
                    let r = self.binary_op(node.text_range(), op, &current, &v);
                    if self.opts.static_resolution && self.direct_op(op, &current, &v) {
                        self.hints.direct_ops.insert(node.text_range());
                    }
                    r
                }
                None => Ty::any(),
            };
            self.store(target, &result);
            return;
        }
        let vals = self.value_list(&values, targets.len());
        if targets.len() > 1 {
            let last_multi = values.last().is_some_and(|v| matches!(v.kind(), CALL_EXPR | COMMAND_CALL | METHOD_CALL | PIPE_EXPR | LUA_PATH | SPAWN_EXPR | PAREN_EXPR | FIELD_EXPR | NAME_REF | AT_EXPR | BANG_EXPR | IF_EXPR | CASE_EXPR | COND_EXPR | TRY_EXPR | UNLESS_EXPR));
            if values.len() > targets.len() || (values.len() < targets.len() && !last_multi) {
                self.warn(node.text_range(), "value-count", format!("this assignment has {} targets but {} values; the missing ones are `nil`", targets.len(), values.len()));
            }
        }
        for (i, t) in targets.iter().enumerate() {
            let v = vals.get(i).cloned().unwrap_or_else(|| Ty::certain(Type::Nil));
            self.store(t, &v);
        }
    }

    /// A declared type that the flow refines: not `any` (opts out of
    /// checking) and not `unknown` (narrowed by `is` alone).
    fn flow_narrows(&self, declared: &Type) -> bool {
        !matches!(self.types.expand(declared), Type::Any | Type::Unknown)
    }

    /// The current type of an assignment target, read.
    fn target_type(&mut self, target: &SyntaxNode) -> Ty {
        match target.kind() {
            NAME_REF => match self.res.reference(target.text_range()).cloned() {
                Some(Ref::Binding(id)) => self.type_of_binding(id),
                _ => Ty::any(),
            },
            FIELD_EXPR => {
                self.place_read = Some(target.text_range());
                let t = self.expr1(target);
                self.place_read = None;
                t
            }
            _ => self.expr1(target),
        }
    }

    /// Stores a value in a target: the declared type of a binding or field
    /// is checked, and the flow type set.
    fn store(&mut self, target: &SyntaxNode, value: &Ty) {
        match target.kind() {
            NAME_REF => {
                let Some(Ref::Binding(id)) = self.res.reference(target.text_range()).cloned() else {
                    if let Some(Ref::Declared(did)) = self.res.reference(target.text_range()).cloned() {
                        if self.decls.get(did).is_some_and(|i| i.kind == crate::decl::DeclItemKind::Const) {
                            let name = self.decls.get(did).map(|i| i.name.clone()).unwrap_or_default();
                            self.error(target.text_range(), "assign-declared-const", format!("`{name}` is declared `const` in a declaration file and cannot be assigned"));
                        } else if let Some(t) = self.decls.get(did).map(|i| i.name.clone()).and_then(|n| self.decl_value_type(&n)) {
                            let name = self.decls.get(did).map(|i| i.name.clone()).unwrap_or_default();
                            self.check_assignable(value, &t, target.text_range(), &format!("declared variable '{name}'"));
                        }
                    }
                    return;
                };
                let name = self.binding(id).map(|b| b.name.clone()).unwrap_or_default();
                let b = self.binding(id).cloned();
                if self.binding_sigs.contains_key(&id) && b.as_ref().is_some_and(|b| b.annotation.is_none()) {
                    // A `let` initialized with `fn` counts as `Function`.
                    let proven = self.check_assignable(value, &Type::Function, target.text_range(), &format!("variable '{name}' (initialized with `fn`)"));
                    if proven && self.opts.static_resolution {
                        self.hints.proven.insert(target.text_range());
                    }
                    return;
                }
                if let Some(ann) = b.as_ref().and_then(|b| b.annotation.clone()) {
                    let ty = self.type_of(&ann);
                    let what = match b.as_ref().map(|b| b.decl) {
                        Some(DeclKind::Global) => format!("global '{name}'"),
                        _ => format!("variable '{name}'"),
                    };
                    let proven = self.check_assignable(value, &ty, target.text_range(), &what);
                    if proven && self.opts.static_resolution {
                        self.hints.proven.insert(target.text_range());
                    }
                    if self.flow_narrows(&ty) && self.types.assignable(&value.t, &ty) && value.t != Type::Any {
                        self.set_flow(id, value.t.clone());
                    } else {
                        self.env.remove(&id);
                    }
                    return;
                }
                self.set_flow(id, value.t.clone());
            }
            FIELD_EXPR => {
                let Some(recv_node) = expr_child(target) else { return };
                let name = field_name(target).map(|t| t.text().to_string()).unwrap_or_default();
                let recv = self.expr1(&recv_node);
                self.store_field(&recv, &name, target, value);
            }
            AT_EXPR => {
                let name = token(target, AT_NAME).map(|t| t.text().trim_start_matches('@').to_string()).unwrap_or_default();
                if let Some(st) = self.self_type() {
                    self.store_field(&Ty::certain(st), &name, target, value);
                }
            }
            INDEX_EXPR => {
                self.expr(target);
            }
            LUA_PATH => {
                self.expr(target);
            }
            _ => {
                self.expr(target);
            }
        }
    }

    /// `x.f = v`: a declared field's type is not checked at runtime
    /// (only at struct construction), so a mismatch is a
    /// warning.
    fn store_field(&mut self, recv: &Ty, name: &str, target: &SyntaxNode, value: &Ty) {
        match self.types.expand(&recv.t) {
            Type::Named(nid) => {
                if let Some(f) = self.types.field(nid, name) {
                    if recv.certain && self.opts.static_resolution && self.types.is_struct(nid) && !matches!(self.types.expand(&f.ty), Type::Function | Type::Fn(_) | Type::Any) {
                        self.hints.members.insert(target.text_range(), MemberHint::Field { key: name.to_string() });
                    }
                    if f.ty != Type::Any && !self.types.assignable(&value.t, &f.ty) && self.types.is_definite(&value.t) {
                        let owner = self.types.name(nid).to_string();
                        let got = self.show(&value.t);
                        let want = self.show(&f.ty);
                        self.warn(target.text_range(), "field-type", format!("field '{name}' of {owner} is declared {want}; this stores {got}"));
                    }
                } else if let Some(key) = self.decl_key(nid, name) {
                    self.hints.members_declared.insert(target.text_range(), MemberHint::Field { key });
                    if let super::expr::Lookup::Field(_, _) = self.lookup_member(recv, name, target.text_range()) {}
                    if matches!(self.types.kind(nid), Some(NamedKind::Module { .. })) {
                        let is_const = matches!(self.types.kind(nid), Some(NamedKind::Module { members }) if matches!(members.get(name), Some(Member::Const(_))));
                        if is_const {
                            self.error(target.text_range(), "assign-declared-const", format!("`{name}` is a `const` member of a declaration file and cannot be assigned"));
                        }
                    }
                }
            }
            Type::Static(nid) => {
                if let Some(key) = self.decl_key(nid, name) {
                    self.hints.members_declared.insert(target.text_range(), MemberHint::Field { key });
                }
                let is_const = matches!(self.types.kind(nid), Some(NamedKind::Module { members }) if matches!(members.get(name), Some(Member::Const(_))));
                if is_const {
                    self.error(target.text_range(), "assign-declared-const", format!("`{name}` is a `const` member and cannot be assigned"));
                }
            }
            _ => {}
        }
    }

    // ---- control flow -------------------------------------------------------------------

    /// A block construct used as a value: `if`, `unless`, `case`, `cond`,
    /// `try`.
    pub(crate) fn construct_value(&mut self, node: &SyntaxNode) -> Ty {
        self.construct(node, true)
    }

    /// A block construct; `value_used` for its branch tails.
    pub(crate) fn construct(&mut self, node: &SyntaxNode, value_used: bool) -> Ty {
        match node.kind() {
            IF_EXPR | UNLESS_EXPR => self.if_expr(node, value_used),
            CASE_EXPR => self.case_expr(node, value_used),
            COND_EXPR => self.cond_expr(node, value_used),
            TRY_EXPR => self.try_expr(node, value_used),
            _ => Ty::any(),
        }
    }

    /// The envs after a condition is true and after it is false.
    pub(crate) fn condition_envs(&mut self, cond: &SyntaxNode) -> (Env, Env) {
        let base = self.env.clone();
        let mut then_env = base.clone();
        let mut else_env = base;
        self.narrow_by(cond, true, &mut then_env, 0);
        self.narrow_by(cond, false, &mut else_env, 0);
        (then_env, else_env)
    }

    /// Narrows `env` by `cond` being `truth`. The negation of `and` and
    /// `or` forks, so the recursion is bounded: a condition nested deeper
    /// than [`NARROW_DEPTH`] narrows nothing.
    fn narrow_by(&mut self, cond: &SyntaxNode, truth: bool, env: &mut Env, depth: u32) {
        if depth > NARROW_DEPTH {
            return;
        }
        match cond.kind() {
            PAREN_EXPR => {
                if let Some(inner) = expr_child(cond) {
                    self.narrow_by(&inner, truth, env, depth + 1);
                }
            }
            PREFIX_EXPR if first_token(cond).is_some_and(|t| matches!(t.kind(), BANG | NOT_KW)) => {
                if let Some(inner) = expr_child(cond) {
                    self.narrow_by(&inner, !truth, env, depth + 1);
                }
            }
            NAME_REF => {
                if let Some(id) = self.local_binding(cond) {
                    let t = self.flow_in(env, id);
                    let n = if truth { self.types.truthy(&t) } else { self.types.falsy(&t) };
                    env.insert(id, n);
                }
            }
            IS_EXPR => {
                let Some(e) = expr_child(cond) else { return };
                let Some(tn) = type_child(cond) else { return };
                if let Some(id) = self.local_binding(&e) {
                    let test = self.type_of(&tn);
                    let t = self.flow_in(env, id);
                    let n = if truth { self.types.narrow(&t, &test) } else { self.types.exclude(&t, &test) };
                    env.insert(id, n);
                }
            }
            BIN_EXPR => {
                let op = bin_op(cond).map(|t| t.kind());
                let parts: Vec<SyntaxNode> = expr_children(cond).collect();
                let (Some(a), Some(b)) = (parts.first(), parts.get(1)) else { return };
                match op {
                    Some(AND_KW | AMP2) => {
                        if truth {
                            self.narrow_by(a, true, env, depth + 1);
                            self.narrow_by(b, true, env, depth + 1);
                        } else {
                            // not (a and b): a false, or a true and b false.
                            let mut e1 = env.clone();
                            self.narrow_by(a, false, &mut e1, depth + 1);
                            let mut e2 = env.clone();
                            self.narrow_by(a, true, &mut e2, depth + 1);
                            self.narrow_by(b, false, &mut e2, depth + 1);
                            *env = self.merge_envs(&[e1, e2]);
                        }
                    }
                    Some(OR_KW | PIPE2) => {
                        if truth {
                            let mut e1 = env.clone();
                            self.narrow_by(a, true, &mut e1, depth + 1);
                            let mut e2 = env.clone();
                            self.narrow_by(a, false, &mut e2, depth + 1);
                            self.narrow_by(b, true, &mut e2, depth + 1);
                            *env = self.merge_envs(&[e1, e2]);
                        } else {
                            self.narrow_by(a, false, env, depth + 1);
                            self.narrow_by(b, false, env, depth + 1);
                        }
                    }
                    Some(EQ2 | NEQ) => {
                        let eq = op == Some(EQ2);
                        let (name, lit) = if is_nil_literal(b) { (a, b) } else if is_nil_literal(a) { (b, a) } else { return };
                        let _ = lit;
                        if let Some(id) = self.local_binding(name) {
                            let t = self.flow_in(env, id);
                            let is_nil = truth == eq;
                            let n = if is_nil { self.types.narrow(&t, &Type::Nil) } else { self.types.exclude(&t, &Type::Nil) };
                            env.insert(id, n);
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    /// The binding of a bare name, when it is one that narrowing applies
    /// to.
    fn local_binding(&self, e: &SyntaxNode) -> Option<BindingId> {
        if e.kind() != NAME_REF {
            return None;
        }
        match self.res.reference(e.text_range())? {
            Ref::Binding(id) => {
                let b = self.binding(*id)?;
                if matches!(b.decl, DeclKind::Global | DeclKind::Fn | DeclKind::Struct | DeclKind::Enum | DeclKind::Interface | DeclKind::TypeAlias | DeclKind::Module | DeclKind::Import) {
                    return None;
                }
                Some(*id)
            }
            _ => None,
        }
    }

    fn flow_in(&self, env: &Env, id: BindingId) -> Type {
        env.get(&id).cloned().or_else(|| self.binding_types.get(&id).map(|t| t.t.clone())).unwrap_or(Type::Any)
    }

    /// The union of several envs: a binding narrowed in all of them keeps
    /// the union of its types; one narrowed in some goes back to its
    /// declared type.
    pub(crate) fn merge_envs(&self, envs: &[Env]) -> Env {
        let mut out = Env::new();
        let mut keys: HashSet<BindingId> = HashSet::new();
        for e in envs {
            keys.extend(e.keys().copied());
        }
        for id in keys {
            let mut types = Vec::new();
            let mut all = true;
            for e in envs {
                match e.get(&id) {
                    Some(t) => types.push(t.clone()),
                    None => all = false,
                }
            }
            if all {
                out.insert(id, self.types.union_all(&types));
            }
        }
        out
    }

    /// `if`/`elseif`/`else` and `unless`.
    fn if_expr(&mut self, node: &SyntaxNode, value_used: bool) -> Ty {
        let unless = node.kind() == UNLESS_EXPR;
        let mut outcomes: Vec<(Env, Ty)> = Vec::new();
        let mut values: Vec<Type> = Vec::new();
        let mut certain = true;
        let mut has_else = false;
        // The env in which the next condition is evaluated.
        let saved = self.env.clone();
        let mut rest_env = saved.clone();
        let mut clauses: Vec<(Option<SyntaxNode>, Option<SyntaxNode>)> = vec![(None, None)];
        // The first clause is the node itself.
        clauses.clear();
        clauses.push((child(node, LET_CONDITION).or_else(|| expr_child(node)), block(node)));
        for c in node.children() {
            match c.kind() {
                ELSEIF_CLAUSE => clauses.push((child(&c, LET_CONDITION).or_else(|| expr_child(&c)), block(&c))),
                ELSE_CLAUSE => {
                    has_else = true;
                    clauses.push((None, block(&c)));
                }
                _ => {}
            }
        }
        let was = self.diverges;
        for (cond, body) in clauses {
            self.env = rest_env.clone();
            let (then_env, else_env) = match &cond {
                Some(c) if c.kind() == LET_CONDITION => {
                    let value = expr_child(c).map(|e| self.expr1(&e)).unwrap_or_else(Ty::any);
                    let mut then_env = self.env.clone();
                    let else_env = self.env.clone();
                    std::mem::swap(&mut self.env, &mut then_env);
                    if let Some(p) = pattern_children(c).next() {
                        self.bind_pattern(&p, &value, false);
                    }
                    std::mem::swap(&mut self.env, &mut then_env);
                    (then_env, else_env)
                }
                Some(c) => {
                    self.expr1(c);
                    let (t, e) = self.condition_envs(c);
                    if unless { (e, t) } else { (t, e) }
                }
                None => (self.env.clone(), self.env.clone()),
            };
            self.env = then_env;
            self.diverges = false;
            let v = match &body {
                Some(b) => self.scoped_block(b, value_used),
                None => Ty::certain(Type::Nil),
            };
            if !self.diverges {
                outcomes.push((self.env.clone(), v.clone()));
                values.push(v.t.clone());
                certain &= v.certain;
            }
            rest_env = else_env;
        }
        self.diverges = was;
        if !has_else {
            outcomes.push((rest_env, Ty::certain(Type::Nil)));
            values.push(Type::Nil);
        }
        let envs: Vec<Env> = outcomes.iter().map(|(e, _)| e.clone()).collect();
        if envs.is_empty() {
            self.env = saved;
            self.diverges = true;
            return Ty::certain(Type::Never);
        }
        self.env = self.merge_envs(&envs);
        Ty::new(self.types.union_all(&values), certain)
    }

    /// `cond`.
    fn cond_expr(&mut self, node: &SyntaxNode, value_used: bool) -> Ty {
        let saved = self.env.clone();
        let mut rest_env = saved.clone();
        let mut envs: Vec<Env> = Vec::new();
        let mut values: Vec<Type> = Vec::new();
        let mut has_else = false;
        let was = self.diverges;
        for c in node.children() {
            match c.kind() {
                COND_CLAUSE => {
                    self.env = rest_env.clone();
                    let (then_env, else_env) = match expr_child(&c) {
                        Some(e) => {
                            self.expr1(&e);
                            self.condition_envs(&e)
                        }
                        None => (self.env.clone(), self.env.clone()),
                    };
                    self.env = then_env;
                    self.diverges = false;
                    let v = block(&c).map(|b| self.scoped_block(&b, value_used)).unwrap_or_else(|| Ty::certain(Type::Nil));
                    if !self.diverges {
                        envs.push(self.env.clone());
                        values.push(v.t);
                    }
                    rest_env = else_env;
                }
                ELSE_CLAUSE => {
                    has_else = true;
                    self.env = rest_env.clone();
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
            envs.push(rest_env);
            values.push(Type::Nil);
        }
        self.env = if envs.is_empty() { saved } else { self.merge_envs(&envs) };
        if envs.is_empty() {
            self.diverges = true;
            return Ty::certain(Type::Never);
        }
        Ty::uncertain(self.types.union_all(&values))
    }

    /// `try`/`catch`/`finally`: the caught value is an
    /// `Exception`; a clause that an earlier one shadows is unreachable.
    fn try_expr(&mut self, node: &SyntaxNode, value_used: bool) -> Ty {
        let saved = self.env.clone();
        let was = self.diverges;
        let mut values: Vec<Type> = Vec::new();
        let mut envs: Vec<Env> = Vec::new();
        self.diverges = false;
        if let Some(b) = block(node) {
            let v = self.scoped_block(&b, value_used);
            if !self.diverges {
                values.push(v.t);
                envs.push(self.env.clone());
            }
        }
        // Inside a catch the body may have stopped anywhere: the flow
        // types from before the try hold, not those the body set.
        let exception = self.prelude_type_id("Exception").map(Type::Named).unwrap_or(Type::Any);
        let mut covered: Vec<Type> = Vec::new();
        for c in node.children() {
            match c.kind() {
                CATCH_CLAUSE => {
                    self.env = saved.clone();
                    self.diverges = false;
                    let ty = match type_child(&c) {
                        Some(t) => self.type_of(&t),
                        None => exception.clone(),
                    };
                    // Unreachable: everything it catches is caught before.
                    if covered.iter().any(|earlier| self.types.assignable(&ty, earlier)) {
                        let at = token(&c, CATCH_KW).map_or(c.text_range(), |t| t.text_range());
                        let shown = self.show(&ty);
                        self.warn(at, "unreachable-catch", format!("this `catch` is unreachable: an earlier clause already catches every {shown}"));
                    }
                    covered.push(ty.clone());
                    if let Some(id) = child(&c, NAME).and_then(|n| self.res.declaration(n.text_range())) {
                        self.declare_type(id, Ty::certain(ty));
                    }
                    let v = block(&c).map(|b| self.scoped_block(&b, value_used)).unwrap_or_else(|| Ty::certain(Type::Nil));
                    if !self.diverges {
                        values.push(v.t);
                        envs.push(self.env.clone());
                    }
                }
                FINALLY_CLAUSE => {
                    self.env = saved.clone();
                    self.diverges = false;
                    if let Some(b) = block(&c) {
                        self.scoped_block(&b, false);
                    }
                }
                _ => {}
            }
        }
        self.diverges = was;
        if envs.is_empty() {
            self.env = saved;
            self.diverges = true;
            return Ty::certain(Type::Never);
        }
        self.env = self.merge_envs(&envs);
        Ty::uncertain(self.types.union_all(&values))
    }

    // ---- loops --------------------------------------------------------------------------

    /// Forgets the flow types of the bindings a loop body assigns, before
    /// the loop, so that no narrowing from before it is relied on inside.
    pub(crate) fn widen_assigned_in(&mut self, body: &SyntaxNode) {
        for n in body.descendants() {
            let targets: Vec<SyntaxNode> = match n.kind() {
                ASSIGN_STMT | COMPOUND_ASSIGN_STMT | MULTI_ASSIGN_STMT => match child(&n, TARGET_LIST) {
                    Some(list) => expr_children(&list).collect(),
                    None => expr_children(&n).take(1).collect(),
                },
                _ => continue,
            };
            for t in targets {
                if t.kind() == NAME_REF {
                    if let Some(Ref::Binding(id)) = self.res.reference(t.text_range()) {
                        self.env.remove(id);
                    }
                }
            }
        }
    }

    fn while_stmt(&mut self, node: &SyntaxNode) {
        self.widen_assigned_in(node);
        let entry = self.env.clone();
        let (body_env, exit_env) = match (node.kind(), expr_child(node)) {
            (WHILE_STMT, Some(c)) => {
                self.expr1(&c);
                self.condition_envs(&c)
            }
            (UNTIL_STMT, Some(c)) => {
                self.expr1(&c);
                let (t, e) = self.condition_envs(&c);
                (e, t)
            }
            _ => (entry.clone(), entry.clone()),
        };
        self.env = body_env;
        let was = self.diverges;
        self.diverges = false;
        self.current_fn().loop_depth += 1;
        if let Some(b) = block(node) {
            self.scoped_block(&b, false);
        }
        self.current_fn().loop_depth -= 1;
        self.diverges = was;
        let after = self.env.clone();
        self.env = self.merge_envs(&[after, exit_env]);
    }

    /// `for patterns in expr`.
    fn for_stmt(&mut self, node: &SyntaxNode) {
        let mut pats = Vec::new();
        let mut iterable = None;
        let mut seen_in = false;
        for el in node.children_with_tokens() {
            match el {
                rowan::NodeOrToken::Token(t) if t.kind() == IN_KW => seen_in = true,
                rowan::NodeOrToken::Node(n) if !seen_in && is_pattern(n.kind()) => pats.push(n),
                rowan::NodeOrToken::Node(n) if seen_in && iterable.is_none() && is_expr(n.kind()) => iterable = Some(n),
                _ => {}
            }
        }
        let Some(iterable) = iterable else { return };
        let it = self.expr(&iterable);
        if let Some(step) = child(node, FOR_STEP).and_then(|s| expr_child(&s)) {
            let st = self.expr1(&step);
            if st.certain && !matches!(self.types.expand(&st.t), Type::Number | Type::Any) {
                let shown = self.show(&st.t);
                self.error(step.text_range(), "bad-step", format!("`step` takes a number other than zero, got {shown}"));
            }
        }
        let n = pats.len();
        let it_t = self.types.expand(&self.types.first(&it.t));
        // The types of the loop variables.
        let vars: Vec<Ty> = match &it_t {
            Type::Range => vec![Ty::certain(Type::Number)],
            Type::String => vec![Ty::certain(Type::String)],
            Type::Array(e) => {
                if n <= 1 {
                    vec![Ty::uncertain((**e).clone())]
                } else {
                    vec![Ty::uncertain(Type::Any), Ty::uncertain((**e).clone())]
                }
            }
            Type::Map(k, v) => {
                if n <= 1 {
                    vec![Ty::uncertain((**v).clone())]
                } else {
                    vec![Ty::uncertain((**k).clone()), Ty::uncertain((**v).clone())]
                }
            }
            Type::Record(fields) => {
                let vt = self.types.union_all(&fields.iter().map(|(_, t)| t.clone()).collect::<Vec<_>>());
                if n <= 1 { vec![Ty::uncertain(vt)] } else { vec![Ty::uncertain(Type::String), Ty::uncertain(vt)] }
            }
            Type::Nil | Type::Number | Type::Bool if it.certain => {
                let shown = self.show(&it_t);
                self.error(iterable.text_range(), "not-iterable", format!("cannot iterate over a {shown}"));
                Vec::new()
            }
            _ => Vec::new(),
        };
        if self.opts.static_resolution && it.certain {
            let hint = match &it_t {
                Type::Array(_) | Type::Map(..) | Type::Record(_) | Type::Table => Some(if n <= 1 { LoopHint::Array } else { LoopHint::Map }),
                Type::Fn(_) | Type::Function => Some(LoopHint::Function),
                _ => None,
            };
            if let Some(h) = hint {
                self.hints.loops.insert(node.text_range(), h);
            }
        }
        self.widen_assigned_in(node);
        let entry = self.env.clone();
        let was = self.diverges;
        self.diverges = false;
        for (i, p) in pats.iter().enumerate() {
            let v = vars.get(i).cloned().unwrap_or_else(Ty::any);
            self.bind_param(p, &v);
        }
        self.current_fn().loop_depth += 1;
        if let Some(b) = block(node) {
            self.scoped_block(&b, false);
        }
        self.current_fn().loop_depth -= 1;
        self.diverges = was;
        let after = self.env.clone();
        self.env = self.merge_envs(&[after, entry]);
    }
}

/// How deep a condition is followed for narrowing (see
/// [`Analyzer::narrow_by`]).
const NARROW_DEPTH: u32 = 12;

fn is_nil_literal(e: &SyntaxNode) -> bool {
    e.kind() == LITERAL && first_token(e).is_some_and(|t| t.kind() == NIL_KW)
}
