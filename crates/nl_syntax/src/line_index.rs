//! [`LineIndex`]: byte offsets to lines and columns, for messages.

use rowan::TextSize;

/// A 1-based line and column. The column counts characters (Unicode scalar
/// values), not bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LineCol {
    pub line: u32,
    pub col: u32,
}

/// The start offsets of the lines of a text, and where its multi-byte
/// characters are, so that [`LineIndex::line_col`] takes logarithmic time
/// however long the line. LF, CRLF and a lone CR are each one line break.
/// A byte-order mark at the start of the text is not counted
/// as a column: the first line starts after it.
#[derive(Clone, Debug)]
pub struct LineIndex {
    starts: Vec<usize>,
    /// For each character of more than one byte: its offset, and the extra
    /// bytes (beyond one per character) of all such characters up to and
    /// including it.
    wide: Vec<(usize, usize)>,
    len: usize,
}

impl LineIndex {
    pub fn new(text: &str) -> LineIndex {
        let b = text.as_bytes();
        let bom = if text.starts_with('\u{FEFF}') { 3 } else { 0 };
        let mut starts = vec![bom];
        let mut wide = Vec::new();
        let mut extra = 0usize;
        for (i, c) in text.char_indices() {
            match c {
                '\n' => starts.push(i + 1),
                '\r' if b.get(i + 1) != Some(&b'\n') => starts.push(i + 1),
                _ if i >= bom && c.len_utf8() > 1 => {
                    extra += c.len_utf8() - 1;
                    wide.push((i, extra));
                }
                _ => {}
            }
        }
        LineIndex { starts, wide, len: text.len() }
    }

    /// The number of lines (at least 1).
    pub fn line_count(&self) -> usize {
        self.starts.len()
    }

    /// Extra bytes of the multi-byte characters that start before `offset`.
    fn extra_before(&self, offset: usize) -> usize {
        let n = self.wide.partition_point(|&(at, _)| at < offset);
        n.checked_sub(1).and_then(|i| self.wide.get(i)).map_or(0, |&(_, e)| e)
    }

    /// The line and column of `offset` in `text`, which must be the text the
    /// index was built from. An offset past the end is clamped; an offset
    /// inside a character counts that character as before it.
    pub fn line_col(&self, text: &str, offset: TextSize) -> LineCol {
        let _ = text;
        let offset = usize::from(offset).min(self.len);
        let line = self.starts.partition_point(|&s| s <= offset).saturating_sub(1);
        let start = self.starts.get(line).copied().unwrap_or(0).min(offset);
        let bytes = offset - start;
        let col = bytes - (self.extra_before(offset) - self.extra_before(start));
        LineCol {
            line: u32::try_from(line + 1).unwrap_or(u32::MAX),
            col: u32::try_from(col + 1).unwrap_or(u32::MAX),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_and_columns() {
        let text = "ab\ncé\r\nx\ry";
        let idx = LineIndex::new(text);
        assert_eq!(idx.line_count(), 4);
        let at = |o: u32| idx.line_col(text, TextSize::new(o));
        assert_eq!(at(0), LineCol { line: 1, col: 1 });
        assert_eq!(at(3), LineCol { line: 2, col: 1 });
        // after `cé`: `é` is two bytes, one column
        assert_eq!(at(6), LineCol { line: 2, col: 3 });
        assert_eq!(at(8), LineCol { line: 3, col: 1 });
        assert_eq!(at(10), LineCol { line: 4, col: 1 });
        assert_eq!(at(99), LineCol { line: 4, col: 2 });
    }

    #[test]
    fn wide_characters_and_bom() {
        let text = "\u{FEFF})é😀x\nyé";
        let idx = LineIndex::new(text);
        let at = |o: u32| idx.line_col(text, TextSize::new(o));
        assert_eq!(at(3), LineCol { line: 1, col: 1 }, "the BOM is not a column");
        assert_eq!(at(0), LineCol { line: 1, col: 1 });
        assert_eq!(at(4), LineCol { line: 1, col: 2 });
        assert_eq!(at(6), LineCol { line: 1, col: 3 });
        assert_eq!(at(10), LineCol { line: 1, col: 4 });
        assert_eq!(at(12), LineCol { line: 2, col: 1 });
        assert_eq!(at(13), LineCol { line: 2, col: 2 });
        assert_eq!(at(15), LineCol { line: 2, col: 3 });
        // Brute force against counting characters.
        let text = "aé\u{1F600}b\r\nc\u{3042}\rd\u{E9}e";
        let idx = LineIndex::new(text);
        for (off, c) in text.char_indices() {
            if c == '\n' && text[..off].ends_with('\r') {
                continue; // inside a CRLF
            }
            let line_start = text[..off].rfind(['\n', '\r']).map_or(0, |i| i + 1);
            let col = text[line_start..off].chars().count() + 1;
            assert_eq!(idx.line_col(text, TextSize::new(off as u32)).col as usize, col, "at {off}");
        }
    }
}
