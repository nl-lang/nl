//! Less common declarations and the expressions that use them: methods
//! on `Number` and `String`, modules with functions and
//! constants, interfaces and their implementations,
//! generic functions, operators on structs beyond `+` (`<`, `==`,
//! `()`), constructors returning `Self`, and heredocs.

use super::*;

impl Gen {
    /// `fn Number.name(k)` or `fn String.name`, at file level.
    pub(super) fn prim_method_decl(&mut self) -> String {
        if self.rng.chance(55) {
            let name = self.fresh("pm");
            let k = self.fresh("k");
            let typed = self.rng.chance(40);
            let body = match self.rng.below(3) {
                0 => format!("(self * {k} + 1) % 1000"),
                1 => format!("(self + {k}) // 2"),
                _ => format!("if self > {k}\n    self - {k}\n  else\n    {k} - self\n  end"),
            };
            self.prim_num.push(name.clone());
            if typed { format!("fn Number.{name}({k}: Number): Number\n  {body}\nend") } else { format!("fn Number.{name}({k})\n  {body}\nend") }
        } else {
            let name = self.fresh("ps");
            let body = match self.rng.below(3) {
                0 => "self.upcase <> \"!\"".to_string(),
                1 => "\"<\" <> self.reverse <> \">\"".to_string(),
                _ => "self[1..3] or \"\"".to_string(),
            };
            self.prim_str.push(name.clone());
            format!("fn String.{name}\n  {body}\nend")
        }
    }

    /// A module with a constant and functions; its functions are methods
    /// of the module, reached as `Mod.f(x)`.
    pub(super) fn module_decl(&mut self) -> String {
        let name = self.fresh("Mod");
        let c = self.fresh("limit");
        let cv = self.rng.range(1, 50);
        let f1 = self.fresh("helper");
        let f2 = self.fresh("twice");
        let x = self.fresh("x");
        let text = format!(
            "module {name}\n  const {c} = {cv}\n  fn {f1}({x})\n    ({x} * 3 + {c}) % 1000\n  end\n  fn {f2}({x})\n    (Self.{f1}({x}) + {name}.{f1}(1)) % 1000\n  end\nend"
        );
        self.modules.push((name.clone(), c, cv));
        for f in [f1, f2] {
            self.declare_fn(Func {
                name: format!("{name}.{f}"),
                params: vec![Param { name: x.clone(), ty: Ty::Int, bound: 1000.0, default: None }],
                rest: false,
                opts: Vec::new(),
                ret: Ret::One(Ty::Int, 1000.0),
                cost: 2.0,
                raises: false,
                assigns: Vec::new(),
                block: None,
                kind: FnKind::Module,
                typed: false,
            });
        }
        text
    }

    /// An interface with one method, implemented for some structs and
    /// sometimes for `Number`.
    pub(super) fn iface_decl(&mut self) -> Vec<String> {
        let name = self.fresh("Show");
        let m = self.fresh("describe");
        let mut out = vec![format!("interface {name}\n  fn {m}: String\nend")];
        let mut impls = Vec::new();
        for i in 0..self.structs.len() {
            if !self.rng.chance(50) {
                continue;
            }
            let sname = self.structs.get(i).map(|s| s.name.clone()).unwrap_or_default();
            let fields = self.all_fields(i);
            let body = match fields.first() {
                Some(f) if f.ty == Ty::Int => format!("\"{sname}:#{{@{} + 0}}\"", f.name),
                Some(f) => format!("\"{sname}:#{{@{}}}\"", f.name),
                None => format!("\"{sname}\""),
            };
            out.push(format!("interface {name} for {sname}\n  fn {m}: String\n    {body}\n  end\nend"));
            impls.push(i);
        }
        let number = self.rng.chance(30);
        if number {
            out.push(format!("interface {name} for Number\n  fn {m}: String\n    \"n#{{self + 0}}\"\n  end\nend"));
        }
        self.ifaces.push(Iface { name, method: m, impls, number });
        out
    }

    /// `fn id<T>(x: T): T`, or a choice between two values of one type.
    pub(super) fn generic_decl(&mut self) -> String {
        let name = self.fresh("pick");
        let a = self.fresh("a");
        let b = self.fresh("b");
        let text = if self.rng.chance(50) {
            format!("fn {name}<T>({a}: T, {b}: T): T\n  {a}\nend")
        } else {
            format!("fn {name}<T>({a}: T, {b}: T): T\n  if {a} == {b}\n    return {b}\n  end\n  {a}\nend")
        };
        self.generics.push(name);
        text
    }

    /// Operators and a constructor for struct `i`: `<`, `==`, `()` and
    /// `make` returning `Self`.
    pub(super) fn struct_extras(&mut self, i: usize) -> Vec<String> {
        let Some(s) = self.structs.get(i).cloned() else { return Vec::new() };
        let fields = self.all_fields(i);
        let ints: Vec<Field> = fields.iter().filter(|f| f.ty == Ty::Int).cloned().collect();
        let mut out = Vec::new();
        if let Some(f) = ints.first() {
            if self.rng.chance(30) {
                out.push(format!("fn {}.<(other)\n  @{} < other.{}\nend", s.name, f.name, f.name));
                out.push(format!("fn {}.<=(other)\n  @{} <= other.{}\nend", s.name, f.name, f.name));
                if let Some(d) = self.structs.get_mut(i) {
                    d.lt = true;
                }
            }
            if self.rng.chance(25) {
                out.push(format!("fn {}.==(other)\n  @{} == other.{}\nend", s.name, f.name, f.name));
                if let Some(d) = self.structs.get_mut(i) {
                    d.eq = true;
                }
            }
            if self.rng.chance(25) {
                let k = self.fresh("k");
                out.push(format!("fn {}.()({k})\n  (@{} + {k}) % 1000\nend", s.name, f.name));
                if let Some(d) = self.structs.get_mut(i) {
                    d.call_op = true;
                }
            }
        }
        if self.rng.chance(30) {
            let m = self.fresh("make");
            let k = self.fresh("k");
            let inits: Vec<String> = fields
                .iter()
                .map(|f| match f.ty {
                    Ty::Str => format!("{}: \"made\"", f.name),
                    _ => format!("{}: {k} % {}", f.name, bound_lit(f.bound)),
                })
                .collect();
            out.push(format!("fn {}.{m}({k}): Self\n  Self {{ {} }}\nend", s.name, inits.join(", ")));
            if let Some(d) = self.structs.get_mut(i) {
                d.make = Some(m);
            }
        }
        out
    }

    /// An integer from one of the declarations above, if there is one.
    pub(super) fn extra_int(&mut self, ctx: &Ctx, d: u32) -> Option<Ex> {
        match self.rng.below(5) {
            0 => {
                let m = self.rng.pick_ref(&self.prim_num.clone())?;
                let recv = self.int_within(ctx, d.saturating_sub(1), 1000.0);
                let k = self.int_within(ctx, d.saturating_sub(1), 1000.0);
                // `7.m(k)`, `x.m(k)`, or the plain function `Number.m!(x, k)`.
                let code = if self.rng.chance(25) { format!("(Number.{m}!)({}, {})", recv.code, k.code) } else { format!("{}.{m}({})", recv.operand(), k.code) };
                Some(Ex::new(code, Ty::Int, 1000.0).atom())
            }
            1 => {
                let (name, c, _) = self.rng.pick_ref(&self.modules.clone())?;
                Some(Ex::new(format!("{name}.{c}"), Ty::Int, 50.0).atom())
            }
            2 => {
                let g = self.rng.pick_ref(&self.generics.clone())?;
                let a = self.int(ctx, d.saturating_sub(1));
                let b = self.int(ctx, d.saturating_sub(1));
                let bound = a.bound.max(b.bound);
                Some(Ex::new(format!("{g}({}, {})", a.code, b.code), Ty::Int, bound).atom())
            }
            3 => {
                let cands: Vec<usize> = (0..self.structs.len()).filter(|&i| self.structs.get(i).is_some_and(|s| s.call_op)).collect();
                let i = self.rng.pick_ref(&cands)?;
                let v = self.pick_var(Ty::Struct(i), false)?;
                let k = self.int_within(ctx, d.saturating_sub(1), 1000.0);
                Some(Ex::new(format!("{}({})", v.name, k.code), Ty::Int, 1000.0).atom())
            }
            _ => {
                let cands: Vec<usize> = (0..self.structs.len()).filter(|&i| self.structs.get(i).is_some_and(|s| s.make.is_some())).collect();
                let i = self.rng.pick_ref(&cands)?;
                let s = self.structs.get(i)?.clone();
                let f = self.all_fields(i).into_iter().find(|f| f.ty == Ty::Int)?;
                let k = self.int_within(ctx, d.saturating_sub(1), 1000.0);
                Some(Ex::new(format!("{}.{}({}).{}", s.name, s.make?, k.code, f.name), Ty::Int, f.bound).atom())
            }
        }
    }

    /// A string from one of the declarations above, if there is one.
    pub(super) fn extra_str(&mut self, ctx: &Ctx, d: u32) -> Option<Ex> {
        match self.rng.below(3) {
            0 => {
                let m = self.rng.pick_ref(&self.prim_str.clone())?;
                let s = self.str_(ctx, d.saturating_sub(1));
                let code = if self.rng.chance(25) { format!("(String.{m}!)({})", s.code) } else { format!("{}.{m}", s.operand()) };
                Some(Ex::new(code, Ty::Str, s.bound + 2.0).ascii(s.ascii).atom())
            }
            1 => {
                let ifc = self.rng.pick_ref(&self.ifaces.clone())?;
                if ifc.number && self.rng.chance(40) {
                    let n = self.int_within(ctx, d.saturating_sub(1), 1000.0);
                    return Some(Ex::new(format!("{}.{}", n.operand(), ifc.method), Ty::Str, 12.0).atom());
                }
                let i = self.rng.pick_ref(&ifc.impls)?;
                // A child of an implementing struct has the method too.
                let v = self.pick_var(Ty::Struct(i), false)?;
                Some(Ex::new(format!("{}.{}", v.name, ifc.method), Ty::Str, 30.0).atom())
            }
            _ => {
                let g = self.rng.pick_ref(&self.generics.clone())?;
                let a = self.str_(ctx, d.saturating_sub(1));
                let b = self.str_(ctx, d.saturating_sub(1));
                Some(Ex::new(format!("{g}({}, {})", a.code, b.code), Ty::Str, a.bound.max(b.bound)).ascii(a.ascii && b.ascii).atom())
            }
        }
    }

    /// A comparison of two struct values or an interface test.
    pub(super) fn extra_bool(&mut self, ctx: &Ctx, d: u32) -> Option<Ex> {
        if self.rng.chance(35) {
            let ifc = self.rng.pick_ref(&self.ifaces.clone())?;
            // A literal, not a variable: `x is I` narrows a variable to
            // `I` alone when no implementation is in sight (reported).
            let i = self.rng.below(self.structs.len().max(1));
            let (code, _) = match self.struct_lit(ctx, i, d.saturating_sub(1)) {
                Some((e, i)) => (e.operand(), i),
                None => (self.int_lit().operand(), 0),
            };
            return Some(Ex::new(format!("{code} is {}", ifc.name), Ty::Bool, 1.0));
        }
        let cands: Vec<usize> = (0..self.structs.len()).filter(|&i| self.structs.get(i).is_some_and(|s| s.lt || s.eq)).collect();
        let i = self.rng.pick_ref(&cands)?;
        let s = self.structs.get(i)?.clone();
        let (a, _) = self.struct_lit_or_var(ctx, i)?;
        let (b, _) = self.struct_lit_or_var(ctx, i)?;
        let mut ops: Vec<&str> = Vec::new();
        if s.lt {
            ops.extend(["<", "<=", ">", ">="]);
        }
        if s.eq {
            ops.extend(["==", "!="]);
        }
        let op = self.rng.pick(&ops);
        Some(Ex::new(format!("{} {op} {}", a.operand(), b.operand()), Ty::Bool, 1.0))
    }

    fn struct_lit_or_var(&mut self, ctx: &Ctx, i: usize) -> Option<(Ex, usize)> {
        if self.rng.chance(60) {
            if let Some(v) = self.pick_var(Ty::Struct(i), false) {
                return Some((Ex::new(v.name, Ty::Struct(i), 0.0).atom(), i));
            }
        }
        self.struct_lit(ctx, i, 0)
    }

    /// A struct value from `S.make(k)`.
    pub(super) fn made_struct(&mut self, ctx: &Ctx) -> Option<(Ex, usize)> {
        let cands: Vec<usize> = (0..self.structs.len()).filter(|&i| self.structs.get(i).is_some_and(|s| s.make.is_some())).collect();
        let i = self.rng.pick_ref(&cands)?;
        let s = self.structs.get(i)?.clone();
        let k = self.int_within(ctx, 1, 1000.0);
        Some((Ex::new(format!("{}.{}({})", s.name, s.make?, k.code), Ty::Struct(i), 0.0).atom(), i))
    }

    /// A dedented heredoc (`"""-`) with an interpolation: its
    /// lines are indented with the code around it, and the dedent takes
    /// that back off.
    pub(super) fn heredoc(&mut self, ctx: &Ctx) -> Ex {
        let n = 1 + self.rng.below(2);
        let mut lines = Vec::new();
        for _ in 0..n {
            let word = self.rng.pick(&["alpha", "two words", "x = 1", "tail", "#not a comment"]);
            if self.rng.chance(50) {
                let e = self.int(ctx, 1);
                lines.push(format!("  {word} #{{{} + 0}}", e.operand()));
            } else {
                lines.push(format!("  {word}"));
            }
        }
        let extra = if self.rng.chance(30) { "    deeper\n" } else { "" };
        Ex::new(format!("\"\"\"-\n{}\n{extra}  \"\"\"", lines.join("\n")), Ty::Str, 40.0 * n as f64).atom()
    }
}
