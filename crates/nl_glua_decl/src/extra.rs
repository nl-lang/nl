//! The hand-written part (`extra.d.nl` beside this crate): what the wiki
//! data does not give, such as the fields and operators of `Vector`, the
//! global values, and `Color` being a Lua class.
//!
//! A `declare type|struct|module Name … end` block whose name the
//! generator also declares is merged into it: its header replaces the
//! generated one, and its members come first and replace generated
//! members of the same name. Every other item is copied as written.

/// One member line of a block, with its doc comment.
pub struct ExtraMember {
    pub doc: Option<String>,
    pub line: String,
    pub name: Option<String>,
}

pub struct ExtraItem {
    pub name: String,
    pub is_block: bool,
    pub doc: Option<String>,
    /// The `declare …` line of a block.
    pub header: Option<String>,
    pub members: Vec<ExtraMember>,
    /// The item's text as written, doc comments included.
    pub text: String,
}

pub struct Extra {
    pub items: Vec<ExtraItem>,
}

/// The name a `declare` line declares.
fn declared_name(line: &str) -> Option<String> {
    let rest = line.strip_prefix("declare ")?;
    let mut words = rest.split_whitespace();
    let kind = words.next()?;
    let name = words.next()?;
    let name = if kind == "fn" { name.split(['(', ':']).next()? } else { name.trim_end_matches(':') };
    Some(name.to_string())
}

/// The NL name a member line declares (`fn get_pos(self)…`, `x: Number`).
pub fn member_name(line: &str) -> Option<String> {
    let rest = line.strip_prefix("fn ").or_else(|| line.strip_prefix("const ")).unwrap_or(line);
    let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(rest.len());
    let name = rest.get(..end)?;
    (!name.is_empty()).then(|| name.to_string())
}

impl Extra {
    pub fn parse(src: &str) -> Result<Extra, String> {
        let mut items = Vec::new();
        let mut doc: Vec<String> = Vec::new();
        let mut lines = src.lines().enumerate();
        while let Some((n, line)) = lines.next() {
            if let Some(d) = line.strip_prefix("##") {
                doc.push(d.trim().to_string());
                continue;
            }
            if line.trim().is_empty() || line.starts_with('#') {
                doc.clear();
                continue;
            }
            let name = declared_name(line).ok_or_else(|| format!("extra.d.nl:{}: expected a `declare` item", n + 1))?;
            let item_doc = (!doc.is_empty()).then(|| doc.join(" "));
            let mut text: String = doc.iter().map(|d| format!("## {d}\n")).collect();
            doc.clear();
            text.push_str(line);
            text.push('\n');
            let is_block = ["declare type ", "declare struct ", "declare module "].iter().any(|p| line.starts_with(p));
            let mut members = Vec::new();
            if is_block {
                let mut mdoc: Option<String> = None;
                loop {
                    let Some((m, l)) = lines.next() else { return Err(format!("extra.d.nl:{}: `{name}` has no `end`", n + 1)) };
                    text.push_str(l);
                    text.push('\n');
                    if l == "end" {
                        break;
                    }
                    let t = l.trim();
                    if let Some(d) = t.strip_prefix("##") {
                        mdoc = Some(d.trim().to_string());
                    } else if !t.is_empty() && !t.starts_with('#') {
                        if !l.starts_with("  ") {
                            return Err(format!("extra.d.nl:{}: a member is indented by two spaces", m + 1));
                        }
                        members.push(ExtraMember { doc: mdoc.take(), line: t.to_string(), name: member_name(t) });
                    }
                }
            }
            items.push(ExtraItem { name, is_block, doc: item_doc, header: is_block.then(|| line.to_string()), members, text });
        }
        Ok(Extra { items })
    }
}
