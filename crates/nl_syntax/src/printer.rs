//! A stable text dump of a syntax tree and of diagnostics, for `nlc parse`
//! and snapshot tests.
//!
//! # Format
//!
//! One element per line, indented by two spaces per level. A node is printed
//! as `KIND@start..end`, a token as `KIND@start..end "text"`, where the
//! offsets are bytes and the text is written as a Rust string literal (`{:?}`
//! of `str`), so line breaks and other control characters are escaped:
//!
//! ```text
//! SOURCE_FILE@0..10
//!   BLOCK@0..10
//!     LET_DECL@0..9
//!       LET_KW@0..3 "let"
//!       WHITESPACE@3..4 " "
//!       BIND_PAT@4..5
//!         NAME@4..5
//!           LNAME@4..5 "x"
//!       WHITESPACE@5..6 " "
//!       EQ@6..7 "="
//!       WHITESPACE@7..8 " "
//!       EXPR_LIST@8..9
//!         LITERAL@8..9
//!           NUMBER@8..9 "1"
//!     NEWLINE@9..10 "\n"
//!   EOF@10..10 ""
//! ```
//!
//! Diagnostics are printed one per line as
//! `error@start..end line:col: message` (`warning@…` for warnings), with a
//! 1-based line and column (in characters) of the start. The format is
//! deterministic: it depends only on the source text.

use std::fmt::Write as _;

use rowan::WalkEvent;

use crate::{Diagnostic, LineIndex, Severity, SyntaxNode, parse, parse_decl_file};

/// Options of [`dump_tree`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DumpOptions {
    /// Leave out trivia tokens (whitespace, line breaks, comments). Doc
    /// comments, `ERROR` tokens and `EOF` are always printed.
    pub skip_trivia: bool,
}

/// The dump of a tree (see the module documentation for the format).
pub fn dump_tree(node: &SyntaxNode, options: DumpOptions) -> String {
    let mut out = String::new();
    let mut depth = 0usize;
    for event in node.preorder_with_tokens() {
        match event {
            WalkEvent::Enter(element) => {
                match &element {
                    rowan::NodeOrToken::Node(n) => {
                        let _ = writeln!(out, "{}{:?}@{:?}", "  ".repeat(depth), n.kind(), n.text_range());
                    }
                    rowan::NodeOrToken::Token(t) => {
                        let skip = options.skip_trivia
                            && t.kind().is_trivia()
                            && t.kind() != crate::SyntaxKind::DOC_COMMENT;
                        if !skip {
                            let _ = writeln!(
                                out,
                                "{}{:?}@{:?} {:?}",
                                "  ".repeat(depth),
                                t.kind(),
                                t.text_range(),
                                t.text()
                            );
                        }
                    }
                }
                if element.as_node().is_some() {
                    depth += 1;
                }
            }
            WalkEvent::Leave(element) => {
                if element.as_node().is_some() {
                    depth = depth.saturating_sub(1);
                }
            }
        }
    }
    out
}

/// Diagnostics of `source`, one per line (see the module documentation).
pub fn dump_diagnostics(source: &str, diagnostics: &[Diagnostic]) -> String {
    let index = LineIndex::new(source);
    let mut out = String::new();
    for d in diagnostics {
        let lc = index.line_col(source, d.range.start());
        let severity = match d.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        let _ = writeln!(out, "{severity}@{:?} {}:{}: {}", d.range, lc.line, lc.col, d.message);
    }
    out
}

/// Parses `source` and returns the tree dump (with trivia) followed by the
/// diagnostics, the two parts separated by a line `---` when there are
/// diagnostics. Meant for snapshot tests: `assert_snapshot!(dump_parse(src))`.
pub fn dump_parse(source: &str) -> String {
    let parse = parse(source);
    let mut out = dump_tree(&parse.syntax(), DumpOptions::default());
    if !parse.diagnostics().is_empty() {
        out.push_str("---\n");
        out.push_str(&dump_diagnostics(source, parse.diagnostics()));
    }
    out
}

/// [`dump_parse`] for a declaration file.
pub fn dump_parse_decl(source: &str) -> String {
    let parse = parse_decl_file(source);
    let mut out = dump_tree(&parse.syntax(), DumpOptions::default());
    if !parse.diagnostics().is_empty() {
        out.push_str("---\n");
        out.push_str(&dump_diagnostics(source, parse.diagnostics()));
    }
    out
}
