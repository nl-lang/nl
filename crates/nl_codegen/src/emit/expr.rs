//! Expressions: discharge of expression descriptors to registers, stores,
//! branches, operators, calls, closures and table constructors (the
//! `expr_*` and `bcemit_*` functions of `src/lj_parse.c`).

use nl_ir::ir::*;

use super::{Exp, Gen, Key, Kind, R, VarId};
use crate::bc::{self, Ins, NO_JMP, NO_REG, Op};
use crate::proto::{KTabValue, KTable};

/// The names of a member call that go through `rt.member.name` rather
/// than `rt.mcall`: Lua identifiers that are not keywords, as the Lua
/// backend decides (`nl_luagen`).
fn is_lua_identifier(s: &[u8]) -> bool {
    const KEYWORDS: &[&[u8]] = &[
        b"and", b"break", b"do", b"else", b"elseif", b"end", b"false", b"for", b"function", b"goto", b"if", b"in", b"local",
        b"nil", b"not", b"or", b"repeat", b"return", b"then", b"true", b"until", b"while",
    ];
    let Some((first, rest)) = s.split_first() else { return false };
    (first.is_ascii_alphabetic() || *first == b'_')
        && rest.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'_')
        && !KEYWORDS.contains(&s)
}

/// The integer a number literal fits in `KSHORT`.
fn short_int(n: f64) -> Option<i16> {
    if n.fract() == 0.0 && n >= f64::from(i16::MIN) && n <= f64::from(i16::MAX) && !(n == 0.0 && n.is_sign_negative()) {
        Some(n as i16)
    } else {
        None
    }
}

/// `checku8` on an integral number.
fn byte_int(n: f64) -> Option<u8> {
    if n.fract() == 0.0 && (0.0..=255.0).contains(&n) && !(n == 0.0 && n.is_sign_negative()) { Some(n as u8) } else { None }
}

/// `lj_vm_foldarith` for the operators the parser folds.
fn fold_arith(op: BinOp, x: f64, y: f64) -> Option<f64> {
    let r = match op {
        BinOp::Add => x + y,
        BinOp::Sub => x - y,
        BinOp::Mul => x * y,
        BinOp::Div => x / y,
        BinOp::Mod => x - (x / y).floor() * y,
        BinOp::Pow => x.powf(y),
        _ => return None,
    };
    // NaN and -0 are never constants.
    if r.is_nan() || (r == 0.0 && r.is_sign_negative()) { None } else { Some(r) }
}

/// The bit operator that a numeric intrinsic of nl_ir is at level 1:
/// `nband` and its siblings for a binary operator on two proven numbers
/// (`binary_op` in crates/nl_ir/src/lower/operator.rs), `nbnot` for `~`
/// (`prefix` there). They are the `bit` functions the runtime holds
/// (runtime/nl_40_ops.lua), and the opcodes compute what those functions
/// compute for numbers.
fn bit_intrinsic(name: &str) -> Option<Op> {
    Some(match name {
        "nband" => Op::BAND,
        "nbor" => Op::BOR,
        "nbxor" => Op::BXOR,
        "nlshift" => Op::BSHL,
        "nrshift" => Op::BSHR,
        "narshift" => Op::BSAR,
        "nbnot" => Op::BNOT,
        _ => return None,
    })
}

/// `lj_num2bit`: a number to 32 bits as the `bit` library and the bit
/// opcodes convert it, by adding 2^52 + 2^51 and keeping the low word.
fn tobit(n: f64) -> i32 {
    (n + 6_755_399_441_055_744.0).to_bits() as u32 as i32
}

/// `foldbitop` (and the `BNOT` case of `bcemit_unop`) in lj_parse.c.
fn fold_bitop(op: Op, x: f64, y: f64) -> f64 {
    let (a, b) = (tobit(x), tobit(y));
    let sh = (b & 31) as u32;
    f64::from(match op {
        Op::BAND => a & b,
        Op::BOR => a | b,
        Op::BXOR => a ^ b,
        Op::BSHL => a.wrapping_shl(sh),
        Op::BSHR => ((a as u32) >> sh) as i32,
        Op::BSAR => a >> sh,
        _ => !a,
    })
}

impl Gen<'_> {
    // ---- discharge -------------------------------------------------------

    /// Turns a variable or indexed expression into an instruction with an
    /// open destination (`expr_discharge`).
    pub(crate) fn discharge(&mut self, e: &mut Exp) -> R<()> {
        let ins = match &e.k {
            Kind::Upval(idx) => bc::ad(Op::UGET, 0, *idx),
            Kind::Global(name) => {
                let k = self.const_str(name)?;
                bc::ad(Op::GGET, 0, k)
            }
            Kind::Indexed(tab, key) => {
                let (tab, key) = (*tab, *key);
                let ins = match key {
                    Key::Str(k) => bc::abc(Op::TGETS, 0, tab, k),
                    Key::Byte(b) => bc::abc(Op::TGETB, 0, tab, u32::from(b)),
                    Key::Reg(r) => {
                        self.free_reg(r);
                        bc::abc(Op::TGETV, 0, tab, r)
                    }
                };
                self.free_reg(tab);
                ins
            }
            Kind::Call(_, base) => {
                e.k = Kind::NonReloc(*base);
                return Ok(());
            }
            Kind::Local(slot) => {
                e.k = Kind::NonReloc(*slot);
                return Ok(());
            }
            _ => return Ok(()),
        };
        let pc = self.emit(ins)?;
        e.k = Kind::Reloc(pc);
        Ok(())
    }

    /// Sets `n` registers from `from` to nil (`bcemit_nil`), merging with
    /// a preceding `KPRI`/`KNIL`.
    pub(crate) fn emit_nil(&mut self, mut from: u32, mut n: u32) -> R<()> {
        let pc = self.fs().pc();
        if pc > self.fs().lasttarget && pc > 0 {
            let ip = self.ins_at(pc - 1);
            let pfrom = bc::a_of(ip);
            match Op::from_ins(ip) {
                Some(Op::KPRI) if bc::d_of(ip) == bc::PRI_NIL => {
                    if from == pfrom {
                        if n == 1 {
                            return Ok(());
                        }
                    } else if from == pfrom + 1 {
                        from = pfrom;
                        n += 1;
                    } else {
                        return self.emit_nil_ins(from, n);
                    }
                    self.set_ins(pc - 1, bc::ad(Op::KNIL, from, from + n - 1));
                    return Ok(());
                }
                Some(Op::KNIL) => {
                    let pto = bc::d_of(ip);
                    if pfrom <= from && from <= pto + 1 {
                        if from + n - 1 > pto {
                            self.patch_ins(pc - 1, |i| bc::set_d(i, from + n - 1));
                        }
                        return Ok(());
                    }
                }
                _ => {}
            }
        }
        self.emit_nil_ins(from, n)
    }

    fn emit_nil_ins(&mut self, from: u32, n: u32) -> R<()> {
        if n == 1 {
            self.emit_ad(Op::KPRI, from, bc::PRI_NIL)?;
        } else {
            self.emit_ad(Op::KNIL, from, from + n - 1)?;
        }
        Ok(())
    }

    /// Discharges to a register, ignoring branches
    /// (`expr_toreg_nobranch`).
    fn toreg_nobranch(&mut self, e: &mut Exp, reg: u32) -> R<()> {
        self.discharge(e)?;
        let ins = match &e.k {
            Kind::Str(s) => {
                let k = self.const_str(s)?;
                bc::ad(Op::KSTR, reg, k)
            }
            Kind::Num(n) => match short_int(*n) {
                Some(k) => bc::ad(Op::KSHORT, reg, u32::from(k as u16)),
                None => {
                    let k = self.const_num(*n)?;
                    bc::ad(Op::KNUM, reg, k)
                }
            },
            Kind::Reloc(pc) => {
                let pc = *pc;
                self.patch_ins(pc, |i| bc::set_a(i, reg));
                e.k = Kind::NonReloc(reg);
                return Ok(());
            }
            Kind::NonReloc(r) => {
                if *r == reg {
                    return Ok(());
                }
                bc::ad(Op::MOV, reg, *r)
            }
            Kind::Nil => {
                self.emit_nil(reg, 1)?;
                e.k = Kind::NonReloc(reg);
                return Ok(());
            }
            Kind::False | Kind::True => bc::ad(Op::KPRI, reg, e.pri()),
            // Void and Jmp: nothing to load.
            _ => return Ok(()),
        };
        self.emit(ins)?;
        e.k = Kind::NonReloc(reg);
        Ok(())
    }

    /// Discharges to a register, resolving its branches (`expr_toreg`).
    pub(crate) fn toreg(&mut self, e: &mut Exp, reg: u32) -> R<()> {
        self.toreg_nobranch(e, reg)?;
        if let Kind::Jmp(pc) = e.k {
            let mut t = e.t;
            self.jmp_append(&mut t, pc)?;
            e.t = t;
        }
        if e.has_jump() {
            let mut jfalse = NO_JMP;
            let mut jtrue = NO_JMP;
            if self.jmp_novalue(e.t) || self.jmp_novalue(e.f) {
                let jval = if matches!(e.k, Kind::Jmp(_)) { NO_JMP } else { self.emit_jmp()? };
                jfalse = self.emit_ad(Op::KPRI, reg, bc::PRI_FALSE)?;
                let a = self.fs().freereg;
                self.emit_aj(Op::JMP, a, 1)?;
                jtrue = self.emit_ad(Op::KPRI, reg, bc::PRI_TRUE)?;
                self.jmp_tohere(jval)?;
            }
            let jend = self.fs().pc();
            self.fs().lasttarget = jend;
            self.jmp_patchval(e.f, jend, reg, jfalse)?;
            self.jmp_patchval(e.t, jend, reg, jtrue)?;
        }
        e.t = NO_JMP;
        e.f = NO_JMP;
        e.k = Kind::NonReloc(reg);
        Ok(())
    }

    /// Discharges to the next free register (`expr_tonextreg`).
    pub(crate) fn tonextreg(&mut self, e: &mut Exp) -> R<()> {
        self.discharge(e)?;
        self.free_exp(e);
        self.reserve(1)?;
        let reg = self.fs().freereg - 1;
        self.toreg(e, reg)
    }

    /// Discharges to any register and returns it (`expr_toanyreg`).
    pub(crate) fn toanyreg(&mut self, e: &mut Exp) -> R<u32> {
        self.discharge(e)?;
        if let Kind::NonReloc(r) = e.k {
            if !e.has_jump() {
                return Ok(r);
            }
            if r >= self.fs().nactvar {
                self.toreg(e, r)?;
                return Ok(r);
            }
        }
        self.tonextreg(e)?;
        Ok(e.reg().unwrap_or(0))
    }

    /// Discharges to a value, resolving branches only when there are any
    /// (`expr_toval`).
    pub(crate) fn toval(&mut self, e: &mut Exp) -> R<()> {
        if e.has_jump() {
            self.toanyreg(e)?;
            Ok(())
        } else {
            self.discharge(e)
        }
    }

    // ---- stores ----------------------------------------------------------

    /// Stores `e` into the variable `var` (`bcemit_store`).
    pub(crate) fn store(&mut self, var: &Exp, e: &mut Exp) -> R<()> {
        let ins = match &var.k {
            Kind::Local(slot) => {
                let slot = *slot;
                self.free_exp(e);
                return self.toreg(e, slot);
            }
            Kind::Upval(uv) => {
                let uv = *uv;
                self.toval(e)?;
                match &e.k {
                    Kind::Nil | Kind::False | Kind::True => bc::ad(Op::USETP, uv, e.pri()),
                    Kind::Str(s) => {
                        let k = self.const_str(s)?;
                        bc::ad(Op::USETS, uv, k)
                    }
                    Kind::Num(n) => {
                        let k = self.const_num(*n)?;
                        bc::ad(Op::USETN, uv, k)
                    }
                    _ => {
                        let r = self.toanyreg(e)?;
                        bc::ad(Op::USETV, uv, r)
                    }
                }
            }
            Kind::Global(name) => {
                let ra = self.toanyreg(e)?;
                let k = self.const_str(name)?;
                bc::ad(Op::GSET, ra, k)
            }
            Kind::Indexed(tab, key) => {
                let (tab, key) = (*tab, *key);
                let ra = self.toanyreg(e)?;
                match key {
                    Key::Str(k) => bc::abc(Op::TSETS, ra, tab, k),
                    Key::Byte(b) => bc::abc(Op::TSETB, ra, tab, u32::from(b)),
                    Key::Reg(r) => bc::abc(Op::TSETV, ra, tab, r),
                }
            }
            _ => return Err(self.err("internal error: assignment to a value")),
        };
        self.emit(ins)?;
        self.free_exp(e);
        Ok(())
    }

    /// Makes `t` the indexed expression `t[key]` (`expr_index`); `key` is
    /// already a value.
    pub(crate) fn index(&mut self, t: &mut Exp, key: &mut Exp) -> R<()> {
        let tab = t.reg().unwrap_or(0);
        let k = match &key.k {
            Kind::Num(n) => match byte_int(*n) {
                Some(b) => Key::Byte(b),
                None => Key::Reg(self.toanyreg(key)?),
            },
            Kind::Str(s) => {
                let idx = self.const_str(s)?;
                if idx <= bc::BCMAX_ABC { Key::Str(idx) } else { Key::Reg(self.toanyreg(key)?) }
            }
            _ => Key::Reg(self.toanyreg(key)?),
        };
        t.k = Kind::Indexed(tab, k);
        Ok(())
    }

    // ---- branches --------------------------------------------------------

    fn invert_cond(&mut self, e: &Exp) {
        if let Kind::Jmp(pc) = e.k {
            self.patch_ins(pc.wrapping_sub(1), |i| {
                let op = bc::op_of(*i) ^ 1;
                *i = (*i & !0xff) | u32::from(op);
            });
        }
    }

    /// Emits a conditional branch (`bcemit_branch`).
    fn branch(&mut self, e: &mut Exp, cond: bool) -> R<u32> {
        if let Kind::Reloc(pc) = e.k {
            let ip = self.ins_at(pc);
            if Op::from_ins(ip) == Some(Op::NOT) {
                self.set_ins(pc, bc::ad(if cond { Op::ISF } else { Op::IST }, 0, bc::d_of(ip)));
                return self.emit_jmp();
            }
        }
        if !matches!(e.k, Kind::NonReloc(_)) {
            self.reserve(1)?;
            let reg = self.fs().freereg - 1;
            self.toreg_nobranch(e, reg)?;
        }
        let r = e.reg().unwrap_or(0);
        self.emit_ad(if cond { Op::ISTC } else { Op::ISFC }, NO_REG, r)?;
        let pc = self.emit_jmp()?;
        self.free_exp(e);
        Ok(pc)
    }

    /// Branches when `e` is true (`bcemit_branch_t`): afterwards, `e.f`
    /// lists the jumps taken when it is false.
    pub(crate) fn branch_t(&mut self, e: &mut Exp) -> R<()> {
        self.discharge(e)?;
        let pc = match e.k {
            Kind::Str(_) | Kind::Num(_) | Kind::True => NO_JMP,
            Kind::Jmp(pc) => {
                self.invert_cond(e);
                pc
            }
            Kind::False | Kind::Nil => {
                self.toreg_nobranch(e, NO_REG)?;
                self.emit_jmp()?
            }
            _ => self.branch(e, false)?,
        };
        let mut f = e.f;
        self.jmp_append(&mut f, pc)?;
        e.f = f;
        self.jmp_tohere(e.t)?;
        e.t = NO_JMP;
        Ok(())
    }

    /// Branches when `e` is false (`bcemit_branch_f`).
    pub(crate) fn branch_f(&mut self, e: &mut Exp) -> R<()> {
        self.discharge(e)?;
        let pc = match e.k {
            Kind::Nil | Kind::False => NO_JMP,
            Kind::Jmp(pc) => pc,
            Kind::Str(_) | Kind::Num(_) | Kind::True => {
                self.toreg_nobranch(e, NO_REG)?;
                self.emit_jmp()?
            }
            _ => self.branch(e, true)?,
        };
        let mut t = e.t;
        self.jmp_append(&mut t, pc)?;
        e.t = t;
        self.jmp_tohere(e.f)?;
        e.f = NO_JMP;
        Ok(())
    }

    /// The false list of a condition (`expr_cond`).
    pub(crate) fn cond(&mut self, cond: &Expr) -> R<u32> {
        let mut v = self.expr(cond)?;
        if v.k == Kind::Nil {
            v.k = Kind::False;
        }
        self.branch_t(&mut v)?;
        Ok(v.f)
    }

    // ---- operators -------------------------------------------------------

    fn arith_op(op: BinOp) -> Option<Op> {
        Some(match op {
            BinOp::Add => Op::ADDVV,
            BinOp::Sub => Op::SUBVV,
            BinOp::Mul => Op::MULVV,
            BinOp::Div => Op::DIVVV,
            BinOp::Mod => Op::MODVV,
            _ => return None,
        })
    }

    fn free_if_temp(&mut self, e: &Exp) {
        if let Kind::NonReloc(r) = e.k {
            if r >= self.fs().nactvar {
                self.fs().freereg -= 1;
            }
        }
    }

    /// Emits an arithmetic operator (`bcemit_arith`).
    fn arith(&mut self, op: BinOp, e1: &mut Exp, e2: &mut Exp) -> R<()> {
        let (opcode, rb, rc) = if op == BinOp::Pow {
            let rc = self.toanyreg(e2)?;
            let rb = self.toanyreg(e1)?;
            (Op::POW, rb, rc)
        } else {
            let vv = Self::arith_op(op).unwrap_or(Op::ADDVV);
            let mut opcode = vv as u32;
            self.toval(e2)?;
            let rc;
            if let Kind::Num(n) = e2.k {
                let k = self.const_num(n)?;
                if k <= bc::BCMAX_ABC {
                    rc = k;
                    opcode -= Op::ADDVV as u32 - Op::ADDVN as u32;
                } else {
                    rc = self.toanyreg(e2)?;
                }
            } else {
                rc = self.toanyreg(e2)?;
            }
            self.toval(e1)?;
            let (rb, rc) = match (e1.k.clone(), e2.is_numk()) {
                (Kind::Num(n), false) => {
                    let t = self.const_num(n)?;
                    if t <= bc::BCMAX_ABC {
                        opcode -= Op::ADDVV as u32 - Op::ADDNV as u32;
                        (rc, t)
                    } else {
                        (self.toanyreg(e1)?, rc)
                    }
                }
                _ => (self.toanyreg(e1)?, rc),
            };
            (Op::from_u8(opcode as u8).unwrap_or(vv), rb, rc)
        };
        self.free_if_temp(e1);
        self.free_if_temp(e2);
        let pc = self.emit_abc(opcode, 0, rb, rc)?;
        e1.k = Kind::Reloc(pc);
        Ok(())
    }

    /// A bit operator on two proven numbers (level 1): the binary operator
    /// of lj_parse.c with the operands in this order, folded when both are
    /// constants (`foldbitop`), else `op dst, b, c` with both operands in
    /// registers, since the bit opcodes have no constant forms.
    fn bitop(&mut self, op: Op, a: &Expr, b: &Expr) -> R<Exp> {
        let mut e1 = self.expr(a)?;
        // `bcemit_binop_left`.
        if !e1.is_numk_nojump() {
            self.toanyreg(&mut e1)?;
        }
        let mut e2 = self.expr(b)?;
        if e1.is_numk_nojump() && e2.is_numk_nojump() {
            return Ok(Exp::new(Kind::Num(fold_bitop(op, e1.num(), e2.num()))));
        }
        let rc = self.toanyreg(&mut e2)?;
        let rb = self.toanyreg(&mut e1)?;
        self.free_if_temp(&e1);
        self.free_if_temp(&e2);
        let pc = self.emit_abc(op, 0, rb, rc)?;
        Ok(Exp::new(Kind::Reloc(pc)))
    }

    /// `~` on a proven number (level 1): `BNOT`, folded for a constant.
    fn bnot(&mut self, a: &Expr) -> R<Exp> {
        let mut e = self.expr(a)?;
        if e.is_numk_nojump() {
            return Ok(Exp::new(Kind::Num(fold_bitop(Op::BNOT, e.num(), 0.0))));
        }
        self.toanyreg(&mut e)?;
        self.free_exp(&e);
        let r = e.reg().unwrap_or(0);
        let pc = self.emit_ad(Op::BNOT, 0, r)?;
        Ok(Exp::new(Kind::Reloc(pc)))
    }

    /// Emits a comparison (`bcemit_comp`): the result is a `Jmp`.
    fn comp(&mut self, op: BinOp, e1: &mut Exp, e2: &mut Exp) -> R<()> {
        self.toval(e1)?;
        let (mut a, mut b) = (e1, e2);
        let swapped;
        let ins: Ins = if matches!(op, BinOp::Eq | BinOp::Ne) {
            let base = if op == BinOp::Eq { Op::ISEQV } else { Op::ISNEV } as u32;
            if a.is_k() {
                std::mem::swap(&mut a, &mut b);
                swapped = true;
            } else {
                swapped = false;
            }
            let ra = self.toanyreg(a)?;
            self.toval(b)?;
            match &b.k {
                Kind::Nil | Kind::False | Kind::True => bc::ad(Op::from_u8((base + 6) as u8).unwrap_or(Op::ISEQP), ra, b.pri()),
                Kind::Str(s) => {
                    let k = self.const_str(s)?;
                    bc::ad(Op::from_u8((base + 2) as u8).unwrap_or(Op::ISEQS), ra, k)
                }
                Kind::Num(n) => {
                    let k = self.const_num(*n)?;
                    bc::ad(Op::from_u8((base + 4) as u8).unwrap_or(Op::ISEQN), ra, k)
                }
                _ => {
                    let rd = self.toanyreg(b)?;
                    bc::ad(Op::from_u8(base as u8).unwrap_or(Op::ISEQV), ra, rd)
                }
            }
        } else {
            // Level 0: GT -> LT and GE -> LE with the operands swapped, as
            // Lua's parser does. Level 1: CMPGT/CMPGE, which ask the left
            // operand's `>`/`>=` first.
            let (opcode, swap) = match op {
                BinOp::Lt => (Op::ISLT, false),
                BinOp::Le => (Op::ISLE, false),
                BinOp::Gt if self.level >= 1 => (Op::CMPGT, false),
                BinOp::Ge if self.level >= 1 => (Op::CMPGE, false),
                BinOp::Gt => (Op::ISLT, true),
                _ => (Op::ISLE, true),
            };
            swapped = swap;
            let (ra, rd);
            if swap {
                std::mem::swap(&mut a, &mut b);
                self.toval(a)?;
                ra = self.toanyreg(a)?;
                rd = self.toanyreg(b)?;
            } else {
                rd = self.toanyreg(b)?;
                ra = self.toanyreg(a)?;
            }
            bc::ad(opcode, ra, rd)
        };
        self.free_if_temp(a);
        self.free_if_temp(b);
        self.emit(ins)?;
        let pc = self.emit_jmp()?;
        // The result goes into the first operand's descriptor.
        let target = if swapped { b } else { a };
        target.k = Kind::Jmp(pc);
        Ok(())
    }

    /// Prepares the left operand of a binary operator before the right
    /// one is compiled (`bcemit_binop_left`).
    pub(crate) fn binop_left(&mut self, op: BinOp, e: &mut Exp) -> R<()> {
        match op {
            BinOp::And => self.branch_t(e),
            BinOp::Or => self.branch_f(e),
            BinOp::Concat => self.tonextreg(e),
            BinOp::Eq | BinOp::Ne => {
                if !e.is_k_nojump() {
                    self.toanyreg(e)?;
                }
                Ok(())
            }
            _ => {
                if !e.is_numk_nojump() {
                    self.toanyreg(e)?;
                }
                Ok(())
            }
        }
    }

    /// Emits a binary operator (`bcemit_binop`); the result is in `e1`.
    pub(crate) fn binop(&mut self, op: BinOp, e1: &mut Exp, e2: &mut Exp) -> R<()> {
        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Mod | BinOp::Pow => {
                if e1.is_numk_nojump() && e2.is_numk_nojump() {
                    if let Some(r) = fold_arith(op, e1.num(), e2.num()) {
                        e1.k = Kind::Num(r);
                        return Ok(());
                    }
                }
                self.arith(op, e1, e2)
            }
            BinOp::And => {
                self.discharge(e2)?;
                let mut f = e2.f;
                self.jmp_append(&mut f, e1.f)?;
                e2.f = f;
                *e1 = e2.clone();
                Ok(())
            }
            BinOp::Or => {
                self.discharge(e2)?;
                let mut t = e2.t;
                self.jmp_append(&mut t, e1.t)?;
                e2.t = t;
                *e1 = e2.clone();
                Ok(())
            }
            BinOp::Concat => {
                self.toval(e2)?;
                let merged = match e2.k {
                    Kind::Reloc(pc) => {
                        let ip = self.ins_at(pc);
                        if Op::from_ins(ip) == Some(Op::CAT) { Some((pc, ip)) } else { None }
                    }
                    _ => None,
                };
                match merged {
                    Some((pc, ip)) => {
                        let r1 = e1.reg().unwrap_or(0);
                        debug_assert_eq!(r1, bc::b_of(ip) - 1, "bad CAT stack layout");
                        self.free_exp(e1);
                        self.patch_ins(pc, |i| bc::set_b(i, r1));
                        e1.k = Kind::Reloc(pc);
                    }
                    None => {
                        self.tonextreg(e2)?;
                        self.free_exp(e2);
                        self.free_exp(e1);
                        let r1 = e1.reg().unwrap_or(0);
                        let r2 = e2.reg().unwrap_or(0);
                        let pc = self.emit_abc(Op::CAT, 0, r1, r2)?;
                        e1.k = Kind::Reloc(pc);
                    }
                }
                Ok(())
            }
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => self.comp(op, e1, e2),
        }
    }

    /// Emits a unary operator (`bcemit_unop`).
    fn unop(&mut self, op: UnOp, e: &mut Exp) -> R<()> {
        let opcode = match op {
            UnOp::Not => {
                std::mem::swap(&mut e.t, &mut e.f);
                self.jmp_dropval(e.f);
                self.jmp_dropval(e.t);
                self.discharge(e)?;
                match e.k {
                    Kind::Nil | Kind::False => {
                        e.k = Kind::True;
                        return Ok(());
                    }
                    Kind::True | Kind::Str(_) | Kind::Num(_) => {
                        e.k = Kind::False;
                        return Ok(());
                    }
                    Kind::Jmp(_) => {
                        self.invert_cond(e);
                        return Ok(());
                    }
                    Kind::Reloc(pc) => {
                        self.reserve(1)?;
                        let reg = self.fs().freereg - 1;
                        self.patch_ins(pc, |i| bc::set_a(i, reg));
                        e.k = Kind::NonReloc(reg);
                    }
                    _ => {}
                }
                Op::NOT
            }
            UnOp::Neg => {
                if !e.has_jump() {
                    if let Kind::Num(n) = e.k {
                        if n != 0.0 {
                            e.k = Kind::Num(-n);
                            return Ok(());
                        }
                    }
                }
                self.toanyreg(e)?;
                Op::UNM
            }
            UnOp::Len => {
                self.toanyreg(e)?;
                Op::LEN
            }
        };
        self.free_exp(e);
        let r = e.reg().unwrap_or(0);
        let pc = self.emit_ad(opcode, 0, r)?;
        e.k = Kind::Reloc(pc);
        Ok(())
    }

    // ---- expressions -----------------------------------------------------

    /// Compiles an expression to a descriptor.
    pub(crate) fn expr(&mut self, e: &Expr) -> R<Exp> {
        let multi = std::mem::take(&mut self.want_multi);
        match e {
            Expr::Nil => Ok(Exp::new(Kind::Nil)),
            Expr::Bool(b) => Ok(Exp::new(if *b { Kind::True } else { Kind::False })),
            Expr::Number(n) => self.number(*n),
            Expr::Str(s) => Ok(Exp::new(Kind::Str(s.clone()))),
            Expr::Local(l) => self.var_lookup(VarId::Local(*l)),
            Expr::Global(g) => Ok(Exp::new(Kind::Global(g.as_bytes().to_vec()))),
            Expr::Index(o, k) => self.index_expr(o, k),
            Expr::Call(f, args) => {
                if self.level >= 1 {
                    if let Some(op) = match f.as_ref() {
                        Expr::Intrinsic(name) => bit_intrinsic(name),
                        _ => None,
                    } {
                        match args.as_slice() {
                            [a] if op == Op::BNOT => return self.bnot(a),
                            [a, b] if op != Op::BNOT => return self.bitop(op, a, b),
                            _ => {}
                        }
                    }
                }
                self.call(f, &args.iter().collect::<Vec<_>>())
            }
            Expr::MethodCall(o, name, args) if self.level >= 1 => self.lua_method_call(o, name, args),
            Expr::MethodCall(o, name, args) => {
                // `rt.lmethod.name(o, args)` (nl_luagen).
                let callee = Expr::index(Expr::Intrinsic("lmethod"), Expr::str(name));
                let mut all: Vec<&Expr> = vec![o];
                all.extend(args.iter());
                self.call(&callee, &all)
            }
            Expr::Function(f) => self.closure(f),
            Expr::Binary(op, a, b) => {
                let mut e1 = self.expr(a)?;
                self.binop_left(*op, &mut e1)?;
                let mut e2 = self.expr(b)?;
                self.binop(*op, &mut e1, &mut e2)?;
                Ok(e1)
            }
            Expr::Unary(op, a) => {
                let mut e1 = self.expr(a)?;
                self.unop(*op, &mut e1)?;
                Ok(e1)
            }
            Expr::Table(entries) => self.table(entries),
            Expr::Vararg => {
                if !self.fs().vararg {
                    return Err(self.err("internal error: `...` outside a vararg function"));
                }
                self.reserve(1)?;
                let base = self.fs().freereg - 1;
                let numparams = self.fs().numparams;
                let pc = self.emit_abc(Op::VARG, base, 2, numparams)?;
                Ok(Exp::new(Kind::Call(pc, base)))
            }
            Expr::Paren(inner) => {
                let mut v = self.expr(inner)?;
                self.discharge(&mut v)?;
                Ok(v)
            }
            Expr::Intrinsic(name) => {
                if self.level >= 1 {
                    if let Some(idx) = crate::intrinsics::index(name) {
                        let pc = self.emit_ad(Op::KINTR, 0, idx)?;
                        return Ok(Exp::new(Kind::Reloc(pc)));
                    }
                }
                let mut t = self.var_lookup(VarId::Rt)?;
                self.toanyreg(&mut t)?;
                let mut key = Exp::new(Kind::Str(name.as_bytes().to_vec()));
                self.index(&mut t, &mut key)?;
                Ok(t)
            }
            Expr::MemberCall { receiver, key, args } if self.level >= 1 => self.member_call(receiver, key, args),
            Expr::MemberCall { receiver, key, args } => match key.as_ref() {
                Expr::Str(k) if is_lua_identifier(k) => {
                    let callee = Expr::index(Expr::Intrinsic("member"), Expr::Str(k.clone()));
                    let mut all: Vec<&Expr> = vec![receiver];
                    all.extend(args.iter());
                    self.call(&callee, &all)
                }
                _ => {
                    let mut all: Vec<&Expr> = vec![receiver, key];
                    all.extend(args.iter());
                    self.call(&Expr::Intrinsic("mcall"), &all)
                }
            },
            Expr::AutoGet { receiver, key } if self.level >= 1 && !multi => self.auto_get(receiver, key),
            Expr::AutoGet { receiver, key } => self.call(&Expr::Intrinsic("mget"), &[receiver, key]),
            Expr::MemberApply { func, receiver, args } if self.level >= 1 => self.member_apply(func, receiver, args),
            Expr::MemberApply { func, receiver, args } => {
                // `rt.mapply(f, x, args)` (nl_luagen).
                let mut all: Vec<&Expr> = vec![func, receiver];
                all.extend(args.iter());
                self.call(&Expr::Intrinsic("mapply"), &all)
            }
        }
    }

    // ---- members (level 1) -----------------------------------------------

    /// The receiver and callee of a member or method call (`bcemit_method`):
    /// `MOV base+2, obj; TGETS base, obj, key`, the receiver copied under
    /// the frame slot first so that an error names a method. Registers
    /// `base`, `base+1` and `base+2` are reserved on return.
    fn method_setup(&mut self, receiver: &Expr, key: &Expr) -> R<u32> {
        let mut obj = self.expr(receiver)?;
        let objreg = self.toanyreg(&mut obj)?;
        // A key that is not a short string constant is computed first, while
        // the receiver's register is still reserved.
        let mut kidx = None;
        if let Expr::Str(s) = key {
            let idx = self.const_str(s)?;
            if idx <= bc::BCMAX_ABC {
                kidx = Some(idx);
            }
        }
        let mut keyexp = None;
        if kidx.is_none() {
            let mut k = self.expr(key)?;
            self.toanyreg(&mut k)?;
            keyexp = Some(k);
        }
        if let Some(k) = &keyexp {
            self.free_exp(k);
        }
        self.free_exp(&obj);
        let base = self.fs().freereg;
        self.emit_ad(Op::MOV, base + 2, objreg)?;
        match (kidx, keyexp.as_ref().and_then(Exp::reg)) {
            (Some(idx), _) => self.emit_abc(Op::TGETS, base, objreg, idx)?,
            (None, Some(r)) => self.emit_abc(Op::TGETV, base, objreg, r)?,
            (None, None) => return Err(self.err("internal error: a member key without a register")),
        };
        self.reserve(3)?;
        Ok(base)
    }

    /// The arguments of a call, from the next free register on: the base of
    /// a multi-value tail (a call or `...` as the last argument), if any.
    fn call_args(&mut self, args: &[&Expr]) -> R<Option<u32>> {
        let n = args.len();
        for (i, a) in args.iter().enumerate() {
            let last = i + 1 == n;
            self.want_multi = last;
            let mut e = self.expr(a)?;
            if last {
                if let Kind::Call(pc, abase) = e.k {
                    self.patch_ins(pc, |ins| bc::set_b(ins, 0));
                    return Ok(Some(abase));
                }
            }
            self.tonextreg(&mut e)?;
        }
        Ok(None)
    }

    /// `x.m(args)` at level 1: the receiver is the first
    /// argument, and `MSELF` keeps or drops it by the method test before a
    /// `CALLM` that takes its argument count from MULTRES.
    fn member_call(&mut self, receiver: &Expr, key: &Expr, args: &[Expr]) -> R<Exp> {
        let base = self.method_setup(receiver, key)?;
        let multi = self.call_args(&args.iter().collect::<Vec<_>>())?;
        let top = multi.unwrap_or(self.fs().freereg);
        let nfixed = top - (base + 2);
        self.emit_abc(Op::MSELF, base, u32::from(multi.is_some()), nfixed)?;
        let pc = self.emit_abc(Op::CALLM, base, 2, 0)?;
        self.fs().freereg = base + 1;
        Ok(Exp::new(Kind::Call(pc, base)))
    }

    /// The call of a member read before, at level 1: the
    /// sequence of [`Self::member_call`] with the held function in `base`
    /// in place of the `TGETS`, so that `MSELF` applies the method test to
    /// it and the member is not read again.
    fn member_apply(&mut self, func: &Expr, receiver: &Expr, args: &[Expr]) -> R<Exp> {
        let mut f = self.expr(func)?;
        self.tonextreg(&mut f)?;
        self.reserve(1)?;
        let base = f.reg().unwrap_or(0);
        let mut r = self.expr(receiver)?;
        self.tonextreg(&mut r)?;
        let multi = self.call_args(&args.iter().collect::<Vec<_>>())?;
        let top = multi.unwrap_or(self.fs().freereg);
        let nfixed = top - (base + 2);
        self.emit_abc(Op::MSELF, base, u32::from(multi.is_some()), nfixed)?;
        let pc = self.emit_abc(Op::CALLM, base, 2, 0)?;
        self.fs().freereg = base + 1;
        Ok(Exp::new(Kind::Call(pc, base)))
    }

    /// Lua's `x:m(args)` at level 1: the stock sequence. A string receiver
    /// still finds NL's library, because the fork decides by the running
    /// function.
    fn lua_method_call(&mut self, receiver: &Expr, name: &str, args: &[Expr]) -> R<Exp> {
        let base = self.method_setup(receiver, &Expr::str(name))?;
        let multi = self.call_args(&args.iter().collect::<Vec<_>>())?;
        let ins = match multi {
            Some(abase) => bc::abc(Op::CALLM, base, 2, abase - base - 2),
            None => {
                let freereg = self.fs().freereg;
                bc::abc(Op::CALL, base, 2, freereg - base - 1)
            }
        };
        let pc = self.emit(ins)?;
        self.fs().freereg = base + 1;
        Ok(Exp::new(Kind::Call(pc, base)))
    }

    /// `x.m` whose one value is used, at level 1: the member,
    /// called with the receiver when `ISMETH` says it is a method. The
    /// result is in `base`; where several values may be taken the caller
    /// uses `rt.mget` instead, which returns all of a method's results.
    fn auto_get(&mut self, receiver: &Expr, key: &Expr) -> R<Exp> {
        let base = self.method_setup(receiver, key)?;
        self.emit_ad(Op::ISMETH, base, base + 2)?;
        let skip = self.emit_jmp()?;
        self.emit_abc(Op::CALL, base, 2, 2)?;
        self.jmp_tohere(skip)?;
        self.fs().freereg = base + 1;
        Ok(Exp::new(Kind::NonReloc(base)))
    }

    /// A number literal. NaN, the infinities and `-0` are never constants
    /// (the parser avoids them too): they are computed, as the Lua backend
    /// prints them (`0/0`, `1/0`, `-0`).
    fn number(&mut self, n: f64) -> R<Exp> {
        if n.is_nan() || n.is_infinite() {
            let (a, b) = if n.is_nan() { (0.0, 0.0) } else { (if n > 0.0 { 1.0 } else { -1.0 }, 0.0) };
            let mut e1 = Exp::new(Kind::Num(a));
            self.binop_left(BinOp::Div, &mut e1)?;
            let mut e2 = Exp::new(Kind::Num(b));
            self.arith(BinOp::Div, &mut e1, &mut e2)?;
            return Ok(e1);
        }
        if n == 0.0 && n.is_sign_negative() {
            let mut e = Exp::new(Kind::Num(0.0));
            self.unop(UnOp::Neg, &mut e)?;
            return Ok(e);
        }
        Ok(Exp::new(Kind::Num(n)))
    }

    fn index_expr(&mut self, o: &Expr, k: &Expr) -> R<Exp> {
        let mut t = self.expr(o)?;
        self.toanyreg(&mut t)?;
        let mut key = self.expr(k)?;
        self.toval(&mut key)?;
        self.index(&mut t, &mut key)?;
        Ok(t)
    }

    /// A call `f(args)` (`parse_args`): the function goes to the next
    /// register, a frame slot follows it (FR2), then the arguments.
    pub(crate) fn call(&mut self, f: &Expr, args: &[&Expr]) -> R<Exp> {
        let mut func = self.expr(f)?;
        self.tonextreg(&mut func)?;
        self.reserve(1)?;
        let base = func.reg().unwrap_or(0);
        let multi = self.call_args(args)?;
        let ins = match multi {
            Some(abase) => bc::abc(Op::CALLM, base, 2, abase - base - 2),
            None => {
                let freereg = self.fs().freereg;
                bc::abc(Op::CALL, base, 2, freereg - base - 1)
            }
        };
        let pc = self.emit(ins)?;
        self.fs().freereg = base + 1;
        Ok(Exp::new(Kind::Call(pc, base)))
    }

    /// A closure: compiles the function and emits `FNEW`.
    pub(crate) fn closure(&mut self, f: &Function) -> R<Exp> {
        let proto = self.function(f)?;
        let idx = self.const_child(proto)?;
        let pc = self.emit_ad(Op::FNEW, 0, idx)?;
        Ok(Exp::new(Kind::Reloc(pc)))
    }

    /// Compiles a function of the IR to a prototype.
    pub(crate) fn function(&mut self, f: &Function) -> R<crate::proto::Proto> {
        let name = f.name.clone().unwrap_or_else(|| "a function".to_owned());
        let params: Vec<VarId> = f.params.iter().map(|p| VarId::Local(*p)).collect();
        let saved_line = self.line;
        self.line = f.line;
        self.func_begin(name, f.line, f.vararg, f.method, &params)?;
        self.fs().has_closures = super::has_closures(f);
        self.stmts(&f.body)?;
        let proto = self.func_end()?;
        self.line = saved_line;
        Ok(proto)
    }

    // ---- table constructors ----------------------------------------------

    /// A table constructor (`expr_table`): a template (`TDUP`) when
    /// entries are constant, else `TNEW` with size hints, and stores for
    /// the rest.
    fn table(&mut self, entries: &[TableEntry]) -> R<Exp> {
        let freg = self.fs().freereg;
        let pc = self.emit_ad(Op::TNEW, freg, 0)?;
        self.reserve(1)?;
        let tab = freg;
        let freg = freg + 1;
        let mut template: Option<u32> = None;
        let mut vcall = false;
        let mut needarr = false;
        let mut narr: u32 = 1;
        let mut nhash: u32 = 0;
        let nentries = entries.len();
        for (i, entry) in entries.iter().enumerate() {
            vcall = false;
            let (mut key, val_expr) = match entry {
                TableEntry::Item(v) => {
                    let k = Exp::new(Kind::Num(f64::from(narr)));
                    narr += 1;
                    needarr = true;
                    vcall = true;
                    (k, v)
                }
                TableEntry::Pair(k, v) => {
                    let mut key = self.expr(k)?;
                    self.toval(&mut key)?;
                    if !key.is_k() {
                        let mut t = Exp::new(Kind::NonReloc(tab));
                        self.index(&mut t, &mut key)?;
                        key = t;
                    }
                    if key.is_numk() && key.num() == 0.0 {
                        needarr = true;
                    } else {
                        nhash += 1;
                    }
                    (key, v)
                }
            };
            self.want_multi = vcall && i + 1 == nentries;
            let mut val = self.expr(val_expr)?;
            let const_entry = key.is_k() && key.k != Kind::Nil && (matches!(key.k, Kind::Str(_)) || val.is_k_nojump());
            if const_entry {
                let idx = match template {
                    Some(i) => i,
                    None => {
                        let i = self.const_table()?;
                        self.set_ins(pc, bc::ad(Op::TDUP, tab, i));
                        template = Some(i);
                        i
                    }
                };
                vcall = false;
                let kv = kvalue(&key);
                let vv = if val.is_k_nojump() { kvalue(&val) } else { KTabValue::Nil };
                if let Some(t) = self.ktable_mut(idx) {
                    ktable_set(t, kv, vv, if needarr { narr } else { 0 });
                }
                if val.is_k_nojump() {
                    self.fs().freereg = freg;
                    continue;
                }
            }
            // A non-constant entry: store it.
            if !matches!(val.k, Kind::Call(..)) {
                self.toanyreg(&mut val)?;
                vcall = false;
            }
            // A constant key is indexed now; a non-constant one was above.
            let t = if key.is_k() {
                let mut t = Exp::new(Kind::NonReloc(tab));
                self.index(&mut t, &mut key)?;
                // A constant key that fits no operand (a string constant
                // past 255, a number that is not a byte) went to a register
                // above the value's: free it first, so that the store frees
                // the value's in order. Nothing is emitted in between, so
                // the key is still in that register for the store.
                if let Kind::Indexed(_, Key::Reg(r)) = t.k {
                    self.free_reg(r);
                }
                t
            } else {
                key
            };
            self.store(&t, &mut val)?;
            self.fs().freereg = freg;
        }
        if vcall {
            // The last item is a call: its values fill the table from
            // `narr - 1` on (`TSETM`), with the count as a biased number.
            let last = self.fs().pc() - 1;
            let en = f64::from_bits((u64::from(0x4330_0000u32) << 32) | u64::from(narr - 1));
            let k = self.const_num(en)?;
            let at = if narr > 256 {
                self.fs().bc.pop();
                self.fs().lines.pop();
                last - 1
            } else {
                last
            };
            self.set_ins(at, bc::ad(Op::TSETM, freg, k));
            self.patch_ins(at.wrapping_sub(1), |i| bc::set_b(i, 0));
        }
        let e;
        if pc == self.fs().pc() - 1 {
            e = Exp::new(Kind::Reloc(pc));
            self.fs().freereg -= 1;
        } else {
            e = Exp::new(Kind::NonReloc(tab));
        }
        match template {
            None => {
                let narr = if !needarr {
                    0
                } else if narr < 3 {
                    3
                } else {
                    narr.min(0x7ff)
                };
                let d = narr | (hsize2hbits(nhash) << 11);
                self.patch_ins(pc, |i| bc::set_d(i, d));
            }
            Some(idx) => {
                if needarr {
                    if let Some(t) = self.ktable_mut(idx) {
                        if t.array.len() < narr as usize {
                            t.array.resize(narr as usize, KTabValue::Nil);
                        }
                    }
                }
            }
        }
        Ok(e)
    }

    /// Compiles an expression list for a call, return or assignment
    /// (`expr_list`): every value but the last goes to the next register;
    /// the last is returned open.
    pub(crate) fn expr_list(&mut self, es: &[Expr], multi_last: bool) -> R<(u32, Exp)> {
        let mut last = Exp::new(Kind::Void);
        let n = es.len();
        for (i, e) in es.iter().enumerate() {
            self.want_multi = multi_last && i + 1 == n;
            let mut v = self.expr(e)?;
            if i + 1 < n {
                self.tonextreg(&mut v)?;
            }
            last = v;
        }
        Ok((u32::try_from(n).unwrap_or(u32::MAX), last))
    }
}

/// `hsize2hbits`: the hash size hint for `n` entries.
fn hsize2hbits(n: u32) -> u32 {
    if n == 0 {
        0
    } else if n <= 1 {
        1
    } else {
        32 - (n - 1).leading_zeros()
    }
}

fn kvalue(e: &Exp) -> KTabValue {
    match &e.k {
        Kind::Nil => KTabValue::Nil,
        Kind::False => KTabValue::False,
        Kind::True => KTabValue::True,
        Kind::Str(s) => KTabValue::Str(s.clone()),
        Kind::Num(n) => KTabValue::Num(*n),
        _ => KTabValue::Nil,
    }
}

/// Sets a template table entry (`lj_tab_set` on the template): an integer
/// key below `asize` goes to the array part, anything else to the hash
/// part, replacing an equal key.
fn ktable_set(t: &mut KTable, key: KTabValue, value: KTabValue, asize: u32) {
    if let KTabValue::Num(n) = key {
        if n.fract() == 0.0 && n >= 0.0 && n < f64::from(asize) {
            let i = n as usize;
            if t.array.len() <= i {
                t.array.resize(i + 1, KTabValue::Nil);
            }
            if let Some(slot) = t.array.get_mut(i) {
                *slot = value;
            }
            return;
        }
    }
    if let Some(entry) = t.hash.iter_mut().find(|(k, _)| *k == key) {
        entry.1 = value;
    } else {
        t.hash.push((key, value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_bits() {
        assert_eq!(hsize2hbits(0), 0);
        assert_eq!(hsize2hbits(1), 1);
        assert_eq!(hsize2hbits(2), 1);
        assert_eq!(hsize2hbits(3), 2);
        assert_eq!(hsize2hbits(4), 2);
        assert_eq!(hsize2hbits(5), 3);
    }

    #[test]
    fn folding() {
        assert_eq!(fold_arith(BinOp::Mod, 7.0, 3.0), Some(1.0));
        assert_eq!(fold_arith(BinOp::Mod, -7.0, 3.0), Some(2.0));
        assert_eq!(fold_arith(BinOp::Div, 0.0, 0.0), None);
        assert_eq!(fold_arith(BinOp::Mul, 0.0, -1.0), None);
        assert_eq!(fold_arith(BinOp::Pow, 2.0, 10.0), Some(1024.0));
    }

    /// `lj_num2bit` and `foldbitop`: the values of
    /// tests/run/expr_bitwise_opcodes.out.
    #[test]
    fn bit_folding() {
        assert_eq!(tobit(5.0), 5);
        assert_eq!(tobit(-7.0), -7);
        assert_eq!(tobit(3.7), 4);
        assert_eq!(tobit(-3.7), -4);
        assert_eq!(tobit(2.5), 2);
        assert_eq!(tobit(3.5), 4);
        assert_eq!(tobit(-0.5), 0);
        assert_eq!(tobit(-0.0), 0);
        assert_eq!(tobit(2147483648.0), i32::MIN);
        assert_eq!(tobit(-2147483649.0), i32::MAX);
        assert_eq!(tobit(4294967301.0), 5);
        assert_eq!(tobit(1e10), 1410065408);
        assert_eq!(tobit(f64::INFINITY), 0);
        assert_eq!(tobit(f64::NEG_INFINITY), 0);
        assert_eq!(fold_bitop(Op::BAND, 5.0, 3.0), 1.0);
        assert_eq!(fold_bitop(Op::BOR, 2147483648.0, 1.0), -2147483647.0);
        assert_eq!(fold_bitop(Op::BXOR, -7.0, 2.0), -5.0);
        assert_eq!(fold_bitop(Op::BSHL, 1.0, 31.0), -2147483648.0);
        assert_eq!(fold_bitop(Op::BSHL, -5.0, 33.0), -10.0);
        assert_eq!(fold_bitop(Op::BSHL, -5.0, -1.0), -2147483648.0);
        assert_eq!(fold_bitop(Op::BSHR, -16.0, 28.0), 15.0);
        assert_eq!(fold_bitop(Op::BSHR, -5.0, 1e10), -5.0);
        assert_eq!(fold_bitop(Op::BSAR, -16.0, 2.0), -4.0);
        assert_eq!(fold_bitop(Op::BSAR, -5.0, 1.5), -2.0);
        assert_eq!(fold_bitop(Op::BNOT, 2.5, 0.0), -3.0);
        assert_eq!(fold_bitop(Op::BNOT, 1e10, 0.0), -1410065409.0);
    }
}
