//! NL types for the wiki's types.
//!
//! | wiki | NL |
//! |---|---|
//! | `number`, `number{DMG}` (an enum) | `Number` |
//! | `string`, `boolean` | `String`, `Bool` |
//! | `table`, `table{TraceResult}` (a structure) | `Table` |
//! | `table<Player>` | `[Player]` |
//! | `table<string, any>` | `{[String]: any}` |
//! | `function` with a known signature | `Function(a: A): R` |
//! | a class or panel (`Entity`, `DFrame`, `Panel{DFrame}`) | its NL type |
//! | `any`, `vararg`, a name the wiki does not declare | `any` |
//! | `A\|B`, `A\|nil` | `A \| B`, `A?` |

use std::collections::BTreeMap;

/// The NL types of the wiki's class and panel names.
pub struct TypeNames<'a> {
    pub classes: &'a BTreeMap<String, String>,
}

/// A type the wiki writes, parsed.
#[derive(Debug, PartialEq)]
enum Wiki {
    /// `name`, `name{Tag}`, `name<A, B>`.
    Named { name: String, tag: Option<String>, args: Vec<Wiki> },
    Union(Vec<Wiki>),
}

struct Parser<'s> {
    s: &'s [u8],
    i: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn skip_ws(&mut self) {
        while self.peek().is_some_and(|c| c == b' ') {
            self.i += 1;
        }
    }

    fn union(&mut self) -> Wiki {
        let mut parts = vec![self.atom()];
        loop {
            self.skip_ws();
            if self.peek() == Some(b'|') {
                self.i += 1;
                parts.push(self.atom());
            } else {
                break;
            }
        }
        if parts.len() == 1 { parts.pop().unwrap_or(Wiki::Union(Vec::new())) } else { Wiki::Union(parts) }
    }

    fn atom(&mut self) -> Wiki {
        self.skip_ws();
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'/' || c == b'.') {
            self.i += 1;
        }
        let name = String::from_utf8_lossy(self.s.get(start..self.i).unwrap_or_default()).to_string();
        let mut tag = None;
        let mut args = Vec::new();
        self.skip_ws();
        match self.peek() {
            Some(b'{') => {
                self.i += 1;
                let t0 = self.i;
                while self.peek().is_some_and(|c| c != b'}') {
                    self.i += 1;
                }
                tag = Some(String::from_utf8_lossy(self.s.get(t0..self.i).unwrap_or_default()).trim().to_string());
                self.i += 1;
            }
            Some(b'<') => {
                self.i += 1;
                loop {
                    args.push(self.union());
                    self.skip_ws();
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b'>') => {
                            self.i += 1;
                            break;
                        }
                        _ => break,
                    }
                }
            }
            _ => {}
        }
        Wiki::Named { name, tag, args }
    }
}

fn parse(s: &str) -> Wiki {
    Parser { s: s.trim().as_bytes(), i: 0 }.union()
}

impl TypeNames<'_> {
    /// The NL type of a wiki type. `vararg` is `any` here; the caller makes
    /// it a rest parameter.
    pub fn map(&self, ty: &str) -> String {
        let t = self.wiki(&parse(ty));
        t.unwrap_or_else(|| "any".to_string())
    }

    /// `None` for `nil` and `NULL`, which a union drops.
    fn wiki(&self, t: &Wiki) -> Option<String> {
        match t {
            Wiki::Union(parts) => {
                let mut out: Vec<String> = Vec::new();
                let mut nil = false;
                for p in parts {
                    match self.wiki(p) {
                        Some(s) if s == "any" => return Some("any".to_string()),
                        Some(s) => {
                            if !out.contains(&s) {
                                out.push(s);
                            }
                        }
                        None => nil |= matches!(p, Wiki::Named { name, .. } if name == "nil"),
                    }
                }
                Some(match (out.len(), nil) {
                    (0, _) => "nil".to_string(),
                    (_, false) => out.join(" | "),
                    (_, true) => optional(&out.join(" | ")),
                })
            }
            Wiki::Named { name, tag, args } => Some(match name.as_str() {
                "nil" | "NULL" => return None,
                "number" => "Number".to_string(),
                "string" => "String".to_string(),
                "boolean" | "bool" => "Bool".to_string(),
                "function" => "Function".to_string(),
                "thread" => "Thread".to_string(),
                "userdata" => "Userdata".to_string(),
                "table" => match args.as_slice() {
                    [v] => format!("[{}]", self.wiki(v).unwrap_or_else(|| "any".to_string())),
                    [k, v] => format!("{{[{}]: {}}}", self.wiki(k).unwrap_or_else(|| "any".to_string()), self.wiki(v).unwrap_or_else(|| "any".to_string())),
                    _ => "Table".to_string(),
                },
                // A panel of a derma class.
                "Panel" => tag.as_ref().and_then(|t| self.classes.get(t)).cloned().unwrap_or_else(|| self.classes.get("Panel").cloned().unwrap_or_else(|| "any".to_string())),
                n if n.starts_with("Structures/") => "Table".to_string(),
                n => self.classes.get(n).cloned().unwrap_or_else(|| "any".to_string()),
            }),
        }
    }
}

/// `T?`, with parentheses around a union.
pub fn optional(t: &str) -> String {
    if t == "any" || t.ends_with('?') || t == "nil" {
        return t.to_string();
    }
    // A union, and a signature whose `?` would apply to its result.
    if t.contains(" | ") || t.starts_with("Function(") { format!("({t})?") } else { format!("{t}?") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optional_types() {
        assert_eq!(optional("Number"), "Number?");
        assert_eq!(optional("Number | String"), "(Number | String)?");
        assert_eq!(optional("Function(a: Number): Table"), "(Function(a: Number): Table)?");
        assert_eq!(optional("any"), "any");
    }

    #[test]
    fn wiki_types() {
        let classes: BTreeMap<String, String> =
            [("Entity", "Entity"), ("Player", "Player"), ("Panel", "Panel"), ("DFrame", "DFrame"), ("File", "FileHandle")].iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        let t = TypeNames { classes: &classes };
        for (wiki, nl) in [
            ("number", "Number"),
            ("number{DMG}", "Number"),
            ("table{TraceResult}", "Table"),
            ("table<Player>", "[Player]"),
            ("table<string,any>", "{[String]: any}"),
            ("table<string, table>", "{[String]: Table}"),
            ("table<Structures/Sky3DParams>|nil", "[Table]?"),
            ("Player|NULL", "Player"),
            ("Player|boolean", "Player | Bool"),
            ("Entity|nil", "Entity?"),
            ("string|number|boolean", "String | Number | Bool"),
            ("table<table>|boolean|nil", "([Table] | Bool)?"),
            ("Panel{DFrame}", "DFrame"),
            ("Panel{DUnknown}", "Panel"),
            ("File", "FileHandle"),
            ("any|nil", "any"),
            ("CUnknownThing", "any"),
            ("vararg", "any"),
        ] {
            assert_eq!(t.map(wiki), nl, "{wiki}");
        }
    }
}
