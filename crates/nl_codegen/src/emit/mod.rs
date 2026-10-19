//! The generator: core IR to prototypes, in the style of LuaJIT's own
//! compiler (`src/lj_parse.c` of the pinned LuaJIT). The parser's
//! machinery is ported as it is, because the bytecode has constraints
//! that are easiest to meet by construction: expression descriptors
//! ([`Exp`]) that are discharged late, jump lists threaded through the
//! instructions, a register stack with the active locals at
//! its bottom, and scopes that close upvalues (`UCLO`) on exit.
//!
//! This module holds the per-function state ([`FuncState`]) and what every
//! statement and expression needs: instruction emission, registers, jump
//! lists, constants, variables, upvalues and scopes, and the finishing of
//! a prototype. `expr.rs` compiles expressions, `stmt.rs` statements and
//! the chunk.
//!
//! # The chunk
//!
//! The dump's top-level function is the loader `P` of the Lua backend's
//! contract (`crates/nl_capi/src/driver.rs`): `P(rt)` stores `rt` in a
//! local, computes the chunk's owner when the chunk has one
//! ([`Chunk::owner`]), and returns a closure of the file's main function.
//! At level 0 an [`Expr::Intrinsic`] `name` compiles to `rt.name` through
//! an upvalue of `P`'s `rt` local; a Lua method call `x:m(…)` to
//! `rt.lmethod.m(x, …)`; a member call to `rt.member.m(x, …)` or
//! `rt.mcall(x, k, …)`; an auto-call read to `rt.mget(x, k)`, as the Lua
//! backend prints them. At level 1 an intrinsic
//! the table of [`crate::intrinsics`] holds loads with `KINTR`; `x:m(…)`
//! is Lua's own `MOV`/`TGETS`/`CALL`; `x.m(…)` is that with `MSELF` before
//! the `CALLM`; `x.m` is `TGETS`, `ISMETH` and a conditional `CALL`,
//! except in a position that takes several values, where `rt.mget` keeps
//! the method's results; `>` and `>=` are `CMPGT`/`CMPGE`.
//!
//! # Where the parser's choices are kept
//!
//! - The hoisted definitions of a block ([`StmtKind::Hoist`]) are emitted
//!   first, then the rest: lines are per instruction, so the Lua backend's
//!   `goto` layout that keeps source order is not needed.
//! - A function that creates closures closes upvalues (`UCLO 0`) before
//!   every return, as the parser does for returns after the first closure.
//! - `continue` is a `goto` to the label at the end of the loop body;
//!   `break` and `goto` leaving a scope with captured locals become `UCLO`
//!   jumps.
//! - `for … in` goes through `rt.iter`, so the `next` prediction
//!   (`ISNEXT`/`ITERN`) applies only when the iterator expression starts
//!   with a global, local or upvalue named `pairs` or `next` (a Lua path);
//!   the VM verifies the prediction at run time, as for Lua source.
//! - Primitive parameter checks are what the IR says, an inline test
//!   (`if type(x) ~= "number" then rt.check(…) end`, with Lua's `type`
//!   read from a local of the main chunk, nl_ir's `types::Inline`), and
//!   not `ISTYPE`/`ISNUM`: those raise Lua's "bad argument" errors, not
//!   NL's `TypeError` exceptions, and a `check` also reports the
//!   parameter name and the expected type.

use std::collections::{HashMap, HashSet};

use nl_ir::ir::*;

use crate::bc::{self, Ins, NO_JMP, NO_REG, Op};
use crate::proto::{self, KGc, Proto, VarInfo, VarName};

mod expr;
mod stmt;

pub(crate) use stmt::compile_chunk;

/// A limit the generated code would exceed, or an inconsistency in the IR.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub line: u32,
    pub message: String,
}

pub(crate) type R<T> = Result<T, Error>;

/// A variable of the chunk: a local of the IR, or the loader's `rt` local.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum VarId {
    Rt,
    Local(LocalId),
}

/// The key of an indexed expression (`VINDEXED`'s `aux`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Key {
    /// A string constant index (`TGETS`/`TSETS`), at most 255.
    Str(u32),
    /// A byte constant (`TGETB`/`TSETB`).
    Byte(u8),
    /// A register (`TGETV`/`TSETV`).
    Reg(u32),
}

/// The kinds of expression descriptors (`ExpKind`).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Kind {
    Nil,
    False,
    True,
    Str(Vec<u8>),
    Num(f64),
    /// A local in a register.
    Local(u32),
    /// An upvalue index.
    Upval(u32),
    /// A global by name.
    Global(Vec<u8>),
    /// `table[key]`: the table register and the key.
    Indexed(u32, Key),
    /// A comparison or test: the pc of its jump.
    Jmp(u32),
    /// An instruction whose destination register is still open: its pc.
    Reloc(u32),
    /// A value in a register.
    NonReloc(u32),
    /// A call or `...`: the pc of the instruction and its base register.
    Call(u32, u32),
    /// No value.
    Void,
}

/// An expression descriptor (`ExpDesc`): a kind plus the lists of jumps
/// taken when the expression is true (`t`) and false (`f`).
#[derive(Clone, Debug)]
pub(crate) struct Exp {
    pub k: Kind,
    pub t: u32,
    pub f: u32,
}

impl Exp {
    pub(crate) fn new(k: Kind) -> Exp {
        Exp { k, t: NO_JMP, f: NO_JMP }
    }

    pub(crate) fn has_jump(&self) -> bool {
        self.t != self.f
    }

    /// A constant (`expr_isk`).
    pub(crate) fn is_k(&self) -> bool {
        matches!(self.k, Kind::Nil | Kind::False | Kind::True | Kind::Str(_) | Kind::Num(_))
    }

    pub(crate) fn is_k_nojump(&self) -> bool {
        self.is_k() && !self.has_jump()
    }

    pub(crate) fn is_numk(&self) -> bool {
        matches!(self.k, Kind::Num(_))
    }

    pub(crate) fn is_numk_nojump(&self) -> bool {
        self.is_numk() && !self.has_jump()
    }

    pub(crate) fn num(&self) -> f64 {
        match self.k {
            Kind::Num(n) => n,
            _ => 0.0,
        }
    }

    /// The primitive operand of a nil/false/true constant.
    pub(crate) fn pri(&self) -> u32 {
        match self.k {
            Kind::False => bc::PRI_FALSE,
            Kind::True => bc::PRI_TRUE,
            _ => bc::PRI_NIL,
        }
    }

    /// The register of a `NonReloc`, or `None`.
    pub(crate) fn reg(&self) -> Option<u32> {
        match self.k {
            Kind::NonReloc(r) => Some(r),
            _ => None,
        }
    }
}

const SCOPE_LOOP: u8 = 0x01;
const SCOPE_BREAK: u8 = 0x02;
const SCOPE_GOLA: u8 = 0x04;
const SCOPE_UPVAL: u8 = 0x08;
const SCOPE_NOCLOSE: u8 = 0x10;

/// Where a pending `break` or `goto` goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Break,
    Label(LabelId),
}

/// A `break` or `goto` whose target is not known yet (`VSTACK_GOTO`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Goto {
    pub target: Target,
    /// The pc of its `JMP` (or `UCLO`, once it leaves a scope with
    /// captured locals).
    pub pc: u32,
    /// The number of active locals at its current scope level.
    pub slot: u32,
    pub line: u32,
}

/// A block scope (`FuncScope`).
#[derive(Debug, Default)]
pub(crate) struct Scope {
    /// The number of active locals outside the scope.
    pub nactvar: u32,
    pub flags: u8,
    /// The pending gotos of this scope, including those propagated out of
    /// inner scopes.
    pub gotos: Vec<Goto>,
}

/// An active local variable, indexed by slot.
#[derive(Clone, Debug)]
struct ActVar {
    id: Option<VarId>,
    /// Its entry in [`FuncState::vars`].
    info: usize,
}

/// An upvalue of the function being compiled.
#[derive(Clone, Debug)]
struct Upval {
    id: VarId,
    desc: u16,
}

/// The state of one function being compiled (`FuncState`).
pub(crate) struct FuncState {
    pub bc: Vec<Ins>,
    pub lines: Vec<u32>,
    /// Pending jumps to the next instruction.
    pub jpc: u32,
    /// The last pc that was a jump target.
    pub lasttarget: u32,
    pub freereg: u32,
    pub nactvar: u32,
    pub framesize: u32,
    pub numparams: u32,
    pub vararg: bool,
    /// The function creates closures: returns must close upvalues.
    pub has_closures: bool,
    /// A method (`FLAG_METHOD` at level 1).
    pub method: bool,
    pub linedefined: u32,
    pub name: String,
    knum: Vec<f64>,
    knum_map: HashMap<u64, u32>,
    kgc: Vec<KGc>,
    kstr_map: HashMap<Vec<u8>, u32>,
    children: Vec<Proto>,
    upvals: Vec<Upval>,
    uv_map: HashMap<VarId, u32>,
    actvars: Vec<ActVar>,
    var_slots: HashMap<VarId, u32>,
    vars: Vec<VarInfo>,
    pub scopes: Vec<Scope>,
}

impl FuncState {
    fn new(name: String, line: u32, vararg: bool, method: bool) -> FuncState {
        FuncState {
            bc: Vec::new(),
            lines: Vec::new(),
            jpc: NO_JMP,
            lasttarget: 0,
            freereg: 0,
            nactvar: 0,
            framesize: 1,
            numparams: 0,
            vararg,
            method,
            has_closures: false,
            linedefined: line,
            name,
            knum: Vec::new(),
            knum_map: HashMap::new(),
            kgc: Vec::new(),
            kstr_map: HashMap::new(),
            children: Vec::new(),
            upvals: Vec::new(),
            uv_map: HashMap::new(),
            actvars: Vec::new(),
            var_slots: HashMap::new(),
            vars: Vec::new(),
            scopes: Vec::new(),
        }
    }

    pub(crate) fn pc(&self) -> u32 {
        u32::try_from(self.bc.len()).unwrap_or(u32::MAX)
    }

    fn ins(&self, pc: u32) -> Ins {
        self.bc.get(pc as usize).copied().unwrap_or(0)
    }

    fn ins_mut(&mut self, pc: u32) -> Option<&mut Ins> {
        self.bc.get_mut(pc as usize)
    }

    /// The debug name of the active local in `slot`.
    pub(crate) fn local_name(&self, slot: u32) -> Option<Vec<u8>> {
        let v = self.actvars.get(slot as usize)?;
        match &self.vars.get(v.info)?.name {
            VarName::Name(n) => Some(n.clone()),
            VarName::Internal(_) => None,
        }
    }

    /// The variable an upvalue refers to.
    pub(crate) fn upval_name(&self, idx: u32) -> Option<VarId> {
        self.upvals.get(idx as usize).map(|u| u.id)
    }

    /// The string constant `idx`.
    pub(crate) fn kstr(&self, idx: u32) -> Option<&[u8]> {
        match self.kgc.get(idx as usize)? {
            KGc::Str(s) => Some(s.as_slice()),
            _ => None,
        }
    }
}

/// The generator over a chunk: the stack of functions being compiled.
pub(crate) struct Gen<'a> {
    pub chunk: &'a Chunk,
    pub funcs: Vec<FuncState>,
    /// The locals assigned after their declaration anywhere in the chunk:
    /// the others are immutable upvalues.
    pub assigned: HashSet<LocalId>,
    /// The line of the statement being compiled.
    pub line: u32,
    /// The feature level (the crate documentation).
    pub level: u8,
    /// The expression about to be compiled stands where several values are
    /// taken (the last argument of a call, the last value of a list): an
    /// auto-call read there keeps all of the method's results. Taken, and
    /// so reset, by every [`Gen::expr`].
    pub want_multi: bool,
}

impl<'a> Gen<'a> {
    pub(crate) fn fs(&mut self) -> &mut FuncState {
        // The stack is never empty while compiling; a missing entry would
        // be a bug of this module, answered with an unused scratch state.
        if self.funcs.is_empty() {
            self.funcs.push(FuncState::new("?".to_owned(), 0, false, false));
        }
        let n = self.funcs.len() - 1;
        #[allow(clippy::indexing_slicing)]
        &mut self.funcs[n]
    }

    pub(crate) fn fs_ref(&self) -> Option<&FuncState> {
        self.funcs.last()
    }

    pub(crate) fn err(&self, message: impl Into<String>) -> Error {
        Error { line: self.line, message: message.into() }
    }

    fn fname(&self) -> String {
        self.fs_ref().map_or_else(|| "a function".to_owned(), |f| f.name.clone())
    }

    // ---- instructions ----------------------------------------------------

    /// Emits an instruction (`bcemit_INS`): pending jumps to here are
    /// patched first.
    pub(crate) fn emit(&mut self, ins: Ins) -> R<u32> {
        let line = self.line;
        let pc = self.fs().pc();
        let jpc = self.fs().jpc;
        self.jmp_patchval(jpc, pc, NO_REG, pc)?;
        self.fs().jpc = NO_JMP;
        if pc >= bc::MAX_BCINS {
            return Err(self.err(format!("{} has more than {} bytecode instructions", self.fname(), bc::MAX_BCINS)));
        }
        let fs = self.fs();
        fs.bc.push(ins);
        fs.lines.push(line);
        Ok(pc)
    }

    pub(crate) fn emit_abc(&mut self, op: Op, a: u32, b: u32, c: u32) -> R<u32> {
        self.emit(bc::abc(op, a, b, c))
    }

    pub(crate) fn emit_ad(&mut self, op: Op, a: u32, d: u32) -> R<u32> {
        self.emit(bc::ad(op, a, d))
    }

    pub(crate) fn emit_aj(&mut self, op: Op, a: u32, j: u32) -> R<u32> {
        self.emit(bc::aj(op, a, j))
    }

    pub(crate) fn ins_at(&mut self, pc: u32) -> Ins {
        self.fs().ins(pc)
    }

    pub(crate) fn set_ins(&mut self, pc: u32, ins: Ins) {
        if let Some(i) = self.fs().ins_mut(pc) {
            *i = ins;
        }
    }

    pub(crate) fn patch_ins(&mut self, pc: u32, f: impl FnOnce(&mut Ins)) {
        if let Some(i) = self.fs().ins_mut(pc) {
            f(i);
        }
    }

    /// Sets the line of an instruction after the fact (loop control
    /// instructions carry the line of their statement).
    pub(crate) fn set_line(&mut self, pc: u32, line: u32) {
        if let Some(l) = self.fs().lines.get_mut(pc as usize) {
            *l = line;
        }
    }

    // ---- registers -------------------------------------------------------

    /// Raises the frame size for `n` more registers (`bcreg_bump`).
    pub(crate) fn bump(&mut self, n: u32) -> R<()> {
        let sz = self.fs().freereg + n;
        if sz > self.fs().framesize {
            if sz >= bc::MAX_SLOTS {
                return Err(self.err(format!(
                    "{} needs more than {} registers at once (LuaJIT's limit); split it into smaller functions",
                    self.fname(),
                    bc::MAX_SLOTS
                )));
            }
            self.fs().framesize = sz;
        }
        Ok(())
    }

    pub(crate) fn reserve(&mut self, n: u32) -> R<()> {
        self.bump(n)?;
        self.fs().freereg += n;
        Ok(())
    }

    /// Frees a temporary register (`bcreg_free`); registers of locals stay.
    pub(crate) fn free_reg(&mut self, reg: u32) {
        let fs = self.fs();
        if reg >= fs.nactvar {
            fs.freereg = fs.freereg.saturating_sub(1);
            debug_assert_eq!(reg, fs.freereg, "register freed out of order");
        }
    }

    pub(crate) fn free_exp(&mut self, e: &Exp) {
        if let Kind::NonReloc(r) = e.k {
            self.free_reg(r);
        }
    }

    // ---- jump lists ------------------------------------------------------

    fn jmp_next(&mut self, pc: u32) -> u32 {
        let delta = bc::j_of(self.ins_at(pc));
        if delta == -1 { NO_JMP } else { (pc as i64 + 1 + i64::from(delta)) as u32 }
    }

    /// True when an instruction on the list produces no value.
    fn jmp_novalue(&mut self, mut list: u32) -> bool {
        while list != NO_JMP {
            let p = self.ins_at(if list >= 1 { list - 1 } else { list });
            let op = Op::from_ins(p);
            if !(op == Some(Op::ISTC) || op == Some(Op::ISFC) || bc::a_of(p) == NO_REG) {
                return true;
            }
            list = self.jmp_next(list);
        }
        false
    }

    /// Patches the register of the test instruction before a jump
    /// (`jmp_patchtestreg`).
    fn jmp_patchtestreg(&mut self, pc: u32, reg: u32) -> bool {
        let tpc = if pc >= 1 { pc - 1 } else { pc };
        let mut ins = self.ins_at(tpc);
        let op = Op::from_ins(ins);
        if op == Some(Op::ISTC) || op == Some(Op::ISFC) {
            if reg != NO_REG && reg != bc::d_of(ins) {
                bc::set_a(&mut ins, reg);
            } else {
                bc::set_op(&mut ins, if op == Some(Op::ISTC) { Op::IST } else { Op::ISF });
                bc::set_a(&mut ins, 0);
            }
            self.set_ins(tpc, ins);
        } else if bc::a_of(ins) == NO_REG {
            if reg == NO_REG {
                let a = bc::a_of(self.ins_at(pc));
                self.set_ins(tpc, bc::aj(Op::JMP, a, 0));
            } else {
                bc::set_a(&mut ins, reg);
                self.set_ins(tpc, ins);
                let next = self.ins_at(tpc + 1);
                if reg >= bc::a_of(next) {
                    self.patch_ins(tpc + 1, |i| bc::set_a(i, reg + 1));
                }
            }
        } else {
            return false;
        }
        true
    }

    fn jmp_dropval(&mut self, mut list: u32) {
        while list != NO_JMP {
            self.jmp_patchtestreg(list, NO_REG);
            list = self.jmp_next(list);
        }
    }

    pub(crate) fn jmp_patchins(&mut self, pc: u32, dest: u32) -> R<()> {
        let offset = dest.wrapping_sub(pc + 1).wrapping_add(bc::BCBIAS_J);
        if offset > bc::BCMAX_D {
            return Err(self.err(format!("a control structure of {} is too long for one jump", self.fname())));
        }
        self.patch_ins(pc, |i| bc::set_d(i, offset));
        Ok(())
    }

    pub(crate) fn jmp_append(&mut self, l1: &mut u32, l2: u32) -> R<()> {
        if l2 == NO_JMP {
            return Ok(());
        }
        if *l1 == NO_JMP {
            *l1 = l2;
            return Ok(());
        }
        let mut list = *l1;
        loop {
            let next = self.jmp_next(list);
            if next == NO_JMP {
                break;
            }
            list = next;
        }
        self.jmp_patchins(list, l2)
    }

    /// Patches a jump list, keeping the values the tests produce
    /// (`jmp_patchval`).
    fn jmp_patchval(&mut self, mut list: u32, vtarget: u32, reg: u32, dtarget: u32) -> R<()> {
        while list != NO_JMP {
            let next = self.jmp_next(list);
            if self.jmp_patchtestreg(list, reg) {
                self.jmp_patchins(list, vtarget)?;
            } else {
                self.jmp_patchins(list, dtarget)?;
            }
            list = next;
        }
        Ok(())
    }

    /// Makes a jump list jump to the next instruction (`jmp_tohere`).
    pub(crate) fn jmp_tohere(&mut self, list: u32) -> R<()> {
        let pc = self.fs().pc();
        self.fs().lasttarget = pc;
        let mut jpc = self.fs().jpc;
        self.jmp_append(&mut jpc, list)?;
        self.fs().jpc = jpc;
        Ok(())
    }

    pub(crate) fn jmp_patch(&mut self, list: u32, target: u32) -> R<()> {
        if target == self.fs().pc() {
            self.jmp_tohere(list)
        } else {
            self.jmp_patchval(list, target, NO_REG, target)
        }
    }

    /// Emits an unconditional jump (`bcemit_jmp`), reusing a `UCLO` that
    /// precedes it when nothing jumps to it.
    pub(crate) fn emit_jmp(&mut self) -> R<u32> {
        let jpc = self.fs().jpc;
        let pc = self.fs().pc();
        self.fs().jpc = NO_JMP;
        let mut j = pc.wrapping_sub(1);
        let prev = if pc > 0 { Op::from_ins(self.ins_at(j)) } else { None };
        if pc > 0 && j >= self.fs().lasttarget && prev == Some(Op::UCLO) {
            self.patch_ins(j, |i| bc::set_j(i, NO_JMP));
            self.fs().lasttarget = j + 1;
        } else {
            let a = self.fs().freereg;
            j = self.emit_aj(Op::JMP, a, NO_JMP)?;
        }
        self.jmp_append(&mut j, jpc)?;
        Ok(j)
    }

    // ---- constants -------------------------------------------------------

    /// The index of a number constant (`const_num`). `-0` is stored as `0`,
    /// as LuaJIT's constant table does.
    pub(crate) fn const_num(&mut self, n: f64) -> R<u32> {
        let n = if n == 0.0 { 0.0 } else { n };
        let bits = n.to_bits();
        if let Some(&i) = self.fs().knum_map.get(&bits) {
            return Ok(i);
        }
        let i = u32::try_from(self.fs().knum.len()).unwrap_or(u32::MAX);
        if i > bc::BCMAX_D {
            return Err(self.err(format!("{} has more than {} number constants", self.fname(), bc::BCMAX_D + 1)));
        }
        self.fs().knum.push(n);
        self.fs().knum_map.insert(bits, i);
        Ok(i)
    }

    fn check_kgc(&mut self) -> R<u32> {
        let i = u32::try_from(self.fs().kgc.len()).unwrap_or(u32::MAX);
        if i > bc::BCMAX_D {
            return Err(self.err(format!("{} has more than {} constants", self.fname(), bc::BCMAX_D + 1)));
        }
        Ok(i)
    }

    /// The index of a string constant (`const_str`).
    pub(crate) fn const_str(&mut self, s: &[u8]) -> R<u32> {
        if let Some(&i) = self.fs().kstr_map.get(s) {
            return Ok(i);
        }
        let i = self.check_kgc()?;
        self.fs().kgc.push(KGc::Str(s.to_vec()));
        self.fs().kstr_map.insert(s.to_vec(), i);
        Ok(i)
    }

    /// Adds a child prototype as a GC constant.
    pub(crate) fn const_child(&mut self, p: Proto) -> R<u32> {
        let i = self.check_kgc()?;
        let c = self.fs().children.len();
        self.fs().children.push(p);
        self.fs().kgc.push(KGc::Child(c));
        Ok(i)
    }

    /// Adds a template table as a GC constant; it is filled in place
    /// through [`Gen::ktable_mut`].
    pub(crate) fn const_table(&mut self) -> R<u32> {
        let i = self.check_kgc()?;
        self.fs().kgc.push(KGc::Table(proto::KTable::default()));
        Ok(i)
    }

    pub(crate) fn ktable_mut(&mut self, idx: u32) -> Option<&mut proto::KTable> {
        match self.fs().kgc.get_mut(idx as usize) {
            Some(KGc::Table(t)) => Some(t),
            _ => None,
        }
    }

    // ---- variables -------------------------------------------------------

    /// The name of a variable for the debug information.
    pub(crate) fn var_name(&self, id: VarId) -> Vec<u8> {
        match id {
            VarId::Rt => b"rt".to_vec(),
            VarId::Local(l) => self.chunk.hint(l).as_bytes().to_vec(),
        }
    }

    /// Activates a local in the next slot (`var_new` plus `var_add`): the
    /// register must already be reserved. Variable ranges count positions
    /// as the VM does, with the function header at 0: a variable active
    /// from the next instruction on starts at `pc + 1`.
    pub(crate) fn var_add(&mut self, id: Option<VarId>, name: VarName) -> R<u32> {
        let slot = self.fs().nactvar;
        if slot + 1 > bc::MAX_LOCVAR {
            return Err(self.err(format!(
                "{} has more than {} local variables at once (LuaJIT's limit); split it into smaller functions",
                self.fname(),
                bc::MAX_LOCVAR
            )));
        }
        let pc = self.fs().pc() + 1;
        let fs = self.fs();
        let info = fs.vars.len();
        fs.vars.push(VarInfo { name, startpc: pc, endpc: pc });
        fs.actvars.push(ActVar { id, info });
        if let Some(id) = id {
            fs.var_slots.insert(id, slot);
        }
        fs.nactvar = slot + 1;
        Ok(slot)
    }

    pub(crate) fn var_add_id(&mut self, id: VarId) -> R<u32> {
        let name = VarName::Name(self.var_name(id));
        self.var_add(Some(id), name)
    }

    /// Deactivates the locals above `level` (`var_remove`).
    fn var_remove(&mut self, level: u32) {
        let pc = self.fs().pc() + 1;
        while self.fs().nactvar > level {
            let fs = self.fs();
            fs.nactvar -= 1;
            let Some(v) = fs.actvars.pop() else { break };
            if let Some(info) = fs.vars.get_mut(v.info) {
                info.endpc = pc;
            }
            if let Some(id) = v.id {
                fs.var_slots.remove(&id);
                // A shadowed declaration of the same local, if any, is
                // visible again.
                if let Some(slot) = fs.actvars.iter().rposition(|a| a.id == Some(id)) {
                    fs.var_slots.insert(id, slot as u32);
                }
            }
        }
    }

    /// Resolves a variable to a local, an upvalue or a global
    /// (`var_lookup`). A local of the IR that is not in scope is an
    /// inconsistency of the IR.
    pub(crate) fn var_lookup(&mut self, id: VarId) -> R<Exp> {
        if let Some(&slot) = self.fs().var_slots.get(&id) {
            return Ok(Exp::new(Kind::Local(slot)));
        }
        let level = self.funcs.len().saturating_sub(1);
        match self.upvalue(level, id)? {
            Some(idx) => Ok(Exp::new(Kind::Upval(idx))),
            None => Err(self.err(format!("internal error: local `{}` is used outside its scope", String::from_utf8_lossy(&self.var_name(id))))),
        }
    }

    /// Finds or creates the upvalue for `id` in function `level` of the
    /// stack, creating it in the enclosing functions as needed.
    fn upvalue(&mut self, level: usize, id: VarId) -> R<Option<u32>> {
        if let Some(&idx) = self.funcs.get(level).and_then(|f| f.uv_map.get(&id)) {
            return Ok(Some(idx));
        }
        let Some(parent) = level.checked_sub(1) else { return Ok(None) };
        let desc = match self.funcs.get(parent).and_then(|f| f.var_slots.get(&id).copied()) {
            Some(slot) => {
                let immutable = match id {
                    VarId::Rt => true,
                    VarId::Local(l) => !self.assigned.contains(&l),
                };
                self.scope_uvmark(parent, slot);
                proto::UV_LOCAL | (slot as u16) | if immutable { proto::UV_IMMUTABLE } else { 0 }
            }
            None => match self.upvalue(parent, id)? {
                Some(idx) => idx as u16,
                None => return Ok(None),
            },
        };
        let Some(fs) = self.funcs.get_mut(level) else { return Ok(None) };
        let idx = u32::try_from(fs.upvals.len()).unwrap_or(u32::MAX);
        if idx >= bc::MAX_UPVAL {
            let name = fs.name.clone();
            return Err(self.err(format!(
                "{name} uses more than {} variables of enclosing functions (LuaJIT's limit on upvalues); pass some of them as arguments or group them in a table",
                bc::MAX_UPVAL
            )));
        }
        fs.upvals.push(Upval { id, desc });
        fs.uv_map.insert(id, idx);
        Ok(Some(idx))
    }

    /// Marks the scope of function `level` that declares slot `slot` as
    /// having a captured local (`fscope_uvmark`).
    fn scope_uvmark(&mut self, level: usize, slot: u32) {
        let Some(fs) = self.funcs.get_mut(level) else { return };
        if let Some(bl) = fs.scopes.iter_mut().rev().find(|bl| bl.nactvar <= slot) {
            bl.flags |= SCOPE_UPVAL;
        }
    }

    // ---- scopes ----------------------------------------------------------

    pub(crate) fn scope_begin(&mut self, is_loop: bool) {
        let fs = self.fs();
        debug_assert_eq!(fs.freereg, fs.nactvar, "registers left over at scope start");
        let nactvar = fs.nactvar;
        fs.scopes.push(Scope { nactvar, flags: if is_loop { SCOPE_LOOP } else { 0 }, gotos: Vec::new() });
    }

    /// Ends the innermost scope (`fscope_end`): closes its captured locals,
    /// resolves its breaks when it is a loop, and hands its other pending
    /// gotos to the enclosing scope.
    pub(crate) fn scope_end(&mut self) -> R<()> {
        let Some(mut bl) = self.fs().scopes.pop() else { return Ok(()) };
        self.var_remove(bl.nactvar);
        let fs = self.fs();
        fs.freereg = fs.nactvar;
        if bl.flags & (SCOPE_UPVAL | SCOPE_NOCLOSE) == SCOPE_UPVAL {
            self.emit_aj(Op::UCLO, bl.nactvar, 0)?;
        }
        let gotos = std::mem::take(&mut bl.gotos);
        let mut remaining = Vec::new();
        for g in gotos {
            if bl.flags & SCOPE_LOOP != 0 && g.target == Target::Break {
                let pc = self.fs().pc();
                self.gola_patch(g, pc, bl.nactvar)?;
            } else {
                remaining.push(g);
            }
        }
        for mut g in remaining {
            g.slot = bl.nactvar;
            if bl.flags & SCOPE_UPVAL != 0 {
                self.gola_close(&g)?;
            }
            match self.fs().scopes.last_mut() {
                Some(outer) => {
                    outer.flags |= if g.target == Target::Break { SCOPE_BREAK } else { SCOPE_GOLA };
                    outer.gotos.push(g);
                }
                None => {
                    return Err(Error {
                        line: g.line,
                        message: match g.target {
                            Target::Break => "internal error: `break` outside a loop".to_owned(),
                            Target::Label(_) => "internal error: `continue` without its loop label".to_owned(),
                        },
                    });
                }
            }
        }
        Ok(())
    }

    /// Records a pending `break` or `goto` (`gola_new`) after its jump.
    pub(crate) fn goto_new(&mut self, target: Target, pc: u32) {
        let line = self.line;
        let slot = self.fs().nactvar;
        if let Some(bl) = self.fs().scopes.last_mut() {
            bl.flags |= if target == Target::Break { SCOPE_BREAK } else { SCOPE_GOLA };
            bl.gotos.push(Goto { target, pc, slot, line });
        }
    }

    /// Resolves the pending gotos of the current scope to a label here.
    pub(crate) fn label_here(&mut self, label: LabelId) -> R<()> {
        let pc = self.fs().pc();
        let slot = self.fs().nactvar;
        self.fs().lasttarget = pc;
        let Some(bl) = self.fs().scopes.last_mut() else { return Ok(()) };
        bl.flags |= SCOPE_GOLA;
        let (mine, rest): (Vec<Goto>, Vec<Goto>) = std::mem::take(&mut bl.gotos).into_iter().partition(|g| g.target == Target::Label(label));
        bl.gotos = rest;
        for g in mine {
            self.gola_patch(g, pc, slot)?;
        }
        Ok(())
    }

    /// Points a goto at its label (`gola_patch`).
    fn gola_patch(&mut self, g: Goto, label_pc: u32, label_slot: u32) -> R<()> {
        self.patch_ins(g.pc, |i| bc::set_a(i, label_slot));
        self.jmp_patch(g.pc, label_pc)
    }

    /// Makes a goto close the upvalues of the scope it leaves
    /// (`gola_close`).
    fn gola_close(&mut self, g: &Goto) -> R<()> {
        let mut ins = self.ins_at(g.pc);
        bc::set_a(&mut ins, g.slot);
        if Op::from_ins(ins) == Some(Op::JMP) {
            let next = self.jmp_next(g.pc);
            if next != NO_JMP {
                self.jmp_patch(next, g.pc)?;
            }
            bc::set_op(&mut ins, Op::UCLO);
            bc::set_j(&mut ins, NO_JMP);
        }
        self.set_ins(g.pc, ins);
        Ok(())
    }

    // ---- functions -------------------------------------------------------

    /// Starts compiling a function: its state, its outermost scope and its
    /// parameters.
    pub(crate) fn func_begin(&mut self, name: String, line: u32, vararg: bool, method: bool, params: &[VarId]) -> R<()> {
        self.funcs.push(FuncState::new(name, line, vararg, method));
        self.scope_begin(false);
        for p in params {
            self.var_add_id(*p)?;
        }
        let n = u32::try_from(params.len()).unwrap_or(u32::MAX);
        self.fs().numparams = n;
        self.fs().freereg = 0;
        self.reserve(n)
    }

    /// Finishes the function being compiled and returns its prototype
    /// (`fs_fixup_ret`, `fs_finish`).
    pub(crate) fn func_end(&mut self) -> R<Proto> {
        let lastpc = self.fs().pc();
        let last_is_ret = lastpc > 0 && Op::from_ins(self.ins_at(lastpc - 1)).is_some_and(Op::is_ret_or_tail);
        if lastpc <= self.fs().lasttarget || !last_is_ret {
            if self.fs().scopes.first().is_some_and(|bl| bl.flags & SCOPE_UPVAL != 0) {
                self.emit_aj(Op::UCLO, 0, 0)?;
            }
            self.emit_ad(Op::RET0, 0, 1)?;
        }
        if let Some(bl) = self.fs().scopes.first_mut() {
            bl.flags |= SCOPE_NOCLOSE;
        }
        self.scope_end()?;
        let Some(fs) = self.funcs.pop() else { return Err(self.err("internal error: no function to finish")) };
        if let Some(g) = fs.scopes.iter().flat_map(|s| s.gotos.iter()).next() {
            return Err(Error { line: g.line, message: "internal error: a `break` or `continue` has no target".to_owned() });
        }
        let maxline = fs.lines.iter().copied().max().unwrap_or(fs.linedefined).max(fs.linedefined);
        let mut flags = 0;
        if fs.vararg {
            flags |= proto::FLAG_VARARG;
        }
        if !fs.children.is_empty() {
            flags |= proto::FLAG_CHILD;
        }
        if self.level >= 1 {
            // Every prototype of an NL chunk, the loader
            // included, is NL code; a method's closures carry the mark.
            flags |= proto::FLAG_NL;
            if fs.method {
                flags |= proto::FLAG_METHOD;
            }
        }
        let uvnames = fs.upvals.iter().map(|u| self.var_name(u.id)).collect();
        Ok(Proto {
            flags,
            numparams: u8::try_from(fs.numparams).unwrap_or(u8::MAX),
            framesize: u8::try_from(fs.framesize).unwrap_or(u8::MAX),
            bc: fs.bc,
            lines: fs.lines,
            uv: fs.upvals.iter().map(|u| u.desc).collect(),
            uvnames,
            kgc: fs.kgc,
            knum: fs.knum,
            vars: fs.vars,
            firstline: fs.linedefined,
            numline: maxline - fs.linedefined,
            children: fs.children,
            name: fs.name,
        })
    }
}

/// The locals assigned after their declaration anywhere in the chunk.
pub(crate) fn assigned_locals(chunk: &Chunk) -> HashSet<LocalId> {
    fn block(b: &Block, out: &mut HashSet<LocalId>) {
        for s in &b.stmts {
            match &s.kind {
                StmtKind::Local { values, .. } => values.iter().for_each(|v| expr(v, out)),
                StmtKind::Assign { targets, values } => {
                    for t in targets {
                        match t {
                            Place::Local(l) => {
                                out.insert(*l);
                            }
                            Place::Index(o, k) => {
                                expr(o, out);
                                expr(k, out);
                            }
                            Place::Global(_) => {}
                        }
                    }
                    values.iter().for_each(|v| expr(v, out));
                }
                StmtKind::Expr(e) => expr(e, out),
                StmtKind::Do(b) => block(b, out),
                StmtKind::If { cond, then, otherwise } => {
                    expr(cond, out);
                    block(then, out);
                    if let Some(o) = otherwise {
                        block(o, out);
                    }
                }
                StmtKind::While { cond, body } => {
                    expr(cond, out);
                    block(body, out);
                }
                StmtKind::NumericFor { start, limit, step, body, .. } => {
                    expr(start, out);
                    expr(limit, out);
                    if let Some(s) = step {
                        expr(s, out);
                    }
                    block(body, out);
                }
                StmtKind::GenericFor { iter, body, .. } => {
                    iter.iter().for_each(|e| expr(e, out));
                    block(body, out);
                }
                StmtKind::Return(vals) => vals.iter().for_each(|v| expr(v, out)),
                StmtKind::Hoist { defs, rest } => {
                    block(defs, out);
                    block(rest, out);
                }
                StmtKind::Break | StmtKind::Goto(_) | StmtKind::Label(_) => {}
            }
        }
    }
    fn expr(e: &Expr, out: &mut HashSet<LocalId>) {
        match e {
            Expr::Function(f) => block(&f.body, out),
            Expr::Index(a, b) | Expr::Binary(_, a, b) => {
                expr(a, out);
                expr(b, out);
            }
            Expr::Call(f, args) => {
                expr(f, out);
                args.iter().for_each(|a| expr(a, out));
            }
            Expr::MethodCall(o, _, args) => {
                expr(o, out);
                args.iter().for_each(|a| expr(a, out));
            }
            Expr::Unary(_, a) | Expr::Paren(a) => expr(a, out),
            Expr::Table(entries) => {
                for en in entries {
                    match en {
                        TableEntry::Item(v) => expr(v, out),
                        TableEntry::Pair(k, v) => {
                            expr(k, out);
                            expr(v, out);
                        }
                    }
                }
            }
            Expr::MemberCall { receiver, key, args } => {
                expr(receiver, out);
                expr(key, out);
                args.iter().for_each(|a| expr(a, out));
            }
            Expr::MemberApply { func, receiver, args } => {
                expr(func, out);
                expr(receiver, out);
                args.iter().for_each(|a| expr(a, out));
            }
            Expr::AutoGet { receiver, key } => {
                expr(receiver, out);
                expr(key, out);
            }
            Expr::Nil
            | Expr::Bool(_)
            | Expr::Number(_)
            | Expr::Str(_)
            | Expr::Local(_)
            | Expr::Global(_)
            | Expr::Vararg
            | Expr::Intrinsic(_) => {}
        }
    }
    let mut out = HashSet::new();
    block(&chunk.main.body, &mut out);
    out
}

/// True when the function creates closures in its own body (not counting
/// the bodies of those closures).
pub(crate) fn has_closures(f: &Function) -> bool {
    fn block(b: &Block) -> bool {
        b.stmts.iter().any(|s| match &s.kind {
            StmtKind::Local { values, .. } | StmtKind::Return(values) => values.iter().any(expr),
            StmtKind::Assign { targets, values } => {
                targets.iter().any(|t| match t {
                    Place::Index(o, k) => expr(o) || expr(k),
                    Place::Local(_) | Place::Global(_) => false,
                }) || values.iter().any(expr)
            }
            StmtKind::Expr(e) => expr(e),
            StmtKind::Do(b) => block(b),
            StmtKind::If { cond, then, otherwise } => expr(cond) || block(then) || otherwise.as_ref().is_some_and(block),
            StmtKind::While { cond, body } => expr(cond) || block(body),
            StmtKind::NumericFor { start, limit, step, body, .. } => {
                expr(start) || expr(limit) || step.as_ref().is_some_and(expr) || block(body)
            }
            StmtKind::GenericFor { iter, body, .. } => iter.iter().any(expr) || block(body),
            StmtKind::Hoist { defs, rest } => block(defs) || block(rest),
            StmtKind::Break | StmtKind::Goto(_) | StmtKind::Label(_) => false,
        })
    }
    fn expr(e: &Expr) -> bool {
        match e {
            Expr::Function(_) => true,
            Expr::Index(a, b) | Expr::Binary(_, a, b) => expr(a) || expr(b),
            Expr::Call(f, args) => expr(f) || args.iter().any(expr),
            Expr::MethodCall(o, _, args) => expr(o) || args.iter().any(expr),
            Expr::Unary(_, a) | Expr::Paren(a) => expr(a),
            Expr::Table(entries) => entries.iter().any(|en| match en {
                TableEntry::Item(v) => expr(v),
                TableEntry::Pair(k, v) => expr(k) || expr(v),
            }),
            Expr::MemberCall { receiver, key, args } => expr(receiver) || expr(key) || args.iter().any(expr),
            Expr::MemberApply { func, receiver, args } => expr(func) || expr(receiver) || args.iter().any(expr),
            Expr::AutoGet { receiver, key } => expr(receiver) || expr(key),
            Expr::Nil
            | Expr::Bool(_)
            | Expr::Number(_)
            | Expr::Str(_)
            | Expr::Local(_)
            | Expr::Global(_)
            | Expr::Vararg
            | Expr::Intrinsic(_) => false,
        }
    }
    block(&f.body)
}
