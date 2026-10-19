//! Declaration files: the names a set of
//! parsed `.d.nl` files declare, with the Lua path each one stands for.
//!
//! A [`Declarations`] table is built once per compile from the `.d.nl`
//! trees the driver read ([`Declarations::load`]) and handed to the
//! resolver, which looks declared names up after the prelude
//! and records a [`crate::Ref::Declared`] for each use, and
//! to the analyzer, which reads the types of the members off the trees.
//! Nothing here runs: a declaration changes what the compiler emits (the
//! Lua path, through [`crate::Resolution::declared_path`]) and what the
//! analyzer knows.
//!
//! The built-in prelude declarations of the Lua and LuaJIT standard
//! libraries and of NL's own library live in `decl/` at the repository root and
//! are embedded here ([`prelude_declarations`]). The names they declare
//! are prelude names already (`Number`, `String`, `print`, …), so the
//! resolver keeps resolving those as prelude names and only the analyzer
//! takes their types from the declarations.

use std::collections::HashMap;
use std::sync::OnceLock;

use nl_syntax::{SyntaxKind, SyntaxNode, SyntaxToken, TextRange};

use crate::binding::{BindingKind, SemaDiagnostic};
use crate::util::*;

use SyntaxKind::*;

/// Index of a declared name in [`Declarations::items`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct DeclId(pub u32);

/// What a top-level `declare` item declares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeclItemKind {
    Module,
    /// `declare struct`: a Lua class, checked by its metatable.
    Struct,
    /// `declare type`: a host type, checked by its registered name.
    HostType,
    Fn,
    Let,
    Const,
}

/// One step of a Lua path after its first name. Only the
/// literal arguments a declaration can hold are kept.
#[derive(Clone, Debug, PartialEq)]
pub enum LuaStep {
    Field(String),
    Index(LuaArg),
    Call(Vec<LuaArg>),
}

/// A literal argument of a call or index in a `lua_root`.
#[derive(Clone, Debug, PartialEq)]
pub enum LuaArg {
    Str(Vec<u8>),
    Number(f64),
    Bool(bool),
    Nil,
}

/// Where a declared value lives: a Lua global and the steps from it.
#[derive(Clone, Debug, PartialEq)]
pub struct LuaRoot {
    pub global: String,
    pub steps: Vec<LuaStep>,
}

/// One declared top-level name.
#[derive(Clone, Debug)]
pub struct DeclItem {
    pub name: String,
    pub kind: DeclItemKind,
    /// The canonical name of the declaration file.
    pub file: String,
    /// The range of the declaring name, in that file.
    pub range: TextRange,
    /// The `DECL_*` node, for the analyzer.
    pub node: SyntaxNode,
    /// The Lua value the name stands for. A host type has none.
    pub root: Option<LuaRoot>,
}

/// The names of a set of declaration files.
#[derive(Clone, Debug, Default)]
pub struct Declarations {
    pub items: Vec<DeclItem>,
    by_name: HashMap<String, DeclId>,
}

impl Declarations {
    /// Loads the items of parsed declaration files, in order. A name
    /// declared twice is an error at the second declaration; the diagnostics are given per file, as (file, diagnostic).
    pub fn load(files: &[(String, SyntaxNode)]) -> (Declarations, Vec<(String, SemaDiagnostic)>) {
        let mut decls = Declarations::default();
        let mut diags = Vec::new();
        for (file, root) in files {
            for item in root.children() {
                let kind = match item.kind() {
                    DECL_MODULE => DeclItemKind::Module,
                    DECL_STRUCT => DeclItemKind::Struct,
                    DECL_TYPE => DeclItemKind::HostType,
                    DECL_FN => DeclItemKind::Fn,
                    DECL_VALUE if has_token(&item, CONST_KW) => DeclItemKind::Const,
                    DECL_VALUE => DeclItemKind::Let,
                    _ => continue,
                };
                let Some(name_node) = child(&item, NAME) else { continue };
                let name = name_text(&name_node).unwrap_or_default();
                let root = match kind {
                    DeclItemKind::HostType => None,
                    _ => Some(match child(&item, LUA_ROOT).and_then(|r| child(&r, LUA_PATH)) {
                        Some(path) => match lua_root(&path) {
                            Ok(r) => r,
                            Err(msg) => {
                                diags.push((file.clone(), SemaDiagnostic { range: path.text_range(), code: "decl-lua-root", message: msg, error: true, lint: false }));
                                LuaRoot { global: name.clone(), steps: Vec::new() }
                            }
                        },
                        None => LuaRoot { global: name.clone(), steps: Vec::new() },
                    }),
                };
                if let Some(&earlier) = decls.by_name.get(&name) {
                    let other = decls.items.get(earlier.0 as usize).map(|i| i.file.clone()).unwrap_or_default();
                    diags.push((
                        file.clone(),
                        SemaDiagnostic {
                            range: name_node.text_range(),
                            code: "decl-duplicate",
                            message: format!("`{name}` is declared twice: here and in {other}; two declaration files cannot declare one name"),
                            error: true,
                            lint: false,
                        },
                    ));
                    continue;
                }
                let id = DeclId(u32::try_from(decls.items.len()).unwrap_or(u32::MAX));
                decls.by_name.insert(name.clone(), id);
                decls.items.push(DeclItem { name, kind, file: file.clone(), range: name_node.text_range(), node: item, root });
            }
        }
        (decls, diags)
    }

    pub fn lookup(&self, name: &str) -> Option<DeclId> {
        self.by_name.get(name).copied()
    }

    pub fn get(&self, id: DeclId) -> Option<&DeclItem> {
        self.items.get(id.0 as usize)
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The binding kind of a declared name.
    pub fn binding_kind(&self, id: DeclId) -> BindingKind {
        let Some(item) = self.get(id) else { return BindingKind::Variable };
        match item.kind {
            DeclItemKind::Module | DeclItemKind::Struct | DeclItemKind::HostType => BindingKind::TypeLevel,
            DeclItemKind::Fn => BindingKind::Function,
            DeclItemKind::Let | DeclItemKind::Const => {
                if type_child(&item.node).is_some_and(|t| is_function_type(&t)) { BindingKind::Function } else { BindingKind::Variable }
            }
        }
    }

    /// True for a name usable as a type (`declare struct`, `declare type`).
    pub fn is_type(&self, id: DeclId) -> bool {
        matches!(self.get(id).map(|i| i.kind), Some(DeclItemKind::Struct | DeclItemKind::HostType))
    }
}

/// The `LuaRoot` of a `LUA_PATH` in a declaration (a
/// global, a nested one, a required module or a member of one).
fn lua_root(path: &SyntaxNode) -> Result<LuaRoot, String> {
    let Some(first) = crate::resolver::lua_path_root(path) else {
        return Err("a `lua_root` names a Lua global: `= :name`".to_string());
    };
    let mut steps = Vec::new();
    for step in path.children() {
        match step.kind() {
            LUA_FIELD => {
                let name = tokens(&step).find(|t| t.kind().is_name() || t.kind().is_keyword()).map(|t| t.text().to_string()).unwrap_or_default();
                steps.push(LuaStep::Field(name));
            }
            LUA_INDEX => {
                let arg = expr_child(&step).and_then(|e| literal_arg(&e)).ok_or_else(|| "an index in a `lua_root` must be a literal: a string, a number, `true`, `false` or `nil`".to_string())?;
                steps.push(LuaStep::Index(arg));
            }
            LUA_CALL => {
                let mut args = Vec::new();
                if let Some(list) = child(&step, ARG_LIST) {
                    for a in list.children() {
                        if is_expr(a.kind()) {
                            args.push(literal_arg(&a).ok_or_else(|| "an argument in a `lua_root` must be a literal: a string, a number, `true`, `false` or `nil`".to_string())?);
                        } else {
                            return Err("a call in a `lua_root` takes literal arguments only".to_string());
                        }
                    }
                }
                if child(&step, TRAILING_BLOCK).is_some() {
                    return Err("a call in a `lua_root` takes no block".to_string());
                }
                steps.push(LuaStep::Call(args));
            }
            LUA_METHOD => return Err("a `lua_root` names a value: it cannot end in a method call `:name()`".to_string()),
            _ => {}
        }
    }
    Ok(LuaRoot { global: first.text().to_string(), steps })
}

fn literal_arg(e: &SyntaxNode) -> Option<LuaArg> {
    match e.kind() {
        LITERAL => {
            let tok: SyntaxToken = first_token(e)?;
            Some(match tok.kind() {
                NUMBER => LuaArg::Number(nl_syntax::number_value(tok.text()).unwrap_or(0.0)),
                STRING => LuaArg::Str(nl_syntax::decode_string(&[tok.text()]).into_iter().next().unwrap_or_default()),
                TRUE_KW => LuaArg::Bool(true),
                FALSE_KW => LuaArg::Bool(false),
                NIL_KW => LuaArg::Nil,
                _ => return None,
            })
        }
        PREFIX_EXPR => {
            let inner = expr_child(e)?;
            match literal_arg(&inner)? {
                LuaArg::Number(n) if first_token(e).is_some_and(|t| t.kind() == MINUS) => Some(LuaArg::Number(-n)),
                _ => None,
            }
        }
        PAREN_EXPR => literal_arg(&expr_child(e)?),
        _ => None,
    }
}

/// The source of each built-in declaration file of `decl/`, by name.
pub const PRELUDE_SOURCES: &[(&str, &str)] = &[
    ("decl/lua.d.nl", include_str!("../../../decl/lua.d.nl")),
    ("decl/nl.d.nl", include_str!("../../../decl/nl.d.nl")),
];

/// The parsed built-in declaration files, parsed once per process.
fn prelude_trees() -> &'static Vec<(String, rowan::GreenNode)> {
    static TREES: OnceLock<Vec<(String, rowan::GreenNode)>> = OnceLock::new();
    TREES.get_or_init(|| {
        PRELUDE_SOURCES
            .iter()
            .map(|(name, src)| {
                let parse = nl_syntax::parse_decl_file(src);
                (name.to_string(), parse.into_parts().0)
            })
            .collect()
    })
}

/// The parsed built-in declaration files of `decl/`, to load together
/// with a host's own ([`Declarations::load`]).
pub fn prelude_files() -> Vec<(String, SyntaxNode)> {
    prelude_trees().iter().map(|(n, g)| (n.clone(), SyntaxNode::new_root(g.clone()))).collect()
}

/// The built-in declarations of `decl/` (see the module documentation).
pub fn prelude_declarations() -> Declarations {
    Declarations::load(&prelude_files()).0
}

/// The syntax errors of the built-in declaration files, for a test.
#[cfg(test)]
pub(crate) fn prelude_syntax_errors() -> Vec<String> {
    PRELUDE_SOURCES
        .iter()
        .flat_map(|(name, src)| {
            let parse = nl_syntax::parse_decl_file(src);
            parse.diagnostics().iter().map(|d| format!("{name}: {}", nl_syntax::dump_diagnostics(src, std::slice::from_ref(d)))).collect::<Vec<_>>()
        })
        .collect()
}
