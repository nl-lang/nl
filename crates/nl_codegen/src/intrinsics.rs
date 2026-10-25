//! The intrinsic table of `KINTR`:
//! the runtime functions level-1 code loads by index instead of through
//! `rt.name`. The list is the VM's (`NLC_INTRINSICS` in `src/nlc.h` of
//! the fork): entry `i`, counting from 1, is `rt[NAMES[i - 1]]`, read once
//! when the `nl` library opens. The two lists must agree; the test below
//! compares them when the fork is checked out beside the crates.

/// The names in the VM's order.
pub const NAMES: &[&str] = &[
    "String", "strindex", "opgeneric", "as", "async", "band", "bnot", "bor", "bxor", "check", "enum", "eq", "ge",
    "gettype", "gt", "idiv", "implement", "import_from", "index", "inrange", "isa", "iskw", "istable", "iter",
    "keyed", "kwlast", "kwpop", "kwtake", "lazy", "le", "len", "listcat", "lmethod", "lt", "matcherror", "mcall",
    "member", "method", "mget", "module", "negate", "new", "nokw", "nokwrest", "on", "opdef", "optional", "owner",
    "primmethod", "primop", "range", "record", "register", "rest", "ret", "retn", "shl", "shr", "spawn", "step",
    "submodule", "tostring", "try", "union", "unpack", "unpackkw", "ushr", "when",
    // The names after the sorted block stand in the order the VM's list
    // gained them (nlc.h: "New names go at the end"), so that the indices
    // of the others stay put.
    "nfloor", "nband", "nbor", "nbxor", "nlshift", "narshift", "nrshift",
];

/// The `KINTR` operand of an intrinsic, or `None` for a name the table
/// does not hold (the emitter then reads `rt.name`).
pub fn index(name: &str) -> Option<u32> {
    NAMES.iter().position(|n| *n == name).and_then(|i| u32::try_from(i + 1).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// How many names the table had while it was kept sorted; later names
    /// are appended in the order they were added.
    const SORTED_LEN: usize = 68;

    #[test]
    fn indices_count_from_one() {
        assert_eq!(index("String"), Some(1));
        assert_eq!(index("strindex"), Some(2));
        assert_eq!(index("opgeneric"), Some(3));
        assert_eq!(index("no such intrinsic"), None);
        let first = NAMES.get(3..SORTED_LEN).unwrap_or_default();
        let mut sorted = first.to_vec();
        sorted.sort_unstable();
        assert_eq!(sorted, first, "the compiler's first intrinsics are sorted after the VM's three");
        let mut unique = NAMES.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), NAMES.len(), "an intrinsic is listed twice");
    }

    /// The list matches `NLC_INTRINSICS` in the fork's `nlc.h`.
    #[test]
    fn matches_the_vm() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../LuaJIT/src/nlc.h");
        let Ok(text) = std::fs::read_to_string(path) else { return }; // No submodule checkout.
        let start = text.find("#define NLC_INTRINSICS(_)").expect("NLC_INTRINSICS in nlc.h");
        let rest = &text[start..];
        let end = rest.find("\n\n").unwrap_or(rest.len());
        let names: Vec<&str> = rest[..end]
            .split("_(")
            .skip(1)
            .map(|piece| piece.split(')').next().unwrap_or(""))
            .filter(|n| !n.is_empty())
            .collect();
        assert_eq!(names, NAMES, "crates/nl_codegen/src/intrinsics.rs and LuaJIT/src/nlc.h differ");
    }
}
