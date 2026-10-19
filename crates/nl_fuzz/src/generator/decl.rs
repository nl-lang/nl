//! Declarations of generated programs: enums, structs with
//! methods, `To<String>` and operators, exception structs, and functions
//! of every kind the generator calls:
//! plain ones with defaults, `*rest` and `**opts`, functions that take a
//! block, closure factories, `let f = fn ... end`, closures made in a
//! loop, and bounded recursion. The checked-only probes of runtime checks
//! are here too.

use super::*;

/// Keys of keyed arguments.
const OPT_KEYS: &[&str] = &["scale", "shift", "bias", "limit", "step_by"];

impl Gen {
    pub(super) fn enum_decl(&mut self) -> String {
        let name = self.fresh("Enum");
        let n = 2 + self.rng.below(3);
        let kind = self.rng.below(3);
        let mut variants = Vec::new();
        let mut used_nums: Vec<i64> = Vec::new();
        let mut lines = vec![format!("enum {name}")];
        for i in 0..n {
            let vname = format!("{}{}", self.rng.pick(&["Red", "Green", "Blue", "North", "South", "Low", "High", "Idle"]), i);
            let val = match kind {
                0 => EnumVal::Name,
                1 => EnumVal::Str(format!("{}{i}", self.rng.pick(&["v", "mode", "x", "on"]))),
                _ => {
                    let mut v = self.rng.range(-5, 40);
                    while used_nums.contains(&v) {
                        v += 1;
                    }
                    used_nums.push(v);
                    EnumVal::Num(v)
                }
            };
            lines.push(match &val {
                EnumVal::Name => format!("  {vname}"),
                EnumVal::Str(s) => format!("  {vname} = \"{s}\""),
                EnumVal::Num(v) => format!("  {vname} = {v}"),
            });
            variants.push((vname, val));
        }
        lines.push("end".into());
        self.enums.push(EnumDef { name, variants });
        lines.join("\n")
    }

    /// A struct, its methods, and sometimes `To<String>`, `+` and `len`.
    pub(super) fn struct_decl(&mut self) -> Vec<String> {
        let idx = self.structs.len();
        let name = self.fresh("Shape");
        let parent = if idx > 0 && self.rng.chance(35) { Some(self.rng.below(idx)) } else { None };
        let mut fields = Vec::new();
        let n = if parent.is_some() { self.rng.below(3) } else { 1 + self.rng.below(3) };
        for _ in 0..n {
            let fname = self.fresh("f");
            let is_str = self.rng.chance(20);
            let typed = self.rng.chance(60);
            let (ty, bound) = if is_str { (Ty::Str, STR_CAP) } else { (Ty::Int, self.rng.pick(&[100.0, 1000.0])) };
            let default = if self.rng.chance(40) {
                Some(if is_str { format!("\"{}\"", self.rng.pick(&["dflt", "", "zz"])) } else { format!("{}", self.rng.range(0, 9)) })
            } else {
                None
            };
            fields.push(Field { name: fname, ty, bound, typed, default });
        }
        let mut lines = vec![match parent.and_then(|p| self.structs.get(p)) {
            Some(p) => format!("struct {name} : {}", p.name),
            None => format!("struct {name}"),
        }];
        for f in &fields {
            let ty = if f.ty == Ty::Str { "String" } else { "Number" };
            lines.push(match (&f.default, f.typed) {
                (Some(d), true) => format!("  {}: {ty} = {d}", f.name),
                (Some(d), false) => format!("  {} = {d}", f.name),
                (None, true) => format!("  {}: {ty}", f.name),
                (None, false) => format!("  {}", f.name),
            });
        }
        lines.push("end".into());
        self.structs.push(StructDef { name: name.clone(), parent, fields, methods: Vec::new(), to_string: false, plus: false, len: None, lt: false, eq: false, call_op: false, make: None });
        let mut out = vec![lines.join("\n")];

        let int_fields: Vec<Field> = self.all_fields(idx).into_iter().filter(|f| f.ty == Ty::Int).collect();
        // Methods: a computing one and a mutating one.
        let n_methods = self.rng.below(3);
        for _ in 0..n_methods {
            if let Some(m) = self.method_decl(idx, &int_fields) {
                out.push(m);
            }
        }
        if !int_fields.is_empty() && self.rng.chance(40) {
            let f = self.rng.pick(&int_fields.iter().map(|f| f.name.clone()).collect::<Vec<_>>());
            out.push(format!("interface To<String> for {name}\n  fn to: String\n    \"{name}<#{{@{f} + 0}}>\"\n  end\nend"));
            if let Some(s) = self.structs.get_mut(idx) {
                s.to_string = true;
            }
        }
        if self.rng.chance(25) {
            // `+` builds a new instance from every field.
            let all = self.all_fields(idx);
            let inits: Vec<String> = all
                .iter()
                .map(|f| match f.ty {
                    Ty::Str => format!("{}: @{}", f.name, f.name),
                    _ => format!("{}: (@{} + other.{}) % {}", f.name, f.name, f.name, bound_lit(f.bound)),
                })
                .collect();
            out.push(format!("fn {name}.+(other)\n  {name} {{ {} }}\nend", inits.join(", ")));
            if let Some(s) = self.structs.get_mut(idx) {
                s.plus = true;
            }
        }
        if !int_fields.is_empty() && self.rng.chance(25) {
            let f = self.rng.pick(&int_fields.iter().map(|f| f.name.clone()).collect::<Vec<_>>());
            out.push(format!("fn {name}.len\n  @{f} % 10\nend"));
            if let Some(s) = self.structs.get_mut(idx) {
                s.len = Some(10.0);
            }
        }
        out
    }

    /// A method: either it computes an integer from its parameters and the
    /// fields, or it changes a field.
    fn method_decl(&mut self, idx: usize, int_fields: &[Field]) -> Option<String> {
        let sname = self.structs.get(idx)?.name.clone();
        let mname = self.fresh("m");
        let n_params = self.rng.below(3);
        let params: Vec<Param> = (0..n_params).map(|_| Param { name: self.fresh("k"), ty: Ty::Int, bound: 1000.0, default: None }).collect();
        let plist = if params.is_empty() && self.rng.chance(50) { String::new() } else { format!("({})", params.iter().map(|p| p.name.clone()).collect::<Vec<_>>().join(", ")) };
        let mutate = !int_fields.is_empty() && self.rng.chance(40);
        let ctx = Ctx { method_of: Some(idx), ..Ctx::top() };
        let (body, ret) = if mutate {
            let f = self.rng.pick_ref(int_fields)?;
            self.frames.push(Frame::new(Ret::Nothing, INT_CAP));
            self.push_scope();
            for p in &params {
                self.declare(Var { name: p.name.clone(), ty: Ty::Int, bound: p.bound, mutable: true, used: true, fn_depth: 1, ..Var::default() });
            }
            let e = self.int(&ctx, 2);
            let mut stmts = Vec::new();
            if self.rng.chance(30) {
                stmts.push(format!("print(\"{mname}\", @{} + 0)", f.name));
                self.add_cost(1.0);
            }
            stmts.push(format!("@{} = (@{} + {}) % {}", f.name, f.name, e.operand(), bound_lit(f.bound)));
            let mut tail_prints = self.pop_scope(&ctx);
            stmts.append(&mut tail_prints);
            let frame = self.frames.pop()?;
            self.structs.get_mut(idx)?.methods.push(Method { name: mname.clone(), params: params.iter().map(|p| p.bound).collect(), ret: None, cost: frame.cost });
            (block_text(&stmts), None)
        } else {
            let (body, frame) = self.fn_body(&ctx, &params, &[], Ret::One(Ty::Int, 0.0), 2, INT_CAP, false);
            let Ret::One(_, b) = frame.ret else { return None };
            self.structs.get_mut(idx)?.methods.push(Method { name: mname.clone(), params: params.iter().map(|p| p.bound).collect(), ret: Some(b), cost: frame.cost });
            (body, Some(b))
        };
        let _ = ret;
        Some(format!("fn {sname}.{mname}{plist}\n{body}\nend"))
    }

    pub(super) fn error_decl(&mut self) -> String {
        let name = self.fresh("Oops");
        let parent = if !self.errors.is_empty() && self.rng.chance(40) { self.rng.pick(&self.errors.clone()) } else { String::new() };
        self.errors.push(name.clone());
        if parent.is_empty() { format!("struct {name} : Exception\n  code: Number = 0\nend") } else { format!("struct {name} : {parent}\nend") }
    }

    /// Generates a function body: parameters in scope, some statements, and
    /// a tail of the return type. Returns the indented body and the frame,
    /// whose `ret` holds the bound of every value returned.
    /// With `may_raise`, the body may raise; the frame says whether it
    /// does.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn fn_body(&mut self, outer: &Ctx, params: &[Param], locals: &[Param], ret: Ret, size: usize, cap: f64, may_raise: bool) -> (String, Frame) {
        self.frames.push(Frame::new(ret, cap));
        self.push_scope();
        let depth = self.fn_depth();
        // Parameters are never reported unused (lints.rs); the locals
        // declared before the body are, so they start unused.
        // A parameter may be annotated: strings assigned to it get a
        // default (`non_nil`).
        for (p, used) in params.iter().map(|p| (p, true)).chain(locals.iter().map(|p| (p, false))) {
            self.declare(Var { name: p.name.clone(), ty: p.ty, bound: p.bound, mutable: true, used, fn_depth: depth, ascii: p.ty != Ty::Str, typed: used, ..Var::default() });
        }
        let ctx = Ctx { depth: outer.depth + 1, loops: 0, mult: 1.0, raise_ok: may_raise, caught: false, method_of: outer.method_of };
        let n = self.rng.below(size + 1);
        let mut stmts: Vec<String> = Vec::new();
        for _ in 0..n {
            stmts.push(self.stmt(&ctx));
        }
        let tail = match ret {
            Ret::One(ty, _) => {
                let e = self.tail_expr(&ctx, ty);
                let e = if matches!(ty, Ty::Int | Ty::Num) { fit(e, cap) } else { e };
                if let Some(f) = self.frames.last_mut() {
                    f.ret_bound = f.ret_bound.max(e.bound);
                }
                Some(e.code)
            }
            Ret::Pair(b) => {
                let a = self.int_within(&ctx, 2, b);
                let c = self.int_within(&ctx, 2, b);
                Some(format!("{}, {}", a.code, c.code))
            }
            Ret::Nothing => None,
        };
        let mut unused = self.pop_scope(&ctx);
        stmts.append(&mut unused);
        if let Some(t) = tail {
            stmts.push(t);
        }
        let mut frame = self.frames.pop().unwrap_or_else(|| Frame::new(ret, cap));
        if let Ret::One(ty, _) = ret {
            frame.ret = Ret::One(ty, frame.ret_bound.max(1.0));
        }
        let body = if stmts.is_empty() { String::new() } else { block_text(&stmts) };
        (body, frame)
    }

    /// One top-level or nested function, with whatever it needs around it.
    pub(super) fn fn_decl(&mut self, ctx: &Ctx) -> Option<String> {
        if ctx.depth > 2 {
            return None;
        }
        match self.rng.weighted(&[40, 10, 8, 10, 10, 6, 6]) {
            0 => Some(self.plain_fn(ctx)),
            1 => Some(self.block_fn(ctx)),
            2 => Some(self.each_fn(ctx)),
            3 => self.factory(ctx),
            4 => Some(self.let_fn(ctx)),
            5 => self.closures_in_loop(ctx),
            _ => Some(self.recursive_fn(ctx)),
        }
    }

    fn plain_fn(&mut self, ctx: &Ctx) -> String {
        let name = self.fresh("fun");
        let typed = self.rng.chance(30);
        let n_params = self.rng.below(4);
        let mut params = Vec::new();
        for i in 0..n_params {
            let pname = self.fresh("a");
            let ty = if self.rng.chance(25) { Ty::Str } else { Ty::Int };
            let bound = if ty == Ty::Str { STR_CAP } else { self.rng.pick(&[100.0, 1000.0, 100_000.0]) };
            // Defaults come last; one may use an earlier
            // parameter.
            let default = if i + 1 == n_params && self.rng.chance(40) {
                Some(match ty {
                    Ty::Str => format!("\"{}\"", self.rng.pick(&["d", "dflt", ""])),
                    _ => match params.iter().find(|p: &&Param| p.ty == Ty::Int) {
                        Some(p) if self.rng.chance(40) => format!("{} % {}", p.name, bound_lit(bound)),
                        _ => format!("{}", self.rng.range(0, 50)),
                    },
                })
            } else {
                None
            };
            params.push(Param { name: pname, ty, bound, default });
        }
        let rest = !typed && self.rng.chance(12);
        let opts: Vec<String> = if self.rng.chance(20) {
            let n = 1 + self.rng.below(2);
            let mut ks: Vec<String> = Vec::new();
            for _ in 0..n {
                let k = self.rng.pick(OPT_KEYS).to_string();
                if !ks.contains(&k) {
                    ks.push(k);
                }
            }
            ks
        } else {
            Vec::new()
        };
        let ret = match self.rng.weighted(&[50, 20, 20, 10]) {
            0 => Ret::One(Ty::Int, 0.0),
            1 => Ret::One(Ty::Str, 0.0),
            2 => Ret::Nothing,
            _ => Ret::Pair(1000.0),
        };
        let opts_name = self.fresh("opts");
        let rest_name = self.fresh("rest");
        // The body reads the options and the rest through variables of
        // its own, declared first.
        let mut pre: Vec<String> = Vec::new();
        let mut extra: Vec<Param> = Vec::new();
        for k in &opts {
            let v = self.fresh("o");
            let d = self.rng.range(0, 9);
            pre.push(format!("let {v} = ({opts_name}.{k} or {d}) % 1000"));
            extra.push(Param { name: v, ty: Ty::Int, bound: 1000.0, default: None });
        }
        if rest {
            let v = self.fresh("n");
            pre.push(format!("let {v} = {rest_name}.len"));
            extra.push(Param { name: v, ty: Ty::Int, bound: 10.0, default: None });
        }
        // Some functions may raise; they are then only called where that
        // is caught.
        let may_raise = !self.errors.is_empty() && self.rng.chance(15);
        // A small return bound lets the function stand in for a block
        // (`with(x, f!)`).
        let cap = if self.rng.chance(50) { 1000.0 } else { INT_CAP };
        let (body, frame) = self.fn_body(ctx, &params, &extra, ret, 3, cap, may_raise);
        let mut plist: Vec<String> = params
            .iter()
            .map(|p| {
                let ann = if typed { if p.ty == Ty::Str { ": String" } else { ": Number" } } else { "" };
                match &p.default {
                    Some(d) => format!("{}{ann} = {d}", p.name),
                    None => format!("{}{ann}", p.name),
                }
            })
            .collect();
        if rest {
            plist.push(format!("*{rest_name}"));
        }
        if !opts.is_empty() {
            plist.push(if typed { format!("**{opts_name}: any") } else { format!("**{opts_name}") });
        }
        let ret_ann = if typed && self.rng.chance(60) {
            match frame.ret {
                Ret::One(Ty::Str, _) => ": String",
                Ret::One(_, _) => ": Number",
                Ret::Pair(_) => ": (Number, Number)",
                Ret::Nothing => ": nil",
            }
        } else {
            ""
        };
        let header = if plist.is_empty() && self.rng.chance(50) { format!("fn {name}{ret_ann}") } else { format!("fn {name}({}){ret_ann}", plist.join(", ")) };
        let doc = if self.rng.chance(15) { format!("## {name} computes something.\n") } else { String::new() };
        let mut text = format!("{doc}{header}\n");
        for p in &pre {
            text.push_str(&indent(p));
            text.push('\n');
        }
        if !body.is_empty() {
            text.push_str(&body);
            text.push('\n');
        }
        text.push_str("end");
        self.declare_fn(Func {
            name,
            params,
            rest,
            opts,
            ret: frame.ret,
            cost: frame.cost + 1.0,
            raises: frame.raises,
            assigns: frame.assigns,
            block: None,
            kind: FnKind::Decl,
            typed,
        });
        text
    }

    /// A print of a call of the function declared last in this scope.
    pub(super) fn call_latest(&mut self, ctx: &Ctx) -> Option<String> {
        let f = self.scopes.last()?.funcs.last()?.clone();
        if f.raises && !ctx.raise_ok || f.block.is_some() {
            return None;
        }
        if !self.affordable(f.cost * ctx.mult + 1.0) {
            return None;
        }
        let call = self.call_text(ctx, &f)?;
        self.note_call(&f, ctx);
        Some(match f.ret {
            Ret::Nothing => call.code,
            Ret::One(Ty::Int | Ty::Num, _) | Ret::Pair(_) => format!("print(\"{}\", {} + 0)", f.name, call.code),
            _ => format!("print(\"{}\", {})", f.name, call.code),
        })
    }

    /// A function with a `Function` parameter that takes a trailing block;
    /// every parameter is annotated.
    fn block_fn(&mut self, _ctx: &Ctx) -> String {
        let name = self.fresh("with");
        let x = self.fresh("x");
        let cb = self.fresh("cb");
        let acc = self.fresh("acc");
        let i = self.fresh("i");
        let body = match self.rng.below(4) {
            0 => format!("  ({cb}({x}) + {cb}(({x} * 2) % 1000)) % 1000"),
            1 => format!("  let {acc} = 0\n  for {i} in 1..3\n    {acc} = ({acc} + {cb}(({x} + {i}) % 1000)) % 1000\n  end\n  {acc}"),
            2 => format!("  {cb}({cb}({x}) % 1000) % 1000"),
            _ => format!("  if {x} > 500\n    return {cb}({x} - 500) % 1000\n  end\n  {cb}({x}) % 1000"),
        };
        let ret = if self.rng.chance(50) { ": Number" } else { "" };
        let ann = if self.rng.chance(30) { "Function(n: Number): Number" } else { "Function" };
        let text = format!("fn {name}({x}: Number, {cb}: {ann}){ret}\n{body}\nend");
        self.declare_fn(Func {
            name,
            params: vec![Param { name: x, ty: Ty::Int, bound: 999.0, default: None }],
            rest: false,
            opts: Vec::new(),
            ret: Ret::One(Ty::Int, 1000.0),
            cost: 4.0,
            raises: false,
            assigns: Vec::new(),
            block: Some((999.0, 1000.0)),
            kind: FnKind::Decl,
            typed: true,
        });
        text
    }

    /// `fn each(l: [Number], cb: Function): nil`, for blocks that print.
    fn each_fn(&mut self, _ctx: &Ctx) -> String {
        let name = self.fresh("each");
        let l = self.fresh("l");
        let cb = self.fresh("cb");
        let v = self.fresh("v");
        let lty = self.rng.pick(&["[Number]", "Table", "any"]);
        let text = format!("fn {name}({l}: {lty}, {cb}: Function): nil\n  for {v} in {l}\n    {cb}({v})\n  end\nend");
        // Marked by a zero bound on the block's argument: the caller binds
        // it to the list's items.
        self.declare_fn(Func {
            name,
            params: vec![Param { name: l, ty: Ty::IntList, bound: LIST_CAP, default: None }],
            rest: false,
            opts: Vec::new(),
            ret: Ret::Nothing,
            cost: LIST_CAP,
            raises: false,
            assigns: Vec::new(),
            block: Some((0.0, 0.0)),
            kind: FnKind::Decl,
            typed: true,
        });
        text
    }

    /// A function that returns a closure over a local and its parameter,
    /// and a variable holding one.
    fn factory(&mut self, ctx: &Ctx) -> Option<String> {
        let name = self.fresh("make");
        let step = self.fresh("step");
        let c = self.fresh("c");
        let v = self.fresh("v");
        let takes = self.rng.chance(40);
        let inner = if takes {
            match self.rng.below(2) {
                0 => format!("  fn({v})\n    {c} = ({c} + {v} + {step}) % 1000\n    {c}\n  end"),
                _ => format!("  {}", self.fn_literal(std::slice::from_ref(&v), &format!("({v} * {step} + {c}) % 1000"), true)),
            }
        } else {
            match self.rng.below(3) {
                0 => format!("  fn()\n    {c} = ({c} + {step}) % 1000\n    {c}\n  end"),
                1 => format!("  fn()\n    {c} += {step}\n    {c} %= 1000\n    {c}\n  end"),
                _ => format!("  {}", self.fn_literal(&[], &format!("({c} + {step}) % 1000"), true)),
            }
        };
        let start = self.rng.range(0, 20);
        let text = format!("fn {name}({step})\n  let {c} = {start}\n{inner}\nend");
        let inst = self.fresh("ctr");
        let arg = self.int_within(ctx, 1, 1000.0);
        let mut out = format!("{text}\nlet {inst} = {name}({})", arg.code);
        let params = if takes { vec![Param { name: self.fresh("u"), ty: Ty::Int, bound: 1000.0, default: None }] } else { Vec::new() };
        let f = Func { name: inst.clone(), params, rest: false, opts: Vec::new(), ret: Ret::One(Ty::Int, 1000.0), cost: 1.0, raises: false, assigns: Vec::new(), block: None, kind: FnKind::Value, typed: false };
        // Read it once, so that the binding is used.
        let call = self.call_text(ctx, &f)?;
        self.add_cost(ctx.mult);
        out.push_str(&format!("\nprint(\"{inst}\", {})", call.code));
        self.declare_fn(f);
        Some(out)
    }

    /// `let name = fn(...) ... end`: a function-kind variable.
    fn let_fn(&mut self, ctx: &Ctx) -> String {
        let name = self.fresh("lam");
        let n = self.rng.below(3);
        let params: Vec<Param> = (0..n).map(|_| Param { name: self.fresh("p"), ty: Ty::Int, bound: 1000.0, default: None }).collect();
        let plist = params.iter().map(|p| p.name.clone()).collect::<Vec<_>>().join(", ");
        let short = self.rng.chance(50);
        let (text, frame) = if short {
            // One line: the body is a single expression.
            self.frames.push(Frame::new(Ret::One(Ty::Int, 0.0), INT_CAP));
            self.push_scope();
            for p in &params {
                self.declare(Var { name: p.name.clone(), ty: Ty::Int, bound: p.bound, mutable: true, used: true, fn_depth: self.fn_depth(), ..Var::default() });
            }
            let inner = Ctx { depth: ctx.depth + 1, loops: 0, mult: 1.0, raise_ok: false, caught: false, method_of: ctx.method_of };
            let e = self.int(&inner, 2);
            let _ = self.pop_scope(&inner);
            let mut frame = self.frames.pop().unwrap_or_else(|| Frame::new(Ret::Nothing, INT_CAP));
            frame.ret = Ret::One(Ty::Int, e.bound.max(1.0));
            let names: Vec<String> = params.iter().map(|p| p.name.clone()).collect();
            (format!("let {name} = {}", self.fn_literal(&names, &e.code, false)), frame)
        } else {
            let may_raise = !self.errors.is_empty() && self.rng.chance(15);
            let (body, frame) = self.fn_body(ctx, &params, &[], Ret::One(Ty::Int, 0.0), 2, INT_CAP, may_raise);
            (format!("let {name} = fn({plist})\n{body}\nend"), frame)
        };
        let f = Func { name: name.clone(), params, rest: false, opts: Vec::new(), ret: frame.ret, cost: frame.cost + 1.0, raises: frame.raises, assigns: frame.assigns, block: None, kind: FnKind::Let, typed: false };
        let mut out = text;
        if let Some(call) = self.call_text(ctx, &f) {
            self.note_call(&f, ctx);
            let line = format!("print(\"{name}\", {} + 0)", call.code);
            if f.raises && !ctx.raise_ok {
                // It may raise and nothing outside would catch it.
                let e = self.fresh("err");
                out.push_str(&format!("\ntry\n  {line}\ncatch {e}\n  print(\"caught\", typeof({e}))\nend"));
            } else {
                out.push('\n');
                out.push_str(&line);
            }
        }
        self.declare_fn(f);
        out
    }

    /// Closures made in a loop, each over its own iteration's variable
    /// (its own upvalue), called afterwards.
    fn closures_in_loop(&mut self, ctx: &Ctx) -> Option<String> {
        if !self.affordable(12.0 * ctx.mult) {
            return None;
        }
        let fs = self.fresh("fs");
        let i = self.fresh("i");
        let d = self.fresh("d");
        let g = self.fresh("g");
        let e = self.fresh("err");
        let k = self.rng.range(2, 9);
        let n = self.rng.range(2, 6);
        // The loop: a `for` over a range, or a `while` over a counter.
        let (head, step, end) = if self.rng.chance(70) {
            (format!("for {i} in 1..{n}"), String::new(), "end".to_string())
        } else {
            (format!("let {i} = 0\nwhile {i} < {n}"), format!("  {i} += 1\n"), "end".to_string())
        };
        let mut body = step;
        // A local of the iteration, maybe changed after it is captured:
        // the closure sees the change (one upvalue).
        let captured = if self.rng.chance(60) {
            body.push_str(&format!("  let {d} = {i} * {k}\n"));
            d.clone()
        } else {
            i.clone()
        };
        if self.rng.chance(40) {
            body.push_str(&format!("  continue if {i} == {}\n", self.rng.range(1, n)));
        }
        let insert = format!("Table.insert({fs}, fn() {captured} + {i} end)");
        if self.rng.chance(25) {
            // Leaving the iteration by an error closes its upvalues too.
            body.push_str(&format!("  try\n    {insert}\n    raise \"stop\" if {i} == {}\n  catch {e}\n    print(\"caught at\", {i})\n  end\n", self.rng.range(1, n)));
        } else {
            body.push_str(&format!("  {insert}\n"));
        }
        if captured == d && self.rng.chance(40) {
            body.push_str(&format!("  {d} = {d} * 10\n"));
        }
        if self.rng.chance(40) {
            body.push_str(&format!("  break if {i} == {}\n", self.rng.range(1, n)));
        }
        let call = match self.rng.below(3) {
            0 => format!("for {g} in {fs}\n  print({g}())\nend"),
            1 => format!("print({fs}.len)\nfor {g} in {fs}\n  print(\"#{{{g}() + 0}}\")\nend"),
            _ => format!("for {g}, {d}x in ipairs({fs})\n  print({g}, {d}x())\nend"),
        };
        self.add_cost(12.0 * ctx.mult);
        Some(format!("let {fs} = []\n{head}\n{body}{end}\n{call}"))
    }

    fn recursive_fn(&mut self, _ctx: &Ctx) -> String {
        let name = self.fresh("rec");
        let n = self.fresh("n");
        let k = self.rng.range(1, 7);
        let body = match self.rng.below(2) {
            0 => format!("  if {n} <= 0\n    return 1\n  end\n  ({n} * {k} + {name}({n} - 1)) % 1000"),
            _ => format!("  return {k} if {n} < 1\n  ({name}({n} - 1) * 2 + {n}) % 1000"),
        };
        let text = format!("fn {name}({n})\n{body}\nend");
        self.declare_fn(Func {
            name,
            params: vec![Param { name: n, ty: Ty::Int, bound: 7.0, default: None }],
            rest: false,
            opts: Vec::new(),
            ret: Ret::One(Ty::Int, 1000.0),
            cost: 8.0,
            raises: false,
            assigns: Vec::new(),
            block: None,
            kind: FnKind::Decl,
            typed: false,
        });
        text
    }

    /// A failed runtime check caught on purpose. The program
    /// becomes checked-only: an unchecked build does not raise.
    pub(super) fn type_error_probe(&mut self, _ctx: &Ctx) -> String {
        self.checked_only = true;
        let bad = self.fresh("bad");
        let val = self.rng.pick(&["\"oops\"", "true", "[1]", "{a: 1}"]);
        let e = self.fresh("e");
        let mut out = format!("let {bad}: any = {val}\n");
        match self.rng.below(3) {
            0 => {
                let f = self.fresh("strict");
                let a = self.fresh("a");
                out.push_str(&format!("fn {f}({a}: Number): Number\n  {a} + 1\nend\ntry\n  print({f}({bad}))\ncatch {e}: TypeError\n  print(\"TypeError from {f}\")\nend"));
            }
            1 => {
                let t = self.fresh("t");
                out.push_str(&format!("let {t}: Number = 5\ntry\n  {t} = {bad}\ncatch {e}: TypeError\n  print(\"TypeError on assignment\")\nend\nprint({t})"));
            }
            _ => {
                let s = self.fresh("s");
                out.push_str(&format!("let {s} = try\n  let {{x: Number}} = {{x: {bad}}}\n  x\ncatch {e}: TypeError\n  \"TypeError in a pattern\"\nend\nprint({s})"));
            }
        }
        out
    }
}
