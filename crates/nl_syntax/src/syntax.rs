//! The syntax tree types and the [`parse`] entry point.

use rowan::GreenNode;

use crate::{Diagnostic, NlLanguage, SyntaxKind};

/// A node of the syntax tree.
pub type SyntaxNode = rowan::SyntaxNode<NlLanguage>;
/// A token of the syntax tree (trivia and `ERROR` tokens included).
pub type SyntaxToken = rowan::SyntaxToken<NlLanguage>;
/// A node or a token.
pub type SyntaxElement = rowan::SyntaxElement<NlLanguage>;

/// The result of [`parse`]: a lossless syntax tree and every diagnostic of
/// the lexer and the parser, ordered by position.
///
/// The tree always exists, whatever the input: its root is a `SOURCE_FILE`
/// and its text is exactly the source (for sources under 4 GiB). Malformed
/// parts are `ERROR_NODE`s or incomplete nodes, with diagnostics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parse {
    green: GreenNode,
    diagnostics: Vec<Diagnostic>,
}

impl Parse {
    /// The root `SOURCE_FILE` node.
    pub fn syntax(&self) -> SyntaxNode {
        SyntaxNode::new_root(self.green.clone())
    }

    /// The green (immutable, shareable) tree.
    pub fn green(&self) -> &GreenNode {
        &self.green
    }

    /// Lexer and parser diagnostics, ordered by position.
    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    /// True when some diagnostic is an error (not just a warning).
    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(Diagnostic::is_error)
    }

    /// The tree and the diagnostics.
    pub fn into_parts(self) -> (GreenNode, Vec<Diagnostic>) {
        (self.green, self.diagnostics)
    }
}

/// Lexes and parses NL source text. Never panics, never
/// overflows the stack, and always returns a tree whose text is `source`.
pub fn parse(source: &str) -> Parse {
    let (green, diagnostics) = crate::parser::parse_source(source);
    Parse { green, diagnostics }
}

/// Lexes and parses a declaration file (`.d.nl`). The
/// root is a `SOURCE_FILE` holding the `DECL_*` items. Never panics.
pub fn parse_decl_file(source: &str) -> Parse {
    let (green, diagnostics) = crate::parser::parse_decl_source(source);
    Parse { green, diagnostics }
}

/// The doc comment of a declaration node: the text of its
/// leading comment lines, joined with line breaks. The first is a
/// `DOC_COMMENT` (`##`); the lines after it belong to the doc comment whether
/// they start with `##` or `#`. Each line is given without its `##` or `#`
/// and one following space. `None` when the node has no doc comment.
///
/// The parser puts a doc comment inside the node of the declaration it
/// documents (`FN_DECL`, `STRUCT_DECL`, `FIELD_DECL`, `INTERFACE_DECL`,
/// `INTERFACE_IMPL`, `IFACE_METHOD`, `IFACE_FIELD`, `ENUM_DECL`, `VARIANT`,
/// `TYPE_DECL`, `MODULE_DECL`, `CONST_DECL`, `LET_DECL`, `GLOBAL_DECL`,
/// `ON_EXPR`), before its first token.
pub fn doc_comment(node: &SyntaxNode) -> Option<String> {
    let mut lines = Vec::new();
    for element in node.children_with_tokens() {
        let Some(token) = element.as_token() else { break };
        let marker = match token.kind() {
            SyntaxKind::DOC_COMMENT => "##",
            // A `#` line continues a doc comment; it never starts one.
            SyntaxKind::COMMENT if !lines.is_empty() => "#",
            k if k.is_trivia() => continue,
            _ => break,
        };
        let text = token.text();
        let text = text.strip_prefix(marker).unwrap_or(text);
        let text = text.strip_prefix(' ').unwrap_or(text);
        lines.push(text.to_string());
    }
    if lines.is_empty() { None } else { Some(lines.join("\n")) }
}
