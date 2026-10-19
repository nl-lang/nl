//! The bytecode backend: the NL core IR to a LuaJIT 2.1 bytecode dump,
//! in the GC64 two-slot-frame layout (`BCDUMP_F_FR2`).
//!
//! Two feature levels ([`Options::level`]):
//!
//! - **Level 0**: stock opcodes only; the dump loads in a stock LuaJIT.
//!   Member calls, auto-call reads, `x[k]`, `.len`, `==` and the ordering
//!   operators go through the runtime's helpers (`rt.mcall`, `rt.index`,
//!   …), as the Lua backend prints them.
//! - **Level 1**: the fork's opcodes and flags ([`bc`]):
//!   intrinsics through `KINTR`, member calls through `MSELF`, auto-call
//!   reads through `ISMETH`, `>` and `>=` through `CMPGT`/`CMPGE`, every
//!   prototype marked `FLAG_NL` and methods `FLAG_METHOD`; the lowering
//!   emits plain index, length and comparison operations, because the
//!   fork's VM gives them NL's semantics. The dump carries `BCDUMP_F_NL`.
//!
//! - [`emit`] (private): the compiler proper, in the style of LuaJIT's own
//!   `lj_parse.c`: register allocation, expression discharge, jump lists,
//!   constants, closures and upvalues, loops and scopes.
//! - [`proto`]: the prototype the generator builds.
//! - [`dump`]: the dump writer.
//! - [`verify`]: the bytecode verifier, run on every chunk in debug builds
//!   and in tests.
//! - [`bc`]: the opcodes.
//!
//! The chunk follows the contract of the Lua backend
//! (`crates/nl_capi/src/driver.rs`): loading the dump gives a function
//! `P`, and `P(rt)` returns the file's main function.
//!
//! This crate is pure: it never calls into Lua, and every problem is an
//! [`Error`] for the driver to report as a diagnostic.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing))]

pub mod bc;
pub mod dump;
mod emit;
pub mod intrinsics;
pub mod proto;
pub mod verify;

pub use emit::Error;

/// Per-emit options.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// Leave debug information (lines, variable and upvalue names, the
    /// chunk name) out of the dump.
    pub strip: bool,
    /// The feature level (see the crate documentation): 0 for stock
    /// LuaJIT, 1 for the fork.
    pub level: u8,
}

/// Compiles a chunk to its top-level prototype at a feature level,
/// verified in debug builds.
pub fn compile(chunk: &nl_ir::ir::Chunk, level: u8) -> Result<proto::Proto, Error> {
    let proto = emit::compile_chunk(chunk, level)?;
    if cfg!(debug_assertions) {
        verify::verify(&proto, level).map_err(|e| Error { line: chunk.main.line, message: format!("internal error: the emitted bytecode is invalid ({e})") })?;
    }
    Ok(proto)
}

/// Compiles a chunk and writes its dump.
pub fn emit(chunk: &nl_ir::ir::Chunk, options: &Options) -> Result<Vec<u8>, Error> {
    let proto = compile(chunk, options.level)?;
    Ok(dump::write(&proto, chunk.name.as_bytes(), options.strip, options.level))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nl_ir::ir::*;

    fn stmt(line: u32, kind: StmtKind) -> Stmt {
        Stmt { line, kind }
    }

    fn chunk(locals: &[&str], body: Vec<Stmt>, owner: Option<LocalId>) -> Chunk {
        Chunk {
            locals: locals.iter().map(|h| LocalInfo { hint: (*h).to_owned() }).collect(),
            main: Function { params: Vec::new(), vararg: true, body: Block::new(body), line: 1, name: None, method: false, transient: false },
            name: "=t".to_owned(),
            owner,
        }
    }

    fn proto_of(c: &Chunk) -> proto::Proto {
        let p = compile(c, 0).unwrap();
        verify::verify(&p, 0).unwrap();
        p
    }

    fn proto_at_1(c: &Chunk) -> proto::Proto {
        let p = compile(c, 1).unwrap();
        verify::verify(&p, 1).unwrap();
        p
    }

    fn main_of(p: &proto::Proto) -> &proto::Proto {
        &p.children[0]
    }

    fn ops(p: &proto::Proto) -> Vec<bc::Op> {
        p.bc.iter().map(|i| bc::Op::from_ins(*i).unwrap()).collect()
    }

    /// The loader stores `rt`, makes the main closure and returns it
    /// with its upvalue closed.
    #[test]
    fn loader_shape() {
        let c = chunk(&[], vec![stmt(1, StmtKind::Return(vec![Expr::Table(Vec::new())]))], None);
        let p = proto_of(&c);
        assert_eq!(ops(&p), vec![bc::Op::VARG, bc::Op::FNEW, bc::Op::UCLO, bc::Op::RET1]);
        assert_eq!(p.uvnames.len(), 0);
        let m = main_of(&p);
        assert_eq!(m.flags & proto::FLAG_VARARG, proto::FLAG_VARARG);
        assert_eq!(ops(m), vec![bc::Op::TNEW, bc::Op::RET1]);
    }

    /// `rt.name` is an upvalue read plus a field read; the owner local is
    /// computed once in the loader.
    #[test]
    fn intrinsics_and_owner() {
        let owner = LocalId(0);
        let body = vec![stmt(2, StmtKind::Expr(Expr::intrinsic_call("primmethod", vec![Expr::Local(owner)]))), stmt(3, StmtKind::Return(Vec::new()))];
        let c = chunk(&["owner"], body, Some(owner));
        let p = proto_of(&c);
        assert_eq!(ops(&p), vec![bc::Op::VARG, bc::Op::TGETS, bc::Op::KSTR, bc::Op::CALL, bc::Op::FNEW, bc::Op::UCLO, bc::Op::RET1]);
        let m = main_of(&p);
        assert_eq!(ops(m), vec![bc::Op::UGET, bc::Op::TGETS, bc::Op::UGET, bc::Op::CALL, bc::Op::RET0]);
        assert_eq!(m.uvnames, vec![b"rt".to_vec(), b"owner".to_vec()]);
        assert_eq!(m.uv, vec![proto::UV_LOCAL | proto::UV_IMMUTABLE, proto::UV_LOCAL | proto::UV_IMMUTABLE | 1]);
    }

    /// Arithmetic with literals uses the VN/NV forms and folds constants.
    #[test]
    fn arithmetic_forms() {
        let a = LocalId(0);
        let body = vec![
            stmt(1, StmtKind::Local { locals: vec![a], values: vec![Expr::binary(BinOp::Add, Expr::Number(1.0), Expr::Number(2.0))] }),
            stmt(2, StmtKind::Assign { targets: vec![Place::Local(a)], values: vec![Expr::binary(BinOp::Sub, Expr::Number(10.0), Expr::Local(a))] }),
            stmt(3, StmtKind::Assign { targets: vec![Place::Local(a)], values: vec![Expr::binary(BinOp::Mul, Expr::Local(a), Expr::Number(2.5))] }),
            stmt(4, StmtKind::Return(vec![Expr::Local(a)])),
        ];
        let p = proto_of(&chunk(&["a"], body, None));
        let m = main_of(&p);
        assert_eq!(ops(m), vec![bc::Op::KSHORT, bc::Op::SUBNV, bc::Op::MULVN, bc::Op::RET1]);
        assert_eq!(bc::d_of(m.bc[0]), 3);
        assert_eq!(m.knum, vec![10.0, 2.5]);
        assert_eq!(m.vars.len(), 1);
        assert_eq!(m.vars[0].name, proto::VarName::Name(b"a".to_vec()));
    }

    /// A loop body that captures a local closes it on every exit, and
    /// `continue` jumps to the label at the end of the body.
    #[test]
    fn loops_close_upvalues() {
        let (i, f, label) = (LocalId(0), LocalId(1), LabelId(0));
        let inner = Function { params: Vec::new(), vararg: false, body: Block::new(vec![stmt(3, StmtKind::Return(vec![Expr::Local(i)]))]), line: 3, name: None, method: false, transient: false };
        let body = Block::new(vec![
            stmt(2, StmtKind::Do(Block::new(vec![
                stmt(3, StmtKind::Local { locals: vec![f], values: vec![Expr::Function(Box::new(inner))] }),
                stmt(4, StmtKind::If { cond: Expr::Local(f), then: Block::new(vec![stmt(4, StmtKind::Goto(label))]), otherwise: None }),
                stmt(5, StmtKind::Break),
            ]))),
            stmt(6, StmtKind::Label(label)),
        ]);
        let stmts = vec![
            stmt(2, StmtKind::NumericFor { var: i, start: Expr::Number(1.0), limit: Expr::Number(3.0), step: None, body }),
            stmt(7, StmtKind::Return(Vec::new())),
        ];
        let p = proto_of(&chunk(&["i", "f"], stmts, None));
        let m = main_of(&p);
        let o = ops(m);
        assert!(o.contains(&bc::Op::FORI) && o.contains(&bc::Op::FORL), "{o:?}");
        assert!(o.iter().filter(|op| **op == bc::Op::UCLO).count() >= 2, "{o:?}");
        assert_eq!(m.children[0].uv, vec![proto::UV_LOCAL | proto::UV_IMMUTABLE | 3]);
    }

    /// Too many locals in one function is an error that names it.
    #[test]
    fn limits_are_errors() {
        let n = 210u32;
        let locals: Vec<String> = (0..n).map(|i| format!("v{i}")).collect();
        let names: Vec<&str> = locals.iter().map(String::as_str).collect();
        let mut body: Vec<Stmt> = (0..n).map(|i| stmt(i + 1, StmtKind::Local { locals: vec![LocalId(i)], values: vec![Expr::Number(f64::from(i))] })).collect();
        body.push(stmt(n + 1, StmtKind::Return(Vec::new())));
        let f = Function { params: Vec::new(), vararg: false, body: Block::new(body), line: 1, name: Some("big".to_owned()), method: false, transient: false };
        let c = chunk(&names, vec![stmt(1, StmtKind::Expr(Expr::call(Expr::Function(Box::new(f)), Vec::new()))), stmt(2, StmtKind::Return(Vec::new()))], None);
        let e = compile(&c, 0).unwrap_err();
        assert!(e.message.starts_with("big has more than 200 local variables"), "{}", e.message);
    }

    /// Level 1: intrinsics load with `KINTR`, every prototype is marked
    /// `FLAG_NL`, methods `FLAG_METHOD`, and the dump carries `BCDUMP_F_NL`.
    #[test]
    fn level_1_intrinsics_and_flags() {
        let owner = LocalId(0);
        let method = Function { params: vec![LocalId(1)], vararg: false, body: Block::new(vec![stmt(3, StmtKind::Return(Vec::new()))]), line: 3, name: Some("T.m".to_owned()), method: true, transient: false };
        let body = vec![
            stmt(2, StmtKind::Expr(Expr::intrinsic_call("primmethod", vec![Expr::Local(owner)]))),
            stmt(3, StmtKind::Expr(Expr::intrinsic_call("method", vec![Expr::Function(Box::new(method))]))),
            stmt(4, StmtKind::Return(Vec::new())),
        ];
        let c = chunk(&["owner", "self"], body, Some(owner));
        let p = proto_at_1(&c);
        assert_eq!(ops(&p), vec![bc::Op::VARG, bc::Op::KINTR, bc::Op::KSTR, bc::Op::CALL, bc::Op::FNEW, bc::Op::UCLO, bc::Op::RET1]);
        assert_eq!(bc::d_of(p.bc[1]), intrinsics::index("owner").unwrap());
        assert_eq!(p.flags & proto::FLAG_NL, proto::FLAG_NL);
        let m = main_of(&p);
        assert_eq!(ops(m), vec![bc::Op::KINTR, bc::Op::UGET, bc::Op::CALL, bc::Op::KINTR, bc::Op::FNEW, bc::Op::CALL, bc::Op::UCLO, bc::Op::RET0]);
        assert_eq!(m.flags & (proto::FLAG_NL | proto::FLAG_METHOD), proto::FLAG_NL);
        assert_eq!(m.children[0].flags & (proto::FLAG_NL | proto::FLAG_METHOD), proto::FLAG_NL | proto::FLAG_METHOD);
        let full = emit(&c, &Options { strip: false, level: 1 }).unwrap();
        assert_eq!(full[4] & 0x20, 0x20);
        let level0 = emit(&c, &Options { strip: false, level: 0 }).unwrap();
        assert_eq!(level0[4] & 0x20, 0);
    }

    /// Level 1: `x.m(a)` is `MOV; TGETS; …; MSELF; CALLM`, `x.m` is
    /// `MOV; TGETS; ISMETH; JMP; CALL`, and `a > b` is `CMPGT`.
    #[test]
    fn level_1_members_and_comparisons() {
        let (x, r) = (LocalId(0), LocalId(1));
        let body = vec![
            stmt(1, StmtKind::Local { locals: vec![x], values: vec![Expr::Table(Vec::new())] }),
            stmt(2, StmtKind::Expr(Expr::MemberCall { receiver: Box::new(Expr::Local(x)), key: Box::new(Expr::str("m")), args: vec![Expr::Number(1.0)] })),
            stmt(3, StmtKind::Local { locals: vec![r], values: vec![Expr::AutoGet { receiver: Box::new(Expr::Local(x)), key: Box::new(Expr::str("f")) }] }),
            stmt(4, StmtKind::If { cond: Expr::binary(BinOp::Gt, Expr::Local(r), Expr::Local(x)), then: Block::new(vec![stmt(4, StmtKind::Return(vec![Expr::Local(r)]))]), otherwise: None }),
            stmt(5, StmtKind::Return(vec![Expr::MemberCall { receiver: Box::new(Expr::Local(x)), key: Box::new(Expr::str("n")), args: vec![Expr::Local(r), Expr::Vararg] }])),
        ];
        let c = Chunk {
            locals: vec![LocalInfo { hint: "x".to_owned() }, LocalInfo { hint: "r".to_owned() }],
            main: Function { params: Vec::new(), vararg: true, body: Block::new(body), line: 1, name: None, method: false, transient: false },
            name: "=t".to_owned(),
            owner: None,
        };
        let p = proto_at_1(&c);
        let m = main_of(&p);
        let o = ops(m);
        use bc::Op::*;
        assert_eq!(
            o,
            vec![
                TNEW, MOV, TGETS, KSHORT, MSELF, CALLM, MOV, TGETS, ISMETH, JMP, CALL, CMPNGT, JMP, RET1, MOV, TGETS, MOV, VARG, MSELF, CALLMT
            ],
            "{o:?}"
        );
        // MSELF x.m(1): callee at 1, B = 0, C = 2 (receiver and one arg).
        assert_eq!((bc::a_of(m.bc[4]), bc::b_of(m.bc[4]), bc::c_of(m.bc[4])), (1, 0, 2));
        assert_eq!((bc::a_of(m.bc[5]), bc::b_of(m.bc[5]), bc::c_of(m.bc[5])), (1, 1, 0));
        // ISMETH tests the member against the receiver copy.
        assert_eq!((bc::a_of(m.bc[8]), bc::d_of(m.bc[8])), (1, 3));
        // The tail call with a vararg tail: MSELF with B = 1, C = 2.
        assert_eq!((bc::b_of(m.bc[18]), bc::c_of(m.bc[18])), (1, 2));
        assert_eq!(bc::d_of(m.bc[19]), 0);
    }

    /// Level 1: the numeric bitwise intrinsics are the VM's bit opcodes,
    /// operands in registers (the opcodes have no constant forms), two
    /// constants folded; the runtime's `band` stays a call; the dump
    /// carries `BCDUMP_F_BITOP`, the prototypes no `PROTO_BITOP`. Level 0
    /// calls `rt.nband`.
    #[test]
    fn level_1_bit_operators() {
        let (a, b, r) = (LocalId(0), LocalId(1), LocalId(2));
        let call = |name: &'static str, args: Vec<Expr>| Expr::intrinsic_call(name, args);
        let body = vec![
            stmt(1, StmtKind::Local { locals: vec![a, b], values: vec![Expr::Vararg] }),
            stmt(2, StmtKind::Local { locals: vec![r], values: vec![call("nband", vec![Expr::Local(a), Expr::Local(b)])] }),
            stmt(3, StmtKind::Assign { targets: vec![Place::Local(r)], values: vec![call("nlshift", vec![Expr::Local(r), Expr::Number(13.0)])] }),
            stmt(4, StmtKind::Assign { targets: vec![Place::Local(r)], values: vec![call("nbnot", vec![call("nrshift", vec![Expr::Number(-16.0), Expr::Local(r)])])] }),
            stmt(5, StmtKind::Assign { targets: vec![Place::Local(r)], values: vec![call("nbxor", vec![Expr::Number(3.7), Expr::Number(4294967301.0)])] }),
            stmt(6, StmtKind::Assign { targets: vec![Place::Local(r)], values: vec![call("nbnot", vec![Expr::Number(2.5)])] }),
            stmt(7, StmtKind::Return(vec![call("band", vec![Expr::Local(r), Expr::Local(a)])])),
        ];
        let c = chunk(&["a", "b", "r"], body, None);
        let p = proto_at_1(&c);
        let m = main_of(&p);
        use bc::Op::*;
        assert_eq!(ops(m), vec![VARG, BAND, KSHORT, BSHL, KSHORT, BSHR, BNOT, KSHORT, KSHORT, KINTR, MOV, MOV, CALL, RET1], "{:?}", ops(m));
        // BAND r = a & b; BSHL r = r << tmp; BSHR tmp = tmp >>> r.
        assert_eq!((bc::a_of(m.bc[1]), bc::b_of(m.bc[1]), bc::c_of(m.bc[1])), (2, 0, 1));
        assert_eq!((bc::a_of(m.bc[2]), bc::d_of(m.bc[2])), (3, 13));
        assert_eq!((bc::a_of(m.bc[3]), bc::b_of(m.bc[3]), bc::c_of(m.bc[3])), (2, 2, 3));
        assert_eq!((bc::b_of(m.bc[5]), bc::c_of(m.bc[5])), (3, 2));
        assert_eq!((bc::a_of(m.bc[6]), bc::d_of(m.bc[6])), (2, 3));
        // 3.7 ^ (2^32 + 5) is 4 ^ 5; ~2.5 is ~2.
        assert_eq!(bc::d_of(m.bc[7]) as u16 as i16, 1);
        assert_eq!(bc::d_of(m.bc[8]) as u16 as i16, -3);
        assert_eq!(bc::d_of(m.bc[9]), intrinsics::index("band").unwrap());
        assert_eq!((p.flags | m.flags) & 0x80, 0);
        let full = emit(&c, &Options { strip: false, level: 1 }).unwrap();
        assert_eq!(full[4] & 0x10, 0x10);

        let p0 = proto_of(&c);
        let m0 = main_of(&p0);
        assert!(!ops(m0).iter().any(|o| o.is_bitop()), "{:?}", ops(m0));
        assert_eq!(emit(&c, &Options { strip: false, level: 0 }).unwrap()[4] & 0x10, 0);

        // Without bit operators, no flag.
        let plain = chunk(&[], vec![stmt(1, StmtKind::Return(vec![call("band", vec![Expr::Number(1.0), Expr::Number(2.0)])]))], None);
        assert_eq!(emit(&plain, &Options { strip: false, level: 1 }).unwrap()[4] & 0x10, 0);
    }

    /// Stripped dumps are smaller and carry the strip flag.
    #[test]
    fn strip() {
        let c = chunk(&[], vec![stmt(1, StmtKind::Return(vec![Expr::Table(Vec::new())]))], None);
        let full = emit(&c, &Options { strip: false, level: 0 }).unwrap();
        let stripped = emit(&c, &Options { strip: true, level: 0 }).unwrap();
        assert!(stripped.len() < full.len());
        assert_eq!(full[4] & 0x02, 0);
        assert_eq!(stripped[4] & 0x02, 0x02);
    }
}
