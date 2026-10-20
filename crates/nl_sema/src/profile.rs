//! Profiles: the subsets of the language a target supports.
//!
//! The full language needs the VM fork. The portable profile is what runs
//! on an unmodified stock LuaJIT, 2.0 or 2.1, through the Lua backend and
//! the pure-Lua runtime: no regex engine (a Rust library), no dedicated
//! workers (a worker needs a state of its own), and no compiler at run
//! time.
//!
//! The gmod-client profile is the portable profile in Garry's Mod's
//! client realm, on the stock game: on top of the portable profile's
//! rules, a use of what gmod's Lua state does not have is an error
//! ([`gmod_missing`]), and so is a use of a gmod function, method or value
//! that the client realm does not have (the realm a declaration's doc
//! comment gives: `## Realm: server. https://…`).
//!
//! # What runs
//!
//! [`check`] runs on a resolved file and reports every use the profile
//! leaves out, at the use. It looks at what the resolver decided a name
//! refers to, so a local that shadows a prelude name is not reported.
//! [`check_members`] runs after the analyzer, on what it recorded
//! (`AnalyzeOptions::record`): the members of declared types and modules
//! that the file reads (`ply.kick`, `Net.send_to_server`).
//!
//! # Realm constants
//!
//! In a realm profile, gmod's `SERVER` and `CLIENT` have one value in
//! every run, so a condition made of them, `not`, `and`, `or` and
//! parentheses ([`static_condition`]) is decided when the file is
//! compiled: `if SERVER … end`, `unless CLIENT … end` and `x if SERVER`
//! are dead code in the client realm. Dead code is not checked against
//! the profile ([`dead_ranges`]), and the lowering leaves it out of the
//! output (nl_ir, control.rs), so a shared file can keep its server code
//! under `if SERVER`. Only the constants declared as gmod's (`declare
//! const SERVER: Bool` with the Lua path `SERVER`) are decided; a local of
//! that name is an ordinary value.

use std::collections::HashMap;

use nl_syntax::{SyntaxKind, SyntaxNode, TextRange};

use crate::analyzer::TypeInfo;
use crate::binding::{Ref, Resolution, SemaDiagnostic};
use crate::decl::{DeclItemKind, Declarations, LuaRoot, LuaStep};
use crate::types::NamedId;
use crate::util::*;

use SyntaxKind::*;

/// A subset of the language that a target supports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Profile {
    /// Everything: the VM fork.
    #[default]
    Full,
    /// What runs on stock LuaJIT 2.0 and 2.1 with the pure-Lua runtime.
    Portable,
    /// The portable profile in Garry's Mod's client realm, on the stock
    /// game.
    GmodClient,
}

impl Profile {
    /// The profile of a command-line name: `full`, `portable` or
    /// `gmod-client`.
    pub fn from_name(name: &str) -> Option<Profile> {
        match name {
            "full" => Some(Profile::Full),
            "portable" => Some(Profile::Portable),
            "gmod-client" => Some(Profile::GmodClient),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Profile::Full => "full",
            Profile::Portable => "portable",
            Profile::GmodClient => "gmod-client",
        }
    }

    /// Runs on a stock LuaJIT, through the Lua backend and the pure-Lua
    /// runtime: every profile but the full one.
    pub fn is_stock(self) -> bool {
        self != Profile::Full
    }

    /// Garry's Mod.
    pub fn is_gmod(self) -> bool {
        self == Profile::GmodClient
    }

    /// The value of a realm constant of gmod (`SERVER`, `CLIENT`) in this
    /// profile, by its Lua name.
    fn realm_constant(self, name: &str) -> Option<bool> {
        match (self, name) {
            (Profile::GmodClient, "SERVER") => Some(false),
            (Profile::GmodClient, "CLIENT") => Some(true),
            _ => None,
        }
    }
}

/// The code of a use the portable profile leaves out.
pub const CODE: &str = "not-in-profile";
/// The code of a use of what gmod's Lua state does not have.
pub const CODE_GMOD: &str = "not-in-gmod";
/// The code of a use of what the realm of the profile does not have.
pub const CODE_REALM: &str = "wrong-realm";

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

/// What Garry's Mod's Lua state does not have, as a Lua global and the
/// first field read from it: why, or `None` when gmod has it.
///
/// Gmod reshapes LuaJIT's standard libraries (gmod-lua-shared, which
/// reimplements gmod's lua_shared library from the game's binary:
/// glua.cpp and CLuaInterface.cpp): no `io`, `load`, `loadfile`,
/// `loadstring` or `dofile`, and no FFI; `os` keeps the time functions;
/// `debug` loses the functions that change other functions' locals and
/// upvalues; `package` has no paths and no searchers. The main branch's
/// client runs LuaJIT 2.0.4, which lacks the 2.1 additions
/// (`coroutine.isyieldable`; the x86-64 branch has 2.1.0-beta3).
pub fn gmod_missing(global: &str, field: Option<&str>) -> Option<String> {
    let removed = |what: &str, instead: &str| Some(format!("Garry's Mod removes `{what}`{instead}"));
    match (global, field) {
        ("io", _) => Some("Garry's Mod has no `io` library; its `file` library (`FileLib`) reads and writes the data folder".to_owned()),
        ("load" | "loadstring", _) => removed(global, "; `CompileString` (`compile_string`) compiles Lua source"),
        ("loadfile" | "dofile", _) => removed(global, "; `include` runs a Lua file, and `CompileFile` (`compile_file`) compiles one"),
        ("ffi", _) => Some("Garry's Mod's LuaJIT is built without the FFI".to_owned()),
        ("os", Some(f)) if !matches!(f, "clock" | "date" | "difftime" | "time") => {
            Some(format!("Garry's Mod's `os` library has only `clock`, `date`, `difftime` and `time`, not `{f}`"))
        }
        ("debug", Some(f @ ("setlocal" | "setupvalue" | "upvalueid" | "upvaluejoin"))) => removed(&format!("debug.{f}"), ""),
        ("package", Some(f @ ("path" | "cpath" | "loadlib" | "searchpath"))) => removed(&format!("package.{f}"), "; `require` finds modules in `lua/includes/modules`"),
        ("package", Some("loaders")) => Some("`package.loaders` is empty in Garry's Mod: its `require` has no searchers".to_owned()),
        ("jit", Some("security")) => removed("jit.security", ""),
        ("coroutine", Some("isyieldable")) => Some(
            "`coroutine.isyieldable` is new in LuaJIT 2.1, and the clients of Garry's Mod's main branch run LuaJIT 2.0.4 (only the x86-64 branch has it)"
                .to_owned(),
        ),
        _ => None,
    }
}

/// Why `require(name)` fails in Garry's Mod, for a module that LuaJIT
/// provides and gmod does not; `None` for any other name.
fn gmod_missing_module(name: &str) -> Option<String> {
    match name {
        "ffi" => Some("Garry's Mod's LuaJIT is built without the FFI".to_owned()),
        "io" => gmod_missing("io", None),
        "table.new" | "table.clear" | "string.buffer" | "jit.profile" => Some(format!(
            "`{name}` is a module of LuaJIT 2.1 that Garry's Mod does not preload (and the clients of its main branch run LuaJIT 2.0.4)"
        )),
        _ => None,
    }
}

// ---- Realm constants ------------------------------------------------

/// The value `cond` has in every run under `profile`, when realm
/// constants decide it (module docs); `None` when it is not known.
/// `SERVER and f()` is false without a call; `f() and SERVER` is not
/// decided, as the call must run.
pub fn static_condition(cond: &SyntaxNode, res: &Resolution, profile: Profile) -> Option<bool> {
    match cond.kind() {
        NAME_REF => match res.reference(cond.text_range())? {
            Ref::Declared(id) => {
                let root = res.declared_path(*id)?;
                if !root.steps.is_empty() || res.declared_kind(*id) != Some(DeclItemKind::Const) {
                    return None;
                }
                profile.realm_constant(&root.global)
            }
            _ => None,
        },
        LUA_PATH => {
            let first = crate::resolver::lua_path_root(cond)?;
            if cond.children().next().is_some() || !matches!(res.reference(first.text_range()), Some(Ref::LuaGlobal(_))) {
                return None;
            }
            profile.realm_constant(first.text())
        }
        PAREN_EXPR => static_condition(&expr_child(cond)?, res, profile),
        PREFIX_EXPR => {
            let op = first_token(cond)?.kind();
            if !matches!(op, NOT_KW | BANG) {
                return None;
            }
            static_condition(&expr_child(cond)?, res, profile).map(|v| !v)
        }
        BIN_EXPR => {
            let op = bin_op(cond)?.kind();
            let mut operands = expr_children(cond);
            let (left, right) = (operands.next()?, operands.next()?);
            let l = static_condition(&left, res, profile);
            match op {
                AND_KW | AMP2 => match l? {
                    false => Some(false),
                    true => static_condition(&right, res, profile),
                },
                OR_KW | PIPE2 => match l? {
                    true => Some(true),
                    false => static_condition(&right, res, profile),
                },
                _ => None,
            }
        }
        _ => None,
    }
}

/// True when a `pub` item declared in the branches `clauses` of the
/// file-level `if` or `unless` `construct` has a value after it, whichever
/// way the `if` goes under `profile`: the branch the realm constants
/// decide is taken declares it, or every branch that may run does and
/// one of them always runs (an `else`). Otherwise the export may be
/// `nil` ([`crate::ExportInfo::optional`]).
pub fn defined_on_every_path(construct: &SyntaxNode, clauses: &[usize], res: &Resolution, profile: Profile) -> bool {
    // Each branch with what decides whether it runs, when the branches
    // before it did not: `Some(true)` for `else`.
    let mut owners: Vec<(SyntaxNode, bool)> = vec![(construct.clone(), construct.kind() == UNLESS_EXPR)];
    owners.extend(construct.children().filter(|c| matches!(c.kind(), ELSEIF_CLAUSE | ELSE_CLAUSE)).map(|c| (c, false)));
    for (i, (owner, negated)) in owners.iter().enumerate() {
        let decided = if owner.kind() == ELSE_CLAUSE { Some(true) } else { plain_condition(owner).and_then(|c| static_condition(&c, res, profile)).map(|v| v != *negated) };
        let declares = clauses.contains(&i);
        match decided {
            Some(false) => {}
            Some(true) => return declares,
            None if declares => {}
            None => return false,
        }
    }
    false
}

/// The file's exports as an importer compiled under `profile` sees them:
/// [`Resolution::exports`], with [`crate::ExportInfo::optional`] decided
/// by the realm constants of the profile for the `pub` items in branches
/// of a file-level `if` (`if CLIENT … end` always runs in the client
/// realm).
pub fn exports(res: &Resolution, profile: Profile) -> Vec<crate::ExportInfo> {
    res.export_bindings()
        .map(|(id, e)| {
            let mut e = e.clone();
            if let Some((c, clauses)) = res.conditional_export(id) {
                e.optional = !defined_on_every_path(c, clauses, res, profile);
            }
            e
        })
        .collect()
}

/// The condition of an `if`, `elseif`, `unless` or modifier: `None` for
/// an `if let`.
fn plain_condition(owner: &SyntaxNode) -> Option<SyntaxNode> {
    if child(owner, LET_CONDITION).is_some() {
        return None;
    }
    expr_child(owner)
}

/// The code that never runs under `profile` because realm constants
/// decide its condition (module docs): the bodies of branches that are
/// never taken, and statements whose modifier is always false.
pub fn dead_ranges(root: &SyntaxNode, res: &Resolution, profile: Profile) -> Vec<TextRange> {
    let mut out = Vec::new();
    if profile.realm_constant("SERVER").is_none() {
        return out;
    }
    let decide = |owner: &SyntaxNode| plain_condition(owner).and_then(|c| static_condition(&c, res, profile));
    for node in root.descendants() {
        match node.kind() {
            IF_EXPR => {
                // Once a branch is always taken, the rest never are.
                let mut taken = false;
                match decide(&node) {
                    Some(false) => out.extend(block(&node).map(|b| b.text_range())),
                    Some(true) => taken = true,
                    None => {}
                }
                for c in node.children() {
                    match c.kind() {
                        ELSEIF_CLAUSE if taken => out.push(c.text_range()),
                        ELSEIF_CLAUSE => match decide(&c) {
                            Some(false) => out.extend(block(&c).map(|b| b.text_range())),
                            Some(true) => taken = true,
                            None => {}
                        },
                        ELSE_CLAUSE if taken => out.push(c.text_range()),
                        _ => {}
                    }
                }
            }
            UNLESS_EXPR => match decide(&node) {
                Some(true) => out.extend(block(&node).map(|b| b.text_range())),
                Some(false) => out.extend(child(&node, ELSE_CLAUSE).map(|c| c.text_range())),
                None => {}
            },
            MODIFIER => {
                let negate = match first_token(&node).map(|t| t.kind()) {
                    Some(IF_KW) => false,
                    Some(UNLESS_KW) => true,
                    _ => continue,
                };
                if decide(&node).map(|v| v != negate) == Some(false) {
                    out.extend(node.parent().map(|s| s.text_range()));
                }
            }
            _ => {}
        }
    }
    out
}

fn is_dead(range: TextRange, dead: &[TextRange]) -> bool {
    dead.iter().any(|d| d.contains_range(range))
}

// ---- The checks -----------------------------------------------------

/// The diagnostics of the uses that `profile` leaves out, in source order:
/// the prelude names of the portable profile, and in gmod what its Lua
/// state lacks and what the realm lacks, through prelude names, declared
/// names and Lua paths. Members of declared types are [`check_members`]'.
pub fn check(root: &SyntaxNode, res: &Resolution, decls: &Declarations, profile: Profile) -> Vec<SemaDiagnostic> {
    let mut out = Vec::new();
    if !profile.is_stock() {
        return out;
    }
    let dead = dead_ranges(root, res, profile);
    let mut lua_paths: Option<HashMap<String, Realm>> = None;
    for node in root.descendants() {
        if is_dead(node.text_range(), &dead) {
            continue;
        }
        match node.kind() {
            NAME_REF => check_name(&node, res, decls, profile, &mut out),
            LUA_PATH if profile.is_gmod() => {
                let index = lua_paths.get_or_insert_with(|| lua_path_realms(decls));
                check_lua_path(&node, res, index, &mut out);
            }
            _ => {}
        }
    }
    out
}

fn check_name(node: &SyntaxNode, res: &Resolution, decls: &Declarations, profile: Profile, out: &mut Vec<SemaDiagnostic>) {
    match res.reference(node.text_range()) {
        Some(Ref::Prelude(p)) => {
            if let Some(why) = excluded(p.name) {
                out.push(error(node, CODE, format!("`{}` is not available in the {} profile: {why}", p.name, profile.name())));
                return;
            }
            // `Worker.new`: a dedicated worker needs a Lua state of its own.
            // `spawn`, `Worker.run` and `Worker.map` stay: their jobs run in
            // the same state on copies of their arguments.
            let field_of_parent = || node.parent().filter(|n| n.kind() == FIELD_EXPR).and_then(|parent| field_name(&parent).map(|t| (parent, t.text().to_owned())));
            if p.name == "Worker" {
                if let Some((parent, _)) = field_of_parent().filter(|(_, f)| f == "new") {
                    out.push(error(
                        &parent,
                        CODE,
                        format!(
                            "`Worker.new` is not available in the {} profile: a dedicated worker needs a Lua state of its own; `spawn`, `Worker.run` and `Worker.map` run their jobs in this state",
                            profile.name()
                        ),
                    ));
                }
                return;
            }
            if !profile.is_gmod() {
                return;
            }
            let crate::prelude::Access::LuaGlobal(global) = p.access else { return };
            if global == "require" {
                check_require(node, out);
                return;
            }
            let field = field_of_parent();
            if let Some(why) = gmod_missing(global, field.as_ref().map(|(_, f)| f.as_str())) {
                let at = match &field {
                    // A missing library is reported at its name; a missing
                    // function at the whole `Lib.f`.
                    Some((parent, _)) if gmod_missing(global, None).is_none() => parent.clone(),
                    _ => node.clone(),
                };
                out.push(error(&at, CODE_GMOD, format!("`{}` does not exist in Garry's Mod: {why}", at.text().to_string().trim())));
            }
        }
        Some(Ref::Declared(id)) if profile.is_gmod() => {
            let Some(item) = decls.get(*id) else { return };
            if !matches!(item.kind, DeclItemKind::Fn | DeclItemKind::Let | DeclItemKind::Const) {
                return;
            }
            if let Some(realm) = doc_realm(&item.node) {
                if !realm.on_client() {
                    out.push(error(node, CODE_REALM, realm.message(&format!("`{}`", item.name))));
                }
            }
        }
        _ => {}
    }
}

/// `require("ffi")` and the other modules of LuaJIT that gmod lacks, for a
/// call of `require` (the name `node`, or the Lua path) with a literal
/// string.
fn check_require(callee: &SyntaxNode, out: &mut Vec<SemaDiagnostic>) {
    let call = match callee.parent() {
        Some(p) if matches!(p.kind(), CALL_EXPR | COMMAND_CALL) && p.children().next().as_ref() == Some(callee) => p,
        _ => return,
    };
    let Some(args) = call.children().find(|c| matches!(c.kind(), ARG_LIST | COMMAND_ARGS)) else { return };
    check_require_args(&call, &args, out);
}

fn check_require_args(at: &SyntaxNode, args: &SyntaxNode, out: &mut Vec<SemaDiagnostic>) {
    let Some(arg) = args.children().next().filter(|a| a.kind() == LITERAL) else { return };
    let Some(tok) = arg.first_token().filter(|t| t.kind() == STRING) else { return };
    let name = crate::resolver::decode_plain_string(&tok);
    if let Some(why) = gmod_missing_module(&name) {
        out.push(error(at, CODE_GMOD, format!("`require(\"{name}\")` fails in Garry's Mod: {why}")));
    }
}

fn check_lua_path(path: &SyntaxNode, res: &Resolution, index: &HashMap<String, Realm>, out: &mut Vec<SemaDiagnostic>) {
    let Some(first) = crate::resolver::lua_path_root(path) else { return };
    let Some(Ref::LuaGlobal(global)) = res.reference(first.text_range()) else { return };
    // The leading fields: `:util.AddNetworkString` is "util", then
    // "util.AddNetworkString".
    let mut fields = Vec::new();
    let mut steps = path.children().peekable();
    while let Some(s) = steps.next_if(|s| s.kind() == LUA_FIELD) {
        match tokens(&s).find(|t| t.kind().is_name() || t.kind().is_keyword()) {
            Some(t) => fields.push(t.text().to_owned()),
            None => break,
        }
    }
    if global == "require" {
        if let Some(call) = path.children().next().filter(|s| s.kind() == LUA_CALL) {
            if let Some(args) = child(&call, ARG_LIST) {
                check_require_args(path, &args, out);
            }
        }
        return;
    }
    if let Some(why) = gmod_missing(global, fields.first().map(String::as_str)) {
        let shown = match fields.first() {
            Some(f) if gmod_missing(global, None).is_none() => format!(":{global}.{f}"),
            _ => format!(":{global}"),
        };
        out.push(error(path, CODE_GMOD, format!("`{shown}` does not exist in Garry's Mod: {why}")));
        return;
    }
    let mut key = global.clone();
    for f in std::iter::once(None).chain(fields.iter().map(Some)) {
        if let Some(f) = f {
            key.push('.');
            key.push_str(f);
        }
        if let Some(realm) = index.get(&key) {
            if !realm.on_client() {
                out.push(error(path, CODE_REALM, realm.message(&format!("`:{key}`"))));
                return;
            }
        }
    }
}

/// The diagnostics of the members of declared types and modules that the
/// file reads and that the realm of `profile` lacks, from what the
/// analyzer recorded. The members of gmod's tables of hooks (`GM`,
/// `ENTITY`, …) are left alone: their realm says where the game calls
/// them, and a file may define or call one in any realm.
pub fn check_members(root: &SyntaxNode, res: &Resolution, info: &TypeInfo, decls: &Declarations, profile: Profile) -> Vec<SemaDiagnostic> {
    let mut out = Vec::new();
    if !profile.is_gmod() {
        return out;
    }
    let dead = dead_ranges(root, res, profile);
    let mut members: Vec<(&TextRange, &(NamedId, String))> = info.members.iter().collect();
    members.sort_by_key(|(r, _)| r.start());
    for (range, (owner, name)) in members {
        if is_dead(*range, &dead) {
            continue;
        }
        let Some((top, member)) = decl_member(info, decls, *owner, name) else { continue };
        if doc_comment_text(&top).is_some_and(|d| d.contains("table of hooks")) {
            continue;
        }
        let Some(realm) = doc_realm(&member) else { continue };
        if realm.on_client() {
            continue;
        }
        let shown = format!("`{}.{name}`", info.types.name(*owner));
        out.push(SemaDiagnostic { range: *range, code: CODE_REALM, message: realm.message(&shown), error: true, lint: false });
    }
    out
}

/// The declaration of member `name` of `owner` or of one of its parents:
/// the top-level item it is in, and the member's node.
fn decl_member(info: &TypeInfo, decls: &Declarations, owner: NamedId, name: &str) -> Option<(SyntaxNode, SyntaxNode)> {
    let mut cur = Some(owner);
    for _ in 0..64 {
        let c = cur?;
        if let Some((id, range)) = info.decl_nodes.get(&c) {
            let item = decls.get(*id)?;
            let node = if item.node.text_range() == *range { Some(item.node.clone()) } else { item.node.descendants().find(|n| n.text_range() == *range) };
            if let Some(node) = node {
                let found = node.children().find(|m| is_member(m) && member_name(m).as_deref() == Some(name));
                if let Some(m) = found {
                    return Some((item.node.clone(), m));
                }
            }
        }
        cur = info.types.parent(c);
    }
    None
}

fn is_member(m: &SyntaxNode) -> bool {
    matches!(m.kind(), DECL_FN | DECL_FIELD | DECL_VALUE | DECL_MODULE | DECL_STRUCT | DECL_TYPE)
}

/// The name a member of a declaration declares.
fn member_name(m: &SyntaxNode) -> Option<String> {
    child(m, NAME).and_then(|n| name_text(&n))
}

/// The Lua key of a member: its `as 'key'`, or its name.
fn member_key(m: &SyntaxNode) -> Option<String> {
    match child(m, LUA_KEY).and_then(|k| token(&k, STRING)) {
        Some(t) => Some(crate::resolver::decode_plain_string(&t)),
        None => member_name(m),
    }
}

/// The realm of every declared value that has one, by its Lua path
/// (`util.AddNetworkString`), for the Lua paths of a file.
fn lua_path_realms(decls: &Declarations) -> HashMap<String, Realm> {
    fn path_of(root: &LuaRoot) -> Option<String> {
        let mut s = root.global.clone();
        for step in &root.steps {
            let LuaStep::Field(f) = step else { return None };
            s.push('.');
            s.push_str(f);
        }
        Some(s)
    }
    fn members(node: &SyntaxNode, base: &str, out: &mut HashMap<String, Realm>) {
        for m in node.children().filter(is_member) {
            let Some(key) = member_key(&m) else { continue };
            let path = format!("{base}.{key}");
            if let Some(r) = doc_realm(&m) {
                out.insert(path.clone(), r);
            }
            if m.kind() == DECL_MODULE {
                members(&m, &path, out);
            }
        }
    }
    let mut out = HashMap::new();
    for item in &decls.items {
        let Some(path) = item.root.as_ref().and_then(path_of) else { continue };
        if let Some(r) = doc_realm(&item.node) {
            out.insert(path.clone(), r);
        }
        if item.kind == DeclItemKind::Module {
            members(&item.node, &path, &mut out);
        }
    }
    out
}

// ---- Realms ---------------------------------------------------------

/// The realm of a declaration, from its doc comment (`Realm: server.`),
/// with the address of its page when the comment has one.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Realm {
    /// As written: `server`, `client and menu`, `shared`, …
    text: String,
    url: Option<String>,
}

impl Realm {
    fn on_client(&self) -> bool {
        self.text.contains("client") || self.text.contains("shared")
    }

    fn message(&self, shown: &str) -> String {
        let only = match self.text.as_str() {
            "server" => "exists only on the server",
            "menu" => "exists only in the main menu",
            _ => "does not exist on the client",
        };
        let mut m = format!("{shown} {only} in Garry's Mod (realm: {}), and this file is compiled for the client realm", self.text);
        if self.text.contains("server") {
            m.push_str("; code under `if SERVER` is left out of a client build");
        }
        if let Some(url) = &self.url {
            m.push_str(" (");
            m.push_str(url);
            m.push(')');
        }
        m
    }
}

fn doc_comment_text(node: &SyntaxNode) -> Option<String> {
    nl_syntax::doc_comment(node)
}

fn doc_realm(node: &SyntaxNode) -> Option<Realm> {
    let doc = doc_comment_text(node)?;
    let at = doc.find("Realm: ")? + "Realm: ".len();
    let rest = doc.get(at..)?;
    let text = rest.split(['.', '\n']).next()?.trim().to_owned();
    if text.is_empty() {
        return None;
    }
    let url = doc.split_whitespace().find(|w| w.starts_with("https://")).map(|w| w.trim_end_matches(['.', ')']).to_owned());
    Some(Realm { text, url })
}

fn error(node: &SyntaxNode, code: &'static str, message: String) -> SemaDiagnostic {
    SemaDiagnostic { range: node.text_range(), code, message, error: true, lint: false }
}
