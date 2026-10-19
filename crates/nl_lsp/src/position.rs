//! Positions and names as LSP wants them: the byte offsets of the syntax
//! tree become line and UTF-16 column positions, the 1-based character
//! columns of the compiler's diagnostics (`nl_capi::Diagnostic`) likewise,
//! and file paths become `file:` URIs and back.
//!
//! Line breaks are what NL and the LSP agree on: LF, CRLF and a
//! lone CR each end a line.

use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

use lsp_types::{Position, Range, Uri};
use nl_syntax::{TextRange, TextSize};

/// The start offset of each line of a text.
#[derive(Clone, Debug)]
pub struct LineMap {
    starts: Vec<usize>,
    len: usize,
}

impl LineMap {
    pub fn new(text: &str) -> LineMap {
        let b = text.as_bytes();
        let mut starts = vec![0];
        for (i, c) in text.char_indices() {
            match c {
                '\n' => starts.push(i + 1),
                '\r' if b.get(i + 1) != Some(&b'\n') => starts.push(i + 1),
                _ => {}
            }
        }
        LineMap { starts, len: text.len() }
    }

    /// The 0-based line of a byte offset, and the offset where it starts.
    fn line_of(&self, offset: usize) -> (usize, usize) {
        let line = self.starts.partition_point(|&s| s <= offset).saturating_sub(1);
        (line, self.starts.get(line).copied().unwrap_or(0))
    }

    /// The byte offset where the 0-based line `line` starts, and where it
    /// ends (before its line break). `None` past the last line.
    fn line_span(&self, line: usize) -> Option<(usize, usize)> {
        let start = *self.starts.get(line)?;
        let end = self.starts.get(line + 1).map_or(self.len, |&next| next);
        Some((start, end))
    }

    /// The LSP position of a byte offset of `text` (the text the map was
    /// built from). An offset past the end is clamped; one inside a
    /// character counts that character as before it.
    pub fn position(&self, text: &str, offset: TextSize) -> Position {
        let offset = usize::from(offset).min(self.len);
        let (line, start) = self.line_of(offset);
        let units: usize = text.get(start..offset).map(|s| s.chars().map(char::len_utf16).sum()).unwrap_or(0);
        Position { line: clamp(line), character: clamp(units) }
    }

    /// The byte offset of an LSP position; a column past the end of its
    /// line is the end of the line, and a line past the end of the text is
    /// its end. `None` only for a position inside a line break's CRLF
    /// pair, which cannot happen for a well-formed position.
    pub fn offset(&self, text: &str, pos: Position) -> Option<TextSize> {
        let Some((start, end)) = self.line_span(pos.line as usize) else {
            return Some(TextSize::new(clamp(self.len)));
        };
        let line = text.get(start..end)?;
        let mut units = 0usize;
        let mut offset = start;
        for (i, c) in line.char_indices() {
            // A position inside a surrogate pair is before the character.
            if units + c.len_utf16() > pos.character as usize || c == '\n' || c == '\r' {
                offset = start + i;
                return Some(TextSize::new(clamp(offset)));
            }
            units += c.len_utf16();
            offset = start + i + c.len_utf8();
        }
        // The last line, without a line break: its end.
        Some(TextSize::new(clamp(offset)))
    }

    pub fn range(&self, text: &str, range: TextRange) -> Range {
        Range { start: self.position(text, range.start()), end: self.position(text, range.end()) }
    }

    /// The position of a 1-based line and character column, as the
    /// compiler's diagnostics give them (`nl_capi::Diagnostic`).
    pub fn from_line_col(&self, text: &str, line: u32, col: u32) -> Position {
        let line0 = (line.max(1) - 1) as usize;
        let Some((start, end)) = self.line_span(line0) else {
            return Position { line: clamp(self.starts.len().saturating_sub(1)), character: 0 };
        };
        let content = text.get(start..end).unwrap_or("");
        let units: usize = content.chars().take((col.max(1) - 1) as usize).map(char::len_utf16).sum();
        Position { line: clamp(line0), character: clamp(units) }
    }
}

fn clamp(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// Lexical normalization of a path: `.` removed, `..` resolved where it
/// can be. No file system access, so a name stays the editor's spelling
/// of it and the URIs the server publishes match the ones it was given.
pub fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// The file path of a `file:` URI; `None` for any other scheme.
pub fn uri_to_path(uri: &Uri) -> Option<PathBuf> {
    if uri.scheme().map(|s| s.as_str()) != Some("file") {
        return None;
    }
    let decoded = percent_decode(uri.path().as_str());
    // `/C:/dir` on Windows is `C:/dir`.
    let bytes = decoded.as_bytes();
    let path = if cfg!(windows) && bytes.len() > 2 && bytes.first() == Some(&b'/') && bytes.get(2) == Some(&b':') { decoded.get(1..).unwrap_or("").to_string() } else { decoded };
    Some(normalize(Path::new(&path)))
}

/// The `file:` URI of an absolute path.
pub fn path_to_uri(path: &Path) -> Option<Uri> {
    let text = path.to_string_lossy().replace('\\', "/");
    let mut s = String::from("file://");
    if !text.starts_with('/') {
        s.push('/');
    }
    for b in text.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => s.push(b as char),
            _ => s.push_str(&format!("%{b:02X}")),
        }
    }
    Uri::from_str(&s).ok()
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let c = b.get(i).copied().unwrap_or(0);
        if c == b'%' {
            let hex = b.get(i + 1..i + 3).and_then(|h| std::str::from_utf8(h).ok()).and_then(|h| u8::from_str_radix(h, 16).ok());
            if let Some(v) = hex {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn ascii_lines() {
        let text = "ab\ncd\r\ne\rf";
        let m = LineMap::new(text);
        let at = |o: u32| m.position(text, TextSize::new(o));
        assert_eq!(at(0), Position::new(0, 0));
        assert_eq!(at(2), Position::new(0, 2));
        assert_eq!(at(3), Position::new(1, 0));
        assert_eq!(at(7), Position::new(2, 0));
        assert_eq!(at(9), Position::new(3, 0));
        assert_eq!(at(99), Position::new(3, 1));
        assert_eq!(m.offset(text, Position::new(1, 1)), Some(TextSize::new(4)));
        assert_eq!(m.offset(text, Position::new(1, 50)), Some(TextSize::new(5)), "past the end of a line is its end");
        assert_eq!(m.offset(text, Position::new(9, 0)), Some(TextSize::new(10)), "past the last line is the end");
    }

    /// Columns count UTF-16 units: `é` is one, `😀` (outside the BMP) two.
    #[test]
    fn non_ascii_columns() {
        let text = "é😀x\ny";
        let m = LineMap::new(text);
        let at = |o: u32| m.position(text, TextSize::new(o));
        assert_eq!(at(0), Position::new(0, 0));
        assert_eq!(at(2), Position::new(0, 1));
        assert_eq!(at(6), Position::new(0, 3));
        assert_eq!(at(7), Position::new(0, 4));
        assert_eq!(at(8), Position::new(1, 0));
        assert_eq!(m.offset(text, Position::new(0, 1)), Some(TextSize::new(2)));
        assert_eq!(m.offset(text, Position::new(0, 3)), Some(TextSize::new(6)));
        assert_eq!(m.offset(text, Position::new(0, 2)), Some(TextSize::new(2)), "inside a surrogate pair: before the character");
        // Round trip at every character boundary.
        for (off, _) in text.char_indices() {
            let p = m.position(text, TextSize::new(off as u32));
            assert_eq!(m.offset(text, p), Some(TextSize::new(off as u32)), "at {off}");
        }
    }

    /// The compiler's columns count characters, 1-based.
    #[test]
    fn from_character_columns() {
        let text = "é😀x\ny";
        let m = LineMap::new(text);
        assert_eq!(m.from_line_col(text, 1, 1), Position::new(0, 0));
        assert_eq!(m.from_line_col(text, 1, 2), Position::new(0, 1));
        assert_eq!(m.from_line_col(text, 1, 3), Position::new(0, 3));
        assert_eq!(m.from_line_col(text, 1, 4), Position::new(0, 4));
        assert_eq!(m.from_line_col(text, 2, 1), Position::new(1, 0));
        assert_eq!(m.from_line_col(text, 7, 3), Position::new(1, 0), "a line past the end is the last line");
    }

    #[test]
    fn uris() {
        let p = Path::new("/tmp/some dir/café.nl");
        let uri = path_to_uri(p).unwrap();
        assert_eq!(uri.as_str(), "file:///tmp/some%20dir/caf%C3%A9.nl");
        assert_eq!(uri_to_path(&uri).unwrap(), p);
        assert!(uri_to_path(&Uri::from_str("untitled:x").unwrap()).is_none());
    }
}
