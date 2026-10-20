//! What the analyzer tells the lowering: runtime checks it has proven, and places where
//! a declared type lets a runtime test be replaced by direct code. Keyed
//! by `TextRange`, like the [`crate::Resolution`] tables, so the lowering
//! finds them while walking the same tree.
//!
//! Every hint must give the result the runtime path would have given.
//! The analyzer records one only from the runtime-checked part of a
//! declared type (from `[Number]` it takes "a table"), from a
//! literal, or from a narrowing test on a local that no other function
//! assigns. Without the analyzer, or with static resolution off, the
//! tables are empty and the lowering tests everything at runtime.
//!
//! The member hints also carry the Lua key of a member declared in a
//! declaration file (`as 'luaName'`), which the lowering needs
//! whether or not static resolution is on, and the calls of `Table`
//! functions as methods of tables (`xs.map(f)`), which have no runtime
//! path to fall back on: a declared type decides what such a line does,
//! in every mode.

use std::collections::{HashMap, HashSet};

use nl_syntax::TextRange;

/// How `x.member` is resolved when the receiver's type is known.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MemberHint {
    /// A field: a plain read or write of `key`.
    Field { key: String },
    /// A method: `x:key(args)`, also when read bare (auto-call).
    Method { key: String },
    /// A function held as data (a library function, a `declare fn` member
    /// without `self`): called as written, read as a value.
    Function { key: String },
    /// A function of NL's `Table` library on a receiver typed as a table:
    /// `Table.key(x, args)`, also when read bare (auto-call); `x.key!` is
    /// the function. Recorded in every mode: at runtime a plain table has
    /// no such member.
    TableFn { key: String },
}

impl MemberHint {
    /// The Lua key of the member.
    pub fn key(&self) -> &str {
        match self {
            MemberHint::Field { key } | MemberHint::Method { key } | MemberHint::Function { key } | MemberHint::TableFn { key } => key,
        }
    }
}

/// The loop form of `for … in x` fixed from the declared type of `x`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoopHint {
    /// A list: the array values in order, stopping at the first `nil`.
    Array,
    /// A map: every key/value pair (`next`).
    Map,
    /// A function: Lua's iterator protocol.
    Function,
}

/// `x as T` resolved at compile time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AsHint {
    /// `x` is already a `T`: the value itself.
    Identity,
    /// A primitive to `String`: `tostring(x)`.
    ToString,
}

/// The tables (see the module documentation).
#[derive(Clone, Debug, Default)]
pub struct Hints {
    /// Check sites whose runtime check is proven: the range of the node
    /// the lowering passes as the site (the pattern of a `let`, the
    /// target of an assignment, the `return` statement or tail
    /// expression, a typed pattern, the `FIELD_INIT` of a struct
    /// literal).
    pub proven: HashSet<TextRange>,
    /// `FIELD_EXPR` nodes, by range.
    pub members: HashMap<TextRange, MemberHint>,
    /// `FOR_STMT` nodes, by range.
    pub loops: HashMap<TextRange, LoopHint>,
    /// `BIN_EXPR` and `COMPOUND_ASSIGN_STMT` nodes whose operands are
    /// primitives of the kinds direct code handles: two numbers, or two
    /// strings for comparisons and concatenation, and the `PREFIX_EXPR`
    /// nodes of a `~` whose operand is a number. Lua's operator, or for
    /// `//` and the bitwise operators a call of the `math.floor` or `bit.*`
    /// function that the runtime holds (`rt.nfloor`, `rt.nband`, ...),
    /// which the bytecode backend emits as the VM's bit opcodes at level 1.
    pub direct_ops: HashSet<TextRange>,
    /// `FIELD_EXPR` nodes `x.len` whose receiver is a string.
    pub len_string: HashSet<TextRange>,
    /// Expressions inside an interpolated string whose value is a string
    /// already: no `tostring`.
    pub interp_string: HashSet<TextRange>,
    /// `AS_EXPR` nodes, by range.
    pub conversions: HashMap<TextRange, AsHint>,
    /// `FIELD_EXPR` nodes of members of declaration files,
    /// with their Lua keys, and of `Table` functions called as methods
    /// ([`MemberHint::TableFn`]); needed whatever the mode.
    pub members_declared: HashMap<TextRange, MemberHint>,
}

impl Hints {
    /// The hints that hold without static resolution: only the Lua keys of
    /// declared members, which generated code needs in every mode.
    pub fn without_static_resolution(&self) -> Hints {
        Hints { members_declared: self.members_declared.clone(), ..Hints::default() }
    }

    pub fn is_proven(&self, site: TextRange) -> bool {
        self.proven.contains(&site)
    }

    pub fn member(&self, field_expr: TextRange) -> Option<&MemberHint> {
        self.members_declared.get(&field_expr).or_else(|| self.members.get(&field_expr))
    }

    pub fn loop_form(&self, for_stmt: TextRange) -> Option<LoopHint> {
        self.loops.get(&for_stmt).copied()
    }

    pub fn is_direct_op(&self, node: TextRange) -> bool {
        self.direct_ops.contains(&node)
    }

    pub fn is_string_len(&self, field_expr: TextRange) -> bool {
        self.len_string.contains(&field_expr)
    }

    pub fn is_string_interp(&self, expr: TextRange) -> bool {
        self.interp_string.contains(&expr)
    }

    pub fn conversion(&self, as_expr: TextRange) -> Option<AsHint> {
        self.conversions.get(&as_expr).copied()
    }
}
