//! A function prototype as the generator builds it, the dump writer
//! serializes it ([`crate::dump`]) and the verifier checks it
//! ([`crate::verify`]): the `GCproto` of `src/lj_obj.h`, without the VM's
//! memory layout.
//!
//! Constants are numbered as the instructions refer to them: GC constant
//! `i` (`FNEW`, `KSTR`, `TDUP`, `TGETS`, `GGET`…) is `kgc[i]`, number
//! constant `i` (`KNUM`, `ADDVN`…) is `knum[i]`. The dump writes `kgc` in
//! reverse order, as LuaJIT stores it below the number constants.

use crate::bc::Ins;

/// `PROTO_CHILD`: the function has child prototypes.
pub const FLAG_CHILD: u8 = 0x01;
/// `PROTO_VARARG`: the function takes `...`.
pub const FLAG_VARARG: u8 = 0x02;
/// `BCDUMP_PF_METHOD`: the function is a method; its closures
/// carry the mark the fork tests with `ISMETH`/`MSELF`. Level 1 only.
pub const FLAG_METHOD: u8 = 0x08;
/// `BCDUMP_PF_NL`: compiled from NL: a string indexed by
/// this code answers with NL's library, and the operators follow NL's
/// rules. Level 1 only.
pub const FLAG_NL: u8 = 0x10;
// `PROTO_BITOP` (0x80), which LuaJIT's parser sets on a function that uses
// the bit operators, is never set: the loader keeps it in the prototype's
// flags, where it is also the top bit of the saturating closure counter
// (`PROTO_CLCOUNT`, lj_obj.h), so the trace recorder would take every such
// function for one whose closures are polymorphic (`PROTO_CLC_POLY`) and
// specialize calls to its prototype, never constifying its upvalues. The
// VM does not need the flag to run the opcodes (crate::bc).

/// `PROTO_UV_LOCAL`: the upvalue refers to a slot of the enclosing function.
pub const UV_LOCAL: u16 = 0x8000;
/// `PROTO_UV_IMMUTABLE`: the variable is never assigned after its
/// declaration.
pub const UV_IMMUTABLE: u16 = 0x4000;

/// A value of a template table (`BCDUMP_KTAB_*`).
#[derive(Clone, Debug, PartialEq)]
pub enum KTabValue {
    Nil,
    False,
    True,
    /// A number; integral values in `i32` range are written as integers.
    Num(f64),
    Str(Vec<u8>),
}

/// A template table (`TDUP`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KTable {
    /// The array part, index 0 first (index 0 is a real slot in LuaJIT's
    /// array part). Trailing nils are dropped by the writer.
    pub array: Vec<KTabValue>,
    /// The hash part. A `Nil` value keeps its key in the table: the
    /// constructor stores the real value with a later instruction.
    pub hash: Vec<(KTabValue, KTabValue)>,
}

/// A GC constant (`BCDUMP_KGC_*`).
#[derive(Clone, Debug, PartialEq)]
pub enum KGc {
    /// A child prototype, by index into [`Proto::children`].
    Child(usize),
    Str(Vec<u8>),
    Table(KTable),
}

/// The name of a local variable for the debug information.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VarName {
    Name(Vec<u8>),
    /// A hidden loop variable (`VARNAME_FOR_*`, `src/lj_debug.h`): 1 to 6.
    Internal(u8),
}

/// `VARNAME_FOR_IDX`.
pub const VARNAME_FOR_IDX: u8 = 1;
/// `VARNAME_FOR_STOP`.
pub const VARNAME_FOR_STOP: u8 = 2;
/// `VARNAME_FOR_STEP`.
pub const VARNAME_FOR_STEP: u8 = 3;
/// `VARNAME_FOR_GEN`.
pub const VARNAME_FOR_GEN: u8 = 4;
/// `VARNAME_FOR_STATE`.
pub const VARNAME_FOR_STATE: u8 = 5;
/// `VARNAME_FOR_CTL`.
pub const VARNAME_FOR_CTL: u8 = 6;

/// The debug information of one local variable: live from `startpc`
/// (inclusive) to `endpc` (exclusive). Variables are listed in the order
/// they become live, which is their slot order among the variables live
/// at any one instruction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VarInfo {
    pub name: VarName,
    pub startpc: u32,
    pub endpc: u32,
}

/// A function prototype.
#[derive(Clone, Debug, Default)]
pub struct Proto {
    /// `PROTO_CHILD | PROTO_VARARG`, plus `FLAG_METHOD` and `FLAG_NL` at
    /// level 1 (the dump's bits for them).
    pub flags: u8,
    pub numparams: u8,
    pub framesize: u8,
    /// The instructions, without the `FUNCF`/`FUNCV` header the loader
    /// adds. Positions in jumps count from this vector's start.
    pub bc: Vec<Ins>,
    /// The NL line of each instruction.
    pub lines: Vec<u32>,
    /// Upvalue descriptors: `UV_LOCAL | slot` for a slot of the enclosing
    /// function, else that function's upvalue index.
    pub uv: Vec<u16>,
    pub uvnames: Vec<Vec<u8>>,
    pub kgc: Vec<KGc>,
    pub knum: Vec<f64>,
    pub vars: Vec<VarInfo>,
    pub firstline: u32,
    /// The last line minus the first.
    pub numline: u32,
    pub children: Vec<Proto>,
    /// A descriptive name for messages.
    pub name: String,
}

impl Proto {
    /// The number of instructions.
    pub fn len(&self) -> u32 {
        u32::try_from(self.bc.len()).unwrap_or(u32::MAX)
    }

    /// True when there are no instructions.
    pub fn is_empty(&self) -> bool {
        self.bc.is_empty()
    }
}
