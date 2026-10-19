//! Name resolution and binding kinds of NL.
//!
//! [`resolve`] walks a parsed file once and produces a [`Resolution`]:
//!
//! - every declared name is a [`Binding`] with its [`BindingKind`]:
//!   function, variable or type-level, read off the declaration
//!   and the written annotations, never inferred;
//! - every name in an expression maps to a [`Ref`]: a binding, a prelude
//!   name ([`prelude`]) or, for the first name of a Lua path, the Lua global;
//! - the targets of `Self`, the owners of `fn Owner.name`, the hidden `self`
//!   of each method and the module of each `module` declaration;
//! - the resolver's diagnostics ([`SemaDiagnostic`]).
//!
//! Results are keyed by the `TextRange` of the node or token they describe
//! (a `NAME_REF` node, a declaring `NAME` node, a `QUAL_NAME`, …), so a later
//! pass that walks the same tree finds them without node identities.
//!
//! # Conventions
//!
//! - One walk, in source order ([`resolver`]). `fn` declarations and
//!   type-level declarations are hoisted to the start of their block; `let`,
//!   `const` and `global` are visible from their declaration on.
//! - Checks that need no names live in [`checks`] (parameter lists,
//!   discarded values). Checks that need names (assignment to constants,
//!   blocks on functions without a `Function` parameter, `spawn` callees,
//!   `Self`) are made where the walk resolves the name.
//! - Decisions the lowering consumes rather than repeats are made here and
//!   recorded in the [`Resolution`]: which expression statements discard
//!   their value ([`Resolution::value_discarded`]), which dotted
//!   names in patterns are enum variants ([`Resolution::is_enum_variant`]),
//!   which bindings a closure assigns ([`Resolution::assigned_in_closure`]),
//!   and one binding per name for the items of a `when`.
//! - Imports are resolved through an [`ImportProvider`]: the driver reads
//!   and resolves the imported file and reports its exports, which are the
//!   [`Resolution::exports`] of that file (the one place where the kind of
//!   an export is decided).
//! - The analyzer is [`analyzer`]: it runs after the resolver on the
//!   same tree and [`Resolution`], infers types ([`types`]), reports type
//!   errors, warnings and lints, and records the [`hints::Hints`] the
//!   lowering consumes. Declaration files are [`decl`]: the
//!   resolver looks their names up after the prelude.

#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing))]

pub mod analyzer;
mod binding;
mod callonly;
mod checks;
pub mod decl;
pub mod hints;
pub mod prelude;
mod resolver;
pub mod types;
pub mod util;

pub use analyzer::{Analysis, AnalyzeOptions, analyze};
pub use binding::*;
pub use decl::{DeclId, Declarations, LuaArg, LuaRoot, LuaStep, prelude_declarations};
pub use hints::{AsHint, Hints, LoopHint, MemberHint};
pub use prelude::{Access, Prelude, PreludeClass};
pub use resolver::{decode_plain_string, has_function_param, lua_path_root};

use nl_syntax::SyntaxNode;

/// One export of an imported file, as the importing file sees it
/// (imported names keep their binding kinds).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportInfo {
    pub name: String,
    pub kind: BindingKind,
    pub params: Option<FnParams>,
    /// What declares it in the exporting file (`Fn`, `Const`, `Struct`,
    /// `Enum`, `Interface`, `TypeAlias`, `Module`);
    /// [`Resolution::declared_as`] gives it for the importing binding.
    pub decl: DeclKind,
}

/// How the resolver learns what an imported file exports.
pub trait ImportProvider {
    /// The exports of the file that the import path `path` names, as seen
    /// from the file being resolved; an error message when the file cannot
    /// be found or compiled.
    fn exports(&self, path: &str) -> Result<Vec<ExportInfo>, String>;
}

/// An [`ImportProvider`] that refuses every import.
pub struct NoImports;

impl ImportProvider for NoImports {
    fn exports(&self, path: &str) -> Result<Vec<ExportInfo>, String> {
        Err(format!("cannot import '{path}': imports are not available here"))
    }
}

/// Resolves the names of a parsed file (`root` is its `SOURCE_FILE`),
/// without declaration files.
pub fn resolve(root: &SyntaxNode, imports: &dyn ImportProvider) -> Resolution {
    resolve_with(root, imports, &Declarations::default())
}

/// Resolves the names of a parsed file with the declaration files in
/// scope after the prelude.
pub fn resolve_with(root: &SyntaxNode, imports: &dyn ImportProvider, decls: &Declarations) -> Resolution {
    let mut r = resolver::Resolver::new(imports, decls);
    r.walk(root);
    r.mark_call_only(root);
    r.collect_exports(root);
    checks::check_discarded(&mut r, root);
    let mut res = r.finish();
    res.diagnostics.sort_by_key(|d| (!d.error, d.range.start()));
    res
}

#[cfg(test)]
mod tests;
