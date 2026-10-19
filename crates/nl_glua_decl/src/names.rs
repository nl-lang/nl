//! NL names for gmod's Lua names.
//!
//! The rules are mechanical and an override list fixes what they get
//! wrong (`names.txt` beside this crate): functions, methods and
//! parameters are snake_case (`GetPos` is `get_pos`, `SteamID64` is
//! `steam_id64`), libraries and classes PascalCase (`player_manager` is
//! `PlayerManager`), enum constants SCREAMING_SNAKE_CASE.

use std::collections::{BTreeMap, BTreeSet};

/// NL's reserved words; a snake_case name that is one gets a trailing `_`.
const KEYWORDS: &[&str] = &[
    "and", "as", "async", "break", "case", "catch", "cond", "const", "continue", "declare", "do", "else", "elseif", "end", "enum", "false", "finally", "fn", "for", "global", "if", "import",
    "in", "interface", "is", "let", "loop", "module", "nil", "not", "on", "or", "pub", "return", "self", "spawn", "struct", "true", "try", "type", "unless", "until", "when", "while",
];

pub fn is_keyword(word: &str) -> bool {
    KEYWORDS.contains(&word)
}

/// Words kept whole when a name is split: `ReadUInt` is `read_uint`, not
/// `read_u_int`.
const WORDS: &[&str] = &["UInt", "UShort", "ULong", "GMod", "CVar", "EFlags", "EFlag", "DnD", "BBox", "PData", "BSpline"];

/// The words of a Lua name: a run of capitals before a capitalized word is
/// an acronym (`HTTPRequest`: `HTTP`, `Request`), digits stay with the word
/// before them (`SteamID64`: `Steam`, `ID64`), and a capital after digits
/// joins them when no lowercase letter follows (`Start3D2D`: `Start3D2D`).
pub fn words(name: &str) -> Vec<String> {
    let chars: Vec<char> = name.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars.get(i).copied().unwrap_or('_');
        if !c.is_ascii_alphanumeric() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            i += 1;
            continue;
        }
        // A whole word from the list.
        if c.is_ascii_uppercase() {
            if let Some(w) = WORDS.iter().find(|w| name.get(byte_index(&chars, i)..).is_some_and(|rest| rest.starts_with(**w) && !rest.get(w.len()..).is_some_and(|r| r.starts_with(|c: char| c.is_ascii_lowercase())))) {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                cur.push_str(w);
                i += w.len();
                continue;
            }
        }
        let prev = if i > 0 { chars.get(i - 1).copied() } else { None };
        let next = chars.get(i + 1).copied();
        let boundary = match prev {
            None => false,
            Some(p) if c.is_ascii_uppercase() => {
                (p.is_ascii_lowercase())
                    // The last capital of an acronym starts the next word.
                    || (p.is_ascii_uppercase() && next.is_some_and(|n| n.is_ascii_lowercase()))
                    // After digits, a capital that starts a word.
                    || (p.is_ascii_digit() && next.is_some_and(|n| n.is_ascii_lowercase()))
            }
            _ => false,
        };
        if boundary && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        cur.push(c);
        i += 1;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn byte_index(chars: &[char], i: usize) -> usize {
    chars.iter().take(i).map(|c| c.len_utf8()).sum()
}

/// `GetPos` → `get_pos`; a keyword gets a trailing `_` (`Spawn` →
/// `spawn_`); an empty name stays empty.
pub fn snake(name: &str) -> String {
    let s = words(name).iter().map(|w| w.to_ascii_lowercase()).collect::<Vec<_>>().join("_");
    let s = if s.starts_with(|c: char| c.is_ascii_digit()) { format!("n{s}") } else { s };
    if is_keyword(&s) { format!("{s}_") } else { s }
}

/// `player_manager` → `PlayerManager`; a name that is already a
/// capitalized NL name (`Entity`, `NPC`, `GWEN`) is kept.
pub fn pascal(name: &str) -> String {
    if is_cname(name) {
        return name.to_string();
    }
    words(name)
        .iter()
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_ascii_uppercase().to_string() + c.as_str(),
                None => String::new(),
            }
        })
        .collect()
}

/// `kRenderFxNone` → `K_RENDER_FX_NONE`; `SNDLVL_100dB` → `SNDLVL_100DB`.
pub fn screaming(name: &str) -> String {
    if is_screaming(name) {
        return name.to_string();
    }
    // Already split by `_`: only the case is wrong.
    if name.contains('_') {
        return name.to_ascii_uppercase();
    }
    words(name).iter().map(|w| w.to_ascii_uppercase()).collect::<Vec<_>>().join("_")
}

/// A capitalized NL name: PascalCase, all capitals or SCREAMING_SNAKE_CASE.
pub fn is_cname(name: &str) -> bool {
    let Some(first) = name.chars().next() else { return false };
    if !first.is_ascii_uppercase() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return false;
    }
    let lower = name.chars().any(|c| c.is_ascii_lowercase());
    let underscore = name.contains('_');
    !(lower && underscore)
}

fn is_screaming(name: &str) -> bool {
    is_cname(name) && !name.chars().any(|c| c.is_ascii_lowercase())
}

/// A snake_case NL name that is not a keyword.
pub fn is_lname(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_lowercase() || c == '_') && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') && !is_keyword(name)
}

/// The override list: a wiki address (`Entity:SetPos`, `net.ReadUInt`,
/// `Global.Vector`, `player(library)`, `file_class`, an enum key) and the
/// NL name to use, or `-` to leave the item out.
#[derive(Debug, Default)]
pub struct Overrides {
    names: BTreeMap<String, String>,
    used: std::cell::RefCell<BTreeSet<String>>,
}

pub enum Override<'a> {
    Name(&'a str),
    Skip,
}

impl Overrides {
    pub fn parse(src: &str) -> Result<Overrides, String> {
        let mut names = BTreeMap::new();
        for (n, line) in src.lines().enumerate() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let mut parts = line.split_whitespace();
            let (Some(address), Some(name), None) = (parts.next(), parts.next(), parts.next()) else {
                return Err(format!("names.txt:{}: expected `<wiki address> <NL name>` or `<wiki address> -`", n + 1));
            };
            if names.insert(address.to_string(), name.to_string()).is_some() {
                return Err(format!("names.txt:{}: `{address}` is listed twice", n + 1));
            }
        }
        Ok(Overrides { names, used: Default::default() })
    }

    pub fn get(&self, address: &str) -> Option<Override<'_>> {
        let name = self.names.get(address)?;
        self.used.borrow_mut().insert(address.to_string());
        Some(if name == "-" { Override::Skip } else { Override::Name(name) })
    }

    /// The entries that matched nothing, to report: the wiki renamed or
    /// removed what they name.
    pub fn unused(&self) -> Vec<String> {
        let used = self.used.borrow();
        self.names.keys().filter(|k| !used.contains(*k)).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snake_names() {
        for (lua, nl) in [
            ("GetPos", "get_pos"),
            ("SteamID", "steam_id"),
            ("SteamID64", "steam_id64"),
            ("HTTPRequest", "http_request"),
            ("GetIPAddress", "get_ip_address"),
            ("CleanUpMap", "clean_up_map"),
            ("MountGMA", "mount_gma"),
            ("Start3D2D", "start3d2d"),
            ("GetNW2String", "get_nw2_string"),
            ("ReadUInt", "read_uint"),
            ("WriteUInt64", "write_uint64"),
            ("isstring", "isstring"),
            ("ScrW", "scr_w"),
            ("Spawn", "spawn_"),
            ("End", "end_"),
            ("eventName", "event_name"),
            ("ply", "ply"),
            ("HSVToColor", "hsv_to_color"),
            ("_G", "g"),
        ] {
            assert_eq!(snake(lua), nl, "{lua}");
        }
    }

    #[test]
    fn pascal_and_screaming_names() {
        assert_eq!(pascal("player_manager"), "PlayerManager");
        assert_eq!(pascal("hook"), "Hook");
        assert_eq!(pascal("GWEN"), "GWEN");
        assert_eq!(pascal("DListView_Column"), "DListViewColumn");
        assert_eq!(pascal("bf_read"), "BfRead");
        assert_eq!(screaming("DMG_BURN"), "DMG_BURN");
        assert_eq!(screaming("kRenderFxNone"), "K_RENDER_FX_NONE");
        assert_eq!(screaming("SNDLVL_100dB"), "SNDLVL_100DB");
        assert!(is_cname("NPC") && is_cname("Entity") && is_cname("MAX_HP") && !is_cname("DTree_Node") && !is_cname("bf_read"));
        assert!(is_lname("get_pos") && !is_lname("end") && !is_lname("GetPos"));
    }
}
