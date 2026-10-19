//! Writing the declaration file.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use crate::extra::{Extra, ExtraItem, member_name};
use crate::names::{self, Override, Overrides};
use crate::source::{Api, Arg, Callback, ClassKind, Func, FuncKind};
use crate::types::{TypeNames, optional};

/// A realm a file is generated for (`--realm`); `None` declares everything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Server,
    Client,
    Menu,
}

impl Side {
    pub fn parse(s: &str) -> Option<Side> {
        match s {
            "server" => Some(Side::Server),
            "client" => Some(Side::Client),
            "menu" => Some(Side::Menu),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Side::Server => "server",
            Side::Client => "client",
            Side::Menu => "menu",
        }
    }
}

/// True when an item of the wiki's realm (`shared`, `client and menu`, …)
/// exists in `side`. An item without a realm is kept.
fn in_realm(realm: Option<&str>, side: Option<Side>) -> bool {
    let (Some(realm), Some(side)) = (realm, side) else { return true };
    let words: Vec<&str> = realm.split(|c: char| !c.is_ascii_alphabetic()).collect();
    let has = |w: &str| words.contains(&w);
    match side {
        Side::Server => has("server") || has("shared"),
        Side::Client => has("client") || has("shared"),
        Side::Menu => has("menu"),
    }
}

/// What the generator saw and did, for its report.
#[derive(Debug, Default)]
pub struct Stats {
    pub classes: usize,
    pub class_methods: usize,
    pub panels: usize,
    pub panel_methods: usize,
    pub hook_tables: usize,
    pub hooks: usize,
    pub libraries: usize,
    pub library_functions: usize,
    pub globals: usize,
    pub enums: usize,
    pub enum_constants: usize,
    pub structures: usize,
    /// Items left out, with the reason.
    pub skipped: Vec<String>,
    /// Wiki type names with no declaration, typed `any`.
    pub unknown_types: BTreeSet<String>,
}

pub struct Generated {
    pub text: String,
    pub stats: Stats,
}

/// The tables of hooks whose fields a structure of the wiki lists.
const HOOK_FIELDS: &[(&str, &str)] = &[("GM", "GM"), ("ENTITY", "ENT"), ("WEAPON", "SWEP"), ("TOOL", "TOOL"), ("PLAYER", "PLAYER")];

/// One member line, with its doc comment.
struct Member {
    doc: Option<String>,
    line: String,
}

/// A `declare type|struct|module` block.
struct Container {
    doc: Option<String>,
    header: String,
    members: Vec<Member>,
}

struct Gen<'a> {
    ov: &'a Overrides,
    side: Option<Side>,
    /// Lua class name → NL type name.
    class_names: BTreeMap<String, String>,
    /// Top-level NL names, to keep them unique and clear of the built-in
    /// declarations.
    taken: BTreeSet<String>,
    stats: Stats,
}

/// The wiki's realm and deprecation, and the page, as a doc comment.
fn doc(realm: Option<&str>, deprecated: bool, address: &str) -> String {
    let mut s = String::new();
    if let Some(r) = realm {
        let _ = write!(s, "Realm: {r}. ");
    }
    if deprecated {
        s.push_str("Deprecated. ");
    }
    let _ = write!(s, "https://wiki.facepunch.com/gmod/{address}");
    s
}

impl<'a> Gen<'a> {
    fn types(&self) -> TypeNames<'_> {
        TypeNames { classes: &self.class_names }
    }

    fn note_unknown(&mut self, ty: &str) {
        // Leave out the tags: `number{DMG}`, `table{TraceResult}`.
        let mut plain = String::new();
        let mut depth = 0;
        for c in ty.chars() {
            match c {
                '{' => depth += 1,
                '}' => depth -= 1,
                _ if depth == 0 => plain.push(c),
                _ => {}
            }
        }
        for part in plain.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '/')) {
            if part.is_empty() || part.starts_with("Structures/") {
                continue;
            }
            let known = matches!(part, "number" | "string" | "boolean" | "bool" | "table" | "function" | "any" | "vararg" | "nil" | "NULL" | "thread" | "userdata") || self.class_names.contains_key(part);
            if !known {
                self.stats.unknown_types.insert(part.to_string());
            }
        }
    }

    fn map_type(&mut self, ty: &str) -> String {
        self.note_unknown(ty);
        self.types().map(ty)
    }

    /// `Function(a: A, b: B): R`; a callback whose returns the wiki does not
    /// list returns `any`, since Lua drops what it returns.
    fn callback_type(&mut self, cb: &Callback) -> String {
        let (params, _) = self.params(std::slice::from_ref(&cb.args), false);
        let ret = self.returns(&cb.rets).unwrap_or_else(|| "any".to_string());
        format!("Function({params}): {ret}")
    }

    fn arg_type(&mut self, a: &Arg) -> String {
        match &a.callback {
            Some(cb) if a.ty.trim() == "function" => self.callback_type(cb),
            _ => self.map_type(&a.ty),
        }
    }

    /// The parameter list of a function with one or more forms: position by
    /// position, the union of the types, optional where a form leaves it
    /// out; `vararg` is the rest parameter.
    fn params(&mut self, overloads: &[Vec<Arg>], method: bool) -> (String, bool) {
        let len = overloads.iter().map(Vec::len).max().unwrap_or(0);
        let mut out: Vec<String> = Vec::new();
        if method {
            out.push("self".to_string());
        }
        let mut used: BTreeSet<String> = BTreeSet::from(["self".to_string()]);
        let mut rest = false;
        for i in 0..len {
            let at: Vec<&Arg> = overloads.iter().filter_map(|o| o.get(i)).collect();
            let mut snakes: Vec<String> = Vec::new();
            for a in &at {
                let s = names::snake(a.name.trim_matches('.'));
                if !s.is_empty() && !snakes.contains(&s) {
                    snakes.push(s);
                }
            }
            let mut name = match snakes.as_slice() {
                [] => format!("arg{}", i + 1),
                [one] => one.clone(),
                [a, b] => format!("{}_or_{}", a.trim_end_matches('_'), b.trim_end_matches('_')),
                [first, ..] => first.clone(),
            };
            if names::is_keyword(&name) {
                name.push('_');
            }
            if at.iter().any(|a| a.ty.trim() == "vararg" || a.name == "...") {
                if name == "arg1" || name.is_empty() || name.starts_with("arg") {
                    name = "args".to_string();
                }
                while used.contains(&name) {
                    name.push('_');
                }
                out.push(format!("*{name}: any"));
                rest = true;
                break;
            }
            while used.contains(&name) {
                name = format!("{name}{}", i + 1);
            }
            used.insert(name.clone());
            let mut tys: Vec<String> = Vec::new();
            for a in &at {
                let t = self.arg_type(a);
                if !tys.contains(&t) {
                    tys.push(t);
                }
            }
            let ty = if tys.iter().any(|t| t == "any") || tys.is_empty() {
                "any".to_string()
            } else {
                tys.join(" | ")
            };
            let opt = at.len() < overloads.len() || at.iter().any(|a| a.optional);
            let ty = if opt { optional(&ty) } else { ty };
            out.push(format!("{name}: {ty}"));
        }
        (out.join(", "), rest)
    }

    /// The return type: none, one, or `(A, B)`; a `vararg` return is `any`.
    fn returns(&mut self, rets: &[Arg]) -> Option<String> {
        let mut tys: Vec<String> = Vec::new();
        for r in rets {
            if r.ty.trim() == "vararg" {
                tys.push("any".to_string());
                break;
            }
            let t = self.arg_type(r);
            tys.push(t);
        }
        match tys.len() {
            0 => None,
            1 => tys.pop(),
            _ => Some(format!("({})", tys.join(", "))),
        }
    }

    /// `fn name(params): ret`, without the Lua key.
    fn fn_sig(&mut self, f: &Func, method: bool) -> String {
        let (params, _) = self.params(&f.overloads, method);
        let mut s = String::new();
        if params.is_empty() {
            s.push_str("()");
        } else {
            let _ = write!(s, "({params})");
        }
        if let Some(r) = self.returns(&f.rets) {
            let _ = write!(s, ": {r}");
        }
        s
    }

    fn claim(&mut self, name: &str, what: &str) -> bool {
        if self.taken.insert(name.to_string()) {
            true
        } else {
            self.stats.skipped.push(format!("{what}: the NL name `{name}` is taken"));
            false
        }
    }

    /// The member functions of a container, as lines, sorted by Lua name
    /// and with unique NL names (the first in Lua order keeps a name).
    fn fn_members(&mut self, funcs: &[&Func], method: bool, used: &mut BTreeSet<String>) -> Vec<Member> {
        let mut out = Vec::new();
        for f in funcs {
            if !in_realm(f.realm.as_deref(), self.side) {
                continue;
            }
            // `jit.opt.start` belongs to the library `jit.opt`.
            let lua = f.name.rsplit('.').next().unwrap_or(&f.name).to_string();
            let nl = match self.ov.get(&f.address) {
                Some(Override::Skip) => {
                    self.stats.skipped.push(format!("{}: listed as left out", f.address));
                    continue;
                }
                Some(Override::Name(n)) => n.to_string(),
                None => names::snake(&lua),
            };
            if !names::is_lname(&nl) {
                self.stats.skipped.push(format!("{}: `{nl}` is not a snake_case name", f.address));
                continue;
            }
            if !used.insert(nl.clone()) {
                self.stats.skipped.push(format!("{}: the NL name `{nl}` is taken in its parent", f.address));
                continue;
            }
            let sig = self.fn_sig(f, method);
            let key = if nl == lua { String::new() } else { format!(" as '{lua}'") };
            out.push(Member { doc: Some(doc(f.realm.as_deref(), f.deprecated, &f.address)), line: format!("fn {nl}{sig}{key}") });
            match f.kind {
                FuncKind::Library => self.stats.library_functions += 1,
                FuncKind::Class => self.stats.class_methods += 1,
                FuncKind::Panel => self.stats.panel_methods += 1,
                FuncKind::Hook => self.stats.hooks += 1,
            }
        }
        out
    }
}

/// The top-level names already declared: the prelude's and those of the
/// built-in declaration files.
pub fn builtin_names() -> BTreeSet<String> {
    let mut names: BTreeSet<String> = nl_sema::prelude::all().iter().map(|p| p.name.to_string()).collect();
    for item in nl_sema::prelude_declarations().items {
        names.insert(item.name);
    }
    names
}

/// For each Lua library table the built-in declarations describe (`math`
/// as `Number`), the keys they declare.
pub fn builtin_library_keys() -> BTreeMap<String, BTreeSet<String>> {
    use nl_sema::util::{child, name_text};
    use nl_syntax::SyntaxKind::*;
    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for item in nl_sema::prelude_declarations().items {
        let Some(root) = item.root.as_ref().filter(|r| r.steps.is_empty()) else { continue };
        let keys = out.entry(root.global.clone()).or_default();
        for m in item.node.children() {
            let Some(name) = child(&m, NAME).and_then(|n| name_text(&n)) else { continue };
            let key = child(&m, LUA_KEY)
                .and_then(|k| k.children_with_tokens().filter_map(|t| t.into_token()).find(|t| t.kind() == STRING))
                .map(|t| nl_sema::decode_plain_string(&t))
                .unwrap_or(name);
            keys.insert(key);
        }
    }
    out
}

pub fn generate(api: &Api, ov: &Overrides, extra: &Extra, side: Option<Side>) -> Generated {
    let mut g = Gen { ov, side, class_names: BTreeMap::new(), taken: builtin_names(), stats: Stats { structures: api.structs.len(), ..Stats::default() } };
    let library_keys = builtin_library_keys();

    // Class names first: types refer to them.
    let mut class_order: Vec<(&str, String)> = Vec::new();
    for c in api.classes.values() {
        let nl = match ov.get(&c.address) {
            Some(Override::Skip) => {
                g.stats.skipped.push(format!("{}: listed as left out", c.address));
                continue;
            }
            Some(Override::Name(n)) => n.to_string(),
            None => names::pascal(&c.name),
        };
        if !names::is_cname(&nl) {
            g.stats.skipped.push(format!("{}: `{nl}` is not a capitalized name", c.address));
            continue;
        }
        if !g.claim(&nl, &c.address) {
            continue;
        }
        g.class_names.insert(c.name.clone(), nl.clone());
        if c.address != c.name {
            g.class_names.entry(c.address.clone()).or_insert_with(|| nl.clone());
        }
        class_order.push((c.name.as_str(), nl));
    }

    let mut containers: Vec<(u8, String, Container)> = Vec::new();

    // Classes, panels and hook tables.
    for (lua, nl) in &class_order {
        let Some(c) = api.classes.get(*lua) else { continue };
        let funcs: Vec<&Func> = api.funcs.iter().filter(|f| f.parent == c.name && matches!(f.kind, FuncKind::Class | FuncKind::Panel | FuncKind::Hook)).collect();
        let parent = match (&c.parent, c.kind) {
            (Some(p), _) => g.class_names.get(p).cloned(),
            (None, ClassKind::Panel) => g.class_names.get("Panel").cloned(),
            (None, _) => None,
        }
        .filter(|p| p != nl);
        let mut header = format!("declare type {nl}");
        if let Some(p) = &parent {
            let _ = write!(header, " : {p}");
        }
        let mut used = BTreeSet::new();
        let mut members = g.fn_members(&funcs, true, &mut used);
        // A table of hooks also holds the fields its structure lists
        // (`GM.Name`, `ENT.PrintName`).
        if let Some(fields) = HOOK_FIELDS.iter().find(|(t, _)| *t == c.name).and_then(|(_, s)| api.structs.get(*s)) {
            for f in fields {
                let nl = names::snake(&f.name);
                if !names::is_lname(&nl) || !used.insert(nl.clone()) {
                    continue;
                }
                let ty = g.arg_type(f);
                let key = if nl == f.name { String::new() } else { format!(" as '{}'", f.name) };
                members.push(Member { doc: None, line: format!("{nl}: {ty}{key}") });
            }
        }
        let (order, what) = match c.kind {
            ClassKind::Class => {
                g.stats.classes += 1;
                (0, "class")
            }
            ClassKind::Panel => {
                g.stats.panels += 1;
                (1, "derma panel class")
            }
            ClassKind::Hooks => {
                g.stats.hook_tables += 1;
                (2, "table of hooks")
            }
        };
        let mut d = format!("gmod's {what} `{}`", c.name);
        if c.deprecated {
            d.push_str(". Deprecated");
        }
        let _ = write!(d, ". https://wiki.facepunch.com/gmod/{}", c.address);
        containers.push((order, nl.clone(), Container { doc: Some(d), header, members }));
    }

    // Libraries, with the nested ones (`jit.opt`) as libraries of their own.
    let mut libs: BTreeMap<String, Vec<&Func>> = BTreeMap::new();
    let mut globals: Vec<&Func> = Vec::new();
    for f in api.funcs.iter().filter(|f| f.kind == FuncKind::Library) {
        if f.parent == "_G" {
            globals.push(f);
            continue;
        }
        let mut path = f.parent.clone();
        if let Some((inner, _)) = f.name.rsplit_once('.') {
            path = format!("{path}.{inner}");
        }
        libs.entry(path).or_default().push(f);
    }
    for (path, funcs) in &libs {
        let top = path.split('.').next().unwrap_or(path);
        let lib = api.libraries.get(path.as_str());
        let address = lib.map(|l| l.address.clone()).unwrap_or_else(|| path.clone());
        let already = library_keys.get(path.as_str());
        let funcs: Vec<&Func> = funcs.iter().copied().filter(|f| !already.is_some_and(|k| k.contains(f.name.as_str()))).collect();
        if funcs.is_empty() {
            continue;
        }
        let nl = match ov.get(&address) {
            Some(Override::Skip) => {
                g.stats.skipped.push(format!("{address}: listed as left out"));
                continue;
            }
            Some(Override::Name(n)) => n.to_string(),
            None => {
                let base = names::pascal(&path.replace('.', "_"));
                if g.taken.contains(&base) { format!("{base}Lib") } else { base }
            }
        };
        if !names::is_cname(&nl) || !g.claim(&nl, &address) {
            continue;
        }
        let mut used = BTreeSet::new();
        let members = g.fn_members(&funcs, false, &mut used);
        if members.is_empty() {
            g.taken.remove(&nl);
            continue;
        }
        g.stats.libraries += 1;
        let mut d = format!("gmod's library `{path}`");
        if lib.is_some_and(|l| l.deprecated) {
            d.push_str(". Deprecated");
        }
        if already.is_some() {
            d.push_str(", the functions the built-in declarations leave out");
        }
        let _ = write!(d, ". https://wiki.facepunch.com/gmod/{}", lib.map(|l| l.address.as_str()).unwrap_or(top));
        containers.push((3, nl.clone(), Container { doc: Some(d), header: format!("declare module {nl} = :{path}"), members }));
    }

    // Extra members (fields and operators the wiki does not list, a struct
    // header in place of a host type's) go into the generated containers.
    let mut extra_items: Vec<&ExtraItem> = Vec::new();
    for item in &extra.items {
        match containers.iter_mut().find(|(_, n, _)| *n == item.name) {
            Some((_, _, c)) if item.is_block => {
                if let Some(h) = &item.header {
                    c.header = h.clone();
                }
                if item.doc.is_some() {
                    c.doc = item.doc.clone();
                }
                let names_in: BTreeSet<String> = item.members.iter().filter_map(|m| m.name.clone()).collect();
                c.members.retain(|m| !member_name(&m.line).is_some_and(|n| names_in.contains(&n)));
                let mut ms: Vec<Member> = item.members.iter().map(|m| Member { doc: m.doc.clone(), line: m.line.clone() }).collect();
                ms.append(&mut c.members);
                c.members = ms;
            }
            _ => extra_items.push(item),
        }
    }
    containers.sort_by(|a, b| (a.0, &a.1).cmp(&(b.0, &b.1)));

    let mut out = String::new();
    out.push_str(&header(api, side));
    let sections = ["Classes", "Derma panel classes", "Tables of hooks", "Libraries"];
    let mut last = u8::MAX;
    for (order, _, c) in &containers {
        if *order != last {
            last = *order;
            let _ = write!(out, "\n# ---- {} ----\n", sections.get(usize::from(*order)).unwrap_or(&""));
        }
        out.push('\n');
        if let Some(d) = &c.doc {
            let _ = writeln!(out, "## {d}");
        }
        let _ = writeln!(out, "{}", c.header);
        for m in &c.members {
            if let Some(d) = &m.doc {
                let _ = writeln!(out, "  ## {d}");
            }
            let _ = writeln!(out, "  {}", m.line);
        }
        out.push_str("end\n");
    }

    // Global functions.
    out.push_str("\n# ---- Global functions ----\n\n");
    for f in globals {
        if !in_realm(f.realm.as_deref(), side) {
            continue;
        }
        let nl = match ov.get(&f.address) {
            Some(Override::Skip) => {
                g.stats.skipped.push(format!("{}: listed as left out", f.address));
                continue;
            }
            Some(Override::Name(n)) => n.to_string(),
            None => names::snake(&f.name),
        };
        if !names::is_lname(&nl) {
            g.stats.skipped.push(format!("{}: `{nl}` is not a snake_case name", f.address));
            continue;
        }
        if !g.claim(&nl, &f.address) {
            continue;
        }
        let sig = g.fn_sig(f, false);
        let root = if nl == f.name { String::new() } else { format!(" = :{}", f.name) };
        let _ = writeln!(out, "## {}", doc(f.realm.as_deref(), f.deprecated, &f.address));
        let _ = writeln!(out, "declare fn {nl}{sig}{root}");
        g.stats.globals += 1;
    }

    // Values and anything else the extra file declares.
    if !extra_items.is_empty() {
        out.push_str("\n# ---- Global values ----\n");
        for item in extra_items {
            if !g.claim(&item.name, "extra.d.nl") {
                continue;
            }
            if item.is_block {
                out.push('\n');
            }
            out.push_str(&item.text);
        }
    }

    // Enums: constants named as in gmod, or in SCREAMING_SNAKE_CASE when
    // gmod's name is not a capitalized NL name; those kept in a table
    // (`COND.NONE`) are the members of a module.
    out.push_str("\n# ---- Enums ----\n");
    for e in &api.enums {
        if !in_realm(e.realm.as_deref(), side) || e.items.is_empty() {
            continue;
        }
        g.stats.enums += 1;
        let realm = e.realm.as_deref().map(|r| format!(", realm: {r}")).unwrap_or_default();
        let _ = writeln!(out, "\n# Enums/{}{realm}", e.name);
        let mut tables: BTreeMap<&str, Vec<(String, &str)>> = BTreeMap::new();
        for item in &e.items {
            if let Some((table, key)) = item.key.split_once('.') {
                tables.entry(table).or_default().push((names::screaming(key), key));
                continue;
            }
            let nl = match ov.get(&item.key) {
                Some(Override::Skip) => continue,
                Some(Override::Name(n)) => n.to_string(),
                None => names::screaming(&item.key),
            };
            if !names::is_cname(&nl) || !g.claim(&nl, &item.key) {
                continue;
            }
            let root = if nl == item.key { String::new() } else { format!(" = :{}", item.key) };
            let _ = writeln!(out, "declare const {nl}: Number{root}");
            g.stats.enum_constants += 1;
        }
        for (table, keys) in tables {
            let nl = names::pascal(table);
            if !g.claim(&nl, table) {
                continue;
            }
            let _ = writeln!(out, "declare module {nl} = :{table}");
            let mut used = BTreeSet::new();
            for (name, key) in keys {
                if !used.insert(name.clone()) {
                    continue;
                }
                let k = if name == key { String::new() } else { format!(" as '{key}'") };
                let _ = writeln!(out, "  const {name}: Number{k}");
                g.stats.enum_constants += 1;
            }
            out.push_str("end\n");
        }
    }
    for u in ov.unused() {
        g.stats.skipped.push(format!("names.txt: `{u}` matches nothing in the wiki data"));
    }
    Generated { text: out, stats: g.stats }
}


fn header(api: &Api, side: Option<Side>) -> String {
    let date = api.last_update.as_deref().unwrap_or("unknown");
    let realm = match side {
        Some(s) => format!("\n#\n# This file declares only what exists in the {} realm.", s.name()),
        None => String::new(),
    };
    format!(
        "\
# Garry's Mod's Lua API (GLua) under NL names, for the analyzer.
#
# Generated by crates/nl_glua_decl from the wiki data of glua-api-snippets
# (https://github.com/luttje/glua-api-snippets, Copyright 2023-2026 Luttje,
# MIT License), which scrapes the Garry's Mod wiki
# (https://wiki.facepunch.com/gmod/). Wiki data of {date}.
# Do not edit this file: change crates/nl_glua_decl/names.txt or
# crates/nl_glua_decl/extra.d.nl and generate it again.{realm}
#
# Names. Functions, methods and parameters are snake_case (`GetPos` is
# `get_pos`, `SteamID64` is `steam_id64`, a keyword gets a trailing `_`:
# `Entity:Spawn` is `spawn_`), libraries PascalCase (`player_manager` is
# `PlayerManager`); a library whose name is taken gets `Lib` (`player` is
# `PlayerLib`, `math` is `MathLib`). Classes keep gmod's names, without
# underscores (`DListView_Column` is `DListViewColumn`), unless a built-in
# declaration has the name (`File` is `FileHandle`, `Task` is `NPCTask`).
# Enum constants keep gmod's names unless a name is not a capitalized NL
# name. names.txt overrides any of these.
#
# Types. `number`, `string`, `boolean`: `Number`, `String`, `Bool`; an enum
# is `Number`; a structure (`TraceResult`) is `Table`; `table<T>` is `[T]`;
# a class is its type; a parameter with a default is optional (`T?`); a
# callback is a `Function(...)` whose result is `any` when the wiki lists
# none; `any`, `vararg` and names the wiki does not describe are `any`.
#
# Realms. Each item's doc comment gives its realm (server, client, shared,
# menu) and its wiki page.
#
# Classes are host types: a class's parent (`Player : Entity`) gives its
# methods to the child. Derma panel classes (`DFrame`) are host types
# under `Panel`, though every panel carries gmod's one Panel metatable,
# and the tables of hooks (`GM`, `ENTITY`, `WEAPON`, `PANEL`, ...) are
# host types too, though they are Lua tables: these types are for the
# analyzer, and a runtime check against one fails unless the host
# supports it.
#
# Tables of hooks also hold the fields of their wiki structure
# (`GAMEMODE.name`, `ENT.PrintName` as `print_name`).
#
# Not declared, because a declaration file cannot say it:
# - gmod's hooks as events: `on PlayerSpawn do |ply| ... end` gets no
#   check of the event name and no parameter types. The hooks are methods
#   of the tables of hooks instead (`GAMEMODE.player_spawn(ply, false)`).
# - the fields of structures, which would need type aliases.
# - that a Lua class implements an interface.
"
    )
}
