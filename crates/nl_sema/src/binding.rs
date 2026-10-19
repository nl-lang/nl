//! Bindings, references and the result of resolution.

use std::collections::{HashMap, HashSet};

use nl_syntax::{SyntaxNode, TextRange};

use crate::ExportInfo;
use crate::decl::{DeclId, DeclItemKind, LuaRoot};
use crate::prelude::Prelude;

/// Index of a [`Binding`] in [`Resolution::bindings`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BindingId(pub u32);

/// The binding kinds. Call resolution uses them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BindingKind {
    /// Called wherever its value is used.
    Function,
    /// Read where its value is used.
    Variable,
    /// A struct, interface, enum, alias, module or type parameter.
    TypeLevel,
}

/// What declared a binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeclKind {
    Let,
    Const,
    Global,
    Param,
    RestParam,
    /// `**opts`.
    KeyedParam,
    /// The hidden `self` parameter of a method.
    SelfParam,
    /// `fn name` in statement position.
    Fn,
    LoopVar,
    PatternVar,
    CatchVar,
    Struct,
    Enum,
    Interface,
    TypeAlias,
    Module,
    TypeParam,
    Import,
}

/// What is known about the parameters of a function a binding names, for
/// the block rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FnParams {
    /// Some parameter is annotated `Function` or `Function(...)`.
    pub has_function_param: bool,
    /// The function is anonymous (`let f = fn ... end`).
    pub anonymous: bool,
    /// The written parameters the function only calls (`callonly.rs`),
    /// one bit per position from the first: a block or `fn` literal
    /// passed there is referenced by nothing once the call returns.
    pub call_only: u64,
}

/// One declared name.
#[derive(Clone, Debug)]
pub struct Binding {
    pub name: String,
    pub kind: BindingKind,
    pub decl: DeclKind,
    /// The range of the declaring name (or of the declaration, for hidden
    /// bindings such as `self`).
    pub range: TextRange,
    pub mutable: bool,
    /// Declared at file level (not inside a function, module or block).
    pub file_level: bool,
    /// The index of the function the binding belongs to, `0` being the
    /// file's main function. Used for `spawn` capture checks.
    pub function: u32,
    /// The written type annotation, checked at runtime on assignment.
    pub annotation: Option<SyntaxNode>,
    /// For a function binding: what its parameter list declares.
    pub params: Option<FnParams>,
    /// For an `fn` in a module body: the module binding. A bare call passes
    /// the module as `self`.
    pub module_owner: Option<BindingId>,
    /// A named function: a file-level `fn` or a module method.
    pub named: bool,
}

/// What a name in an expression refers to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ref {
    /// A binding of this file.
    Binding(BindingId),
    /// A prelude name.
    Prelude(&'static Prelude),
    /// The first name of a Lua path that is no NL local: the Lua global of
    /// that name.
    LuaGlobal(String),
    /// A name of a declaration file: emitted as its Lua path
    /// ([`Resolution::declared_path`]).
    Declared(DeclId),
}

/// What `Self` stands for at one place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelfTarget {
    Binding(BindingId),
    /// A table reached from a binding through a path: `Self` in
    /// `fn Inventory.Filters.add` is `Inventory.Filters`.
    Member { id: BindingId, path: Vec<String> },
    Prelude(&'static Prelude),
}

impl SelfTarget {
    /// The binding the target is, or is reached from.
    pub fn binding(&self) -> Option<BindingId> {
        match self {
            SelfTarget::Binding(id) | SelfTarget::Member { id, .. } => Some(*id),
            SelfTarget::Prelude(_) => None,
        }
    }
}

/// What the owner of `fn Owner.name` is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MethodOwner {
    /// A struct or module of this file (possibly reached through a path:
    /// `fn Inventory.Filters.add` has the binding of `Inventory` and the
    /// path `["Filters"]`).
    Binding { id: BindingId, path: Vec<String> },
    /// `Number`, `String` or `Bool`.
    Primitive(&'static Prelude),
}

/// A diagnostic of the resolver.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemaDiagnostic {
    pub range: TextRange,
    pub code: &'static str,
    pub message: String,
    pub error: bool,
    /// A lint (`NL_DIAG_LINT`): not an error and not a warning about the
    /// program's behaviour, but about its style (unused bindings, …).
    pub lint: bool,
}

/// The result of [`crate::resolve`].
#[derive(Debug, Default)]
pub struct Resolution {
    pub bindings: Vec<Binding>,
    /// `NAME_REF` nodes, `SELF_EXPR`/`AT_EXPR` nodes (to the `self`
    /// binding) and the first-name tokens of Lua paths, by range.
    pub(crate) refs: HashMap<TextRange, Ref>,
    /// Declaring `NAME` nodes (and `BIND_PAT`s) by range.
    pub(crate) decls: HashMap<TextRange, BindingId>,
    /// The first segment of each `QUAL_NAME` used as a type, struct literal
    /// path or pattern type, by the range of the `QUAL_NAME`.
    pub(crate) type_refs: HashMap<TextRange, Ref>,
    /// `Self` expressions and types, by range.
    pub(crate) self_refs: HashMap<TextRange, SelfTarget>,
    /// The hidden `self` binding of each method, by the range of the
    /// function node (`FN_DECL`).
    pub(crate) method_self: HashMap<TextRange, BindingId>,
    /// The owner of each `fn Owner.name`, by the range of the `FN_DECL`.
    pub(crate) method_owner: HashMap<TextRange, MethodOwner>,
    /// The module binding of each `MODULE_DECL`, by its range, and whether
    /// the declaration reopens an existing module.
    pub(crate) modules: HashMap<TextRange, (BindingId, bool)>,
    /// `const` declarations directly in a module body, by the range of the
    /// `CONST_DECL`, with the module they are members of.
    pub(crate) module_consts: HashMap<TextRange, BindingId>,
    /// For a name bound by `import name` or `import { names }`: what
    /// declares it in the exporting file.
    pub(crate) imported: HashMap<BindingId, DeclKind>,
    /// Expression statements whose value is discarded ("a
    /// statement that is not last"), by the range of the `EXPR_STMT`. The
    /// lowering consumes this decision instead of making its own.
    pub(crate) discarded: HashSet<TextRange>,
    /// `QUAL_NAME`s in patterns that name an enum variant (compared with
    /// `==`); every other dotted name in a pattern names a type.
    pub(crate) variants: HashSet<TextRange>,
    /// Bindings that a function other than their own assigns: a call can
    /// change them between two reads (evaluation order).
    pub(crate) assigned_in_closure: HashSet<BindingId>,
    /// The file's `pub` items in source order, each with the binding that
    /// holds its value. The one place where the name and kind
    /// of an export are decided: the driver hands [`Resolution::exports`]
    /// to the files that import this one, and the lowering builds the
    /// export table from it.
    pub(crate) exports: Vec<(BindingId, ExportInfo)>,
    /// The Lua path of each declared name the file uses.
    pub(crate) decl_paths: HashMap<DeclId, LuaRoot>,
    /// What each declared name the file uses declares.
    pub(crate) decl_kinds: HashMap<DeclId, DeclItemKind>,
    /// The binding kind of each declared name the file uses (a `declare
    /// fn` is function-kind).
    pub(crate) decl_binding_kinds: HashMap<DeclId, BindingKind>,
    pub diagnostics: Vec<SemaDiagnostic>,
}

impl Resolution {
    /// The binding with this id.
    pub fn binding(&self, id: BindingId) -> Option<&Binding> {
        self.bindings.get(id.0 as usize)
    }

    /// What the name at `range` refers to (`NAME_REF`, `SELF_EXPR` `self`,
    /// `AT_EXPR`, Lua path first name).
    pub fn reference(&self, range: TextRange) -> Option<&Ref> {
        self.refs.get(&range)
    }

    /// The binding declared by the `NAME` at `range`.
    pub fn declaration(&self, range: TextRange) -> Option<BindingId> {
        self.decls.get(&range).copied()
    }

    /// What the first segment of the `QUAL_NAME` at `range` refers to.
    pub fn type_ref(&self, range: TextRange) -> Option<&Ref> {
        self.type_refs.get(&range)
    }

    /// What `Self` at `range` stands for.
    pub fn self_target(&self, range: TextRange) -> Option<&SelfTarget> {
        self.self_refs.get(&range)
    }

    /// The primitive type when the member `name` of the receiver `recv` is
    /// a plain function: `recv` names a primitive type's library directly
    /// (the prelude's `Number` or `String`, not shadowed, or `Self` in a
    /// method defined on `Number`, `String` or `Bool`). A method read
    /// from it is not bound to the table: `String.shout(s)` passes `s` as
    /// `self`. `from`, which `From<T>` defines, is the exception: it is
    /// called on the type, which is its `self` (`Number.from(x)`).
    pub fn plain_primitive_member(&self, recv: &SyntaxNode, name: &str) -> Option<&'static Prelude> {
        if name == "from" {
            return None;
        }
        use nl_syntax::SyntaxKind::{NAME_REF, SELF_EXPR, SELF_TYPE_KW};
        let p = match recv.kind() {
            NAME_REF => match self.reference(recv.text_range())? {
                Ref::Prelude(p) => *p,
                _ => return None,
            },
            SELF_EXPR if crate::util::has_token(recv, SELF_TYPE_KW) => match self.self_target(recv.text_range())? {
                SelfTarget::Prelude(p) => *p,
                _ => return None,
            },
            _ => return None,
        };
        p.is_primitive().then_some(p)
    }

    /// The hidden `self` binding of the method declared by the node at
    /// `range` (an `FN_DECL` of a method, or of a module function).
    pub fn method_self(&self, range: TextRange) -> Option<BindingId> {
        self.method_self.get(&range).copied()
    }

    /// The owner of the `fn Owner.name` declared by the node at `range`.
    pub fn method_owner(&self, range: TextRange) -> Option<&MethodOwner> {
        self.method_owner.get(&range)
    }

    /// The module binding of the `MODULE_DECL` at `range`, and whether it
    /// reopens an existing module.
    pub fn module(&self, range: TextRange) -> Option<(BindingId, bool)> {
        self.modules.get(&range).copied()
    }

    /// The module that the `CONST_DECL` at `range` is a member of, when it
    /// stands directly in a module body.
    pub fn module_const(&self, range: TextRange) -> Option<BindingId> {
        self.module_consts.get(&range).copied()
    }

    /// What declares a binding, seeing through imports: for a name bound
    /// by `import name` or `import { names }`, the declaration kind in the
    /// exporting file (imported names keep their kinds); for
    /// `import from … as Name`, `Import`.
    pub fn declared_as(&self, id: BindingId) -> Option<DeclKind> {
        self.imported.get(&id).copied().or_else(|| self.binding(id).map(|b| b.decl))
    }

    /// True when the value of the expression statement at `range` (an
    /// `EXPR_STMT`) is discarded: a name or member there is always called.
    /// False when its value is used, as the tail of a
    /// function or block expression, or as the last statement of a block.
    pub fn value_discarded(&self, range: TextRange) -> bool {
        self.discarded.contains(&range)
    }

    /// True when the dotted name of a pattern (the `QUAL_NAME` at `range`)
    /// is an enum variant, compared with `==`; otherwise it names a type,
    /// tested with `is`, and after a key it binds the key.
    pub fn is_enum_variant(&self, range: TextRange) -> bool {
        self.variants.contains(&range)
    }

    /// True when a function other than the binding's own assigns it.
    pub fn assigned_in_closure(&self, id: BindingId) -> bool {
        self.assigned_in_closure.contains(&id)
    }

    /// What the file exports, as a file that imports it sees it.
    pub fn exports(&self) -> Vec<ExportInfo> {
        self.exports.iter().map(|(_, e)| e.clone()).collect()
    }

    /// The exports with the binding that holds each one's value (for an
    /// export declared by `type`, the alias).
    pub fn export_bindings(&self) -> impl Iterator<Item = (BindingId, &ExportInfo)> + '_ {
        self.exports.iter().map(|(id, e)| (*id, e))
    }

    /// The Lua path a declared name stands for, recorded
    /// when the resolver resolved a use of it.
    pub fn declared_path(&self, id: DeclId) -> Option<&LuaRoot> {
        self.decl_paths.get(&id)
    }

    /// What a declared name the file uses declares (a module, a Lua class,
    /// a host type, …).
    pub fn declared_kind(&self, id: DeclId) -> Option<DeclItemKind> {
        self.decl_kinds.get(&id).copied()
    }

    /// The binding kind of a declared name the file uses.
    pub fn declared_binding_kind(&self, id: DeclId) -> Option<BindingKind> {
        self.decl_binding_kinds.get(&id).copied()
    }

    /// True when the binding is a `const` directly in a module body: a
    /// member of the module.
    pub fn module_const_of(&self, id: BindingId) -> bool {
        self.binding(id).is_some_and(|b| self.module_consts.keys().any(|r| r.contains_range(b.range)))
    }

    /// True when some diagnostic is an error.
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.error)
    }
}
