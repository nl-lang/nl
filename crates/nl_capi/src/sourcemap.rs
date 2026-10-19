//! The sourcemap of a bundle (`nlc bundle --map <path>`), and the text
//! remapping that reads one (`nlc remap`).
//!
//! LuaJIT reports a position as a chunk name and a line, nothing finer:
//! in an error message (`name:12: attempt to …`), in a traceback, and in
//! `debug.getinfo`'s `short_src` and `currentline`, which a host that
//! catches errors reads (Garry's Mod's `lua_shared` records them for each
//! stack level). So the map is keyed the same way: for each chunk of the
//! bundle, by the `short_src` LuaJIT shows for it, the source file and
//! line of every line.
//!
//! # Format
//!
//! ```json
//! {
//!   "version": 1,
//!   "bundle": "myaddon",
//!   "build": "5f0c2b7e9a14d3c8",
//!   "parts": ["myaddon.lua", "myaddon_2.lua"],
//!   "files": ["myaddon/main.nl", "runtime/nl.lua", …],
//!   "chunks": {
//!     "myaddon/main.nl": [[1, 0, 1, 40], [41, 0, 33, 1], [42, 0, 42, 9]],
//!     "nl runtime": [[1, 1, 1, 238], [239, 2, 1, 512], …],
//!     "nl host": [[1, 9, 1, 279]]
//!   }
//! }
//! ```
//!
//! Each range `[line, file, source_line, count]` maps the chunk's lines
//! `line … line + count - 1` to the lines `source_line …` of
//! `files[file]`; a line in no range has no source. `build` is a hash of
//! the bundle's content, which every part file carries too, so that a map
//! can be matched to the code that runs. The chunks are:
//!
//! - one per NL file, under its canonical name (the bundle loads it with
//!   the chunk name `@name`, so `short_src` is `name`);
//! - `nl runtime` and `nl host`, the runtime and its gmod host, mapped to
//!   `runtime/*.lua` (the same in every bundle of one `nlc` build).
//!
//! With a map, the bundle's files carry no line maps of their own
//! (`nl_luagen::take_line_map`): every line that a message or a trace
//! shows, the runtime's own traces included, is a line of the chunk, and
//! the map is the one translation, so nothing is translated twice.
//!
//! LuaJIT shortens a `short_src` that does not fit its buffer to `...`
//! and the end of the name (gmod's buffer holds 127 bytes, stock
//! LuaJIT's 59); a reader matches such a name by its end, as
//! [`remap_text`] does.

use std::collections::BTreeMap;

/// The current version of the format.
pub const VERSION: u64 = 1;

/// A sourcemap (module docs).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceMap {
    pub bundle: String,
    pub build: String,
    pub parts: Vec<String>,
    pub files: Vec<String>,
    /// Chunk name (`short_src`) to ranges `[line, file, source_line, count]`.
    pub chunks: BTreeMap<String, Vec<[u32; 4]>>,
}

impl SourceMap {
    /// The index of a source file, added when new.
    pub fn file(&mut self, name: &str) -> u32 {
        let i = match self.files.iter().position(|f| f == name) {
            Some(i) => i,
            None => {
                self.files.push(name.to_owned());
                self.files.len() - 1
            }
        };
        u32::try_from(i).unwrap_or(u32::MAX)
    }

    /// A chunk whose `lines` lines are the lines of one file, line for
    /// line, except the (chunk line, file line) pairs of `moved`.
    pub fn add_file_chunk(&mut self, chunk: &str, file: &str, lines: u32, moved: &[(u32, u32)]) {
        let f = self.file(file);
        let moved: BTreeMap<u32, u32> = moved.iter().copied().collect();
        let mut ranges: Vec<[u32; 4]> = Vec::new();
        for line in 1..=lines {
            let src = moved.get(&line).copied().unwrap_or(line);
            match ranges.last_mut() {
                Some([start, file, from, count]) if *file == f && *start + *count == line && *from + *count == src => *count += 1,
                _ => ranges.push([line, f, src, 1]),
            }
        }
        self.chunks.insert(chunk.to_owned(), ranges);
    }

    /// A chunk made of whole files, one after another: (file, lines).
    pub fn add_concatenation(&mut self, chunk: &str, parts: &[(&str, u32)]) {
        let mut ranges = Vec::new();
        let mut line = 1;
        for (name, lines) in parts {
            if *lines == 0 {
                continue;
            }
            let f = self.file(name);
            ranges.push([line, f, 1, *lines]);
            line += lines;
        }
        self.chunks.insert(chunk.to_owned(), ranges);
    }

    /// The source file and line of a line of a chunk.
    pub fn lookup(&self, chunk: &str, line: u32) -> Option<(&str, u32)> {
        let ranges = self.chunks.get(chunk)?;
        let r = ranges.iter().find(|[start, _, _, count]| line >= *start && line < start.saturating_add(*count))?;
        let file = self.files.get(r[1] as usize)?;
        Some((file, r[2] + (line - r[0])))
    }

    /// [`Self::lookup`] for a chunk as LuaJIT names it in `short_src`: the
    /// chunk name itself, or `...` and its end when LuaJIT shortened it,
    /// which matches when one chunk ends that way.
    pub fn lookup_short_src(&self, short_src: &str, line: u32) -> Option<(&str, u32)> {
        if self.chunks.contains_key(short_src) {
            return self.lookup(short_src, line);
        }
        let tail = short_src.strip_prefix("...").filter(|t| !t.is_empty())?;
        let mut hits = self.chunks.keys().filter(|c| c.len() > tail.len() && c.ends_with(tail));
        match (hits.next(), hits.next()) {
            (Some(chunk), None) => self.lookup(chunk, line),
            _ => None,
        }
    }

    pub fn to_json(&self) -> String {
        let chunks: serde_json::Map<String, serde_json::Value> = self.chunks.iter().map(|(k, v)| (k.clone(), serde_json::json!(v))).collect();
        let value = serde_json::json!({
            "version": VERSION,
            "bundle": self.bundle,
            "build": self.build,
            "parts": self.parts,
            "files": self.files,
            "chunks": chunks,
        });
        let mut text = serde_json::to_string(&value).unwrap_or_default();
        text.push('\n');
        text
    }

    pub fn from_json(text: &str) -> Result<SourceMap, String> {
        let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
        let version = v.get("version").and_then(serde_json::Value::as_u64);
        if version != Some(VERSION) {
            return Err(format!("not a sourcemap of version {VERSION} (`nlc bundle --map`)"));
        }
        let string = |key: &str| v.get(key).and_then(serde_json::Value::as_str).unwrap_or_default().to_owned();
        let strings = |key: &str| -> Vec<String> {
            v.get(key).and_then(serde_json::Value::as_array).map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_owned)).collect()).unwrap_or_default()
        };
        let mut chunks = BTreeMap::new();
        for (name, ranges) in v.get("chunks").and_then(serde_json::Value::as_object).ok_or("no `chunks`")? {
            let mut out = Vec::new();
            for r in ranges.as_array().ok_or_else(|| format!("chunk `{name}`: not a list of ranges"))? {
                let nums: Vec<u32> = r.as_array().map(|a| a.iter().filter_map(|n| n.as_u64().and_then(|n| u32::try_from(n).ok())).collect()).unwrap_or_default();
                let range: [u32; 4] = nums.try_into().map_err(|_| format!("chunk `{name}`: a range is four numbers"))?;
                out.push(range);
            }
            chunks.insert(name.clone(), out);
        }
        Ok(SourceMap { bundle: string("bundle"), build: string("build"), parts: strings("parts"), files: strings("files"), chunks })
    }
}

/// `text` (an error message, a traceback, a console log) with every
/// `chunk:line` of a chunk in `maps` replaced by the file and line it
/// comes from. A chunk name shortened by LuaJIT (`...end/of/name.nl`)
/// is matched by its end when one chunk ends that way. Positions of other
/// chunks, and lines with no source, are left as they are.
pub fn remap_text(maps: &[&SourceMap], text: &str) -> String {
    // (start, end, replacement), found anywhere; overlaps resolved below.
    let mut found: Vec<(usize, usize, String)> = Vec::new();
    let line_after = |at: usize| -> Option<(u32, usize)> {
        let rest = text.get(at..)?.strip_prefix(':')?;
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        let line = rest.get(..digits)?.parse().ok()?;
        Some((line, at + 1 + digits))
    };
    let boundary = |at: usize| at == 0 || text[..at].chars().next_back().is_some_and(|c| !(c.is_alphanumeric() || matches!(c, '_' | '/' | '.' | '-')));
    for map in maps {
        for chunk in map.chunks.keys() {
            for (at, _) in text.match_indices(chunk.as_str()) {
                if !boundary(at) {
                    continue;
                }
                let Some((line, end)) = line_after(at + chunk.len()) else { continue };
                if let Some((file, l)) = map.lookup(chunk, line) {
                    found.push((at, end, format!("{file}:{l}")));
                }
            }
        }
    }
    // `...tail:12`: a name LuaJIT shortened.
    for (at, _) in text.match_indices("...") {
        let tail_start = at + 3;
        let Some(colon) = text[tail_start..].find(':').map(|k| tail_start + k) else { continue };
        let tail = &text[tail_start..colon];
        if tail.is_empty() || tail.contains(char::is_whitespace) {
            continue;
        }
        let Some((line, end)) = line_after(colon) else { continue };
        // One chunk name must end that way; the first map that has it
        // answers, as for a whole name.
        let mut names: Vec<&String> = maps.iter().flat_map(|m| m.chunks.keys().filter(|c| c.len() > tail.len() && c.ends_with(tail))).collect();
        names.sort();
        names.dedup();
        let [chunk] = names.as_slice() else { continue };
        let Some(map) = maps.iter().find(|m| m.chunks.contains_key(*chunk)) else { continue };
        if let Some((file, l)) = map.lookup(chunk, line) {
            found.push((at, end, format!("{file}:{l}")));
        }
    }
    // The longest match at each place wins; later overlapping ones go.
    found.sort_by_key(|(start, end, _)| (*start, std::cmp::Reverse(*end)));
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    for (start, end, replacement) in found {
        if start < pos {
            continue;
        }
        out.push_str(&text[pos..start]);
        out.push_str(&replacement);
        pos = end;
    }
    out.push_str(&text[pos..]);
    out
}

/// A 64-bit FNV-1a hash of `bytes`, in hex: the build of a bundle.
pub fn build_hash(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn sample() -> SourceMap {
        let mut m = SourceMap { bundle: "b".into(), build: "00".into(), parts: vec!["b.lua".into()], ..SourceMap::default() };
        m.add_file_chunk("addon/main.nl", "addon/main.nl", 50, &[(41, 33), (42, 33)]);
        m.add_concatenation("nl runtime", &[("runtime/a.lua", 10), ("runtime/b.lua", 0), ("runtime/c.lua", 5)]);
        m
    }

    #[test]
    fn ranges_and_lookup() {
        let m = sample();
        assert_eq!(m.chunks["addon/main.nl"], [[1, 0, 1, 40], [41, 0, 33, 1], [42, 0, 33, 1], [43, 0, 43, 8]]);
        assert_eq!(m.lookup("addon/main.nl", 12), Some(("addon/main.nl", 12)));
        assert_eq!(m.lookup("addon/main.nl", 42), Some(("addon/main.nl", 33)));
        assert_eq!(m.lookup("addon/main.nl", 51), None);
        assert_eq!(m.lookup("nl runtime", 12), Some(("runtime/c.lua", 2)));
        assert_eq!(m.lookup("nl runtime", 16), None);
        assert_eq!(m.lookup_short_src("...on/main.nl", 42), Some(("addon/main.nl", 33)));
        assert_eq!(m.lookup_short_src("nl runtime", 12), Some(("runtime/c.lua", 2)));
        assert_eq!(m.lookup_short_src("...", 12), None);
        assert_eq!(m.lookup_short_src("other.nl", 1), None);
    }

    #[test]
    fn json_round_trip() {
        let m = sample();
        assert_eq!(SourceMap::from_json(&m.to_json()).unwrap(), m);
        assert!(SourceMap::from_json("{\"version\": 7}").is_err());
    }

    #[test]
    fn text_is_remapped() {
        let m = sample();
        let text = "addon/main.nl:41: attempt to call a nil value\nstack traceback:\n\taddon/main.nl:42: in function <addon/main.nl:3>\n\tnl runtime:12: in function 'x'\n\tother/addon/main.nl:41: in main chunk\n\t...on/main.nl:41: in function 'y'\n\tunknown.lua:41: in main chunk\n";
        let want = "addon/main.nl:33: attempt to call a nil value\nstack traceback:\n\taddon/main.nl:33: in function <addon/main.nl:3>\n\truntime/c.lua:2: in function 'x'\n\tother/addon/main.nl:41: in main chunk\n\taddon/main.nl:33: in function 'y'\n\tunknown.lua:41: in main chunk\n";
        assert_eq!(remap_text(&[&m], text), want);
    }
}
