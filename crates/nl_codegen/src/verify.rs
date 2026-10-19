//! The bytecode verifier. LuaJIT does not check
//! what it loads, so an emitter bug would show up as a VM
//! crash; this checks every prototype the generator produces before it is
//! written, in debug builds and in tests.
//!
//! What is checked, per instruction according to its operand modes
//! ([`crate::bc::modes`]): slots against the frame size, including the
//! ranges a call, return, `KNIL`, `CAT`, `VARG` or loop instruction uses;
//! jump targets inside the function; constant, upvalue and child
//! prototype indices; that a comparison or test is followed by the `JMP`
//! it conditions; that the function ends in a return or tail call. Per
//! prototype: the parameter count, the frame size against LuaJIT's slot
//! limit, the flags, upvalue descriptors against the enclosing function,
//! line and variable information. Children are verified recursively.
//!
//! The fork's opcodes (level 1): `ISMETH` and the `CMP*` comparisons are
//! followed by their `JMP`; `MSELF` by a `CALLM`/`CALLMT` at its base with
//! no fixed count of its own; `KINTR` names an entry of the intrinsic
//! table. None of them, no bit operator (`BNOT`…`BSAR`) and no
//! `FLAG_NL`/`FLAG_METHOD` may appear in a level-0 chunk; no prototype
//! carries `PROTO_BITOP` (see [`crate::proto`]).

use crate::bc::{self, Mode, Op};
use crate::proto::{KGc, Proto, UV_IMMUTABLE, UV_LOCAL, VarName};

/// `PROTO_BITOP` of lj_obj.h.
const PROTO_BITOP: u8 = 0x80;

/// A verification failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifyError {
    /// The function's descriptive name.
    pub function: String,
    /// The instruction, when the problem is one.
    pub pc: Option<u32>,
    pub message: String,
}

impl std::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.pc {
            Some(pc) => write!(f, "{}, instruction {pc}: {}", self.function, self.message),
            None => write!(f, "{}: {}", self.function, self.message),
        }
    }
}

/// Verifies a chunk's top-level prototype and everything below it, as
/// emitted at feature `level`.
pub fn verify(main: &Proto, level: u8) -> Result<(), VerifyError> {
    if main.flags & crate::proto::FLAG_VARARG == 0 {
        return Err(VerifyError { function: main.name.clone(), pc: None, message: "the chunk's function is not vararg".to_owned() });
    }
    verify_proto(main, None, level)
}

fn verify_proto(p: &Proto, parent: Option<&Proto>, level: u8) -> Result<(), VerifyError> {
    let fail = |pc: Option<u32>, message: String| Err(VerifyError { function: p.name.clone(), pc, message });
    let frame = u32::from(p.framesize);
    let nl_flags = crate::proto::FLAG_NL | crate::proto::FLAG_METHOD;
    if p.flags & PROTO_BITOP != 0 {
        return fail(None, "PROTO_BITOP, the top bit of the closure counter, is set".to_owned());
    }
    if level == 0 && p.flags & nl_flags != 0 {
        return fail(None, "level-1 flags in a level-0 chunk".to_owned());
    }
    if level >= 1 && p.flags & crate::proto::FLAG_NL == 0 {
        return fail(None, "a level-1 prototype without FLAG_NL".to_owned());
    }
    if frame >= bc::MAX_SLOTS {
        return fail(None, format!("frame size {frame} reaches the limit of {} slots", bc::MAX_SLOTS));
    }
    if u32::from(p.numparams) > frame {
        return fail(None, format!("{} parameters do not fit in a frame of {frame}", p.numparams));
    }
    if p.bc.is_empty() {
        return fail(None, "no instructions".to_owned());
    }
    if p.lines.len() != p.bc.len() {
        return fail(None, format!("{} lines for {} instructions", p.lines.len(), p.bc.len()));
    }
    for (i, line) in p.lines.iter().enumerate() {
        if *line < p.firstline || *line - p.firstline > p.numline {
            return fail(Some(i as u32), format!("line {line} is outside the function's lines {}..={}", p.firstline, p.firstline + p.numline));
        }
    }
    if p.uv.len() > bc::MAX_UPVAL as usize {
        return fail(None, format!("{} upvalues, more than {}", p.uv.len(), bc::MAX_UPVAL));
    }
    if p.uvnames.len() != p.uv.len() {
        return fail(None, format!("{} upvalue names for {} upvalues", p.uvnames.len(), p.uv.len()));
    }
    for (i, uv) in p.uv.iter().enumerate() {
        let Some(parent) = parent else { return fail(None, "the top-level function has upvalues".to_owned()) };
        if uv & UV_LOCAL != 0 {
            let slot = u32::from(uv & !(UV_LOCAL | UV_IMMUTABLE));
            if slot >= u32::from(parent.framesize) {
                return fail(None, format!("upvalue {i} refers to slot {slot} of the enclosing function, whose frame has {}", parent.framesize));
            }
        } else if usize::from(*uv) >= parent.uv.len() {
            return fail(None, format!("upvalue {i} refers to upvalue {uv} of the enclosing function, which has {}", parent.uv.len()));
        }
    }
    let len = p.len();
    let mut last_start = 0;
    for (i, v) in p.vars.iter().enumerate() {
        if v.startpc > v.endpc || v.endpc > len + 1 {
            return fail(None, format!("variable {i} is live from {} to {}, outside 0..={}", v.startpc, v.endpc, len + 1));
        }
        if v.startpc < last_start {
            return fail(None, format!("variable {i} starts at {} before the variable listed before it", v.startpc));
        }
        if let VarName::Internal(code) = v.name {
            if !(1..=6).contains(&code) {
                return fail(None, format!("variable {i} has the internal name code {code}"));
            }
        }
        last_start = v.startpc;
    }
    let mut nchildren = 0;
    for (i, k) in p.kgc.iter().enumerate() {
        if let KGc::Child(c) = k {
            nchildren += 1;
            if *c >= p.children.len() {
                return fail(None, format!("constant {i} names child {c} of {}", p.children.len()));
            }
        }
    }
    if (nchildren > 0) != (p.flags & crate::proto::FLAG_CHILD != 0) {
        return fail(None, "the child flag does not match the child constants".to_owned());
    }
    if p.kgc.len() > bc::BCMAX_D as usize + 1 || p.knum.len() > bc::BCMAX_D as usize + 1 {
        return fail(None, "more than 65536 constants".to_owned());
    }
    let Some(last) = p.bc.last().copied().and_then(Op::from_ins) else { return fail(None, "unknown last opcode".to_owned()) };
    if !last.is_ret_or_tail() {
        return fail(Some(len - 1), format!("the function ends in {last:?}, not a return"));
    }
    for (pc, ins) in p.bc.iter().copied().enumerate() {
        let pc = pc as u32;
        verify_ins(p, pc, ins, level).map_err(|message| VerifyError { function: p.name.clone(), pc: Some(pc), message })?;
    }
    for c in &p.children {
        verify_proto(c, Some(p), level)?;
    }
    Ok(())
}

fn verify_ins(p: &Proto, pc: u32, ins: bc::Ins, level: u8) -> Result<(), String> {
    let Some(op) = Op::from_ins(ins) else { return Err(format!("unknown opcode {}", bc::op_of(ins))) };
    if !op.is_emittable() {
        return Err(format!("{op:?} is not an opcode a dump may contain"));
    }
    if level == 0 && (op.is_nl() || op.is_bitop()) {
        return Err(format!("{op:?} in a level-0 chunk"));
    }
    let frame = u32::from(p.framesize);
    let len = p.len();
    let (ma, mb, mc) = bc::modes(op);
    let a = bc::a_of(ins);
    let b = bc::b_of(ins);
    let c = bc::c_of(ins);
    let d = bc::d_of(ins);
    let slot = |m: Mode, v: u32, what: &str| -> Result<(), String> {
        match m {
            Mode::Var | Mode::Dst | Mode::Base if v >= frame => Err(format!("{what} = {v} is outside the frame of {frame}")),
            Mode::RBase if v > frame => Err(format!("{what} = {v} is beyond the frame of {frame}")),
            _ => Ok(()),
        }
    };
    let kind = |m: Mode, v: u32, what: &str| -> Result<(), String> {
        match m {
            Mode::Var | Mode::Dst | Mode::Base | Mode::RBase => slot(m, v, what),
            Mode::Uv if v as usize >= p.uv.len() => Err(format!("{what} = {v} names an upvalue of {}", p.uv.len())),
            Mode::Pri if v > 2 => Err(format!("{what} = {v} is not a primitive")),
            Mode::Num if v as usize >= p.knum.len() => Err(format!("{what} = {v} names a number constant of {}", p.knum.len())),
            Mode::Str => match p.kgc.get(v as usize) {
                Some(KGc::Str(_)) => Ok(()),
                _ => Err(format!("{what} = {v} is not a string constant")),
            },
            Mode::Tab => match p.kgc.get(v as usize) {
                Some(KGc::Table(_)) => Ok(()),
                _ => Err(format!("{what} = {v} is not a template table")),
            },
            Mode::Func => match p.kgc.get(v as usize) {
                Some(KGc::Child(_)) => Ok(()),
                _ => Err(format!("{what} = {v} is not a child prototype")),
            },
            Mode::Jump => {
                let target = i64::from(pc) + 1 + i64::from(bc::j_of(ins));
                if target < 0 || target >= i64::from(len) {
                    Err(format!("jump to {target}, outside 0..{len}"))
                } else {
                    Ok(())
                }
            }
            Mode::Cdata => Err("cdata constants are never emitted".to_owned()),
            _ => Ok(()),
        }
    };
    kind(ma, a, "A")?;
    if mb == Mode::None {
        kind(mc, d, "D")?;
    } else {
        kind(mb, b, "B")?;
        kind(mc, c, "C")?;
    }
    // Ranges and pairs that the modes alone do not express.
    let check = |cond: bool, msg: String| if cond { Ok(()) } else { Err(msg) };
    match op {
        Op::ISLT
        | Op::ISGE
        | Op::ISLE
        | Op::ISGT
        | Op::ISEQV
        | Op::ISNEV
        | Op::ISEQS
        | Op::ISNES
        | Op::ISEQN
        | Op::ISNEN
        | Op::ISEQP
        | Op::ISNEP
        | Op::ISTC
        | Op::ISFC
        | Op::IST
        | Op::ISF
        | Op::CMPGT
        | Op::CMPNGT
        | Op::CMPGE
        | Op::CMPNGE
        | Op::ISMETH => {
            let next = p.bc.get(pc as usize + 1).copied().and_then(Op::from_ins);
            check(next == Some(Op::JMP), format!("{op:?} is not followed by JMP"))?;
            if matches!(op, Op::ISTC | Op::ISFC) {
                check(a < frame, format!("A = {a} is outside the frame of {frame}"))?;
            }
        }
        Op::KINTR => check(d >= 1 && (d as usize) <= crate::intrinsics::NAMES.len(), format!("KINTR {d} is not an intrinsic"))?,
        Op::MSELF => {
            check(c >= 1 && a + 2 + c <= frame, format!("MSELF at {a} with {c} arguments does not fit the frame of {frame}"))?;
            check(b <= 1, format!("MSELF B = {b} is not a flag"))?;
            let next = p.bc.get(pc as usize + 1).copied();
            let ok = next.is_some_and(|n| {
                matches!(Op::from_ins(n), Some(Op::CALLM | Op::CALLMT))
                    && bc::a_of(n) == a
                    && (if Op::from_ins(n) == Some(Op::CALLM) { bc::c_of(n) } else { bc::d_of(n) }) == 0
            });
            check(ok, format!("MSELF at {a} is not followed by a CALLM or CALLMT at {a} taking its count from MULTRES"))?;
        }
        Op::KNIL => check(a <= d && d < frame, format!("KNIL {a}..{d} is outside the frame of {frame}"))?,
        Op::CAT => check(b <= c && c < frame, format!("CAT {b}..{c} is outside the frame of {frame}"))?,
        Op::CALL | Op::CALLM => {
            let nargs = if op == Op::CALL { c.max(1) } else { c + 1 };
            check(a + nargs < frame, format!("the arguments of the call at {a} end beyond the frame of {frame}"))?;
            check(b < 2 || a + b - 2 < frame, format!("the {} results of the call at {a} end beyond the frame of {frame}", b.wrapping_sub(1)))?;
        }
        Op::CALLT | Op::CALLMT => {
            let nargs = if op == Op::CALLT { d.max(1) } else { d + 1 };
            check(a + nargs < frame, format!("the arguments of the tail call at {a} end beyond the frame of {frame}"))?;
        }
        Op::ITERC | Op::ITERN => {
            check(a >= 3 && a + 3 < frame, format!("the iterator call at {a} needs slots {}..={}", a.wrapping_sub(3), a + 3))?;
            check(b >= 2 && a + b - 2 < frame, format!("the {} results of the iterator call at {a} end beyond the frame", b.wrapping_sub(1)))?;
            check(c == 3, "the iterator call must pass 2 arguments".to_owned())?;
            let next = p.bc.get(pc as usize + 1).copied().and_then(Op::from_ins);
            check(next == Some(Op::ITERL), format!("{op:?} is not followed by ITERL"))?;
        }
        Op::ITERL | Op::ISNEXT => check(a >= 3 && a + 3 < frame, format!("{op:?} at {a} needs slots {}..={}", a.wrapping_sub(3), a + 3))?,
        Op::VARG => check(b < 2 || a + b - 2 < frame, format!("the {} values of VARG at {a} end beyond the frame", b.wrapping_sub(1)))?,
        Op::RET => check(d >= 1 && (d < 2 || a + d - 2 < frame), format!("RET {a} with {d} does not fit the frame of {frame}"))?,
        Op::RETM => check(a + d <= frame, format!("RETM {a} with {d} values does not fit the frame of {frame}"))?,
        Op::RET1 => check(a < frame, format!("RET1 {a} is outside the frame of {frame}"))?,
        Op::TSETM => check(a >= 1 && a < frame, format!("TSETM {a} needs its table at {}", a.wrapping_sub(1)))?,
        Op::FORI | Op::FORL => check(a + 3 < frame, format!("{op:?} at {a} needs slots {a}..={}", a + 3))?,
        Op::TNEW => check(d >> 11 <= 15, "TNEW hash size hint is too large".to_owned())?,
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bc::{ad, aj, Op};
    use crate::proto::FLAG_VARARG;

    fn chunk(bc: Vec<u32>, framesize: u8) -> Proto {
        let lines = vec![1; bc.len()];
        Proto { flags: FLAG_VARARG, framesize, bc, lines, firstline: 1, name: "t".to_owned(), ..Proto::default() }
    }

    #[test]
    fn accepts_a_return() {
        assert_eq!(verify(&chunk(vec![ad(Op::RET0, 0, 1)], 1), 0), Ok(()));
    }

    #[test]
    fn rejects_bad_slots_and_jumps() {
        let e = verify(&chunk(vec![ad(Op::KSHORT, 3, 1), ad(Op::RET0, 0, 1)], 2), 0).unwrap_err();
        assert!(e.message.contains("outside the frame"), "{e}");
        let e = verify(&chunk(vec![aj(Op::JMP, 0, 5), ad(Op::RET0, 0, 1)], 2), 0).unwrap_err();
        assert!(e.message.contains("jump to 6"), "{e}");
        let e = verify(&chunk(vec![ad(Op::ISEQP, 0, 0), ad(Op::RET0, 0, 1)], 2), 0).unwrap_err();
        assert!(e.message.contains("not followed by JMP"), "{e}");
        let e = verify(&chunk(vec![ad(Op::KSHORT, 0, 1)], 2), 0).unwrap_err();
        assert!(e.message.contains("not a return"), "{e}");
        let e = verify(&chunk(vec![ad(Op::KSTR, 0, 0), ad(Op::RET0, 0, 1)], 2), 0).unwrap_err();
        assert!(e.message.contains("not a string constant"), "{e}");
    }

    #[test]
    fn level_1_opcodes() {
        use crate::bc::abc;
        let nl = |bc: Vec<u32>, framesize: u8| {
            let mut p = chunk(bc, framesize);
            p.flags |= crate::proto::FLAG_NL;
            p
        };
        let e = verify(&chunk(vec![ad(Op::KINTR, 0, 1), ad(Op::RET0, 0, 1)], 2), 0).unwrap_err();
        assert!(e.message.contains("level-0 chunk"), "{e}");
        let e = verify(&chunk(vec![ad(Op::RET0, 0, 1)], 2), 1).unwrap_err();
        assert!(e.message.contains("without FLAG_NL"), "{e}");
        assert_eq!(verify(&nl(vec![ad(Op::KINTR, 0, 1), ad(Op::RET0, 0, 1)], 2), 1), Ok(()));
        let e = verify(&nl(vec![ad(Op::KINTR, 0, 0), ad(Op::RET0, 0, 1)], 2), 1).unwrap_err();
        assert!(e.message.contains("not an intrinsic"), "{e}");
        let e = verify(&nl(vec![ad(Op::ISMETH, 0, 1), ad(Op::RET0, 0, 1)], 2), 1).unwrap_err();
        assert!(e.message.contains("not followed by JMP"), "{e}");
        let ok = nl(vec![abc(Op::MSELF, 0, 0, 1), abc(Op::CALLM, 0, 2, 0), ad(Op::RET0, 0, 1)], 3);
        assert_eq!(verify(&ok, 1), Ok(()));
        let e = verify(&nl(vec![abc(Op::MSELF, 0, 0, 1), abc(Op::CALLM, 0, 2, 1), ad(Op::RET0, 0, 1)], 3), 1).unwrap_err();
        assert!(e.message.contains("MULTRES"), "{e}");
    }

    #[test]
    fn bit_operators() {
        use crate::bc::abc;
        use crate::proto::FLAG_NL;
        let band = vec![abc(Op::BAND, 2, 0, 1), ad(Op::RET1, 2, 2)];
        let mut p = chunk(band.clone(), 3);
        p.flags |= FLAG_NL;
        assert_eq!(verify(&p, 1), Ok(()));
        p.flags |= PROTO_BITOP;
        let e = verify(&p, 1).unwrap_err();
        assert!(e.message.contains("PROTO_BITOP"), "{e}");
        let e = verify(&chunk(band, 3), 0).unwrap_err();
        assert!(e.message.contains("BAND in a level-0 chunk"), "{e}");
        let mut q = chunk(vec![abc(Op::BOR, 0, 0, 3), ad(Op::RET0, 0, 1)], 3);
        q.flags |= FLAG_NL;
        let e = verify(&q, 1).unwrap_err();
        assert!(e.message.contains("outside the frame"), "{e}");
    }
}
