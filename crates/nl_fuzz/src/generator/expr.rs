//! Expressions of generated programs, by type: numbers with the operators
//! and the conversions, strings with interpolation and the string
//! library, booleans, lists and maps, struct values, enum values, calls
//! in every form (keyed arguments, pipes, `f!`, trailing blocks), and the
//! block expressions (`if`, `case`, `cond`, `try` as values).
//!
//! Every numeric expression knows a bound on its magnitude and every
//! string a bound on its length (see the module above); an operator is
//! only chosen when its result stays defined and exact.

use super::*;

/// Words used as map keys: keywords included, since every word is a key.
const MAP_KEYS: &[&str] = &["a", "b", "zed", "end", "type", "count", "if", "q1"];

/// String literals: plain, with escapes, raw, and some UTF-8.
const STRINGS: &[(&str, bool)] = &[
    ("\"\"", true),
    ("\"a\"", true),
    ("\"ab\"", true),
    ("\"hello\"", true),
    ("\"Fox\"", true),
    ("\"x y z\"", true),
    ("\"a,b,,c\"", true),
    ("\" pad \"", true),
    ("\"tab\\there\"", true),
    ("\"q\\\"uote\"", true),
    ("\"back\\\\slash\"", true),
    ("\"MiXeD\"", true),
    ("\"end\"", true),
    ("\"42\"", true),
    ("'raw\\'s'", true),
    ("'\\d+'", true),
    ("\"\\#{not}\"", true),
    ("\"héllo\"", false),
    ("\"naïve\"", false),
    ("\"日本\"", false),
    ("\"über\"", false),
];

impl Gen {
    // ---- numbers ----

    pub(super) fn int_lit(&mut self) -> Ex {
        match self.rng.weighted(&[60, 10, 6, 6, 6, 6, 6]) {
            0 => {
                let v = self.rng.range(0, 99);
                Ex::new(v.to_string(), Ty::Int, v as f64).atom()
            }
            1 => {
                let v = self.rng.range(-20, -1);
                Ex::new(v.to_string(), Ty::Int, v.unsigned_abs() as f64)
            }
            2 => {
                let v = self.rng.range(0, 0xffff);
                Ex::new(format!("0x{v:x}"), Ty::Int, v as f64).atom()
            }
            3 => {
                let v = self.rng.range(0, 64);
                Ex::new(format!("0b{v:b}"), Ty::Int, v as f64).atom()
            }
            4 => Ex::new("1_000_000", Ty::Int, 1.0e6).atom(),
            5 => {
                let v = self.rng.pick(&[0x7fff_ffff_i64, 0x8000_0000, 0xffff_ffff, 4_294_967_296, 2_147_483_647]);
                Ex::new(format!("{v}"), Ty::Int, v as f64).atom()
            }
            _ => Ex::new("0", Ty::Int, 0.0).atom(),
        }
    }

    fn num_lit(&mut self) -> Ex {
        let (code, b) = self.rng.pick(&[
            ("0.5", 0.5),
            ("1.5", 1.5),
            ("2.25", 2.25),
            ("0.1", 0.1),
            ("3.75", 3.75),
            ("1e3", 1000.0),
            ("2.5e-3", 0.0025),
            ("0.3e1", 3.0),
            ("123.456", 123.456),
            ("7.0", 7.0),
        ]);
        Ex::new(code, Ty::Num, b).atom()
    }

    /// A variable or a field of `self` of the type.
    fn var_ex(&mut self, ctx: &Ctx, ty: Ty) -> Option<Ex> {
        if let Some(i) = ctx.method_of {
            if self.rng.chance(40) {
                let fields: Vec<Field> = self.all_fields(i).into_iter().filter(|f| f.ty == ty).collect();
                if !fields.is_empty() {
                    let f = self.rng.pick_ref(&fields)?;
                    return Some(Ex::new(format!("@{}", f.name), ty, f.bound).atom());
                }
            }
        }
        let v = self.pick_var(ty, false)?;
        Some(Ex::new(v.name.clone(), ty, v.bound).elem(v.elem).ascii(v.ascii).atom())
    }

    /// An integer within `b`: generated freely, then wrapped in `% b` if
    /// it could be larger.
    pub(super) fn int_within(&mut self, ctx: &Ctx, d: u32, b: f64) -> Ex {
        let e = self.int(ctx, d);
        fit(e, b)
    }

    pub(super) fn int(&mut self, ctx: &Ctx, d: u32) -> Ex {
        if d == 0 || self.rng.chance(25) {
            return self.int_atom(ctx);
        }
        let e = match self.rng.weighted(&[16, 6, 5, 6, 6, 4, 4, 3, 6, 3, 8, 6, 3, 3, 3, 3, 3, 3, 3, 3, 4]) {
            0 => {
                let a = self.int(ctx, d - 1);
                let b = self.int(ctx, d - 1);
                let op = self.rng.pick(&["+", "-"]);
                Ex::new(format!("{} {op} {}", a.operand(), b.operand()), Ty::Int, a.bound + b.bound)
            }
            1 => {
                let a = self.int(ctx, d - 1);
                let b = self.int(ctx, d - 1);
                let (a, b) = if a.bound * b.bound > MUL_CAP { (fit(a, 1000.0), fit(b, 1000.0)) } else { (a, b) };
                Ex::new(format!("{} * {}", a.operand(), b.operand()), Ty::Int, a.bound * b.bound)
            }
            2 => {
                let a = self.int(ctx, d - 1);
                let k = self.nonzero(9);
                Ex::new(format!("{} // {}", a.operand(), lit(k)), Ty::Int, a.bound / k.unsigned_abs() as f64 + 1.0)
            }
            3 => {
                let a = self.int(ctx, d - 1);
                let k = if self.rng.chance(70) { self.nonzero(12) } else { self.rng.pick(&[100, 1000, -7, 256]) };
                Ex::new(format!("{} % {}", a.operand(), lit(k)), Ty::Int, k.unsigned_abs() as f64)
            }
            4 => {
                let a = self.int(ctx, d - 1);
                let b = self.int(ctx, d - 1);
                let op = self.rng.pick(&["&", "|", "^"]);
                Ex::new(format!("{} {op} {}", a.operand(), b.operand()), Ty::Int, BIT_BOUND)
            }
            5 => {
                let a = self.int(ctx, d - 1);
                let op = self.rng.pick(&["<<", ">>", ">>>"]);
                let s = if self.rng.chance(80) { self.rng.range(0, 31).to_string() } else { format!("({} % 8)", self.int_atom(ctx).operand()) };
                Ex::new(format!("{} {op} {s}", a.operand()), Ty::Int, BIT_BOUND)
            }
            6 => {
                let a = self.int(ctx, d - 1);
                if self.rng.chance(50) { Ex::new(format!("-{}", a.operand()), Ty::Int, a.bound) } else { Ex::new(format!("~{}", a.operand()), Ty::Int, BIT_BOUND) }
            }
            7 => {
                let a = self.int(ctx, d - 1);
                if a.bound <= 1.0e5 {
                    Ex::new(format!("{} ** 2", a.operand()), Ty::Int, a.bound * a.bound)
                } else {
                    let k = self.rng.range(0, 20);
                    Ex::new(format!("2 ** {k}"), Ty::Int, 2f64.powi(k as i32))
                }
            }
            8 => {
                let s = self.str_(ctx, d - 1);
                match self.rng.below(6) {
                    0 => Ex::new(format!("{}.length", s.operand()), Ty::Int, s.bound).atom(),
                    1 => Ex::new(format!("{}.len", s.operand()), Ty::Int, s.bound * 4.0).atom(),
                    2 => {
                        let sub = self.rng.pick(&["\"a\"", "\"l\"", "\" \"", "\"é\"", "\"ab\""]);
                        Ex::new(format!("({}.index({sub}) or 0)", s.operand()), Ty::Int, s.bound).atom()
                    }
                    3 => Ex::new(format!("({} <> \"x\").ord", s.operand()), Ty::Int, 1_114_111.0).atom(),
                    4 => Ex::new(format!("{}.split(\",\").len", s.operand()), Ty::Int, s.bound + 1.0).atom(),
                    _ => Ex::new(format!("{}.chars.len", s.operand()), Ty::Int, s.bound).atom(),
                }
            }
            9 => {
                let l = if self.rng.chance(60) { self.int_list(ctx, d - 1) } else { self.str_list(ctx, d - 1) };
                Ex::new(format!("{}.len", l.operand()), Ty::Int, l.bound).atom()
            }
            10 => match self.call_returning(ctx, Ty::Int) {
                Some(e) => e,
                None => self.int_atom(ctx),
            },
            11 => match self.struct_int(ctx, d) {
                Some(e) => e,
                None => self.int_atom(ctx),
            },
            12 => {
                let c = self.bool_(ctx, d - 1);
                let a = self.int(ctx, d - 1);
                let b = self.int(ctx, d - 1);
                let bound = a.bound.max(b.bound);
                // `c and a or b` would be `Bool | Number` to the analyzer.
                if self.rng.chance(75) {
                    Ex::new(format!("(if {}; {} else {} end)", c.code, a.code, b.code), Ty::Int, bound).atom()
                } else {
                    Ex::new(format!("{} or {}", a.operand(), b.operand()), Ty::Int, bound)
                }
            }
            13 => {
                let a = self.int(ctx, d - 1);
                let b = self.int(ctx, d - 1);
                match self.rng.below(4) {
                    0 => Ex::new(format!("Number.max({}, {})", a.code, b.code), Ty::Int, a.bound.max(b.bound)).atom(),
                    1 => Ex::new(format!("Number.min({}, {})", a.code, b.code), Ty::Int, a.bound.max(b.bound)).atom(),
                    2 => Ex::new(format!("Number.abs({})", a.code), Ty::Int, a.bound).atom(),
                    _ => Ex::new(format!("{}.abs", a.operand()), Ty::Int, a.bound).atom(),
                }
            }
            14 => {
                let f = self.num(ctx, d - 1);
                match self.rng.below(3) {
                    0 => Ex::new(format!("Number.floor({})", f.code), Ty::Int, f.bound + 1.0).atom(),
                    1 => Ex::new(format!("{}.floor", f.operand()), Ty::Int, f.bound + 1.0).atom(),
                    _ => Ex::new(format!("Number.ceil({})", f.code), Ty::Int, f.bound + 1.0).atom(),
                }
            }
            15 => {
                let a = self.int(ctx, d - 1);
                if self.rng.chance(70) {
                    Ex::new(format!("({} as String) as Number", a.operand()), Ty::Int, a.bound)
                } else {
                    let s = self.rng.pick(&["\"12\"", "\"0x10\"", "\" 7 \"", "\"-3\"", "\"1e2\""]);
                    Ex::new(format!("({s} as Number)"), Ty::Int, 100.0).atom()
                }
            }
            16 => {
                let m = self.map_(ctx, d - 1);
                let k = self.rng.pick(MAP_KEYS);
                if self.rng.chance(50) {
                    // A value of a map is a number, never `false`, so `??`
                    // gives what `or` gives. Chosen by the key,
                    // so that the random stream, and with it every other
                    // part of a seed's program, stays as it was.
                    if k.len() % 2 == 0 {
                        Ex::new(format!("({}?.{k} ?? 0)", m.operand()), Ty::Int, m.elem).atom()
                    } else {
                        Ex::new(format!("({}.{k} or 0)", m.operand()), Ty::Int, m.elem).atom()
                    }
                } else {
                    Ex::new(format!("({}[\"{k}\"] or 0)", m.operand()), Ty::Int, m.elem).atom()
                }
            }
            17 => {
                let l = self.int_list(ctx, d - 1);
                let i = self.int_within(ctx, d - 1, 6.0);
                // As above: `?.[i] ??` when the index's text has an odd
                // length, without a draw of its own.
                if i.code.len() % 2 == 1 {
                    Ex::new(format!("({}?.[{}] ?? 0)", l.operand(), i.code), Ty::Int, l.elem).atom()
                } else {
                    Ex::new(format!("({}[{}] or 0)", l.operand(), i.code), Ty::Int, l.elem).atom()
                }
            }
            18 => match self.pipe_call(ctx) {
                Some(e) => e,
                None => self.int_atom(ctx),
            },
            20 => match self.extra_int(ctx, d) {
                Some(e) => e,
                None => self.int_atom(ctx),
            },
            _ => match self.block_call_inline(ctx) {
                Some(e) => e,
                None => self.int_atom(ctx),
            },
        };
        if e.bound > INT_CAP { fit(e, 1000.0) } else { e }
    }

    fn int_atom(&mut self, ctx: &Ctx) -> Ex {
        if self.rng.chance(55) {
            if let Some(v) = self.var_ex(ctx, Ty::Int) {
                return v;
            }
        }
        if self.rng.chance(8) {
            // A numeric enum value, or the number of variants.
            let numeric: Vec<usize> = (0..self.enums.len()).filter(|&i| self.enums.get(i).is_some_and(|e| e.variants.iter().all(|v| matches!(v.1, EnumVal::Num(_))))).collect();
            if let Some(&i) = numeric.get(self.rng.below(numeric.len())) {
                if let Some(e) = self.enums.get(i) {
                    let v = self.rng.below(e.variants.len());
                    if let Some((name, EnumVal::Num(_))) = e.variants.get(v) {
                        return Ex::new(format!("{}.{name}", e.name), Ty::Int, 40.0).atom();
                    }
                }
            }
            if let Some(e) = self.enums.get(self.rng.below(self.enums.len())) {
                return Ex::new(format!("{}.values.len", e.name), Ty::Int, 8.0).atom();
            }
        }
        self.int_lit()
    }

    fn nonzero(&mut self, max: i64) -> i64 {
        let v = self.rng.range(1, max);
        if self.rng.chance(25) { -v } else { v }
    }

    pub(super) fn num(&mut self, ctx: &Ctx, d: u32) -> Ex {
        if d == 0 || self.rng.chance(20) {
            if self.rng.chance(40) {
                if let Some(v) = self.var_ex(ctx, Ty::Num) {
                    return v;
                }
            }
            return if self.rng.chance(60) { self.num_lit() } else { self.int_atom(ctx) };
        }
        let e = match self.rng.weighted(&[20, 10, 15, 10, 10, 6, 6, 10]) {
            0 => {
                let a = self.num(ctx, d - 1);
                let b = self.num(ctx, d - 1);
                let op = self.rng.pick(&["+", "-"]);
                Ex::new(format!("{} {op} {}", a.operand(), b.operand()), Ty::Num, a.bound + b.bound)
            }
            1 => {
                let a = self.num(ctx, d - 1);
                let b = self.num(ctx, d - 1);
                let (a, b) = if a.bound * b.bound > MUL_CAP { (fit(a, 1000.0), fit(b, 1000.0)) } else { (a, b) };
                Ex::new(format!("{} * {}", a.operand(), b.operand()), Ty::Num, a.bound * b.bound)
            }
            2 => {
                let a = self.num(ctx, d - 1);
                let (k, kb) = self.rng.pick(&[("3", 3.0), ("2", 2.0), ("-4", 4.0), ("0.5", 0.5), ("7", 7.0), ("1.5", 1.5), ("-0.25", 0.25)]);
                let k = if k.starts_with('-') { format!("({k})") } else { k.to_string() };
                Ex::new(format!("{} / {k}", a.operand()), Ty::Num, a.bound / kb)
            }
            3 => {
                let a = self.num(ctx, d - 1);
                let (k, kb) = self.rng.pick(&[("2.5", 2.5), ("1", 1.0), ("0.75", 0.75), ("-3", 3.0), ("10", 10.0)]);
                let k = if k.starts_with('-') { format!("({k})") } else { k.to_string() };
                let op = self.rng.pick(&["%", "//"]);
                let bound = if op == "%" { kb } else { a.bound / kb + 1.0 };
                Ex::new(format!("{} {op} {k}", a.operand()), Ty::Num, bound)
            }
            4 => {
                let a = self.num(ctx, d - 1);
                Ex::new(format!("Number.sqrt(Number.abs({}))", a.code), Ty::Num, a.bound.sqrt() + 1.0).atom()
            }
            5 => {
                let a = self.num(ctx, d - 1);
                if a.bound <= 1.0e5 {
                    Ex::new(format!("{} ** 2", a.operand()), Ty::Num, a.bound * a.bound)
                } else {
                    Ex::new(format!("{} ** 0.5", fit(a, 1000.0).operand()), Ty::Num, 32.0)
                }
            }
            6 => {
                let s = self.rng.pick(&["\"2.5\"", "\"1e-2\"", "\" 3.25\"", "\"0x1p4\""]);
                Ex::new(format!("({s} as Number)"), Ty::Num, 16.0).atom()
            }
            _ => self.int(ctx, d - 1),
        };
        let mut e = e;
        e.ty = Ty::Num;
        if e.bound > INT_CAP { fit(e, 1000.0) } else { e }
    }

    // ---- strings ----

    fn str_lit(&mut self) -> Ex {
        let (code, ascii) = self.rng.pick(STRINGS);
        // The length bound counts escapes as characters: an overestimate.
        Ex::new(code, Ty::Str, code.chars().count() as f64).ascii(ascii).atom()
    }

    pub(super) fn str_(&mut self, ctx: &Ctx, d: u32) -> Ex {
        let e = self.str_raw(ctx, d);
        if e.bound > STR_CAP { Ex::new(format!("{}[1..20]", e.operand()), Ty::Str, 20.0).ascii(e.ascii).atom() } else { e }
    }

    fn str_raw(&mut self, ctx: &Ctx, d: u32) -> Ex {
        if d == 0 || self.rng.chance(25) {
            if self.rng.chance(50) {
                if let Some(v) = self.var_ex(ctx, Ty::Str) {
                    return v;
                }
            }
            return self.str_lit();
        }
        match self.rng.weighted(&[14, 10, 8, 14, 8, 6, 5, 4, 4, 6, 5, 4, 4]) {
            0 => {
                // Interpolation of any scalar (`tostring`).
                let pre = self.rng.pick(&["", "v=", "<", "n "]);
                let post = self.rng.pick(&["", ">", "!", " end"]);
                let (inner, b, ascii) = match self.rng.below(5) {
                    0 => {
                        let e = self.int(ctx, d - 1);
                        (unsigned_zero(&e), 24.0, true)
                    }
                    1 => {
                        let e = self.num(ctx, d - 1);
                        (unsigned_zero(&e), 24.0, true)
                    }
                    2 => {
                        let e = self.bool_(ctx, d - 1);
                        (e.code, 5.0, true)
                    }
                    3 => match self.struct_with_to_string(ctx) {
                        Some(e) => (e.code, 30.0, true),
                        None => ("1".into(), 1.0, true),
                    },
                    _ => {
                        let e = self.str_(ctx, d - 1);
                        (e.code, e.bound, e.ascii)
                    }
                };
                Ex::new(format!("\"{pre}#{{{inner}}}{post}\""), Ty::Str, b + 6.0).ascii(ascii).atom()
            }
            1 => {
                let a = self.str_(ctx, d - 1);
                let b = self.str_(ctx, d - 1);
                Ex::new(format!("{} <> {}", a.operand(), b.operand()), Ty::Str, a.bound + b.bound).ascii(a.ascii && b.ascii)
            }
            2 => {
                let (code, b) = match self.rng.below(4) {
                    0 => {
                        let e = self.int(ctx, d - 1);
                        (format!("({}) as String", unsigned_zero(&e)), 24.0)
                    }
                    1 => {
                        let e = self.num(ctx, d - 1);
                        (format!("tostring({})", unsigned_zero(&e)), 24.0)
                    }
                    2 => {
                        let e = self.bool_(ctx, d - 1);
                        (format!("{} as String", e.operand()), 5.0)
                    }
                    _ => {
                        let e = self.num(ctx, d - 1);
                        (format!("({}) as String", unsigned_zero(&e)), 24.0)
                    }
                };
                Ex::new(code, Ty::Str, b)
            }
            3 => {
                let s = self.str_(ctx, d - 1);
                let m = self.rng.pick(&["upcase", "downcase", "capitalize", "swapcase", "reverse", "strip", "lstrip", "rstrip"]);
                Ex::new(format!("{}.{m}", s.operand()), Ty::Str, s.bound).ascii(s.ascii).atom()
            }
            4 => {
                let s = self.str_(ctx, d - 1);
                let n = self.rng.range(0, 16);
                let m = self.rng.pick(&["center", "ljust", "rjust"]);
                let pad = if self.rng.chance(40) { format!(", \"{}\"", self.rng.pick(&["*", "-=", "ab"])) } else { String::new() };
                Ex::new(format!("{}.{m}({n}{pad})", s.operand()), Ty::Str, s.bound.max(n as f64)).ascii(s.ascii).atom()
            }
            5 => {
                let s = self.str_(ctx, d - 1);
                let pat = self.rng.pick(&["\"a\"", "\"l\"", "\" \"", "\"o\"", "\"é\"", "\"ab\""]);
                let rep = self.rng.pick(&["\"_\"", "\"XY\"", "\"\""]);
                let m = self.rng.pick(&["sub", "gsub"]);
                Ex::new(format!("{}.{m}({pat}, {rep})", s.operand()), Ty::Str, s.bound * 2.0).ascii(s.ascii).atom()
            }
            6 => {
                let s = self.str_(ctx, d - 1);
                let a = self.rng.range(-4, 4);
                let b = self.rng.range(-3, 6);
                let op = self.rng.pick(&["..", "..."]);
                Ex::new(format!("{}[{a}{op}{b}]", s.operand()), Ty::Str, s.bound).ascii(s.ascii).atom()
            }
            7 => {
                let s = self.str_(ctx, d - 1);
                let i = self.rng.range(-3, 5);
                Ex::new(format!("({}[{i}] or \"-\")", s.operand()), Ty::Str, 1.0).ascii(s.ascii).atom()
            }
            8 => {
                let s = self.str_(ctx, d - 1);
                if s.ascii {
                    let a = self.rng.range(1, 4);
                    let b = self.rng.range(0, 6);
                    Ex::new(format!("{}.byteslice({a}, {b})", s.operand()), Ty::Str, s.bound).atom()
                } else {
                    let k = self.rng.range(0, 2);
                    Ex::new(format!("{}.rep({k})", s.operand()), Ty::Str, s.bound * k as f64).ascii(s.ascii).atom()
                }
            }
            9 => {
                let i = self.int(ctx, d - 1);
                let s = self.str_(ctx, d - 1);
                let f = self.num(ctx, d - 1);
                let (fmt, args) = match self.rng.below(3) {
                    0 => ("%d|%s", format!("{}, {}", i.code, s.code)),
                    1 => ("%5.2f", unsigned_zero(&f)),
                    _ => ("[%3d] %-4s", format!("{}, {}", fit(i, 1000.0).code, s.code)),
                };
                Ex::new(format!("\"{fmt}\".format({args})"), Ty::Str, s.bound + 40.0).ascii(s.ascii).atom()
            }
            10 => {
                let l = self.str_list(ctx, d - 1);
                let sep = self.rng.pick(&["\",\"", "\"\"", "\" | \""]);
                Ex::new(format!("Table.concat({}, {sep})", l.code), Ty::Str, l.bound * (l.elem + 3.0)).ascii(false).atom()
            }
            11 => {
                let i = self.int(ctx, d - 1);
                Ex::new(format!("({} % 26 + 97).chr", i.operand()), Ty::Str, 1.0).atom()
            }
            _ => match self.rng.below(4) {
                3 => match self.extra_str(ctx, d) {
                    Some(e) => e,
                    None => self.str_lit(),
                },
                0 => match self.call_returning(ctx, Ty::Str) {
                    Some(e) => e,
                    None => self.str_lit(),
                },
                1 => match self.enum_val(ctx) {
                    Some((e, i)) => {
                        let ename = self.enums.get(i).map(|x| x.name.clone()).unwrap_or_default();
                        Ex::new(format!("{ename}.name({})", e.code), Ty::Str, 12.0).atom()
                    }
                    None => self.str_lit(),
                },
                _ => {
                    let t = self.rng.below(4);
                    let inner = match t {
                        0 => self.int(ctx, d - 1).code,
                        1 => self.str_(ctx, d - 1).code,
                        2 => self.bool_(ctx, d - 1).code,
                        _ => self.int_list(ctx, d - 1).code,
                    };
                    Ex::new(format!("typeof({inner})"), Ty::Str, 8.0).atom()
                }
            },
        }
    }

    // ---- booleans ----

    pub(super) fn bool_(&mut self, ctx: &Ctx, d: u32) -> Ex {
        if d == 0 || self.rng.chance(20) {
            if self.rng.chance(40) {
                if let Some(v) = self.var_ex(ctx, Ty::Bool) {
                    return v;
                }
            }
            if self.rng.chance(30) {
                return Ex::new(self.rng.pick(&["true", "false"]), Ty::Bool, 1.0).atom();
            }
            let a = self.int_atom(ctx);
            let b = self.int_atom(ctx);
            let op = self.rng.pick(&["<", "<=", ">", ">=", "==", "!="]);
            return Ex::new(format!("{} {op} {}", a.operand(), b.operand()), Ty::Bool, 1.0);
        }
        match self.rng.weighted(&[25, 8, 8, 10, 10, 6, 6, 5, 4, 4, 4]) {
            0 => {
                let a = self.int(ctx, d - 1);
                let b = self.int(ctx, d - 1);
                let op = self.rng.pick(&["<", "<=", ">", ">=", "==", "!="]);
                Ex::new(format!("{} {op} {}", a.operand(), b.operand()), Ty::Bool, 1.0)
            }
            1 => {
                let a = self.num(ctx, d - 1);
                let b = self.num(ctx, d - 1);
                let op = self.rng.pick(&["<", "<=", ">", ">=", "==", "!="]);
                Ex::new(format!("{} {op} {}", a.operand(), b.operand()), Ty::Bool, 1.0)
            }
            2 => {
                let a = self.str_(ctx, d - 1);
                let b = self.str_(ctx, d - 1);
                let op = self.rng.pick(&["==", "!=", "<", ">="]);
                Ex::new(format!("{} {op} {}", a.operand(), b.operand()), Ty::Bool, 1.0)
            }
            3 => {
                let a = self.bool_(ctx, d - 1);
                let neg = self.rng.pick(&["not ", "!"]);
                Ex::new(format!("{neg}{}", a.operand()), Ty::Bool, 1.0)
            }
            4 => {
                let a = self.bool_(ctx, d - 1);
                let b = self.bool_(ctx, d - 1);
                let op = self.rng.pick(&["and", "or", "&&", "||"]);
                Ex::new(format!("{} {op} {}", a.operand(), b.operand()), Ty::Bool, 1.0)
            }
            5 => {
                let s = self.str_(ctx, d - 1);
                let m = self.rng.pick(&["start_with", "end_with"]);
                let p = self.rng.pick(&["\"a\"", "\"h\"", "\"\"", "\"o\"", "\"Fox\""]);
                if self.rng.chance(15) {
                    Ex::new(format!("{}.valid_encoding", s.operand()), Ty::Bool, 1.0).atom()
                } else {
                    Ex::new(format!("{}.{m}({p})", s.operand()), Ty::Bool, 1.0).atom()
                }
            }
            6 => {
                // `is` on a value of a known type.
                let (code, ty) = self.any_scalar(ctx, d - 1);
                // A variable is narrowed by `is` in what follows, so a
                // variable is only tested against its own type (an untyped
                // one would become a union); anything else may be tested
                // freely.
                let is_name = code.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '@');
                // A string variable may be `String?` to the analyzer (it
                // types `s[a..b]` so), and the `else` of `is String` would
                // then make it `nil`: test a copy instead.
                let code = if is_name && ty == Ty::Str { format!("({code} <> \"\")") } else { code };
                let is_name = is_name && ty != Ty::Str;
                let t = if is_name {
                    match ty {
                        Ty::Str => "String",
                        Ty::Bool => "Bool",
                        _ => "Number",
                    }
                } else {
                    self.rng.pick(&["Number", "String", "Bool", "Table", "nil", "Number | String", "~String"])
                };
                Ex::new(format!("{code} is {t}"), Ty::Bool, 1.0)
            }
            7 => match self.enum_val(ctx) {
                Some((e, i)) => {
                    let en = self.enums.get(i).cloned();
                    match en {
                        Some(en) if self.rng.chance(50) => {
                            let v = self.rng.below(en.variants.len());
                            let vn = en.variants.get(v).map(|x| x.0.clone()).unwrap_or_default();
                            Ex::new(format!("{} == {}.{vn}", e.operand(), en.name), Ty::Bool, 1.0)
                        }
                        Some(en) => Ex::new(format!("{} is {}", e.operand(), en.name), Ty::Bool, 1.0),
                        None => Ex::new("true", Ty::Bool, 1.0).atom(),
                    }
                }
                None => Ex::new("false", Ty::Bool, 1.0).atom(),
            },
            8 => {
                let (code, _) = self.any_scalar(ctx, d - 1);
                Ex::new(format!("({code}) as Bool"), Ty::Bool, 1.0)
            }
            10 => match self.extra_bool(ctx, d) {
                Some(e) => e,
                None => Ex::new("true", Ty::Bool, 1.0).atom(),
            },
            _ => match self.struct_val(ctx, d - 1) {
                Some((e, i)) => {
                    // Any struct in the program, the value's own or not.
                    let j = if self.rng.chance(50) { i } else { self.rng.below(self.structs.len()) };
                    let name = self.structs.get(j).map(|s| s.name.clone()).unwrap_or_default();
                    Ex::new(format!("{} is {name}", e.operand()), Ty::Bool, 1.0)
                }
                None => Ex::new("true", Ty::Bool, 1.0).atom(),
            },
        }
    }

    /// A value of a random scalar type, for `is`, `as Bool` and `typeof`.
    fn any_scalar(&mut self, ctx: &Ctx, d: u32) -> (String, Ty) {
        match self.rng.below(4) {
            0 => (self.int(ctx, d).operand(), Ty::Int),
            1 => (self.str_(ctx, d).operand(), Ty::Str),
            2 => (self.bool_(ctx, d).operand(), Ty::Bool),
            _ => (self.num(ctx, d).operand(), Ty::Num),
        }
    }

    // ---- lists and maps ----

    pub(super) fn int_list(&mut self, ctx: &Ctx, d: u32) -> Ex {
        if self.rng.chance(45) {
            if let Some(v) = self.var_ex(ctx, Ty::IntList) {
                return v;
            }
        }
        match self.rng.weighted(&[50, 10, 10, 10]) {
            0 | 1 if d == 0 => self.int_list_lit(ctx, 0),
            0 => self.int_list_lit(ctx, d - 1),
            1 => {
                let l = self.int_list(ctx, d - 1);
                let e = self.int_within(ctx, d - 1, 1000.0);
                if l.bound + 1.0 > LIST_CAP {
                    return self.int_list_lit(ctx, 0);
                }
                Ex::new(format!("[*{}, {}]", l.code, e.code), Ty::IntList, l.bound + 1.0).elem(l.elem.max(e.bound)).atom()
            }
            2 => {
                let s = self.str_(ctx, d.saturating_sub(1));
                let m = self.rng.pick(&["codepoints", "as_bytes"]);
                Ex::new(format!("{}.{m}", s.operand()), Ty::IntList, s.bound * 4.0).elem(1_114_111.0).atom()
            }
            _ => self.int_list_lit(ctx, d.saturating_sub(1)),
        }
    }

    pub(super) fn int_list_lit(&mut self, ctx: &Ctx, d: u32) -> Ex {
        let n = self.rng.below(5);
        let mut items = Vec::new();
        let mut eb: f64 = 0.0;
        for _ in 0..n {
            let e = self.int_within(ctx, d, 1000.0);
            eb = eb.max(e.bound);
            items.push(e.code);
        }
        let trailing = if n > 0 && self.rng.chance(15) { "," } else { "" };
        let mut e = Ex::new(format!("[{}{trailing}]", items.join(", ")), Ty::IntList, n as f64).elem(eb).atom();
        e.min_len = n;
        e
    }

    pub(super) fn str_list(&mut self, ctx: &Ctx, d: u32) -> Ex {
        if self.rng.chance(40) {
            if let Some(v) = self.var_ex(ctx, Ty::StrList) {
                return v;
            }
        }
        let dd = d.saturating_sub(1);
        match self.rng.below(5) {
            0 => {
                let s = self.str_(ctx, dd);
                Ex::new(format!("{}.chars", s.operand()), Ty::StrList, s.bound).elem(1.0).atom()
            }
            1 => {
                let s = self.str_(ctx, dd);
                let sep = self.rng.pick(&["\",\"", "\" \"", "\"l\""]);
                Ex::new(format!("{}.split({sep})", s.operand()), Ty::StrList, s.bound + 1.0).elem(s.bound).atom()
            }
            2 => {
                let s = self.str_(ctx, dd);
                Ex::new(format!("{}.split", s.operand()), Ty::StrList, s.bound + 1.0).elem(s.bound).atom()
            }
            _ => self.str_list_lit(ctx, dd),
        }
    }

    pub(super) fn str_list_lit(&mut self, ctx: &Ctx, d: u32) -> Ex {
        let n = self.rng.below(4);
        let mut items = Vec::new();
        let mut eb: f64 = 0.0;
        for _ in 0..n {
            let e = self.str_(ctx, d);
            eb = eb.max(e.bound);
            items.push(e.code);
        }
        let mut e = Ex::new(format!("[{}]", items.join(", ")), Ty::StrList, n as f64).elem(eb).atom();
        e.min_len = n;
        e
    }

    pub(super) fn map_(&mut self, ctx: &Ctx, d: u32) -> Ex {
        if self.rng.chance(55) {
            if let Some(v) = self.var_ex(ctx, Ty::Map) {
                return v;
            }
        }
        self.map_lit(ctx, d.saturating_sub(1))
    }

    pub(super) fn map_lit(&mut self, ctx: &Ctx, d: u32) -> Ex {
        let n = self.rng.below(4);
        let mut keys: Vec<&str> = Vec::new();
        let mut items = Vec::new();
        let mut eb: f64 = 0.0;
        for _ in 0..n {
            let k = self.rng.pick(MAP_KEYS);
            if keys.contains(&k) {
                continue;
            }
            keys.push(k);
            let e = self.int_within(ctx, d, 1000.0);
            eb = eb.max(e.bound);
            // A key is a word, or a plain string.
            let key = if self.rng.chance(20) { format!("\"{k}\"") } else { k.to_string() };
            items.push(format!("{key}: {}", e.code));
        }
        Ex::new(format!("{{{}}}", items.join(", ")), Ty::Map, keys.len() as f64).elem(eb.max(1000.0)).atom()
    }

    // ---- structs and enums ----

    /// A struct value: a variable or a literal. Returns its struct.
    pub(super) fn struct_val(&mut self, ctx: &Ctx, d: u32) -> Option<(Ex, usize)> {
        if self.structs.is_empty() {
            return None;
        }
        let i = self.rng.below(self.structs.len());
        if self.rng.chance(60) {
            if let Some(v) = self.pick_var(Ty::Struct(i), false) {
                return Some((Ex::new(v.name, Ty::Struct(i), 0.0).atom(), i));
            }
        }
        if self.rng.chance(15) {
            if let Some(m) = self.made_struct(ctx) {
                return Some(m);
            }
        }
        if self.structs.get(i).is_some_and(|s| s.plus) && d > 0 && self.rng.chance(20) {
            let (a, _) = self.struct_lit(ctx, i, d - 1)?;
            let (b, _) = self.struct_lit(ctx, i, d - 1)?;
            return Some((Ex::new(format!("{} + {}", a.operand(), b.operand()), Ty::Struct(i), 0.0), i));
        }
        self.struct_lit(ctx, i, d.saturating_sub(1))
    }

    /// `Name { field: value, ... }`, with every field that has no default
    /// and some that have one.
    pub(super) fn struct_lit(&mut self, ctx: &Ctx, i: usize, d: u32) -> Option<(Ex, usize)> {
        let name = self.structs.get(i)?.name.clone();
        let fields = self.all_fields(i);
        let mut inits = Vec::new();
        for f in fields {
            if f.default.is_some() && self.rng.chance(50) {
                continue;
            }
            let v = match f.ty {
                Ty::Str if f.typed => non_nil(&self.str_(ctx, d)),
                Ty::Str => self.str_(ctx, d).code,
                _ => self.int_within(ctx, d, f.bound).code,
            };
            inits.push(format!("{}: {v}", f.name));
        }
        let body = if inits.is_empty() { "{}".to_string() } else { format!("{{ {} }}", inits.join(", ")) };
        Some((Ex::new(format!("{name} {body}"), Ty::Struct(i), 0.0).atom(), i))
    }

    /// An integer read from a struct: a field, a method call, `len`.
    fn struct_int(&mut self, ctx: &Ctx, d: u32) -> Option<Ex> {
        let (s, i) = self.struct_val(ctx, d.saturating_sub(1))?;
        let fields: Vec<Field> = self.all_fields(i).into_iter().filter(|f| f.ty == Ty::Int).collect();
        let methods: Vec<Method> = self.all_methods(i).into_iter().filter(|m| m.ret.is_some()).collect();
        let len = self.structs.get(i).and_then(|st| st.len);
        let choice = self.rng.below(3);
        if choice == 1 && !methods.is_empty() {
            let m = self.rng.pick_ref(&methods)?;
            if !self.affordable(m.cost * ctx.mult) {
                return None;
            }
            self.add_cost(m.cost * ctx.mult);
            let args: Vec<String> = m.params.iter().map(|&b| self.int_within(ctx, d.saturating_sub(1), b).code).collect();
            // A method without arguments may be auto-called.
            let code = if args.is_empty() && self.rng.chance(40) { format!("{}.{}", s.operand(), m.name) } else { format!("{}.{}({})", s.operand(), m.name, args.join(", ")) };
            return Some(Ex::new(code, Ty::Int, m.ret.unwrap_or(1000.0)).atom());
        }
        if choice == 2 {
            if let Some(b) = len {
                return Some(Ex::new(format!("{}.len", s.operand()), Ty::Int, b).atom());
            }
        }
        let f = self.rng.pick_ref(&fields)?;
        Some(Ex::new(format!("{}.{}", s.operand(), f.name), Ty::Int, f.bound).atom())
    }

    fn struct_with_to_string(&mut self, ctx: &Ctx) -> Option<Ex> {
        let cands: Vec<usize> = (0..self.structs.len()).filter(|&i| self.inherits_to_string(i)).collect();
        let &i = cands.get(self.rng.below(cands.len()))?;
        if self.rng.chance(50) {
            if let Some(v) = self.pick_var(Ty::Struct(i), false) {
                return Some(Ex::new(v.name, Ty::Struct(i), 0.0).atom());
            }
        }
        self.struct_lit(ctx, i, 0).map(|(e, _)| e)
    }

    pub(super) fn inherits_to_string(&self, i: usize) -> bool {
        let mut cur = Some(i);
        let mut n = 0;
        while let Some(c) = cur {
            let Some(s) = self.structs.get(c) else { return false };
            if s.to_string {
                return true;
            }
            cur = s.parent;
            n += 1;
            if n > 16 {
                return false;
            }
        }
        false
    }

    /// An enum value: a variable or a variant. Returns its enum.
    pub(super) fn enum_val(&mut self, _ctx: &Ctx) -> Option<(Ex, usize)> {
        if self.enums.is_empty() {
            return None;
        }
        let i = self.rng.below(self.enums.len());
        if self.rng.chance(50) {
            if let Some(v) = self.pick_var(Ty::Enum(i), false) {
                return Some((Ex::new(v.name, Ty::Enum(i), 0.0).atom(), i));
            }
        }
        let e = self.enums.get(i)?;
        let (vn, _) = e.variants.get(self.rng.below(e.variants.len()))?;
        Some((Ex::new(format!("{}.{vn}", e.name), Ty::Enum(i), 0.0).atom(), i))
    }

    /// An expression of any type the generator tracks.
    pub(super) fn expr(&mut self, ctx: &Ctx, ty: Ty, d: u32) -> Ex {
        match ty {
            Ty::Int => self.int(ctx, d),
            Ty::Num => self.num(ctx, d),
            Ty::Str => self.str_(ctx, d),
            Ty::Bool => self.bool_(ctx, d),
            Ty::IntList => self.int_list(ctx, d),
            Ty::StrList => self.str_list(ctx, d),
            Ty::Map => self.map_(ctx, d),
            Ty::Struct(i) => match self.struct_lit(ctx, i, d) {
                Some((e, _)) => e,
                None => Ex::new("nil", ty, 0.0),
            },
            Ty::Enum(i) => {
                let e = self.enums.get(i).and_then(|e| e.variants.first().map(|v| format!("{}.{}", e.name, v.0))).unwrap_or_default();
                Ex::new(e, ty, 0.0).atom()
            }
        }
    }

    // ---- calls ----

    /// A call of a visible function that returns one value of the type.
    fn call_returning(&mut self, ctx: &Ctx, ty: Ty) -> Option<Ex> {
        let fs = self.callable(ctx, |f| matches!(f.ret, Ret::One(t, _) if t == ty) && f.block.is_none());
        let f = self.rng.pick_ref(&fs)?;
        let e = self.call_text(ctx, &f)?;
        self.note_call(&f, ctx);
        Some(e)
    }

    /// The text of a call of `f`, with arguments within its parameters'
    /// bounds; the type and bound of its result. Does not note the call.
    pub(super) fn call_text(&mut self, ctx: &Ctx, f: &Func) -> Option<Ex> {
        if f.block.is_some() {
            return None;
        }
        let mut args: Vec<String> = Vec::new();
        let mut omit_from = f.params.len();
        // Trailing parameters with defaults may be left out.
        if let Some(last) = f.params.last() {
            if last.default.is_some() && self.rng.chance(50) {
                omit_from -= 1;
            }
        }
        for p in f.params.iter().take(omit_from) {
            let a = self.arg_for(ctx, p, f.typed);
            args.push(a);
        }
        if f.rest && omit_from == f.params.len() {
            for _ in 0..self.rng.below(4) {
                args.push(self.int_within(ctx, 1, 1000.0).code);
            }
        }
        let mut keyed = Vec::new();
        for k in &f.opts {
            if self.rng.chance(60) {
                let v = self.int_within(ctx, 1, 1000.0);
                keyed.push(format!("{k}: {}", v.code));
            }
        }
        // Without arguments a function-kind name may be written bare: its
        // value is used, so it is called.
        let all: Vec<String> = args.into_iter().chain(keyed).collect();
        let code = if all.is_empty() && f.kind != FnKind::Value && self.rng.chance(25) && omit_from == f.params.len() { f.name.clone() } else { format!("{}({})", f.name, all.join(", ")) };
        Some(match f.ret {
            Ret::One(ty, b) => Ex::new(code, ty, b).atom(),
            Ret::Pair(b) => Ex::new(code, Ty::Int, b).atom(),
            Ret::Nothing => Ex::new(code, Ty::Int, 0.0).atom(),
        })
    }

    fn arg_for(&mut self, ctx: &Ctx, p: &Param, typed: bool) -> String {
        match p.ty {
            Ty::Str if typed => non_nil(&self.str_(ctx, 1)),
            Ty::Str => self.str_(ctx, 1).code,
            Ty::IntList => self.int_list(ctx, 1).code,
            _ => self.int_within(ctx, 1, p.bound).code,
        }
    }

    /// `(x |> f)` or `(x |> f(y))`.
    fn pipe_call(&mut self, ctx: &Ctx) -> Option<Ex> {
        let fs = self.callable(ctx, |f| matches!(f.ret, Ret::One(Ty::Int, _)) && f.block.is_none() && f.kind != FnKind::Value && f.params.first().is_some_and(|p| p.ty == Ty::Int));
        let f = self.rng.pick_ref(&fs)?;
        let first = f.params.first()?.clone();
        let x = self.int_within(ctx, 1, first.bound);
        let mut rest = Vec::new();
        for p in f.params.iter().skip(1) {
            if p.default.is_some() && self.rng.chance(50) {
                break;
            }
            let a = self.arg_for(ctx, p, f.typed);
            rest.push(a);
        }
        self.note_call(&f, ctx);
        let b = match f.ret {
            Ret::One(_, b) => b,
            _ => 1000.0,
        };
        let code = if rest.is_empty() && self.rng.chance(50) { format!("({} |> {})", x.code, f.name) } else { format!("({} |> {}({}))", x.code, f.name, rest.join(", ")) };
        Some(Ex::new(code, Ty::Int, b).atom())
    }

    /// A call of a block-taking function with an inline `fn` or short
    /// function, or a function passed with `!`.
    fn block_call_inline(&mut self, ctx: &Ctx) -> Option<Ex> {
        let fs = self.callable(ctx, |f| f.block.is_some_and(|(p, _)| p > 0.0));
        let f = self.rng.pick_ref(&fs)?;
        let (pb, qb) = f.block?;
        let x = self.int_within(ctx, 1, f.params.first().map_or(pb, |p| p.bound));
        let cands = self.callable(ctx, |g| g.kind == FnKind::Decl && g.block.is_none() && g.params.len() == 1 && g.params.first().is_some_and(|p| p.ty == Ty::Int && p.bound >= pb) && !g.rest && g.opts.is_empty() && matches!(g.ret, Ret::One(Ty::Int, b) if b <= qb));
        let cb = match self.rng.pick_ref(&cands) {
            Some(g) if self.rng.chance(50) => {
                self.note_call(&g, &Ctx { mult: ctx.mult * 4.0, ..ctx.clone() });
                format!("{}!", g.name)
            }
            _ => {
                let v = self.fresh("n");
                let k = self.rng.range(1, 9);
                let body = match self.rng.below(3) {
                    0 => format!("({v} * {k}) % {}", bound_lit(qb)),
                    1 => format!("({v} + {k}) % {}", bound_lit(qb)),
                    _ => format!("{v} // {k}"),
                };
                self.fn_literal(&[v], &body, false)
            }
        };
        self.note_call(&f, ctx);
        let b = match f.ret {
            Ret::One(_, b) => b,
            _ => 1000.0,
        };
        Some(Ex::new(format!("{}({}, {cb})", f.name, x.code), Ty::Int, b).atom())
    }

    // ---- block expressions ----

    /// A value for the end of a function or a branch: sometimes a block
    /// expression spanning lines.
    pub(super) fn tail_expr(&mut self, ctx: &Ctx, ty: Ty) -> Ex {
        if ctx.depth < 3 && self.rng.chance(20) {
            if let Some(e) = self.rich(ctx, ty) {
                return e;
            }
        }
        self.expr(ctx, ty, 2)
    }

    /// A multi-line expression of the type, for a `let`, an assignment or
    /// a tail: `if`, `unless`, `case`, `cond`, `try`, or a call with a
    /// trailing block.
    pub(super) fn rich(&mut self, ctx: &Ctx, ty: Ty) -> Option<Ex> {
        if ctx.depth > 3 {
            return None;
        }
        let inner = ctx.nested();
        match self.rng.weighted(&[30, 8, 25, 10, 20, 15]) {
            0 => {
                let c = self.bool_(ctx, 2);
                let (b1, e1) = self.block(&inner, 2, Some(ty));
                let mut text = format!("if {}\n{b1}\n", c.code);
                let mut bound = e1.map_or(0.0, |e| e.bound);
                let mut ascii = true;
                if self.rng.chance(40) {
                    let c2 = self.bool_(ctx, 2);
                    let (b2, e2) = self.block(&inner, 1, Some(ty));
                    bound = bound.max(e2.as_ref().map_or(0.0, |e| e.bound));
                    ascii &= e2.is_none_or(|e| e.ascii);
                    text.push_str(&format!("elseif {}\n{b2}\n", c2.code));
                }
                let (b3, e3) = self.block(&inner, 1, Some(ty));
                bound = bound.max(e3.as_ref().map_or(0.0, |e| e.bound));
                ascii &= e3.is_none_or(|e| e.ascii);
                text.push_str(&format!("else\n{b3}\nend"));
                Some(Ex::new(text, ty, bound).ascii(ascii).atom())
            }
            1 => {
                let c = self.bool_(ctx, 2);
                let (b1, e1) = self.block(&inner, 1, Some(ty));
                let (b2, e2) = self.block(&inner, 1, Some(ty));
                let bound = e1.as_ref().map_or(0.0, |e| e.bound).max(e2.as_ref().map_or(0.0, |e| e.bound));
                let ascii = e1.is_none_or(|e| e.ascii) && e2.is_none_or(|e| e.ascii);
                Some(Ex::new(format!("unless {}\n{b1}\nelse\n{b2}\nend", c.code), ty, bound).ascii(ascii).atom())
            }
            2 => self.case_expr(ctx, ty),
            3 => {
                let n = 1 + self.rng.below(3);
                let mut text = "cond\n".to_string();
                let mut bound: f64 = 0.0;
                let mut ascii = true;
                for _ in 0..n {
                    let c = self.bool_(ctx, 2);
                    let (b, e) = self.block(&inner, 1, Some(ty));
                    bound = bound.max(e.as_ref().map_or(0.0, |e| e.bound));
                    ascii &= e.is_none_or(|e| e.ascii);
                    text.push_str(&format!("when {}\n{b}\n", c.code));
                }
                let (b, e) = self.block(&inner, 1, Some(ty));
                bound = bound.max(e.as_ref().map_or(0.0, |e| e.bound));
                ascii &= e.is_none_or(|e| e.ascii);
                text.push_str(&format!("else\n{b}\nend"));
                Some(Ex::new(text, ty, bound).ascii(ascii).atom())
            }
            4 => self.try_expr(ctx, Some(ty)).map(|(t, b, a)| Ex::new(t, ty, b).ascii(a).atom()),
            _ => {
                if ty != Ty::Int {
                    return None;
                }
                self.block_call(ctx)
            }
        }
    }

    /// `case` as a value, over an integer, a string or an enum value, with
    /// an `else` so that it always yields one.
    fn case_expr(&mut self, ctx: &Ctx, ty: Ty) -> Option<Ex> {
        let inner = ctx.nested();
        let (subject, arms) = self.case_arms(ctx)?;
        let mut text = format!("case {subject}\n");
        let mut bound: f64 = 0.0;
        let mut ascii = true;
        for arm in arms {
            let (b, e) = self.block(&inner, 1, Some(ty));
            bound = bound.max(e.as_ref().map_or(0.0, |e| e.bound));
            ascii &= e.is_none_or(|e| e.ascii);
            text.push_str(&format!("when {arm}\n{b}\n"));
        }
        let (b, e) = self.block(&inner, 1, Some(ty));
        bound = bound.max(e.as_ref().map_or(0.0, |e| e.bound));
        ascii &= e.is_none_or(|e| e.ascii);
        text.push_str(&format!("else\n{b}\nend"));
        Some(Ex::new(text, ty, bound).ascii(ascii).atom())
    }

    /// A subject and the items of its `when` clauses: literals, ranges,
    /// variables compared with `==`, enum variants.
    pub(super) fn case_arms(&mut self, ctx: &Ctx) -> Option<(String, Vec<String>)> {
        let n = 1 + self.rng.below(3);
        let mut arms = Vec::new();
        match self.rng.below(4) {
            0 | 1 => {
                let s = self.int(ctx, 2);
                let subject = fit(s, 12.0).code;
                for _ in 0..n {
                    arms.push(match self.rng.below(4) {
                        0 => {
                            let a = self.rng.range(-2, 12);
                            let b = self.rng.range(-2, 12);
                            format!("{a}, {b}")
                        }
                        1 => {
                            let a = self.rng.range(-2, 8);
                            let b = a + self.rng.range(0, 5);
                            let op = self.rng.pick(&["..", "..."]);
                            format!("{a}{op}{b}")
                        }
                        2 => match self.var_ex(ctx, Ty::Int) {
                            Some(v) => v.code,
                            None => "3".into(),
                        },
                        _ => format!("{}", self.rng.range(0, 11)),
                    });
                }
                Some((subject, arms))
            }
            2 => {
                let s = self.str_(ctx, 2);
                for _ in 0..n {
                    let a = self.rng.pick(&["\"a\"", "\"hello\"", "\"\"", "\"Fox\"", "\"x y z\""]);
                    arms.push(if self.rng.chance(30) { format!("{a}, \"ab\"") } else { a.to_string() });
                }
                Some((s.code, arms))
            }
            _ => {
                let (e, i) = self.enum_val(ctx)?;
                let en = self.enums.get(i)?.clone();
                for _ in 0..n {
                    let v = en.variants.get(self.rng.below(en.variants.len()))?.0.clone();
                    let w = en.variants.get(self.rng.below(en.variants.len()))?.0.clone();
                    arms.push(if self.rng.chance(30) && v != w { format!("{}.{v}, {}.{w}", en.name, en.name) } else { format!("{}.{v}", en.name) });
                }
                Some((e.code, arms))
            }
        }
    }

    /// A call with a trailing block: `f(x) do |n| ... end` or
    /// the command form `f x do |n| ... end`.
    pub(super) fn block_call(&mut self, ctx: &Ctx) -> Option<Ex> {
        let fs = self.callable(ctx, |f| f.block.is_some_and(|(p, _)| p > 0.0));
        let f = self.rng.pick_ref(&fs)?;
        let (pb, qb) = f.block?;
        let x = self.int_within(ctx, 1, f.params.first().map_or(pb, |p| p.bound));
        let n = self.fresh("n");
        let params = vec![Param { name: n.clone(), ty: Ty::Int, bound: pb, default: None }];
        // The block's result must stay within what the function expects.
        let may_raise = ctx.raise_ok && self.rng.chance(30);
        let (body, frame) = self.fn_body(ctx, &params, &[], Ret::One(Ty::Int, 0.0), 2, qb, may_raise);
        self.block_raises(ctx, &frame);
        let body = if body.is_empty() { "  0".to_string() } else { body };
        self.add_cost((frame.cost + 1.0) * 4.0 * ctx.mult);
        self.note_call(&f, ctx);
        let fb = match f.ret {
            Ret::One(_, b) => b,
            _ => 1000.0,
        };
        let head = if self.rng.chance(50) && !x.code.starts_with('(') && !x.code.starts_with('-') && !x.code.starts_with('[') { format!("{} {}", f.name, x.code) } else { format!("{}({})", f.name, x.code) };
        Some(Ex::new(format!("{head} do |{n}|\n{body}\nend"), Ty::Int, fb))
    }
}

/// A number about to become text, as `x + 0`: adding zero turns `-0`
/// into `0` and leaves everything else as it is (the module doc says why
/// the sign of zero is not stable on a dual-number LuaJIT).
fn unsigned_zero(e: &Ex) -> String {
    format!("{} + 0", e.operand())
}

/// A literal operand: negative numbers in parentheses.
fn lit(k: i64) -> String {
    if k < 0 { format!("({k})") } else { k.to_string() }
}
