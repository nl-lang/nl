//! Localized library paths: the
//! `local format = string.format` idiom of Lua, done by the compiler.
//!
//! A Lua path is read where it stands: `:string.format(…)` reads the
//! global `string` and its field `format` every time it runs (`GGET` and
//! `TGETS`). Under the option ([`Localize`], `nlc --localize`,
//! `NL_OPT_LOCALIZE`), the paths of a file that qualify are read once,
//! when the file loads, into hidden locals of its main function, and every
//! use, at any function depth, reads that local (an immutable upvalue
//! inside functions, which a trace treats as a constant):
//!
//! ```lua
//! local string_format, math_floor = string.format, math.floor
//! …
//! print(string_format("%d", math_floor(x)))
//! ```
//!
//! What qualifies is decided here, on the syntax tree, before the file is
//! lowered ([`plan`]):
//!
//! - `std` (the default): a path whose root is a Lua global (no NL name is
//!   in scope, the resolver's `Ref::LuaGlobal`) named `string`, `math`,
//!   `table`, `bit`, `coroutine`, `os` or `io`, and whose first step is
//!   `.name`: the root and that step are localized, the later steps apply
//!   to the local (`:table.sort(t)` is `table_sort(t)`);
//! - `all`: the same for every Lua global root, and a root whose path is
//!   not localized that way (a root alone, `:print(x)`, or followed by an
//!   index, a call or a method call) is localized alone (`print`). A path
//!   rooted at `_G` localizes `_G` only: its field is a global variable;
//! - in both, nothing is localized that the file assigns: a path `root.name`
//!   that is an assignment target anywhere in the file (`:math.pi = 3`),
//!   and every path of a root the file assigns (`:_G.string = …`, or a
//!   `global` declaration of that name). Under `all`, the `global` names of
//!   the files the file imports ([`super::LowerOptions::imported_globals`])
//!   are excluded too: those are variables by design.
//!
//! The locals are made on first use ([`Lowerer::localized`]) and declared
//! by one statement after the file's imports ([`Lowerer::prologue_at`]), so
//! that what an imported file did to a library when it ran (a function it
//! added to `string`) is seen. A root other than the five libraries every
//! NL state has (`string`, `math`, `table`, `bit`, `coroutine`) may be nil
//! when the file loads (`io` and `os` in the sandbox; a global
//! that no file has set yet), so its field is read as `io and io.write`,
//! which leaves the error to the use, where it was.
//!
//! The locals are a speed-up only. limits.rs drops one, and reads the path
//! where it stands again, when it would push a function over LuaJIT's
//! upvalue budget or the main function over its budget of locals.

use std::collections::HashSet;

use nl_sema::util::*;
use nl_sema::{Ref, Resolution};
use nl_syntax::{SyntaxKind, SyntaxNode};

use crate::ir::*;

use SyntaxKind::*;

/// Which Lua paths a file reads once, into hidden locals
/// (`NL_OPT_LOCALIZE`, `nlc --localize`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Localize {
    /// None: every path is read where it stands (0).
    None,
    /// The fields of the standard libraries: `:string.format`,
    /// `:math.floor`, … (1, the default).
    #[default]
    Std,
    /// Every Lua global root and its first field (2).
    All,
}

impl Localize {
    /// The setting of `NL_OPT_LOCALIZE`: 0, 1 or 2.
    pub fn from_level(level: i64) -> Option<Localize> {
        match level {
            0 => Some(Localize::None),
            1 => Some(Localize::Std),
            2 => Some(Localize::All),
            _ => None,
        }
    }

    /// The `NL_OPT_LOCALIZE` value of this setting.
    pub fn level(self) -> i64 {
        match self {
            Localize::None => 0,
            Localize::Std => 1,
            Localize::All => 2,
        }
    }

    /// The setting named as `nlc --localize=` names it.
    pub fn from_name(name: &str) -> Option<Localize> {
        match name {
            "none" => Some(Localize::None),
            "std" => Some(Localize::Std),
            "all" => Some(Localize::All),
            _ => None,
        }
    }
}

/// The libraries whose fields `std` localizes.
pub const STD_LIBRARIES: &[&str] = &["string", "math", "table", "bit", "coroutine", "os", "io"];

/// The libraries every state that runs NL has (`luaopen_nl` needs them,
/// and the sandbox has copies): their fields are read without
/// a guard.
const ALWAYS_PRESENT: &[&str] = &["string", "math", "table", "bit", "coroutine"];

/// What one file localizes ([`plan`]).
#[derive(Debug, Default)]
pub(crate) struct Plan {
    /// Paths `root.name` read through a local.
    paths: HashSet<(String, String)>,
    /// Roots read through a local of their own (`all`).
    roots: HashSet<String>,
}

impl Plan {
    pub(crate) fn is_empty(&self) -> bool {
        self.paths.is_empty() && self.roots.is_empty()
    }

    /// Whether `root.name` is localized.
    pub(crate) fn has_path(&self, root: &str, name: &str) -> bool {
        // Avoids building a pair for the lookup in the common case.
        !self.paths.is_empty() && self.paths.contains(&(root.to_string(), name.to_string()))
    }

    /// Whether the root `root` is localized on its own.
    pub(crate) fn has_root(&self, root: &str) -> bool {
        self.roots.contains(root)
    }
}

/// The names of the `global` declarations of a file, in order.
/// The driver gives the importing file those of the files it imports
/// ([`super::LowerOptions::imported_globals`]).
pub fn declared_globals(root: &SyntaxNode) -> Vec<String> {
    let mut out = Vec::new();
    for g in root.descendants().filter(|n| n.kind() == GLOBAL_DECL) {
        if let Some(name) = child(&g, NAME).and_then(|n| name_text(&n)) {
            if !out.contains(&name) {
                out.push(name);
            }
        }
    }
    out
}

/// The steps of a `LUA_PATH`.
pub(crate) fn path_steps(node: &SyntaxNode) -> Vec<SyntaxNode> {
    node.children().filter(|c| matches!(c.kind(), LUA_FIELD | LUA_INDEX | LUA_CALL | LUA_METHOD)).collect()
}

/// The name of a `LUA_FIELD` step.
pub(crate) fn field_step_name(step: &SyntaxNode) -> Option<String> {
    if step.kind() != LUA_FIELD {
        return None;
    }
    name_token(step).map(|t| t.text().to_string())
}

/// True when a `LUA_PATH` is the target of an assignment (`=`, a compound
/// assignment, or one target of a multiple assignment).
fn is_target(node: &SyntaxNode) -> bool {
    let Some(parent) = node.parent() else { return false };
    match parent.kind() {
        TARGET_LIST => true,
        ASSIGN_STMT | COMPOUND_ASSIGN_STMT => parent.children().next().is_some_and(|first| first == *node),
        _ => false,
    }
}

/// The Lua global a path's root names, or `None` when it is an NL name.
fn global_root(node: &SyntaxNode, res: &Resolution) -> Option<String> {
    let tok = nl_sema::lua_path_root(node)?;
    match res.reference(tok.text_range()) {
        Some(Ref::LuaGlobal(name)) => Some(name.clone()),
        _ => None,
    }
}

/// Decides what a file localizes (see the module documentation).
/// `imported_globals` are the `global` names of the files it imports.
pub(crate) fn plan(root: &SyntaxNode, res: &Resolution, mode: Localize, imported_globals: &[String]) -> Plan {
    let mut plan = Plan::default();
    if mode == Localize::None {
        return plan;
    }
    // The roots and paths the file assigns.
    let mut assigned_roots: HashSet<String> = declared_globals(root).into_iter().collect();
    let mut assigned_paths: HashSet<(String, String)> = HashSet::new();
    // The paths read: the root, and the name of the first step when it is
    // a field.
    let mut reads: Vec<(String, Option<String>)> = Vec::new();
    for node in root.descendants().filter(|n| n.kind() == LUA_PATH) {
        let Some(global) = global_root(&node, res) else { continue };
        let steps = path_steps(&node);
        let first = steps.first().and_then(field_step_name);
        if is_target(&node) {
            // Only a path of one step assigns the path itself; a longer
            // one writes into the value of its first steps.
            if steps.len() == 1 {
                if let Some(name) = first {
                    if global == "_G" {
                        assigned_roots.insert(name.clone());
                    }
                    assigned_paths.insert((global, name));
                }
                continue;
            }
        }
        reads.push((global, first));
    }
    for (global, first) in reads {
        if assigned_roots.contains(&global) {
            continue;
        }
        let path_ok = |name: &String| !assigned_paths.contains(&(global.clone(), name.clone()));
        match mode {
            Localize::None => {}
            Localize::Std => {
                if let Some(name) = first.filter(|n| STD_LIBRARIES.contains(&global.as_str()) && path_ok(n)) {
                    plan.paths.insert((global, name));
                }
            }
            Localize::All => {
                if imported_globals.contains(&global) {
                    continue;
                }
                match first.filter(|n| global != "_G" && path_ok(n)) {
                    Some(name) => {
                        plan.paths.insert((global, name));
                    }
                    None => {
                        plan.roots.insert(global);
                    }
                }
            }
        }
    }
    plan
}

/// The value a localized path stands for, read where it stands: `root` or
/// `root.name`. limits.rs puts it back where a local is dropped.
pub(crate) fn path_value(root: &str, name: Option<&str>) -> Expr {
    let g = Expr::Global(root.to_string());
    match name {
        Some(n) => Expr::index(g, Expr::str(n)),
        None => g,
    }
}

/// The value a localized path's local is set to when the file loads: the
/// path, guarded against a root that may be nil (see the module
/// documentation).
pub(crate) fn initial_value(root: &str, name: Option<&str>) -> Expr {
    let read = path_value(root, name);
    if name.is_none() || ALWAYS_PRESENT.contains(&root) {
        return read;
    }
    Expr::binary(BinOp::And, Expr::Global(root.to_string()), read)
}

/// The name hint of a localized path's local, which VM error messages
/// show (`attempt to call upvalue 'string_format'`): `string_format`, and
/// `_G_print` for a root alone (the global is `_G.print`). A root alone is
/// not named after itself: the Lua backend would then read the global of
/// that name, and every other use of it in the file, through `__G`.
pub(crate) fn hint(root: &str, name: Option<&str>) -> String {
    match name {
        Some(n) => format!("{root}_{n}"),
        None => format!("_G_{root}"),
    }
}

impl super::Lowerer<'_> {
    /// The local that holds a localized path (`root.name`, or `root` alone
    /// for `name` `None`), made on first use; the main function declares
    /// it ([`Plan`], module documentation).
    pub(crate) fn localized_path(&mut self, root: &str, name: Option<&str>) -> Expr {
        if let Some((l, _, _)) = self.localized.iter().find(|(_, r, n)| r == root && n.as_deref() == name) {
            return Expr::Local(*l);
        }
        let l = self.new_local(&hint(root, name));
        self.localized.push((l, root.to_string(), name.map(str::to_string)));
        Expr::Local(l)
    }
}
