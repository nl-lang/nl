//! Statements of generated programs: declarations, assignment
//! and compound assignment, conditionals and modifiers,
//! `case` with value and pattern items, loops with `break` and
//! `continue`, `try`/`catch`/`finally` with `raise`,
//! calls with trailing blocks, and scope blocks.
//!
//! A statement that is not the last of its block must not discard a
//! value, so the branches of a statement-level construct end
//! in statements, never in bare values. Loops keep the rules of the
//! module above: small literal limits, counters changed first, frozen
//! collections while they are walked.

use super::*;

impl Gen {
    /// A block of up to `n` statements, then a tail of the type when one
    /// is asked for. Returns the indented text and the tail.
    pub(super) fn block(&mut self, ctx: &Ctx, n: usize, tail: Option<Ty>) -> (String, Option<Ex>) {
        self.push_scope();
        let count = self.rng.below(n + 1);
        let mut stmts: Vec<String> = Vec::new();
        for _ in 0..count {
            stmts.push(self.stmt(ctx));
        }
        let t = tail.map(|ty| self.tail_expr(ctx, ty));
        let mut unused = self.pop_scope(ctx);
        stmts.append(&mut unused);
        if let Some(e) = &t {
            stmts.push(e.code.clone());
        }
        (block_text(&self.join_some(stmts)), t)
    }

    /// Joins some neighbouring one-line statements with `;`.
    fn join_some(&mut self, stmts: Vec<String>) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for s in stmts {
            let joinable = !s.contains('\n') && self.rng.chance(8);
            match out.last_mut() {
                Some(prev) if joinable && !prev.contains('\n') && !prev.starts_with('#') => {
                    prev.push_str("; ");
                    prev.push_str(&s);
                }
                _ => out.push(s),
            }
        }
        out
    }

    /// One statement.
    pub(super) fn stmt(&mut self, ctx: &Ctx) -> String {
        let deep = ctx.depth >= 3;
        let in_fn = !self.frames.is_empty();
        let weights = [
            20,                                            // 0 let
            14,                                            // 1 print
            14,                                            // 2 assignment
            if deep { 0 } else { 8 },                      // 3 if / unless
            5,                                             // 4 modifier
            if deep || ctx.loops >= 2 { 0 } else { 9 },    // 5 loop
            if ctx.loops > 0 { 5 } else { 0 },             // 6 break / continue
            if deep { 0 } else { 4 },                      // 7 case
            if deep { 0 } else { 5 },                      // 8 try
            7,                                             // 9 call
            if ctx.depth == 1 || ctx.depth == 2 { 2 } else { 0 }, // 10 nested fn
            if ctx.depth <= 1 { 2 } else { 0 },            // 11 classify
            if deep { 0 } else { 3 },                      // 12 if let
            2,                                             // 13 multiple assignment
            if deep { 0 } else { 2 },                      // 14 do block
            if in_fn { 3 } else { 0 },                     // 15 return
            if ctx.raise_ok { 4 } else { 0 },              // 16 raise
            if ctx.raise_ok { 2 } else { 0 },              // 17 match or conversion error
            5,                                             // 18 list and map changes
            if self.structs.is_empty() { 0 } else { 3 },   // 19 struct changes
            2,                                             // 20 let with several names
        ];
        let s = match self.rng.weighted(&weights) {
            0 => self.let_stmt(ctx),
            1 => self.print_stmt(ctx),
            2 => self.assign_stmt(ctx),
            3 => self.if_stmt(ctx),
            4 => self.modifier_stmt(ctx),
            5 => self.loop_stmt(ctx),
            6 => self.break_stmt(ctx),
            7 => self.case_stmt(ctx),
            8 => self.try_stmt(ctx),
            9 => self.call_stmt(ctx),
            10 => self.fn_decl(ctx),
            11 => self.classify(ctx),
            12 => self.if_let(ctx),
            13 => self.multi_assign(ctx),
            14 => {
                let (b, _) = self.block(&ctx.nested(), 3, None);
                Some(format!("do\n{b}\nend"))
            }
            15 => self.return_stmt(ctx),
            16 => self.raise_stmt(ctx),
            17 => self.error_probe(ctx),
            18 => self.collection_stmt(ctx),
            19 => self.struct_stmt(ctx),
            _ => self.multi_let(ctx),
        };
        let s = s.unwrap_or_else(|| self.print_stmt(ctx).unwrap_or_else(|| "print(0)".into()));
        if self.rng.chance(3) { format!("# step {}\n{s}", self.names) } else { s }
    }

    fn random_ty(&mut self) -> Ty {
        let s = self.structs.len();
        let e = self.enums.len();
        match self.rng.weighted(&[30, 10, 20, 8, 8, 5, 6, if s > 0 { 6 } else { 0 }, if e > 0 { 5 } else { 0 }]) {
            0 => Ty::Int,
            1 => Ty::Num,
            2 => Ty::Str,
            3 => Ty::Bool,
            4 => Ty::IntList,
            5 => Ty::StrList,
            6 => Ty::Map,
            7 => Ty::Struct(self.rng.below(s)),
            _ => Ty::Enum(self.rng.below(e)),
        }
    }

    fn annotation(&mut self, ty: Ty) -> String {
        match ty {
            Ty::Int | Ty::Num => "Number".into(),
            Ty::Str => "String".into(),
            Ty::Bool => "Bool".into(),
            Ty::IntList => self.rng.pick(&["[Number]", "Table"]).into(),
            Ty::StrList => self.rng.pick(&["[String]", "Table"]).into(),
            Ty::Map => self.rng.pick(&["{[String]: Number}", "Table"]).into(),
            Ty::Struct(i) => self.structs.get(i).map(|s| s.name.clone()).unwrap_or_default(),
            Ty::Enum(i) => self.enums.get(i).map(|e| e.name.clone()).unwrap_or_default(),
        }
    }

    fn let_stmt(&mut self, ctx: &Ctx) -> Option<String> {
        let ty = self.random_ty();
        let (e, ty) = match ty {
            Ty::Struct(_) => {
                let (s, i) = self.struct_val(ctx, 2)?;
                (s, Ty::Struct(i))
            }
            Ty::Enum(_) => {
                let (v, i) = self.enum_val(ctx)?;
                (v, Ty::Enum(i))
            }
            Ty::Str if self.rng.chance(8) => (self.heredoc(ctx), ty),
            _ if self.rng.chance(15) => (self.rich(ctx, ty).unwrap_or_else(|| self.expr(ctx, ty, 3)), ty),
            _ => (self.expr(ctx, ty, 3), ty),
        };
        let prefix = match ty {
            Ty::Int | Ty::Num | Ty::Str | Ty::Bool => "v",
            Ty::IntList | Ty::StrList => "l",
            Ty::Map => "m",
            Ty::Struct(_) => "s",
            Ty::Enum(_) => "e",
        };
        let name = self.fresh(prefix);
        let is_const = self.rng.chance(12);
        let ann = if self.rng.chance(30) { format!(": {}", self.annotation(ty)) } else { String::new() };
        let bound = match ty {
            Ty::Int | Ty::Num => e.bound.max(self.rng.pick(&[100.0, 1000.0, 100_000.0, BIG])),
            Ty::Str => STR_CAP,
            Ty::IntList | Ty::StrList => e.bound.clamp(4.0, LIST_CAP),
            _ => e.bound,
        };
        let elem = match ty {
            Ty::IntList | Ty::Map => e.elem.max(1000.0),
            Ty::StrList => STR_CAP,
            _ => 0.0,
        };
        self.add_cost(ctx.mult * 0.2);
        let kw = if is_const { "const" } else { "let" };
        let typed = !ann.is_empty();
        let init = if typed && ty == Ty::Str { non_nil(&e) } else { e.code.clone() };
        let text = format!("{kw} {name}{ann} = {init}");
        self.declare(Var { name, ty, bound, elem, ascii: e.ascii, min_len: e.min_len, mutable: !is_const, used: false, fn_depth: self.fn_depth(), typed });
        Some(text)
    }

    fn print_stmt(&mut self, ctx: &Ctx) -> Option<String> {
        self.add_cost(ctx.mult);
        if self.rng.chance(10) {
            // All the values of a call that returns two.
            let fs = self.callable(ctx, |f| matches!(f.ret, Ret::Pair(_)));
            if let Some(f) = self.rng.pick_ref(&fs) {
                let call = self.call_text(ctx, &f)?;
                self.note_call(&f, ctx);
                // A final call passes on all its values; an
                // operand keeps the first.
                return Some(if self.rng.chance(70) { format!("print(show_pair({}))", call.code) } else { format!("print({} + 0, \"first only\")", call.code) });
            }
        }
        if self.rng.chance(6) {
            // Literals at the edges of what a double holds: each backend
            // encodes constants its own way.
            let n = 1 + self.rng.below(3);
            // `%.17g` shows every bit of the double, which `print`'s
            // `%.14g` would round away.
            let mut lits: Vec<String> = Vec::new();
            for _ in 0..n {
                let l = self.rng.pick(NUMBER_ZOO);
                lits.push(if self.rng.chance(50) { format!("\"%.17g\".format({l})") } else { l.to_string() });
            }
            return Some(format!("print({})", lits.join(", ")));
        }
        let n = 1 + self.rng.below(3);
        let mut parts = Vec::new();
        for _ in 0..n {
            let ty = self.random_ty();
            let e = self.expr(ctx, ty, 3);
            if ty == Ty::Num && self.rng.chance(35) {
                // Every bit of a computed double: a constant folded by
                // one backend and computed by the VM must agree exactly.
                parts.push(format!("\"%.17g\".format({} + 0)", e.operand()));
                continue;
            }
            parts.push(self.show(&e.code, e.ty));
        }
        if self.rng.chance(30) {
            parts.insert(0, format!("\"{}\"", self.fresh("at")));
        }
        let first = parts.first().cloned().unwrap_or_default();
        // The command form needs an argument that cannot continue the
        // callee (`print (x)` is `print(x)`).
        let command = parts.len() == 1 && !first.starts_with(['(', '[', '-', '{']) && self.rng.chance(40);
        Some(if command { format!("print {first}") } else { format!("print({})", parts.join(", ")) })
    }

    /// An assignment that keeps the variable within its bound.
    fn assign_stmt(&mut self, ctx: &Ctx) -> Option<String> {
        let ty = self.random_ty();
        let v = self.pick_var(ty, true)?;
        let n = v.name.clone();
        match ty {
            Ty::Int => {
                if self.rng.chance(45) {
                    let e = self.int_within(ctx, 3, v.bound);
                    return Some(format!("{n} = {}", e.code));
                }
                let (op, rhs, nb) = match self.rng.below(9) {
                    0 | 1 => {
                        let e = self.int(ctx, 2);
                        let op = self.rng.pick(&["+=", "-="]);
                        (op, e.code, v.bound + e.bound)
                    }
                    2 => {
                        let e = self.int_within(ctx, 2, 1000.0);
                        if v.bound * e.bound > MUL_CAP {
                            // A modulus is always a nonzero literal.
                            let k = self.rng.range(2, 999);
                            ("%=", k.to_string(), k as f64)
                        } else {
                            ("*=", e.code, v.bound * e.bound)
                        }
                    }
                    3 => {
                        let k = self.rng.range(1, 9);
                        ("//=", k.to_string(), v.bound)
                    }
                    4 => {
                        let k = self.rng.range(1, 50);
                        ("%=", k.to_string(), k as f64)
                    }
                    5 => {
                        let e = self.int(ctx, 2);
                        let op = self.rng.pick(&["|=", "&=", "^="]);
                        (op, e.code, BIT_BOUND)
                    }
                    6 => {
                        let k = self.rng.range(0, 31);
                        let op = self.rng.pick(&["<<=", ">>=", ">>>="]);
                        (op, k.to_string(), BIT_BOUND)
                    }
                    7 if v.bound <= 1.0e5 => ("**=", "2".to_string(), v.bound * v.bound),
                    _ => {
                        let e = self.int(ctx, 1);
                        ("+=", e.code, v.bound + e.bound)
                    }
                };
                let mut s = format!("{n} {op} {rhs}");
                if nb > v.bound {
                    s.push_str(&format!("\n{n} %= {}", bound_lit(v.bound)));
                }
                Some(s)
            }
            Ty::Num => {
                if self.rng.chance(50) {
                    let e = self.num(ctx, 3);
                    let e = fit(e, v.bound);
                    return Some(format!("{n} = {}", e.code));
                }
                let (op, rhs, nb) = match self.rng.below(4) {
                    0 => {
                        let e = self.num(ctx, 2);
                        let op = self.rng.pick(&["+=", "-="]);
                        (op, e.code, v.bound + e.bound)
                    }
                    1 => ("/=", self.rng.pick(&["2", "4", "0.5", "3"]).to_string(), v.bound * 2.0),
                    2 => ("%=", self.rng.pick(&["2.5", "7", "0.75"]).to_string(), 7.0),
                    _ => ("*=", self.rng.pick(&["1.5", "0.5", "3"]).to_string(), v.bound * 3.0),
                };
                let mut s = format!("{n} {op} {rhs}");
                if nb > v.bound {
                    s.push_str(&format!("\n{n} %= {}", bound_lit(v.bound)));
                }
                Some(s)
            }
            Ty::Str => {
                let e = self.str_(ctx, 2);
                if v.ascii && !e.ascii {
                    return Some(format!("{n} = {}", self.rng.pick(&["\"reset\"", "\"\"", "\"b\""])));
                }
                if v.typed {
                    // `s[1..n]` is `String?` to the analyzer.
                    Some(format!("{n} = {}", non_nil(&e)))
                } else if self.rng.chance(40) {
                    Some(format!("{n} <>= {}\n{n} = {n}[1..{}]", e.code, STR_CAP as i64))
                } else {
                    Some(format!("{n} = {}", e.code))
                }
            }
            Ty::Bool => {
                let e = self.bool_(ctx, 2);
                let op = self.rng.pick(&["=", "=", "||=", "&&="]);
                Some(format!("{n} {op} {}", e.code))
            }
            Ty::IntList => {
                let e = self.int_list_lit(ctx, 1);
                self.update_var(&n, |var| var.min_len = var.min_len.min(e.min_len));
                Some(format!("{n} = {}", e.code))
            }
            Ty::StrList => {
                let e = self.str_list_lit(ctx, 1);
                self.update_var(&n, |var| var.min_len = var.min_len.min(e.min_len));
                Some(format!("{n} = {}", e.code))
            }
            Ty::Map => {
                let e = self.map_lit(ctx, 1);
                Some(format!("{n} = {}", e.code))
            }
            Ty::Struct(i) => {
                let (e, _) = self.struct_lit(ctx, i, 1)?;
                Some(format!("{n} = {}", e.code))
            }
            Ty::Enum(i) => {
                let en = self.enums.get(i)?.clone();
                let (vn, _) = en.variants.get(self.rng.below(en.variants.len()))?;
                Some(format!("{n} = {}.{vn}", en.name))
            }
        }
    }

    fn if_stmt(&mut self, ctx: &Ctx) -> Option<String> {
        let inner = ctx.nested();
        let c = self.bool_(ctx, 2);
        if self.rng.chance(20) {
            let (b1, _) = self.block(&inner, 3, None);
            let mut s = format!("unless {}\n{b1}\n", c.code);
            if self.rng.chance(40) {
                let (b2, _) = self.block(&inner, 2, None);
                s.push_str(&format!("else\n{b2}\n"));
            }
            s.push_str("end");
            return Some(s);
        }
        let (b1, _) = self.block(&inner, 3, None);
        let mut s = format!("if {}\n{b1}\n", c.code);
        for _ in 0..self.rng.below(3) {
            let c2 = self.bool_(ctx, 2);
            let (b, _) = self.block(&inner, 2, None);
            s.push_str(&format!("elseif {}\n{b}\n", c2.code));
        }
        if self.rng.chance(50) {
            let (b, _) = self.block(&inner, 2, None);
            s.push_str(&format!("else\n{b}\n"));
        }
        s.push_str("end");
        Some(s)
    }

    /// A simple statement with `if` or `unless` after it, or
    /// a counter driven by a `while`/`until` modifier.
    fn modifier_stmt(&mut self, ctx: &Ctx) -> Option<String> {
        if self.rng.chance(15) && self.affordable(8.0 * ctx.mult) {
            let w = self.fresh("w");
            let k = self.rng.range(1, 7);
            self.add_cost(k as f64 * ctx.mult);
            self.declare(Var { name: w.clone(), ty: Ty::Int, bound: 10.0, mutable: true, used: true, fn_depth: self.fn_depth(), ..Var::default() });
            return Some(if self.rng.chance(50) { format!("let {w} = 0\n{w} += 1 while {w} < {k}\nprint(\"{w}\", {w})") } else { format!("let {w} = 0\n{w} += 2 until {w} >= {k}\nprint(\"{w}\", {w})") });
        }
        let c = self.bool_(ctx, 2);
        let kw = self.rng.pick(&["if", "unless"]);
        let s = match self.rng.below(3) {
            0 => self.print_stmt(ctx)?,
            1 => {
                let ty = self.rng.pick(&[Ty::Int, Ty::Str, Ty::Bool]);
                let v = self.pick_var(ty, true)?;
                let e = match ty {
                    Ty::Int => self.int_within(ctx, 2, v.bound),
                    Ty::Str => {
                        let e = self.str_(ctx, 2);
                        let e = if v.ascii && !e.ascii { Ex::new("\"m\"", Ty::Str, 1.0) } else { e };
                        if v.typed { Ex::new(non_nil(&e), Ty::Str, e.bound) } else { e }
                    }
                    _ => self.bool_(ctx, 2),
                };
                format!("{} = {}", v.name, e.code)
            }
            _ => self.call_stmt_simple(ctx)?,
        };
        Some(format!("{s} {kw} {}", c.code))
    }

    fn loop_stmt(&mut self, ctx: &Ctx) -> Option<String> {
        match self.rng.weighted(&[30, 6, 18, 8, 8, 10, 8, 6, 5]) {
            0 => self.for_range(ctx),
            1 => self.for_range_var(ctx),
            2 => self.for_list(ctx),
            3 => self.for_chars(ctx),
            4 => self.for_map(ctx),
            5 | 6 => self.while_loop(ctx),
            7 => self.loop_loop(ctx),
            _ => self.for_pattern(ctx),
        }
    }

    /// Declares a loop variable for the body: never assigned.
    fn loop_var(&mut self, name: &str, ty: Ty, bound: f64, ascii: bool) {
        self.declare(Var { name: name.to_string(), ty, bound, ascii, mutable: false, used: false, fn_depth: self.fn_depth(), elem: 0.0, min_len: 0, typed: false });
    }

    /// The body of a loop that runs at most `iters` times, in its own
    /// scope with the loop variables already declared by `declare`.
    fn loop_body(&mut self, ctx: &Ctx, iters: f64, declare: impl FnOnce(&mut Gen), first: Option<String>) -> String {
        let inner = ctx.in_loop(iters);
        self.add_cost(iters * ctx.mult);
        self.push_scope();
        declare(self);
        let mut stmts: Vec<String> = first.into_iter().collect();
        let n = 1 + self.rng.below(3);
        for _ in 0..n {
            stmts.push(self.stmt(&inner));
        }
        let mut unused = self.pop_scope(&inner);
        stmts.append(&mut unused);
        block_text(&self.join_some(stmts))
    }

    fn for_range(&mut self, ctx: &Ctx) -> Option<String> {
        let i = self.fresh("i");
        let (header, iters, ty, bound) = match self.rng.weighted(&[40, 15, 15, 10, 10, 10]) {
            0 => {
                let a = self.rng.range(-2, 3);
                let b = a + self.rng.range(-1, 5);
                (format!("{a}..{b}"), (b - a + 1).max(0) as f64, Ty::Int, a.abs().max(b.abs()) as f64)
            }
            1 => {
                let a = self.rng.range(0, 3);
                let b = a + self.rng.range(0, 5);
                (format!("{a}...{b}"), (b - a).max(0) as f64, Ty::Int, b.abs() as f64)
            }
            2 => {
                let a = self.rng.range(5, 9);
                let b = self.rng.range(-2, 2);
                let s = self.rng.range(1, 3);
                (format!("{a}..{b} step -{s}"), ((a - b) / s + 1) as f64, Ty::Int, a as f64)
            }
            3 => {
                let a = self.rng.range(0, 2);
                let b = a + self.rng.range(2, 9);
                let s = self.rng.range(2, 3);
                (format!("({a}..{b}) step {s}"), ((b - a) / s + 1) as f64, Ty::Int, b as f64)
            }
            4 => {
                let e = self.int_atom_pub(ctx);
                (format!("1..({} % 4)", e.operand()), 3.0, Ty::Int, 3.0)
            }
            _ => {
                let b = self.rng.pick(&["1", "2", "1.5"]);
                let s = self.rng.pick(&["0.5", "0.25", "0.75"]);
                (format!("0..{b} step {s}"), 9.0, Ty::Num, 2.0)
            }
        };
        if !self.affordable(iters * ctx.mult * 2.0) || ctx.mult * iters > 60.0 {
            return None;
        }
        let iname = i.clone();
        let body = self.loop_body(ctx, iters, move |g| g.loop_var(&iname, ty, bound, true), None);
        Some(format!("for {i} in {header}\n{body}\nend"))
    }

    /// A range held in a variable.
    fn for_range_var(&mut self, ctx: &Ctx) -> Option<String> {
        let r = self.fresh("r");
        let i = self.fresh("i");
        let a = self.rng.range(-1, 3);
        let b = a + self.rng.range(0, 4);
        let op = self.rng.pick(&["..", "..."]);
        let iters = (b - a + 1) as f64;
        if !self.affordable(iters * ctx.mult * 2.0) || ctx.mult * iters > 60.0 {
            return None;
        }
        let iname = i.clone();
        let bound = a.abs().max(b.abs()) as f64;
        let body = self.loop_body(ctx, iters, move |g| g.loop_var(&iname, Ty::Int, bound, true), None);
        Some(format!("let {r} = {a}{op}{b}\nfor {i} in {r}\n{body}\nend"))
    }

    fn for_list(&mut self, ctx: &Ctx) -> Option<String> {
        let int = self.rng.chance(65);
        let l = if int { self.int_list(ctx, 2) } else { self.str_list(ctx, 2) };
        let iters = l.bound.max(1.0);
        if !self.affordable(iters * ctx.mult * 2.0) || ctx.mult * iters > 60.0 {
            return None;
        }
        let v = self.fresh("v");
        let (ety, eb) = if int { (Ty::Int, l.elem.max(1.0)) } else { (Ty::Str, STR_CAP) };
        let ascii = false;
        // The collection must not grow while it is walked.
        let frozen = l.atomic && !l.code.starts_with('[') && !l.code.contains('.');
        if frozen {
            self.frozen.push(l.code.clone());
        }
        let text = if self.rng.chance(30) {
            let i = self.fresh("i");
            let (iname, vname) = (i.clone(), v.clone());
            let body = self.loop_body(ctx, iters, move |g| {
                g.loop_var(&iname, Ty::Int, LIST_CAP, true);
                g.loop_var(&vname, ety, eb, ascii);
            }, None);
            format!("for {i}, {v} in ipairs({})\n{body}\nend", l.code)
        } else {
            let vname = v.clone();
            let body = self.loop_body(ctx, iters, move |g| g.loop_var(&vname, ety, eb, ascii), None);
            format!("for {v} in {}\n{body}\nend", l.code)
        };
        if frozen {
            self.frozen.pop();
        }
        Some(text)
    }

    /// `for ch in s`: characters.
    fn for_chars(&mut self, ctx: &Ctx) -> Option<String> {
        let s = self.str_(ctx, 1);
        let iters = s.bound.max(1.0);
        if !self.affordable(iters * ctx.mult * 2.0) || ctx.mult * iters > 60.0 {
            return None;
        }
        let ch = self.fresh("ch");
        let cname = ch.clone();
        let ascii = s.ascii;
        let body = self.loop_body(ctx, iters, move |g| g.loop_var(&cname, Ty::Str, 1.0, ascii), None);
        Some(format!("for {ch} in {}\n{body}\nend", s.code))
    }

    /// A map walked through its sorted keys.
    fn for_map(&mut self, ctx: &Ctx) -> Option<String> {
        let m = self.map_(ctx, 1);
        let iters = (MAP_KEY_COUNT as f64).max(1.0);
        if !self.affordable(iters * ctx.mult * 2.0) || ctx.mult * iters > 60.0 {
            return None;
        }
        let k = self.fresh("k");
        let val = self.fresh("val");
        let mcode = m.code.clone();
        let (kn, vn) = (k.clone(), val.clone());
        let first = format!("let {val} = {}[{k}] or 0", m.operand());
        let elem = m.elem;
        let body = self.loop_body(ctx, iters, move |g| {
            g.loop_var(&kn, Ty::Str, 8.0, true);
            g.declare(Var { name: vn, ty: Ty::Int, bound: elem, mutable: true, used: false, fn_depth: g.fn_depth(), ..Var::default() });
        }, Some(first));
        Some(format!("for {k} in sorted_keys({mcode})\n{body}\nend"))
    }

    /// `while` or `until` over a fresh counter, changed first in the body.
    fn while_loop(&mut self, ctx: &Ctx) -> Option<String> {
        let w = self.fresh("w");
        let k = self.rng.range(0, 5);
        if !self.affordable(k as f64 * ctx.mult * 2.0) || ctx.mult * k as f64 > 60.0 {
            return None;
        }
        let header = if self.rng.chance(60) { format!("while {w} < {k}") } else { format!("until {w} >= {k}") };
        let step = if self.rng.chance(70) { format!("{w} += 1") } else { format!("{w} = {w} + 1") };
        self.frozen.push(w.clone());
        // The counter is declared outside, so it stays readable after the
        // loop.
        self.declare(Var { name: w.clone(), ty: Ty::Int, bound: k.max(1) as f64, mutable: true, used: true, fn_depth: self.fn_depth(), ..Var::default() });
        let body = self.loop_body(ctx, k as f64, |_| {}, Some(step));
        self.frozen.retain(|x| x != &w);
        Some(format!("let {w} = 0\n{header}\n{body}\nend"))
    }

    /// `loop` with the exit test right after the counter, so that a
    /// `continue` cannot skip it.
    fn loop_loop(&mut self, ctx: &Ctx) -> Option<String> {
        let w = self.fresh("w");
        let k = self.rng.range(0, 5);
        if !self.affordable(k as f64 * ctx.mult * 2.0) || ctx.mult * k as f64 > 60.0 {
            return None;
        }
        self.frozen.push(w.clone());
        self.declare(Var { name: w.clone(), ty: Ty::Int, bound: (k + 1) as f64, mutable: true, used: true, fn_depth: self.fn_depth(), ..Var::default() });
        let body = self.loop_body(ctx, k as f64 + 1.0, |_| {}, Some(format!("{w} += 1\nbreak if {w} > {k}")));
        self.frozen.retain(|x| x != &w);
        Some(format!("let {w} = 0\nloop\n{body}\nend"))
    }

    /// `for {v, w: x} in [...]`: a pattern in a `for`.
    fn for_pattern(&mut self, ctx: &Ctx) -> Option<String> {
        let n = 1 + self.rng.below(3);
        if !self.affordable(n as f64 * ctx.mult * 2.0) || ctx.mult * n as f64 > 60.0 {
            return None;
        }
        let a = self.fresh("pa");
        let b = self.fresh("pb");
        let positional = self.rng.chance(40);
        let mut items = Vec::new();
        for _ in 0..n {
            let x = self.int_within(ctx, 1, 1000.0);
            let y = self.int_within(ctx, 1, 1000.0);
            items.push(if positional { format!("[{}, {}]", x.code, y.code) } else { format!("{{v: {}, w: {}}}", x.code, y.code) });
        }
        let pat = if positional { format!("[{a}, {b}]") } else { format!("{{v: {a}, w: {b}}}") };
        let (an, bn) = (a.clone(), b.clone());
        let body = self.loop_body(ctx, n as f64, move |g| {
            g.loop_var(&an, Ty::Int, 1000.0, true);
            g.loop_var(&bn, Ty::Int, 1000.0, true);
        }, None);
        Some(format!("for {pat} in [{}]\n{body}\nend", items.join(", ")))
    }

    fn break_stmt(&mut self, ctx: &Ctx) -> Option<String> {
        let c = self.bool_(ctx, 2);
        let kw = self.rng.pick(&["break", "continue"]);
        let m = self.rng.pick(&["if", "unless"]);
        Some(format!("{kw} {m} {}", c.code))
    }

    fn case_stmt(&mut self, ctx: &Ctx) -> Option<String> {
        let inner = ctx.nested();
        let (subject, arms) = self.case_arms(ctx)?;
        let mut s = format!("case {subject}\n");
        for arm in arms {
            let (b, _) = self.block(&inner, 2, None);
            let guard = if self.rng.chance(15) { format!(" if {}", self.bool_(ctx, 1).code) } else { String::new() };
            s.push_str(&format!("when {arm}{guard}\n{b}\n"));
        }
        if self.rng.chance(60) {
            let (b, _) = self.block(&inner, 2, None);
            s.push_str(&format!("else\n{b}\n"));
        }
        s.push_str("end");
        Some(s)
    }

    /// `try` as a statement.
    fn try_stmt(&mut self, ctx: &Ctx) -> Option<String> {
        self.try_expr(ctx, None).map(|(t, _, _)| t)
    }

    /// `try` with a body that may raise, `catch` clauses and a `finally`.
    /// With a type, every branch yields a value of it.
    /// Returns the text, the bound of the value and whether it is ASCII.
    pub(super) fn try_expr(&mut self, ctx: &Ctx, ty: Option<Ty>) -> Option<(String, f64, bool)> {
        if ctx.depth > 3 {
            return None;
        }
        let body_ctx = Ctx { depth: ctx.depth + 1, raise_ok: true, caught: true, ..ctx.clone() };
        let (body, e) = self.block(&body_ctx, 3, ty);
        let mut bound = e.as_ref().map_or(0.0, |e| e.bound);
        let mut ascii = e.as_ref().is_none_or(|e| e.ascii);
        let mut text = String::from("try\n");
        // A raise near the top of the body, so that the catches run.
        if self.rng.chance(60) {
            let r = self.raise_text(&body_ctx)?;
            text.push_str(&indent(&r));
            text.push('\n');
        }
        text.push_str(&body);
        text.push('\n');
        let inner = ctx.nested();
        let n_catch = if ty.is_some() { 1 + self.rng.below(2) } else { self.rng.below(3) };
        let mut catches = Vec::new();
        let mut last_catches_all = false;
        for _ in 0..n_catch {
            let e = self.fresh("err");
            let (head, report) = match self.rng.weighted(&[30, 20, 10, 10, 8, 8]) {
                0 if !self.errors.is_empty() => {
                    let t = self.rng.pick(&self.errors.clone());
                    (format!("catch {e}: {t}"), format!("print(\"caught {t}\", {e}.message, {e}.code)"))
                }
                1 => (format!("catch {e}"), format!("print(\"caught\", typeof({e}))\nprint({e}.message) if typeof({e}) == \"Exception\"")),
                2 => (format!("catch {e}: MatchError"), format!("print(\"MatchError\", typeof({e}))")),
                3 => (format!("catch {e}: ConversionError"), format!("print(\"ConversionError\", typeof({e}))")),
                4 => ("catch".to_string(), "print(\"caught something\")".to_string()),
                _ => (format!("catch {e}: Exception"), format!("print(\"Exception\", typeof({e}))")),
            };
            last_catches_all = head == "catch" || head == format!("catch {e}") || head.ends_with(": Exception");
            let (b, v) = self.block(&inner, 1, ty);
            if let Some(v) = &v {
                bound = bound.max(v.bound);
                ascii &= v.ascii;
            }
            self.add_cost(ctx.mult);
            catches.push(format!("{head}\n{}\n{b}", indent(&report)));
        }
        // Unless an outer handler takes it, what no clause matches must
        // not leave the `try`: the last clause then catches everything.
        if !last_catches_all {
            if ctx.raise_ok {
                if !ctx.caught {
                    if let Some(f) = self.frames.last_mut() {
                        f.raises = true;
                    }
                }
            } else {
                let e = self.fresh("err");
                let (b, v) = self.block(&inner, 1, ty);
                if let Some(v) = &v {
                    bound = bound.max(v.bound);
                    ascii &= v.ascii;
                }
                self.add_cost(ctx.mult);
                catches.push(format!("catch {e}\n  print(\"caught\", typeof({e}))\n{b}"));
            }
        }
        let finally = catches.is_empty() || self.rng.chance(35);
        for c in catches {
            text.push_str(&c);
            text.push('\n');
        }
        if finally {
            let (b, _) = self.block(&inner, 1, None);
            self.add_cost(ctx.mult);
            text.push_str(&format!("finally\n  print(\"finally {}\")\n{b}\n", self.names));
        }
        text.push_str("end");
        Some((text, bound, ascii))
    }

    /// `raise` with a guard, of a string or of an exception struct.
    fn raise_text(&mut self, ctx: &Ctx) -> Option<String> {
        if !ctx.raise_ok {
            return None;
        }
        if !ctx.caught {
            if let Some(f) = self.frames.last_mut() {
                f.raises = true;
            }
        }
        let value = if !self.errors.is_empty() && self.rng.chance(55) {
            let t = self.rng.pick(&self.errors.clone());
            let code = self.int_within(ctx, 1, 1000.0);
            if self.rng.chance(30) { format!("{t} {{ code: {} }}", code.code) } else { format!("{t} {{ message: \"{}\", code: {} }}", self.fresh("bad"), code.code) }
        } else {
            let s = self.rng.pick(&["\"boom\"", "\"oops #{1 + 1}\"", "\"\""]);
            s.to_string()
        };
        let c = self.bool_(ctx, 1);
        // Always guarded: a function that always raises is typed as
        // returning `nil` by the analyzer, which then rejects any use of
        // its result (reported for triage).
        Some(match self.rng.below(3) {
            0 => format!("raise({value}) unless {}", c.code),
            _ => format!("raise {value} if {}", c.code),
        })
    }

    fn raise_stmt(&mut self, ctx: &Ctx) -> Option<String> {
        self.raise_text(ctx)
    }

    /// A `MatchError` or a `ConversionError`: neither
    /// is a check an unchecked build leaves out.
    fn error_probe(&mut self, ctx: &Ctx) -> Option<String> {
        if !ctx.raise_ok {
            return None;
        }
        if !ctx.caught {
            if let Some(f) = self.frames.last_mut() {
                f.raises = true;
            }
        }
        let a = self.fresh("pa");
        let b = self.fresh("pb");
        let c = self.bool_(ctx, 1);
        Some(match self.rng.below(3) {
            0 => format!("if {}\n  let [{a}, {b}] = [1, 2, 3]\n  print({a}, {b})\nend", c.code),
            1 => format!("if {}\n  let {a} = \"x{b}\" as Number\n  print({a})\nend", c.code),
            _ => format!("if {}\n  let {{zed: [{a}, *{b}]}} = {{zed: 7}}\n  print({a}, {b}.len)\nend", c.code),
        })
    }

    /// A call statement: a function, a method, a block call, an insert.
    fn call_stmt(&mut self, ctx: &Ctx) -> Option<String> {
        match self.rng.below(4) {
            0 => self.each_call(ctx),
            1 => self.block_call(ctx).map(|e| e.code),
            _ => self.call_stmt_simple(ctx),
        }
    }

    /// A call on one line: a function (command form too), a method.
    fn call_stmt_simple(&mut self, ctx: &Ctx) -> Option<String> {
        if !self.structs.is_empty() && self.rng.chance(25) {
            return self.method_stmt(ctx);
        }
        let fs = self.callable(ctx, |f| f.block.is_none());
        let f = self.rng.pick_ref(&fs)?;
        let call = self.call_text(ctx, &f)?;
        self.note_call(&f, ctx);
        // `f(a, b)` as `f a, b`, when the first argument
        // cannot continue the callee.
        if let Some(open) = call.code.find('(') {
            let args = call.code.get(open + 1..call.code.len().saturating_sub(1)).unwrap_or("");
            if !args.is_empty() && !args.starts_with(['(', '[', '-', '{', '"']) && self.rng.chance(35) && call.code.ends_with(')') {
                return Some(format!("{} {args}", f.name));
            }
        }
        Some(call.code)
    }

    fn method_stmt(&mut self, ctx: &Ctx) -> Option<String> {
        let i = self.rng.below(self.structs.len());
        let v = self.pick_var(Ty::Struct(i), false)?;
        let ms = self.all_methods(i);
        let m = self.rng.pick_ref(&ms)?;
        if !self.affordable(m.cost * ctx.mult) {
            return None;
        }
        self.add_cost(m.cost * ctx.mult);
        let args: Vec<String> = m.params.iter().map(|&b| self.int_within(ctx, 1, b).code).collect();
        Some(if args.is_empty() && self.rng.chance(50) { format!("{}.{}", v.name, m.name) } else { format!("{}.{}({})", v.name, m.name, args.join(", ")) })
    }

    /// `each(list) do |v| ... end` with a block that prints.
    fn each_call(&mut self, ctx: &Ctx) -> Option<String> {
        let fs = self.callable(ctx, |f| f.block == Some((0.0, 0.0)));
        let f = self.rng.pick_ref(&fs)?;
        let l = self.int_list(ctx, 1);
        let iters = l.bound.max(1.0);
        if !self.affordable(iters * ctx.mult * 4.0) {
            return None;
        }
        let v = self.fresh("v");
        let frozen = l.atomic && !l.code.starts_with('[') && !l.code.contains('.');
        if frozen {
            self.frozen.push(l.code.clone());
        }
        let params = vec![Param { name: v.clone(), ty: Ty::Int, bound: l.elem.max(1.0), default: None }];
        let may_raise = ctx.raise_ok && self.rng.chance(30);
        let (body, frame) = self.fn_body(ctx, &params, &[], Ret::Nothing, 2, INT_CAP, may_raise);
        self.block_raises(ctx, &frame);
        if frozen {
            self.frozen.pop();
        }
        self.add_cost((frame.cost + 1.0) * iters * ctx.mult);
        self.note_call(&f, ctx);
        let body = if body.is_empty() { format!("  print({v})") } else { body };
        let head = if self.rng.chance(50) && !l.code.starts_with(['[', '(', '-']) { format!("{} {}", f.name, l.code) } else { format!("{}({})", f.name, l.code) };
        Some(format!("{head} do |{v}|\n{body}\nend"))
    }

    /// Patterns in `case` over a list of mixed values:
    /// a function of `when` clauses and a loop that prints its answers.
    fn classify(&mut self, ctx: &Ctx) -> Option<String> {
        if !self.affordable(8.0 * ctx.mult) || ctx.mult > 4.0 {
            return None;
        }
        let name = self.fresh("classify");
        let item = self.fresh("item");
        let mut arms: Vec<String> = Vec::new();
        let n = 2 + self.rng.below(5);
        let mut struct_ids: Vec<usize> = Vec::new();
        for _ in 0..n {
            let a = self.fresh("a");
            let b = self.fresh("b");
            let arm = match self.rng.below(15) {
                0 => format!("when {}, {}\n  \"num {}\"", self.rng.range(0, 9), self.rng.range(0, 9), self.names),
                1 => {
                    let x = self.rng.range(0, 6);
                    let y = x + self.rng.range(0, 6);
                    format!("when {x}..{y}\n  \"range #{{{item}}}\"")
                }
                2 => format!("when is String\n  \"str #{{{item}}}\""),
                3 => format!("when is Bool\n  \"bool #{{{item}}}\""),
                4 => format!("when is Number if {item} > {}\n  \"big #{{{item}}}\"", self.rng.range(0, 50)),
                5 => format!("when {{kind: {a}}}\n  \"kind #{{{a}}}\""),
                6 => format!("when {{kind: \"{}\", v: {a}}}\n  \"lit #{{{a}}}\"", self.rng.pick(&["move", "stop"])),
                7 => "when {v}\n  \"v #{v}\"".to_string(),
                8 => "when {v: Number}\n  \"vnum #{v}\"".to_string(),
                9 => format!("when {{w: {a}}} if {a} > 2\n  \"w #{{{a}}}\""),
                10 => format!("when [{a}, {b}]\n  \"pair #{{{a}}} #{{{b}}}\""),
                11 => format!("when [{a}, *{b}]\n  \"head #{{{a}}} rest #{{{b}.len}}\""),
                12 => format!("when [{a}, *] as {b}\n  \"long #{{{a}}} #{{{b}.len}}\""),
                _ => {
                    if self.structs.is_empty() {
                        format!("when [{a}, *]\n  \"first #{{{a}}}\"")
                    } else {
                        let i = self.rng.below(self.structs.len());
                        struct_ids.push(i);
                        let sname = self.structs.get(i).map(|s| s.name.clone()).unwrap_or_default();
                        let fields = self.all_fields(i);
                        match self.rng.pick_ref(&fields) {
                            Some(f) if f.ty == Ty::Int && self.rng.chance(50) => format!("when {sname} {{{}: {a}}} if {a} > 3\n  \"{sname} big #{{{a}}}\"", f.name),
                            Some(f) => format!("when {sname} {{{}: {a}}}\n  \"{sname} #{{{a}}}\"", f.name),
                            None => format!("when is {sname}\n  \"a {sname}\""),
                        }
                    }
                }
            };
            arms.push(arm);
        }
        let else_arm = if self.rng.chance(70) { "else\n  \"other\"\n".to_string() } else { String::new() };
        let body: Vec<String> = arms.iter().map(|a| indent(a)).collect();
        let func = format!("fn {name}({item})\n  case {item}\n{}\n{}  end\nend", body.join("\n"), indent(&else_arm).trim_end_matches("  "));
        // The items: one of each shape, in a random order.
        let mut items: Vec<String> = Vec::new();
        let count = 3 + self.rng.below(5);
        for _ in 0..count {
            let it = match self.rng.below(10) {
                0 | 1 => format!("{}", self.rng.range(-3, 60)),
                2 => self.rng.pick(&["\"hi\"", "\"\"", "\"move\""]).to_string(),
                3 => self.rng.pick(&["true", "false"]).to_string(),
                4 => format!("{{kind: \"{}\", v: {}}}", self.rng.pick(&["move", "stop", "go"]), self.rng.range(0, 9)),
                5 => format!("{{v: \"{}\", w: {}}}", self.rng.pick(&["s", "t"]), self.rng.range(0, 9)),
                6 => format!("[{}, {}]", self.rng.range(0, 9), self.rng.range(0, 9)),
                7 => format!("[{}, {}, {}]", self.rng.range(0, 9), self.rng.range(0, 9), self.rng.range(0, 9)),
                8 => "[]".to_string(),
                _ => {
                    let id = struct_ids.first().copied().or_else(|| if self.structs.is_empty() { None } else { Some(self.rng.below(self.structs.len())) });
                    match id.and_then(|i| self.struct_lit(ctx, i, 0)) {
                        Some((e, _)) => e.code,
                        None => "{w: 5}".to_string(),
                    }
                }
            };
            items.push(it);
        }
        let it = self.fresh("it");
        self.add_cost(count as f64 * ctx.mult * 2.0);
        Some(format!("{func}\nfor {it} in [{}]\n  print({name}({it}))\nend", items.join(", ")))
    }

    /// `if let` over a value that may be `nil`, or a pattern.
    fn if_let(&mut self, ctx: &Ctx) -> Option<String> {
        let inner = ctx.nested();
        let v = self.fresh("got");
        let (head, ty, bound) = match self.rng.below(4) {
            0 => {
                let m = self.map_(ctx, 1);
                let k = self.rng.pick(&["a", "b", "zed", "type"]);
                (format!("if let {v} = {}.{k}", m.operand()), Ty::Int, m.elem)
            }
            1 => {
                let l = self.int_list(ctx, 1);
                let i = self.rng.range(1, 4);
                (format!("if let {v} = {}[{i}]", l.operand()), Ty::Int, l.elem)
            }
            2 => {
                let s = self.str_(ctx, 1);
                (format!("if let {v} = {}.index(\"a\")", s.operand()), Ty::Int, s.bound)
            }
            _ => {
                let l = self.int_list(ctx, 1);
                let w = self.fresh("got");
                self.push_scope();
                self.declare(Var { name: w.clone(), ty: Ty::Int, bound: l.elem, used: false, fn_depth: self.fn_depth(), ..Var::default() });
                self.declare(Var { name: v.clone(), ty: Ty::Int, bound: l.elem, used: false, fn_depth: self.fn_depth(), ..Var::default() });
                let (b, _) = self.block(&inner, 2, None);
                let mut unused = self.pop_scope(&inner);
                let extra = if unused.is_empty() { String::new() } else { format!("\n{}", block_text(&std::mem::take(&mut unused))) };
                let (e, _) = self.block(&inner, 1, None);
                return Some(format!("if let [{v}, {w}] = {}\n{b}{extra}\nelse\n{e}\nend", l.code));
            }
        };
        self.push_scope();
        self.declare(Var { name: v.clone(), ty, bound: bound.max(1.0), used: false, fn_depth: self.fn_depth(), ..Var::default() });
        let (b, _) = self.block(&inner, 2, None);
        let unused = self.pop_scope(&inner);
        let extra = if unused.is_empty() { String::new() } else { format!("\n{}", block_text(&unused)) };
        let mut s = format!("{head}\n{b}{extra}\n");
        if self.rng.chance(50) {
            let (e, _) = self.block(&inner, 1, None);
            s.push_str(&format!("else\n{e}\n"));
        }
        s.push_str("end");
        Some(s)
    }

    /// `a, b = b, a` and friends.
    fn multi_assign(&mut self, ctx: &Ctx) -> Option<String> {
        let a = self.pick_var(Ty::Int, true)?;
        let b = self.pick_var(Ty::Int, true)?;
        if a.name == b.name {
            return None;
        }
        if self.rng.chance(50) && a.bound == b.bound {
            return Some(format!("{}, {} = {}, {}", a.name, b.name, b.name, a.name));
        }
        let x = self.int_within(ctx, 2, a.bound);
        let y = self.int_within(ctx, 2, b.bound);
        Some(format!("{}, {} = {}, {}", a.name, b.name, x.code, y.code))
    }

    /// `let a, b = ...`: from a call that returns two values, or a list of
    /// values.
    fn multi_let(&mut self, ctx: &Ctx) -> Option<String> {
        let a = self.fresh("v");
        let b = self.fresh("v");
        let fs = self.callable(ctx, |f| matches!(f.ret, Ret::Pair(_)));
        let (rhs, bound) = match self.rng.pick_ref(&fs) {
            Some(f) => {
                let call = self.call_text(ctx, &f)?;
                self.note_call(&f, ctx);
                (call.code, call.bound)
            }
            None => {
                let x = self.int_within(ctx, 2, 1000.0);
                let y = self.int_within(ctx, 2, 1000.0);
                (format!("{}, {}", x.code, y.code), 1000.0)
            }
        };
        self.declare(Var { name: a.clone(), ty: Ty::Int, bound, mutable: true, used: false, fn_depth: self.fn_depth(), ..Var::default() });
        self.declare(Var { name: b.clone(), ty: Ty::Int, bound, mutable: true, used: false, fn_depth: self.fn_depth(), ..Var::default() });
        Some(format!("let {a}, {b} = {rhs}"))
    }

    /// `return value if cond` in a function.
    fn return_stmt(&mut self, ctx: &Ctx) -> Option<String> {
        let (ret, cap) = {
            let f = self.frames.last()?;
            (f.ret, f.ret_cap)
        };
        let c = self.bool_(ctx, 1);
        let m = self.rng.pick(&["if", "unless"]);
        match ret {
            Ret::Nothing => Some(format!("return {m} {}", c.code)),
            Ret::One(ty, _) => {
                let e = self.expr(ctx, ty, 2);
                let e = if matches!(ty, Ty::Int | Ty::Num) { fit(e, cap) } else { e };
                if let Some(f) = self.frames.last_mut() {
                    f.ret_bound = f.ret_bound.max(e.bound);
                }
                Some(format!("return {} {m} {}", e.code, c.code))
            }
            Ret::Pair(b) => {
                let x = self.int_within(ctx, 1, b);
                let y = self.int_within(ctx, 1, b);
                Some(format!("return {}, {} {m} {}", x.code, y.code, c.code))
            }
        }
    }

    /// Changes to lists and maps: inserts, indexed stores, keyed stores.
    fn collection_stmt(&mut self, ctx: &Ctx) -> Option<String> {
        match self.rng.below(3) {
            0 => {
                let ty = if self.rng.chance(70) { Ty::IntList } else { Ty::StrList };
                let v = self.pick_var(ty, true)?;
                if v.bound + ctx.mult > LIST_CAP {
                    return None;
                }
                let e = if ty == Ty::IntList { self.int_within(ctx, 2, v.elem.max(1.0)) } else { self.str_(ctx, 2) };
                let mult = ctx.mult;
                self.update_var(&v.name, |var| var.bound += mult);
                Some(format!("Table.insert({}, {})", v.name, e.code))
            }
            1 => {
                let ty = if self.rng.chance(70) { Ty::IntList } else { Ty::StrList };
                let v = self.pick_var(ty, true)?;
                if v.min_len == 0 {
                    return None;
                }
                let i = self.rng.range(1, v.min_len as i64);
                let e = if ty == Ty::IntList { self.int_within(ctx, 2, v.elem.max(1.0)) } else { self.str_(ctx, 2) };
                if ty == Ty::IntList && self.rng.chance(30) {
                    let op = self.rng.pick(&["+=", "-=", "^=", "//="]);
                    let small = if op == "//=" { self.rng.range(1, 9).to_string() } else { self.int_within(ctx, 1, 100.0).code };
                    return Some(format!("{}[{i}] {op} {small}\n{}[{i}] %= {}", v.name, v.name, bound_lit(v.elem.max(1.0))));
                }
                Some(format!("{}[{i}] = {}", v.name, e.code))
            }
            _ => {
                let v = self.pick_var(Ty::Map, false)?;
                if self.frozen.contains(&v.name) {
                    return None;
                }
                let e = self.int_within(ctx, 2, v.elem.max(1.0));
                let k = self.rng.pick(&["a", "b", "zed", "end", "type", "count", "if", "q1"]);
                // On a map, `m.count` is `Table.count(m)`: the name of a
                // function of the Table library is assigned as a key only
                // in brackets (the draw is made either way, so that the
                // random stream stays as it was).
                let dotted = self.rng.chance(60) && k != "count";
                let target = if dotted { format!("{}.{k}", v.name) } else { format!("{}[\"{k}\"]", v.name) };
                if self.rng.chance(30) {
                    // A compound assignment evaluates its target once;
                    // the key may be missing, so it starts
                    // from a default.
                    let op = self.rng.pick(&["+=", "-=", "|=", "*="]);
                    let small = self.int_within(ctx, 1, 100.0);
                    return Some(format!("{target} = {target} or 0\n{target} {op} {}\n{target} %= {}", small.code, bound_lit(v.elem.max(1.0))));
                }
                Some(format!("{target} = {}", e.code))
            }
        }
    }

    /// Changes to a struct value: a field store or a mutating method.
    fn struct_stmt(&mut self, ctx: &Ctx) -> Option<String> {
        if self.rng.chance(40) {
            return self.method_stmt(ctx);
        }
        let i = self.rng.below(self.structs.len());
        let v = self.pick_var(Ty::Struct(i), false)?;
        let fields = self.all_fields(i);
        let f = self.rng.pick_ref(&fields)?;
        if f.ty == Ty::Int && self.rng.chance(30) {
            let op = self.rng.pick(&["+=", "-=", "*=", "<<=", "&="]);
            let small = if op == "<<=" { self.rng.range(0, 8).to_string() } else { self.int_within(ctx, 1, 100.0).code };
            return Some(format!("{}.{} {op} {small}\n{}.{} %= {}", v.name, f.name, v.name, f.name, bound_lit(f.bound)));
        }
        let e = match f.ty {
            Ty::Str if f.typed => non_nil(&self.str_(ctx, 2)),
            Ty::Str => self.str_(ctx, 2).code,
            _ => self.int_within(ctx, 2, f.bound).code,
        };
        Some(format!("{}.{} = {e}", v.name, f.name))
    }

    pub(super) fn int_atom_pub(&mut self, ctx: &Ctx) -> Ex {
        self.int(ctx, 0)
    }
}

/// Number literals for the zoo of `print_stmt`.
const NUMBER_ZOO: &[&str] = &[
    "0x1fffffffffffff", "0x20000000000001", "9007199254740993", "9223372036854775807", "18446744073709551615",
    "1e308", "1.7976931348623157e308", "2.5e-310", "4.9e-324", "0.1", "0.30000000000000004", "123456789.123456789",
    "1_000.5", "0xdead_beef", "0b1111_0000", "5e+20", "1e15", "1e16", "99999999999999.9", "0.000001", "1e-7",
    "2147483647", "2147483648", "4294967296", "-2147483649", "0x7fffffff", "0x80000000", "65536.0", "3.0e0", "1E3",
];

/// How many keys a map can have: the size of the key pool of `expr.rs`.
const MAP_KEY_COUNT: usize = 8;
/// A large variable bound that still keeps products exact.
const BIG: f64 = 2_147_483_648.0;
