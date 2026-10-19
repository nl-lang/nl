//! The bytecode dump writer: the format of `src/lj_bcdump.h` and
//! `src/lj_bcwrite.c` of the pinned LuaJIT.
//!
//! ```text
//! dump   = header proto+ 0U
//! header = ESC 'L' 'J' versionB flagsU [namelenU nameB*]
//! proto  = lengthU pdata
//! pdata  = phead bcinsW* uvdataH* kgc* knum* [debugB*]
//! phead  = flagsB numparamsB framesizeB numuvB numkgcU numknU numbcU
//!          [debuglenU [firstlineU numlineU]]
//! ```
//!
//! Prototypes are written children first (each in the order of its
//! parent's GC constants), because the loader pushes each one it reads and
//! pops a child when the parent's constants name one. The GC constants
//! themselves are written from the highest index down, as LuaJIT lays them
//! out in memory. The header always carries `BCDUMP_F_FR2`
//! (two-slot frames, the GC64 layout), `BCDUMP_F_STRIP` when debug
//! information is left out, `BCDUMP_F_NL` for a level-1 dump, which
//! only the fork loads, and `BCDUMP_F_BITOP`
//! when a prototype uses the bit operators (level 1; the loader accepts
//! the flag and does nothing else with it, and the prototypes do not
//! carry `PROTO_BITOP`: see [`crate::proto`]).

use crate::bc::Op;
use crate::proto::{KGc, KTabValue, KTable, Proto, VarName};

const HEAD: [u8; 3] = [0x1b, b'L', b'J'];
const VERSION: u8 = 2;
const F_STRIP: u32 = 0x02;
const F_FR2: u32 = 0x08;
const F_BITOP: u32 = 0x10;
const F_NL: u32 = 0x20;

const KGC_CHILD: u32 = 0;
const KGC_TAB: u32 = 1;
const KGC_STR: u32 = 5;

const KTAB_NIL: u8 = 0;
const KTAB_FALSE: u8 = 1;
const KTAB_TRUE: u8 = 2;
const KTAB_INT: u8 = 3;
const KTAB_NUM: u8 = 4;
const KTAB_STR: u32 = 5;

/// Writes the dump of a chunk whose top-level prototype is `main`,
/// emitted at feature `level` (0: stock opcodes; 1: the fork's).
pub fn write(main: &Proto, chunk_name: &[u8], strip: bool, level: u8) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&HEAD);
    out.push(VERSION);
    let bitop = if uses_bitops(main) { F_BITOP } else { 0 };
    uleb(&mut out, F_FR2 | if strip { F_STRIP } else { 0 } | bitop | if level >= 1 { F_NL } else { 0 });
    if !strip {
        uleb(&mut out, len_u32(chunk_name.len()));
        out.extend_from_slice(chunk_name);
    }
    proto(&mut out, main, strip);
    out.push(0);
    out
}

/// True when the prototype or one nested in it has a bit operator.
fn uses_bitops(p: &Proto) -> bool {
    p.bc.iter().any(|i| Op::from_ins(*i).is_some_and(Op::is_bitop)) || p.children.iter().any(uses_bitops)
}

fn len_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// ULEB128 of a 32-bit value.
fn uleb(out: &mut Vec<u8>, v: u32) {
    uleb64(out, u64::from(v));
}

fn uleb64(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v & 0x7f) as u8 | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

/// The integer a number constant is narrowed to, as `lj_num2int_check`
/// decides: integral, in `i32` range, and not `-0`.
fn narrow(n: f64) -> Option<i32> {
    if n.is_finite() && n.fract() == 0.0 && n >= f64::from(i32::MIN) && n <= f64::from(i32::MAX) && !(n == 0.0 && n.is_sign_negative())
    {
        Some(n as i32)
    } else {
        None
    }
}

fn proto(out: &mut Vec<u8>, p: &Proto, strip: bool) {
    // Children first, in the order of the GC constants.
    for k in &p.kgc {
        if let KGc::Child(i) = k {
            if let Some(c) = p.children.get(*i) {
                proto(out, c, strip);
            }
        }
    }
    let mut d = vec![p.flags, p.numparams, p.framesize, u8::try_from(p.uv.len()).unwrap_or(u8::MAX)];
    uleb(&mut d, len_u32(p.kgc.len()));
    uleb(&mut d, len_u32(p.knum.len()));
    uleb(&mut d, len_u32(p.bc.len()));
    let debug = if strip { Vec::new() } else { debug_info(p) };
    if !strip {
        uleb(&mut d, len_u32(debug.len()));
        if !debug.is_empty() {
            uleb(&mut d, p.firstline);
            uleb(&mut d, p.numline);
        }
    }
    for ins in &p.bc {
        d.extend_from_slice(&ins.to_le_bytes());
    }
    for uv in &p.uv {
        d.extend_from_slice(&uv.to_le_bytes());
    }
    for k in p.kgc.iter().rev() {
        match k {
            KGc::Child(_) => uleb(&mut d, KGC_CHILD),
            KGc::Str(s) => {
                uleb(&mut d, KGC_STR + len_u32(s.len()));
                d.extend_from_slice(s);
            }
            KGc::Table(t) => {
                uleb(&mut d, KGC_TAB);
                ktab(&mut d, t);
            }
        }
    }
    for n in &p.knum {
        knum(&mut d, *n);
    }
    d.extend_from_slice(&debug);
    uleb(out, len_u32(d.len()));
    out.extend_from_slice(&d);
}

/// A number constant: a 33-bit ULEB128 whose low bit says whether a
/// second ULEB128 with the high word follows (`bcwrite_knum`).
fn knum(out: &mut Vec<u8>, n: f64) {
    match narrow(n) {
        Some(k) => uleb64(out, ((i64::from(k) << 1) as u64) & 0x1_ffff_ffff),
        None => {
            let bits = n.to_bits();
            let lo = bits & 0xffff_ffff;
            let hi = (bits >> 32) as u32;
            uleb64(out, (lo << 1) | 1);
            uleb(out, hi);
        }
    }
}

fn ktab(out: &mut Vec<u8>, t: &KTable) {
    let narray = t.array.iter().rposition(|v| *v != KTabValue::Nil).map_or(0, |i| i + 1);
    uleb(out, len_u32(narray));
    uleb(out, len_u32(t.hash.len()));
    for v in t.array.iter().take(narray) {
        ktabk(out, v);
    }
    for (k, v) in &t.hash {
        ktabk(out, k);
        ktabk(out, v);
    }
}

/// A key or value of a template table. Integral numbers are written as
/// integers, keys included: that is what a dual-number VM stores, and the
/// loader accepts either form for a key.
fn ktabk(out: &mut Vec<u8>, v: &KTabValue) {
    match v {
        KTabValue::Nil => out.push(KTAB_NIL),
        KTabValue::False => out.push(KTAB_FALSE),
        KTabValue::True => out.push(KTAB_TRUE),
        KTabValue::Num(n) => match narrow(*n) {
            Some(k) => {
                out.push(KTAB_INT);
                uleb(out, k as u32);
            }
            None => {
                let bits = n.to_bits();
                out.push(KTAB_NUM);
                uleb(out, (bits & 0xffff_ffff) as u32);
                uleb(out, (bits >> 32) as u32);
            }
        },
        KTabValue::Str(s) => {
            uleb(out, KTAB_STR + len_u32(s.len()));
            out.extend_from_slice(s);
        }
    }
}

/// Line info, upvalue names and variable info (`fs_fixup_line`,
/// `fs_prep_var`).
fn debug_info(p: &Proto) -> Vec<u8> {
    let mut d = Vec::new();
    let first = p.firstline;
    for line in &p.lines {
        let delta = line.saturating_sub(first);
        if p.numline < 256 {
            d.push(delta as u8);
        } else if p.numline < 65536 {
            d.extend_from_slice(&(delta as u16).to_le_bytes());
        } else {
            d.extend_from_slice(&delta.to_le_bytes());
        }
    }
    for name in &p.uvnames {
        d.extend_from_slice(name);
        d.push(0);
    }
    let mut lastpc = 0;
    for v in &p.vars {
        match &v.name {
            VarName::Name(n) => {
                d.extend_from_slice(n);
                d.push(0);
            }
            VarName::Internal(code) => d.push(*code),
        }
        uleb(&mut d, v.startpc.saturating_sub(lastpc));
        uleb(&mut d, v.endpc.saturating_sub(v.startpc));
        lastpc = v.startpc;
    }
    d.push(0);
    d
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(f: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
        let mut v = Vec::new();
        f(&mut v);
        v
    }

    /// The 33-bit encodings `lj_bcwrite.c` produces (checked against the
    /// reader's `bcread_uleb128_33`).
    #[test]
    fn number_constants() {
        assert_eq!(bytes(|o| knum(o, 1.0)), vec![0x02]);
        assert_eq!(bytes(|o| knum(o, 64.0)), vec![0x80, 0x01]);
        assert_eq!(bytes(|o| knum(o, -1.0)), vec![0xfe, 0xff, 0xff, 0xff, 0x1f]);
        // 1.5 = 0x3ff8000000000000: lo = 0, hi = 0x3ff80000.
        assert_eq!(bytes(|o| knum(o, 1.5)), vec![0x01, 0x80, 0x80, 0xe0, 0xff, 0x03]);
        // -0 is never narrowed.
        assert_eq!(bytes(|o| knum(o, -0.0)), vec![0x01, 0x80, 0x80, 0x80, 0x80, 0x08]);
    }

    #[test]
    fn uleb128() {
        assert_eq!(bytes(|o| uleb(o, 0)), vec![0]);
        assert_eq!(bytes(|o| uleb(o, 127)), vec![127]);
        assert_eq!(bytes(|o| uleb(o, 128)), vec![0x80, 1]);
        assert_eq!(bytes(|o| uleb(o, u32::MAX)), vec![0xff, 0xff, 0xff, 0xff, 0x0f]);
    }

    #[test]
    fn header_and_footer() {
        let p = Proto { bc: vec![crate::bc::ad(crate::bc::Op::RET0, 0, 1)], lines: vec![1], framesize: 1, ..Proto::default() };
        let d = write(&p, b"=t", false, 0);
        assert_eq!(&d[..5], &[0x1b, b'L', b'J', 2, 0x08]);
        assert_eq!(&d[5..8], &[2, b'=', b't']);
        assert_eq!(d.last(), Some(&0));
        let s = write(&p, b"=t", true, 0);
        assert_eq!(&s[..5], &[0x1b, b'L', b'J', 2, 0x0a]);
        assert!(s.len() < d.len());
    }
}
