//! Reading the wiki data: the JSON release of glua-api-snippets, one
//! directory per wiki section, each file an array of entries.
//!
//! Only the facts an API declaration needs are kept: names, parents,
//! realms, argument and return types, enum values. The wiki's prose is
//! not read.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::Value;

/// One argument of a function or callback.
#[derive(Clone, Debug, PartialEq)]
pub struct Arg {
    pub name: String,
    /// The wiki's type, such as `number`, `Entity`, `table<Player>`,
    /// `number{DMG}` or `vararg`.
    pub ty: String,
    /// The wiki gives a default value: the argument may be left out.
    pub optional: bool,
    pub callback: Option<Callback>,
}

/// The signature of a function passed or returned.
#[derive(Clone, Debug, PartialEq)]
pub struct Callback {
    pub args: Vec<Arg>,
    pub rets: Vec<Arg>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum FuncKind {
    /// A function of a library table, or a global (`parent` `_G`).
    Library,
    /// A method of a class (`Entity:GetPos`).
    Class,
    /// A method of a derma panel class (`DFrame:SetTitle`).
    Panel,
    /// A hook: a method of a table the game calls (`GM:PlayerSpawn`).
    Hook,
}

#[derive(Clone, Debug)]
pub struct Func {
    pub kind: FuncKind,
    /// The library, class, panel or hook table.
    pub parent: String,
    pub name: String,
    /// The wiki's name of the page (`Entity:GetPos`, `Global.Vector`).
    pub address: String,
    pub realm: Option<String>,
    pub deprecated: bool,
    /// The argument lists of each form of the function.
    pub overloads: Vec<Vec<Arg>>,
    pub rets: Vec<Arg>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClassKind {
    /// A class of the game (`Entity`, `Vector`).
    Class,
    /// A derma panel class (`DFrame`).
    Panel,
    /// A table of hooks (`GM`, `ENTITY`).
    Hooks,
}

#[derive(Clone, Debug)]
pub struct Class {
    pub name: String,
    pub address: String,
    pub parent: Option<String>,
    pub kind: ClassKind,
    pub deprecated: bool,
}

#[derive(Clone, Debug)]
pub struct Library {
    pub address: String,
    pub deprecated: bool,
}

#[derive(Clone, Debug)]
pub struct EnumItem {
    pub key: String,
}

#[derive(Clone, Debug)]
pub struct Enum {
    pub name: String,
    pub realm: Option<String>,
    pub items: Vec<EnumItem>,
}

/// The whole API, in a fixed order (the maps are sorted by name, the
/// functions by kind, parent and name).
#[derive(Debug, Default)]
pub struct Api {
    pub libraries: BTreeMap<String, Library>,
    pub classes: BTreeMap<String, Class>,
    pub funcs: Vec<Func>,
    pub enums: Vec<Enum>,
    /// The structures (tables of named fields) the wiki describes, with
    /// their fields; a value of one is a `Table`.
    pub structs: BTreeMap<String, Vec<Arg>>,
    /// The wiki's date, from `__metadata.json`.
    pub last_update: Option<String>,
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_string)
}

fn realm(v: &Value) -> Option<String> {
    text(v, "realm").map(|r| r.trim().to_lowercase()).filter(|r| !r.is_empty())
}

fn deprecated(v: &Value) -> bool {
    match v.get("deprecated") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(_) => true,
    }
}

fn arg(v: &Value) -> Arg {
    Arg {
        name: text(v, "name").unwrap_or_default(),
        ty: text(v, "type").unwrap_or_else(|| "any".to_string()),
        optional: v.get("default").is_some(),
        callback: v.get("callback").map(callback),
    }
}

fn args(v: Option<&Value>) -> Vec<Arg> {
    v.and_then(Value::as_array).map(|a| a.iter().map(arg).collect()).unwrap_or_default()
}

fn callback(v: &Value) -> Callback {
    Callback { args: args(v.get("arguments")), rets: args(v.get("returns")) }
}

impl Api {
    /// Reads every `<section>/<page>.json` under `dir`, in name order.
    pub fn load(dir: &Path) -> Result<Api, String> {
        let mut api = Api::default();
        if let Ok(meta) = std::fs::read_to_string(dir.join("__metadata.json")) {
            let meta: Value = serde_json::from_str(&meta).map_err(|e| format!("__metadata.json: {e}"))?;
            api.last_update = text(&meta, "lastUpdate");
        }
        let mut sections: Vec<_> = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.is_dir()).collect();
        sections.sort();
        if sections.is_empty() {
            return Err(format!("{}: no wiki sections; pass the directory the JSON release was unpacked into", dir.display()));
        }
        for section in sections {
            let mut pages: Vec<_> = std::fs::read_dir(&section).map_err(|e| format!("{}: {e}", section.display()))?.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
            pages.sort();
            for page in pages {
                let src = std::fs::read_to_string(&page).map_err(|e| format!("{}: {e}", page.display()))?;
                let json: Value = serde_json::from_str(&src).map_err(|e| format!("{}: {e}", page.display()))?;
                let entries = match json {
                    Value::Array(a) => a,
                    other => vec![other],
                };
                for e in &entries {
                    api.add(e);
                }
            }
        }
        api.sort();
        Ok(api)
    }

    fn sort(&mut self) {
        self.funcs.sort_by(|a, b| (a.kind, &a.parent, &a.name, &a.address).cmp(&(b.kind, &b.parent, &b.name, &b.address)));
        self.funcs.dedup_by(|a, b| a.kind == b.kind && a.address == b.address);
        self.enums.sort_by(|a, b| a.name.cmp(&b.name));
    }

    /// Adds the functions of the release's Lua annotations (`<date>.lua.zip`,
    /// unpacked in `dir`) that the JSON data lacks. The JSON release writes
    /// one file per page, named by the page in lowercase, so pages whose names
    /// differ only in case or in a `library.` prefix overwrite each other
    /// (`file.Read` and `File:Read` are both `file/read.json`; every
    /// `math.ease` function is `math/ease.json`). Returns the addresses added.
    pub fn recover_from_lua(&mut self, dir: &Path) -> Result<Vec<String>, String> {
        let have: BTreeSet<(String, String)> = self.funcs.iter().map(|f| split_name(&f.parent, &f.name)).collect();
        let mut files: Vec<_> = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "lua")).collect();
        files.sort();
        if files.is_empty() {
            return Err(format!("{}: no .lua files; pass the directory the Lua release was unpacked into", dir.display()));
        }
        let mut found: BTreeMap<(String, String), Func> = BTreeMap::new();
        for file in files {
            let src = std::fs::read_to_string(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            for f in lua_functions(&src, self) {
                let key = (f.parent.clone(), f.name.clone());
                if have.contains(&key) {
                    continue;
                }
                match found.get_mut(&key) {
                    // Each form of an overloaded function is a definition.
                    Some(existing) => existing.overloads.extend(f.overloads),
                    None => {
                        found.insert(key, f);
                    }
                }
            }
        }
        let added: Vec<String> = found.values().map(|f| f.address.clone()).collect();
        self.funcs.extend(found.into_values());
        self.sort();
        Ok(added)
    }

    /// The wiki's type for a LuaLS type: `Player[]` is `table<Player>`, a
    /// signature `fun(…)` is `function`, an enum is `number`, a structure
    /// `table`.
    fn wiki_type(&self, ty: &str) -> String {
        ty.split('|')
            .map(|t| {
                let t = t.trim();
                if t.starts_with("fun(") {
                    return "function".to_string();
                }
                if let Some(inner) = t.strip_suffix("[]") {
                    return format!("table<{}>", self.wiki_type(inner));
                }
                if self.enums.iter().any(|e| e.name == t) {
                    return "number".to_string();
                }
                if self.structs.contains_key(t) {
                    return "table".to_string();
                }
                t.to_string()
            })
            .collect::<Vec<_>>()
            .join("|")
    }

    fn add(&mut self, e: &Value) {
        let Some(kind) = text(e, "type") else { return };
        let name = text(e, "name").unwrap_or_default();
        let address = text(e, "address").unwrap_or_else(|| name.clone());
        match kind.as_str() {
            "library" => {
                self.libraries.insert(name.clone(), Library { address, deprecated: deprecated(e) });
            }
            "class" | "panel" => {
                let kind = if kind == "panel" {
                    ClassKind::Panel
                } else if address.ends_with("_Hooks") {
                    ClassKind::Hooks
                } else {
                    ClassKind::Class
                };
                let parent = text(e, "parent").filter(|p| !p.is_empty());
                self.classes.insert(name.clone(), Class { name, address, parent, kind, deprecated: deprecated(e) });
            }
            "libraryfunc" | "classfunc" | "panelfunc" | "hook" => {
                let kind = match kind.as_str() {
                    "libraryfunc" => FuncKind::Library,
                    "classfunc" => FuncKind::Class,
                    "panelfunc" => FuncKind::Panel,
                    _ => FuncKind::Hook,
                };
                let overloads = e.get("arguments").and_then(Value::as_array).map(|a| a.iter().map(|o| args(o.get("args"))).collect()).unwrap_or_default();
                self.funcs.push(Func {
                    kind,
                    parent: text(e, "parent").unwrap_or_default(),
                    name,
                    address,
                    realm: realm(e),
                    deprecated: deprecated(e),
                    overloads,
                    rets: args(e.get("returns")),
                });
            }
            "enum" => {
                let items = e
                    .get("items")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(|i| Some(EnumItem { key: text(i, "key")? })).collect())
                    .unwrap_or_default();
                self.enums.push(Enum { name, realm: realm(e), items });
            }
            "struct" => {
                self.structs.insert(name, args(e.get("fields")));
            }
            _ => {}
        }
    }
}

/// `("jit", "opt.start")` is `("jit.opt", "start")`.
fn split_name(parent: &str, name: &str) -> (String, String) {
    match name.rsplit_once('.') {
        Some((inner, last)) => (format!("{parent}.{inner}"), last.to_string()),
        None => (parent.to_string(), name.to_string()),
    }
}

/// The type at the start of `s`: up to the first space outside brackets;
/// a `fun(…)` signature is the whole rest of the line.
fn lua_type(s: &str) -> &str {
    if s.starts_with("fun(") {
        return s;
    }
    let mut depth = 0i32;
    for (i, c) in s.char_indices() {
        match c {
            '(' | '<' | '{' | '[' => depth += 1,
            ')' | '>' | '}' | ']' => depth -= 1,
            c if c.is_whitespace() && depth <= 0 => return s.get(..i).unwrap_or(s),
            _ => {}
        }
    }
    s
}

/// The functions an annotated Lua file defines (`function net.Start(…) end`
/// with its `---@param`, `---@return`, `---@deprecated` and realm badge).
fn lua_functions(src: &str, api: &Api) -> Vec<Func> {
    let mut out = Vec::new();
    let mut realm: Option<String> = None;
    let mut deprecated = false;
    let mut params: Vec<Arg> = Vec::new();
    let mut rets: Vec<Arg> = Vec::new();
    for line in src.lines() {
        if let Some(rest) = line.strip_prefix("---![(") {
            realm = rest.split_once(")]").map(|(r, _)| r.trim().to_lowercase());
        } else if line.starts_with("---@deprecated") {
            deprecated = true;
        } else if let Some(rest) = line.strip_prefix("---@param ") {
            let (name, rest) = rest.split_once(' ').unwrap_or((rest, "any"));
            let (name, optional) = match name.strip_suffix('?') {
                Some(n) => (n, true),
                None => (name, false),
            };
            let ty = if name == "..." { "vararg".to_string() } else { api.wiki_type(lua_type(rest.trim_start())) };
            params.push(Arg { name: name.to_string(), ty, optional, callback: None });
        } else if let Some(rest) = line.strip_prefix("---@return ") {
            rets.push(Arg { name: String::new(), ty: api.wiki_type(lua_type(rest.trim_start())), optional: false, callback: None });
        } else if let Some(rest) = line.strip_prefix("function ") {
            let path = rest.split('(').next().unwrap_or("").trim();
            let (parent, name, method) = match path.rsplit_once(':') {
                Some((p, n)) => (p, n, true),
                None => match path.rsplit_once('.') {
                    Some((p, n)) => (p, n, false),
                    None => ("_G", path, false),
                },
            };
            let kind = match api.classes.get(parent).map(|c| c.kind) {
                _ if !method => FuncKind::Library,
                Some(ClassKind::Panel) => FuncKind::Panel,
                Some(ClassKind::Hooks) => FuncKind::Hook,
                _ => FuncKind::Class,
            };
            let address = if parent == "_G" { format!("Global.{name}") } else if method { format!("{parent}:{name}") } else { format!("{parent}.{name}") };
            out.push(Func {
                kind,
                parent: parent.to_string(),
                name: name.to_string(),
                address,
                realm: realm.take(),
                deprecated,
                overloads: vec![std::mem::take(&mut params)],
                rets: std::mem::take(&mut rets),
            });
            deprecated = false;
        } else if !line.starts_with("---") {
            realm = None;
            deprecated = false;
            params.clear();
            rets.clear();
        }
    }
    out
}
