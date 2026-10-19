//! Source snippets for diagnostics:
//! the line a diagnostic points at, with a caret underline, in the manner
//! of rustc. `nlc` prints them under each diagnostic; the language server
//! and hosts can use the same text.
//!
//! ```text
//! tests/run/x.nl:12:7: error: undeclared name `foo` [undeclared-name]
//!    |
//! 12 |   let y = foo + 1
//!    |           ^^^
//! ```
//!
//! The first line is the diagnostic's header, unchanged from the one-line
//! form (`file:line:column: severity: message [code]`), so that editors'
//! error matchers still read it. The snippet follows: a gutter as wide as
//! the line number, the source line, and carets under the range.
//!
//! - Positions are those of [`Diagnostic`]: 1-based lines, and columns that
//!   count characters (Unicode scalar values), not bytes. The end column is
//!   exclusive. An empty range gets one caret.
//! - A range that ends on a later line is underlined from its start to the
//!   end of its first line, and the carets are followed by `to line N`:
//!   only the first line is shown.
//! - A tab is shown expanded to the next multiple of four columns, in the
//!   source line and in the caret line alike, so that the carets line up.
//!   Every other character takes one column, which holds for the text of
//!   most programs (wide East Asian characters take two cells in most
//!   terminals and push the carets left by one each).
//! - Lines are split as the lexer splits them: LF, CRLF and
//!   a lone CR; a byte-order mark at the start is not a column.
//! - When the line is not in the source (the source was changed, or is
//!   another text), there is no snippet: [`snippet`] gives an empty string
//!   and [`render`] the header alone.

use crate::driver::{Diagnostic, Severity};

/// The columns a tab advances to the next multiple of.
const TAB_WIDTH: usize = 4;

/// The header line of a diagnostic, the one-line form:
/// `file:line:column: severity: message [code]`.
pub fn header(d: &Diagnostic) -> String {
    let severity = match d.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
        Severity::Lint => "lint",
    };
    format!("{}:{}:{}: {severity}: {} [{}]", d.file, d.line, d.column, d.message, d.code)
}

/// A diagnostic with its snippet: the header line, then the snippet's
/// lines, joined by newlines, with no newline at the end.
pub fn render(d: &Diagnostic, source: &str) -> String {
    let header = header(d);
    let s = snippet(source, d.line, d.column, d.end_line, d.end_column);
    if s.is_empty() { header } else { format!("{header}\n{s}") }
}

/// The snippet of the range from `line`:`column` to `end_line`:`end_column`
/// of `source` (see the module doc): three lines joined by newlines, or an
/// empty string when `line` is not in `source`.
pub fn snippet(source: &str, line: u32, column: u32, end_line: u32, end_column: u32) -> String {
    let Some(text) = line_text(source, line) else { return String::new() };
    let chars: Vec<char> = text.chars().collect();
    let start = (column.max(1) - 1) as usize;
    let end = if end_line > line {
        chars.len().max(start + 1)
    } else if end_line == line && end_column > column {
        (end_column - 1) as usize
    } else {
        start + 1
    };

    // The shown line, and the display column where each character starts
    // (one more entry, for the end of the line).
    let mut shown = String::new();
    let mut at = Vec::with_capacity(chars.len() + 1);
    let mut col = 0usize;
    for &c in &chars {
        at.push(col);
        if c == '\t' {
            let next = (col / TAB_WIDTH + 1) * TAB_WIDTH;
            shown.extend(std::iter::repeat_n(' ', next - col));
            col = next;
        } else {
            shown.push(c);
            col += 1;
        }
    }
    at.push(col);
    // A position past the end of the line is a column past its last one.
    let display = |i: usize| at.get(i).copied().unwrap_or_else(|| col + i.saturating_sub(chars.len()));
    let from = display(start);
    let to = display(end).max(from + 1);

    let number = line.to_string();
    let pad = " ".repeat(number.len());
    let mut underline = format!("{pad} | {}{}", " ".repeat(from), "^".repeat(to - from));
    if end_line > line {
        underline.push_str(&format!(" to line {end_line}"));
    }
    let source_line = format!("{number} | {shown}");
    format!("{pad} |\n{}\n{underline}", source_line.trim_end())
}

/// The text of the 1-based line `line` of `source`, without its line
/// break; `None` when there is no such line.
fn line_text(source: &str, line: u32) -> Option<&str> {
    if line == 0 {
        return None;
    }
    let source = source.strip_prefix('\u{FEFF}').unwrap_or(source);
    let mut chars = source.char_indices().peekable();
    let (mut start, mut n) = (0, 1);
    while let Some((i, c)) = chars.next() {
        if c != '\n' && c != '\r' {
            continue;
        }
        if n == line {
            return source.get(start..i);
        }
        start = i + 1;
        if c == '\r' && chars.next_if(|&(_, d)| d == '\n').is_some() {
            start += 1;
        }
        n += 1;
    }
    if n == line { source.get(start..) } else { None }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn diag(line: u32, column: u32, end_line: u32, end_column: u32) -> Diagnostic {
        Diagnostic {
            severity: Severity::Error,
            code: "undeclared-name",
            message: "undeclared name `foo`".to_string(),
            file: "x.nl".to_string(),
            line,
            column,
            end_line,
            end_column,
        }
    }

    #[test]
    fn one_line_range() {
        let src = "let a = 1\n".repeat(11) + "  let y = foo + 1\n";
        assert_eq!(
            render(&diag(12, 11, 12, 14), &src),
            "x.nl:12:11: error: undeclared name `foo` [undeclared-name]\n   |\n12 |   let y = foo + 1\n   |           ^^^"
        );
    }

    #[test]
    fn empty_range_and_end_of_line() {
        assert_eq!(snippet("print(\n", 1, 7, 1, 7), "  |\n1 | print(\n  |       ^");
        // Past the last character: the caret follows the line.
        assert_eq!(snippet("ab\n", 1, 3, 1, 3), "  |\n1 | ab\n  |   ^");
        // An end before the start is one caret.
        assert_eq!(snippet("abc", 1, 2, 1, 1), "  |\n1 | abc\n  |  ^");
    }

    #[test]
    fn multi_line_range() {
        assert_eq!(snippet("x = foo(\n  1\n)\n", 1, 5, 3, 2), "  |\n1 | x = foo(\n  |     ^^^^ to line 3");
    }

    #[test]
    fn tabs_line_up() {
        // The tab expands to column 4; `b` is the third character.
        assert_eq!(snippet("\tab", 1, 3, 1, 4), "  |\n1 |     ab\n  |      ^");
        assert_eq!(snippet("a\tb", 1, 2, 1, 4), "  |\n1 | a   b\n  |  ^^^^");
    }

    #[test]
    fn columns_count_characters() {
        // `é` and `€` are one column each, whatever their bytes.
        assert_eq!(snippet("s = \"é€\" + x", 1, 12, 1, 13), "  |\n1 | s = \"é€\" + x\n  |            ^");
    }

    #[test]
    fn line_breaks_and_missing_lines() {
        assert_eq!(snippet("a\r\nbc\rd", 2, 2, 2, 3), "  |\n2 | bc\n  |  ^");
        assert_eq!(snippet("a\r\nbc\rd", 3, 1, 3, 2), "  |\n3 | d\n  | ^");
        assert_eq!(snippet("\u{FEFF}ab", 1, 2, 1, 3), "  |\n1 | ab\n  |  ^");
        assert_eq!(snippet("a\n", 5, 1, 5, 2), "");
        assert_eq!(snippet("a\n", 0, 1, 0, 2), "");
        assert_eq!(render(&diag(9, 1, 9, 2), "a\n"), "x.nl:9:1: error: undeclared name `foo` [undeclared-name]");
    }
}
