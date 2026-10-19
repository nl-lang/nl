//! Events to a `rowan` green tree, with every lexer token in it.
//!
//! The parser's events cover the tokens it consumed. This builder interleaves
//! the rest of the lexer's tokens, all of them trivia (whitespace, line
//! breaks, comments) or lexer `ERROR` tokens, at their place in the source:
//!
//! - trivia before a node's first token goes before the node (into the
//!   parent), except for a `BLOCK`, which takes the trivia before its first
//!   statement and the trivia before the token that ends it (`end`, `else`,
//!   …), so that comments in a body are inside the body. The `BLOCK` that
//!   holds the expression body of a short function (one directly in a
//!   `LAMBDA_EXPR`, not after `do`; see [`SyntaxKind::LAMBDA_EXPR`]) has no
//!   closing token: it is treated as any other node, so the comments and
//!   blank lines after the expression stay with what follows;
//! - a doc comment directly before a declaration, which
//!   is a `##` line and the comment lines after it, goes into the
//!   declaration's node, as its first children. [`crate::doc_comment`] reads
//!   it back;
//! - trivia at the end of the file goes before `EOF`.
//!
//! The builder creates the root itself and always closes every node, so the
//! tree is well formed whatever the events are, and its text is the source.

use rowan::{GreenNode, GreenNodeBuilder, TextRange};

use super::event::Event;
use crate::{Lexed, SyntaxKind};

use SyntaxKind::*;

/// Node kinds that a doc comment attaches to (`fn`, `struct`, a
/// struct field, `interface`, an interface member, `enum`, an enum variant,
/// `type`, `module`, `const`, `let`, `global`, `on`).
pub(crate) fn takes_doc_comment(kind: SyntaxKind) -> bool {
    matches!(
        kind,
        FN_DECL
            | STRUCT_DECL
            | FIELD_DECL
            | INTERFACE_DECL
            | INTERFACE_IMPL
            | IFACE_METHOD
            | IFACE_FIELD
            | ENUM_DECL
            | VARIANT
            | TYPE_DECL
            | MODULE_DECL
            | CONST_DECL
            | LET_DECL
            | GLOBAL_DECL
            | ON_EXPR
            | DECL_MODULE
            | DECL_STRUCT
            | DECL_TYPE
            | DECL_FN
            | DECL_VALUE
            | DECL_FIELD
    )
}

struct Builder<'a> {
    lexed: &'a Lexed<'a>,
    inner: GreenNodeBuilder<'static>,
    /// Next lexer token not yet in the tree.
    lex_idx: usize,
    /// Open nodes, the root included.
    depth: usize,
    /// The open nodes below the root: each one's kind, and whether it is a
    /// `BLOCK` that takes the trivia before its closing token.
    stack: Vec<(SyntaxKind, bool)>,
    /// The kind of the last token the parser consumed.
    last_token: Option<SyntaxKind>,
    /// A doc comment run is being held back for a declaration node that
    /// opens before the next token.
    doc_reserved: bool,
}

pub(crate) fn build_tree(lexed: &Lexed<'_>, mut events: Vec<Event>) -> GreenNode {
    let mut b = Builder {
        lexed,
        inner: GreenNodeBuilder::new(),
        lex_idx: 0,
        depth: 0,
        stack: Vec::new(),
        last_token: None,
        doc_reserved: false,
    };
    b.inner.start_node(SOURCE_FILE.into());
    b.depth = 1;
    // The parser's own root `SOURCE_FILE` is replaced by the builder's.
    let mut skipped_root = false;
    let mut kinds: Vec<SyntaxKind> = Vec::new();
    for i in 0..events.len() {
        let Some(event) = events.get_mut(i).map(|e| std::mem::replace(e, TOMBSTONE)) else {
            continue;
        };
        match event {
            Event::Start { kind, forward_parent } => {
                kinds.clear();
                if let Some(kind) = kind {
                    kinds.push(kind);
                }
                let mut fp = forward_parent;
                let mut idx = i;
                while let Some(distance) = fp {
                    idx = idx.saturating_add(distance as usize);
                    fp = None;
                    if let Some(parent) = events.get_mut(idx) {
                        if let Event::Start { kind, forward_parent } = std::mem::replace(parent, TOMBSTONE) {
                            if let Some(kind) = kind {
                                kinds.push(kind);
                            }
                            fp = forward_parent;
                        }
                    }
                }
                // Parents first.
                let count = kinds.len();
                for n in (0..count).rev() {
                    let Some(&kind) = kinds.get(n) else { continue };
                    if kind == SOURCE_FILE && !skipped_root && b.depth == 1 {
                        skipped_root = true;
                        continue;
                    }
                    let later = kinds.get(..n).is_some_and(|ks| ks.iter().any(|&k| takes_doc_comment(k)))
                        || later_starts_take_doc(&events, i + 1);
                    b.open(kind, later);
                }
            }
            Event::Finish => b.close(),
            Event::Token { kind, range } => b.token(kind, range),
        }
    }
    // Everything not yet in the tree (normally nothing) goes into the root.
    while b.depth > 1 {
        b.close_node();
    }
    b.flush_all_remaining();
    b.inner.finish_node();
    b.inner.finish()
}

const TOMBSTONE: Event = Event::Start { kind: None, forward_parent: None };

/// A run of `Start` events from `from` (before the next token) contains a
/// node that takes a doc comment.
fn later_starts_take_doc(events: &[Event], from: usize) -> bool {
    for e in events.iter().skip(from) {
        match e {
            Event::Start { kind, .. } => {
                if kind.is_some_and(takes_doc_comment) {
                    return true;
                }
            }
            _ => return false,
        }
    }
    false
}

impl Builder<'_> {
    fn lex_kind(&self, i: usize) -> Option<SyntaxKind> {
        self.lexed.tokens.get(i).map(|t| t.kind)
    }

    /// The lexer token at `i` is one the parser does not consume.
    fn is_pending(&self, i: usize) -> bool {
        self.lex_kind(i).is_some_and(|k| k.is_trivia() || k == ERROR)
    }

    /// End (exclusive) of the run of pending tokens at `lex_idx`.
    fn pending_end(&self) -> usize {
        let mut j = self.lex_idx;
        while self.is_pending(j) {
            j += 1;
        }
        j
    }

    fn emit_lexer_token(&mut self, i: usize) {
        if let Some(t) = self.lexed.tokens.get(i) {
            self.inner.token(t.kind.into(), t.text(self.lexed.source));
        }
    }

    /// Emits pending tokens up to (not including) lexer index `end`.
    fn flush_to(&mut self, end: usize) {
        while self.lex_idx < end && self.is_pending(self.lex_idx) {
            self.emit_lexer_token(self.lex_idx);
            self.lex_idx += 1;
        }
    }

    fn flush_all(&mut self) {
        let end = self.pending_end();
        self.flush_to(end);
        self.doc_reserved = false;
    }

    fn flush_all_remaining(&mut self) {
        while self.lex_idx < self.lexed.tokens.len() {
            self.emit_lexer_token(self.lex_idx);
            self.lex_idx += 1;
        }
    }

    /// Start (lexer index) of the doc comment at the end of the pending
    /// trivia, or the end of the pending trivia if there is none. The
    /// comment lines directly above the declaration, each first on its line,
    /// form one block; the doc comment runs from the block's first `##` line
    /// to the declaration, `#` lines after that `##` line included.
    fn doc_run_start(&self) -> usize {
        let lo = self.lex_idx;
        let end = self.pending_end();
        let is = |i: usize, k: SyntaxKind| i >= lo && i < end && self.lex_kind(i) == Some(k);
        let mut j = end;
        if j > lo && is(j - 1, WHITESPACE) {
            j -= 1;
        }
        if !(j > lo && is(j - 1, NEWLINE)) {
            return end;
        }
        j -= 1; // `j` is the line break before the declaration
        let mut run_start = end;
        loop {
            if !(j > lo && (is(j - 1, DOC_COMMENT) || is(j - 1, COMMENT))) {
                break;
            }
            let comment = j - 1;
            let mut k = comment;
            if k > lo && is(k - 1, WHITESPACE) {
                k -= 1;
            }
            // The comment is first on its line, or at the very start of the
            // file.
            let line_start = k > lo && is(k - 1, NEWLINE);
            if !line_start && k != 0 {
                break;
            }
            if is(comment, DOC_COMMENT) {
                run_start = comment;
            }
            if !line_start {
                break;
            }
            j = k - 1;
        }
        run_start
    }

    /// A `BLOCK` opened now is the expression body of a short function:
    /// directly in a `LAMBDA_EXPR`, and not after `do`.
    fn opens_expression_body(&self) -> bool {
        self.stack.last().is_some_and(|&(k, _)| k == LAMBDA_EXPR) && self.last_token != Some(DO_KW)
    }

    fn open(&mut self, kind: SyntaxKind, later_takes_doc: bool) {
        if kind == BLOCK && !self.opens_expression_body() {
            self.start_node(kind);
            return;
        }
        if self.doc_reserved {
            self.start_node(kind);
            if takes_doc_comment(kind) {
                self.flush_all();
            }
            return;
        }
        if takes_doc_comment(kind) {
            let split = self.doc_run_start();
            self.flush_to(split);
            self.start_node(kind);
            self.flush_all();
        } else if later_takes_doc {
            let split = self.doc_run_start();
            self.flush_to(split);
            self.doc_reserved = self.pending_end() > self.lex_idx;
            self.start_node(kind);
        } else {
            self.flush_all();
            self.start_node(kind);
        }
    }

    fn start_node(&mut self, kind: SyntaxKind) {
        self.inner.start_node(kind.into());
        let takes_closing_trivia = kind == BLOCK && !self.opens_expression_body();
        self.stack.push((kind, takes_closing_trivia));
        self.depth += 1;
    }

    fn close_node(&mut self) {
        if self.stack.pop().is_some_and(|(_, closing)| closing) && !self.doc_reserved {
            self.flush_all();
        }
        self.inner.finish_node();
        self.depth -= 1;
    }

    fn close(&mut self) {
        if self.depth > 1 {
            self.close_node();
        }
    }

    fn token(&mut self, kind: SyntaxKind, range: TextRange) {
        // Pending tokens that lie before this one.
        while self.is_pending(self.lex_idx)
            && self.lexed.tokens.get(self.lex_idx).is_some_and(|t| t.range.start() < range.start())
        {
            self.emit_lexer_token(self.lex_idx);
            self.lex_idx += 1;
        }
        self.doc_reserved = false;
        self.last_token = Some(kind);
        let text = self.lexed.source.get(std::ops::Range::<usize>::from(range)).unwrap_or("");
        self.inner.token(kind.into(), text);
        if self.lexed.tokens.get(self.lex_idx).is_some_and(|t| t.range.end() <= range.end()) {
            self.lex_idx += 1;
        }
    }
}
