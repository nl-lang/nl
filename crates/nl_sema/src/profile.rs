//! Profiles: the subsets of the language a target supports.
//!
//! The full language needs the VM fork. The portable profile is what runs
//! on an unmodified stock LuaJIT, 2.0 or 2.1, through the Lua backend and
//! the pure-Lua runtime, as Garry's Mod clients do: no regex engine (a
//! Rust library), no dedicated workers (a worker needs a state of its own),
//! and no compiler at run time.
//!
//! [`check`] runs on a resolved file and reports every use of a prelude
//! name the profile leaves out, at the use. It looks at what the resolver
//! decided a name refers to, so a local that shadows a prelude name is not
//! reported.

use nl_syntax::{SyntaxKind, SyntaxNode};

use crate::binding::{Ref, Resolution, SemaDiagnostic};
use crate::util::field_name;

/// A subset of the language that a target supports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Profile {
    /// Everything: the VM fork.
    #[default]
    Full,
    /// What runs on stock LuaJIT 2.0 and 2.1 with the pure-Lua runtime.
    Portable,
}

impl Profile {
    /// The profile of a command-line name: `full` or `portable`.
    pub fn from_name(name: &str) -> Option<Profile> {
        match name {
            "full" => Some(Profile::Full),
            "portable" => Some(Profile::Portable),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Profile::Full => "full",
            Profile::Portable => "portable",
        }
    }
}

/// The code of every diagnostic of [`check`].
pub const CODE: &str = "not-in-profile";

/// Why the portable profile leaves out a prelude value, by name; `None`
/// when it keeps it.
fn excluded(name: &str) -> Option<&'static str> {
    Some(match name {
        "Regex" => "the regex engine is a Rust library that stock LuaJIT cannot load",
        "load_nl" => "there is no NL compiler at run time; compile the source ahead of time",
        "add_path" => "imports are resolved when the program is built, so a search path set at run time changes nothing; give the directory to the build",
        _ => return None,
    })
}

/// The diagnostics of the uses that `profile` leaves out, in source order.
pub fn check(root: &SyntaxNode, res: &Resolution, profile: Profile) -> Vec<SemaDiagnostic> {
    let mut out = Vec::new();
    if profile == Profile::Full {
        return out;
    }
    for node in root.descendants().filter(|n| n.kind() == SyntaxKind::NAME_REF) {
        let Some(Ref::Prelude(p)) = res.reference(node.text_range()) else { continue };
        if let Some(why) = excluded(p.name) {
            out.push(error(&node, format!("`{}` is not available in the portable profile: {why}", p.name)));
            continue;
        }
        // `Worker.new`: a dedicated worker needs a Lua state of its own.
        // `spawn`, `Worker.run` and `Worker.map` stay: their jobs run in
        // the same state on copies of their arguments.
        if p.name == "Worker" {
            let Some(parent) = node.parent().filter(|n| n.kind() == SyntaxKind::FIELD_EXPR) else { continue };
            if field_name(&parent).is_some_and(|t| t.text() == "new") {
                out.push(error(
                    &parent,
                    "`Worker.new` is not available in the portable profile: a dedicated worker needs a Lua state of its own; `spawn`, `Worker.run` and `Worker.map` run their jobs in this state".to_owned(),
                ));
            }
        }
    }
    out
}

fn error(node: &SyntaxNode, message: String) -> SemaDiagnostic {
    SemaDiagnostic { range: node.text_range(), code: CODE, message, error: true, lint: false }
}
