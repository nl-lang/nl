//! The formatter of `nlc fmt`, on the lossless
//! syntax tree.
//!
//! [`format`] parses a source text and prints its tree again with normalized
//! spacing and indentation; [`format_decl`] does the same for a declaration
//! file. A file with a syntax error is not formatted: its
//! diagnostics come back instead. Warnings do not stop formatting.
//!
//! # Rules
//!
//! The formatter is conservative, like gofmt: it keeps the user's line
//! breaks and never rewrites a construct into another form (a command call
//! stays a command call, a modifier a modifier). What it does:
//!
//! - **Indentation.** Two spaces per level; tabs become spaces. The
//!   statements of a block are one level deeper than the line that opened
//!   the construct (`fn`, `if`, `do`, `struct`, …), and `end` and the clause
//!   keywords `elseif`, `else`, `when`, `catch`, `finally` stand at that
//!   line's indentation. A line that continues a statement inside brackets
//!   (a bracket context) is one level deeper than the line that
//!   opened the bracket, and a closer that starts a line gets the opening
//!   line's indentation; a line that continues a statement outside brackets
//!   (after an operator or `,`, or a line starting with `.` or an
//!   operator) is one level deeper than the statement's
//!   first line. The lines inside a multi-line interpolation of a heredoc
//!   keep their indentation as written.
//! - **Spacing.** One space around binary operators, `=`, `|>`, `is`, `as`,
//!   the `->` of a short function and keywords, after `,` and `;`, and
//!   after the `:` of an annotation, a key, a keyed argument or a return
//!   type (`a: Number`, `{a: 1}`, `f(k: v)`, `fn f(): T`) but none before
//!   it. No space inside `()`,
//!   `[]`, `{}` and `#{}` (except after a trailing comma, `{a: 1, }`),
//!   before the `(` of a call or parameter list, the
//!   `[` of an index or the `<` of type arguments, around `.`, `?.`, `..`
//!   and `...` and the `:` of a method call or a Lua path, after a prefix
//!   operator or a spread `*`, or before a postfix `!`. The braces of a
//!   struct literal and of an import list are spaced inside, with a space
//!   before the brace (`Foo { a: 1 }`, `import { a, b } from 'x'`); a struct
//!   pattern has the space before its brace only (`Circle {r}`); every other
//!   brace is tight (`{a: 1}`, `{x, y}`, `{x: Number}`). `struct A : B` keeps
//!   its spaces. This is the dominant style of example.nl and tests/run.
//! - **Blank lines.** Trailing whitespace is removed, runs of blank lines
//!   are collapsed to one, blank lines at the start of the file are removed,
//!   and the file ends with exactly one line break, of the file's own kind
//!   (LF, CRLF or CR).
//! - **Comments** keep their text and their place. A comment on its own line
//!   takes the indentation of the code it sits with: the statements of its
//!   block, the items of its bracket, the declaration it documents; a
//!   comment line aligned under the trailing comment of the line before
//!   stays aligned with it. A comment at the end of a line keeps the width
//!   of the space before it, so that aligned comments stay aligned.
//! - **Strings, numbers and heredocs** keep their source text, except for
//!   the `--fix` rewrite below.
//!
//! The formatter never changes whether two tokens touch where touching
//! matters (`sp_before`, the "Layout" section of the crate docs): postfix
//! `!`, the four kinds of `:` (each is told apart by
//! the node it is in), `@name`, the `(` of `Function(…)`, and the
//! tokens that must share a line. It never adds or removes a line
//! break between two tokens, so `nl_before` is the same for every token of
//! the output. Three invariants follow, which the tests check on every
//! corpus: the output parses to the same tree (trivia aside) with the same
//! diagnostics, and formatting it again changes nothing.
//!
//! # Continued strings and `--fix`
//!
//! A `"…"` string that a backslash continues on the next line is
//! reported in [`Formatted::continued`] and, with
//! [`FormatOptions::fix`], rewritten as a `"""` heredoc with the same value.
//! A `\` before a line break stands for one `\n`, and the next line
//! is content from its first column, so the heredoc is written without the
//! `-` flag: the opener `"""` ends the line where the string began, each
//! line of the string is a content line as written, and the closer `"""`
//! stands on a line of its own; whatever followed the closing quote follows
//! the closer (`""".strip`, `""", x)`). Every other escape and every `#{…}`
//! stays as written, since a heredoc without `=` has the escapes and the
//! interpolation of a `"…"` string. No content line can start with `"""`, since an
//! unescaped `"` would have closed the `"…"` string; the rewrite still
//! escapes the first quote of such a line, as a safeguard. A continued
//! string inside an interpolation is reported but left alone: a heredoc
//! cannot be opened there.

use rowan::{TextRange, TextSize};

use crate::{Diagnostic, LineIndex, Parse, StringForm, SyntaxKind, SyntaxNode, SyntaxToken, parse, parse_decl_file, string_form};

use SyntaxKind::*;

#[cfg(test)]
mod tests;

/// Spaces per level of indentation.
const INDENT: usize = 2;

/// What [`format`] may change beyond spacing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FormatOptions {
    /// Rewrite each `"…"` string continued with a backslash as a heredoc
    /// (the module docs).
    pub fix: bool,
}

/// A `"…"` string continued on the next line with a backslash, as
/// [`format`] found it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Continued {
    /// The whole literal in the source (its `STRING` token, or
    /// `STR_BEGIN … STR_END`).
    pub range: TextRange,
    /// The string sits inside an interpolation `#{…}`, where a heredoc
    /// cannot be written; it is never fixed.
    pub in_interpolation: bool,
    /// It was rewritten as a heredoc in [`Formatted::text`].
    pub fixed: bool,
}

/// The result of [`format`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Formatted {
    /// The formatted source.
    pub text: String,
    /// Every continued string, in source order.
    pub continued: Vec<Continued>,
}

/// Formats NL source text (the module docs). `Err` holds the diagnostics of
/// a source with syntax errors, which is not formatted.
pub fn format(source: &str, options: FormatOptions) -> Result<Formatted, Vec<Diagnostic>> {
    format_parse(source, &parse(source), options)
}

/// [`format`] for a declaration file (`.d.nl`).
pub fn format_decl(source: &str, options: FormatOptions) -> Result<Formatted, Vec<Diagnostic>> {
    format_parse(source, &parse_decl_file(source), options)
}

fn format_parse(source: &str, parse: &Parse, options: FormatOptions) -> Result<Formatted, Vec<Diagnostic>> {
    if parse.has_errors() {
        return Err(parse.diagnostics().to_vec());
    }
    let mut f = Formatter::new(source, options.fix);
    for element in parse.syntax().descendants_with_tokens() {
        if let Some(token) = element.as_token() {
            f.token(token);
        }
    }
    Ok(f.finish())
}

struct Formatter<'a> {
    src: &'a str,
    index: LineIndex,
    fix: bool,
    /// The file's line break, used for every line break the formatter
    /// writes.
    newline: &'a str,
    out: String,
    /// Something was printed: the next line break is written out.
    started: bool,
    /// The indentation given to each source line (1-based) whose first token
    /// has been printed. Later lines are indented relative to these.
    line_indent: Vec<Option<usize>>,
    /// Line breaks seen since the last printed token or comment.
    pending_newlines: usize,
    /// The whitespace seen since the last line break, token or comment.
    last_ws: Option<&'a str>,
    /// The last printed non-trivia token.
    prev: Option<SyntaxToken>,
    /// The last printed comment: its source column, its new column and its
    /// source line, for a comment line aligned under it.
    last_comment: Option<(usize, usize, usize)>,
    /// The openers of the brackets open at the current position, innermost
    /// last (see [`bracket_step`]): kept as the tokens are printed, so that
    /// no line start rescans its bracket.
    brackets: Vec<SyntaxToken>,
    continued: Vec<Continued>,
}

impl<'a> Formatter<'a> {
    fn new(src: &'a str, fix: bool) -> Formatter<'a> {
        let index = LineIndex::new(src);
        let newline = match src.find(['\n', '\r']) {
            Some(i) if src.get(i..i + 2) == Some("\r\n") => "\r\n",
            Some(i) if src.get(i..i + 1) == Some("\r") => "\r",
            _ => "\n",
        };
        Formatter {
            src,
            line_indent: vec![None; index.line_count() + 2],
            index,
            fix,
            newline,
            out: String::with_capacity(src.len() + src.len() / 8),
            started: false,
            pending_newlines: 0,
            last_ws: None,
            prev: None,
            last_comment: None,
            brackets: Vec::new(),
            continued: Vec::new(),
        }
    }

    fn finish(mut self) -> Formatted {
        if self.started {
            while self.out.ends_with(['\n', '\r']) {
                self.out.pop();
            }
            self.out.push_str(self.newline);
        }
        Formatted { text: self.out, continued: self.continued }
    }

    // ---- Positions -----------------------------------------------------------

    /// The 1-based source line where the token starts.
    fn line_of(&self, t: &SyntaxToken) -> usize {
        self.index.line_col(self.src, t.text_range().start()).line as usize
    }

    /// The source column (in characters) where the token starts.
    fn col_of(&self, t: &SyntaxToken) -> usize {
        self.index.line_col(self.src, t.text_range().start()).col as usize
    }

    fn set_line_indent(&mut self, line: usize, indent: usize) {
        if let Some(slot) = self.line_indent.get_mut(line) {
            *slot = Some(indent);
        }
    }

    /// The new indentation of the line the token starts on. A line whose
    /// first token has not been printed is treated as indented like the
    /// token printed before it (a line that starts inside a heredoc token).
    fn line_indent_of(&self, t: &SyntaxToken) -> usize {
        self.line_indent.get(self.line_of(t)).copied().flatten().unwrap_or(0)
    }

    /// The column the next character will be printed at.
    fn column(&self) -> usize {
        // A byte-order mark on the first line is not a column (as in
        // `LineIndex`).
        self.out.rsplit(['\n', '\r']).next().map_or(0, |l| l.trim_start_matches('\u{FEFF}').chars().count())
    }

    // ---- Printing ------------------------------------------------------------

    fn token(&mut self, t: &SyntaxToken) {
        match t.kind() {
            WHITESPACE => {
                // A byte-order mark is kept (the lexer skips it).
                if t.text_range().start() == TextSize::new(0) && t.text().starts_with('\u{FEFF}') {
                    self.out.push('\u{FEFF}');
                } else {
                    // Borrowed from the source, which outlives the token.
                    self.last_ws = self.src.get(std::ops::Range::<usize>::from(t.text_range()));
                }
            }
            NEWLINE => {
                self.pending_newlines += 1;
                self.last_ws = None;
            }
            COMMENT | DOC_COMMENT => self.comment(t),
            EOF => {}
            _ => self.significant(t),
        }
    }

    /// Starts a new output line: the pending line breaks (at most one blank
    /// line, none at the start of the file), then `indent` spaces.
    fn start_line(&mut self, indent: usize) {
        if self.started {
            for _ in 0..self.pending_newlines.min(2) {
                self.out.push_str(self.newline);
            }
        }
        self.pending_newlines = 0;
        self.out.extend(std::iter::repeat_n(' ', indent));
    }

    /// Starts a new output line keeping the source's indentation (inside a
    /// multi-line interpolation of a heredoc).
    fn start_line_verbatim(&mut self) -> usize {
        let ws = self.last_ws.unwrap_or("");
        self.start_line(0);
        self.out.push_str(ws);
        ws.chars().count()
    }

    fn significant(&mut self, t: &SyntaxToken) {
        let line = self.line_of(t);
        if !self.started || self.pending_newlines > 0 {
            let indent = if in_heredoc_interpolation(t) {
                self.start_line_verbatim()
            } else {
                let indent = self.indent_for(t);
                self.start_line(indent);
                indent
            };
            self.set_line_indent(line, indent);
        } else if let Some(prev) = self.prev.clone() {
            if space_between(&prev, t) {
                self.out.push(' ');
            }
            if self.line_indent.get(line).copied().flatten().is_none() {
                let inherited = self.line_indent_of(&prev);
                self.set_line_indent(line, inherited);
            }
        }
        self.push_text(t);
        let parent = t.parent().map_or(ERROR, |p| p.kind());
        bracket_step(&mut self.brackets, t, parent);
        self.prev = Some(t.clone());
        self.last_ws = None;
        self.started = true;
    }

    fn comment(&mut self, t: &SyntaxToken) {
        let line = self.line_of(t);
        if self.started && self.pending_newlines == 0 {
            // At the end of a line: the width of the space before it stays.
            let width = self.last_ws.map_or(1, |w| w.chars().count().max(1));
            self.out.extend(std::iter::repeat_n(' ', width));
        } else {
            let indent = if in_heredoc_interpolation(t) {
                self.start_line_verbatim()
            } else {
                let indent = self.comment_indent(t);
                self.start_line(indent);
                indent
            };
            self.set_line_indent(line, indent);
        }
        let new_col = self.column();
        self.out.push_str(t.text());
        self.last_comment = Some((self.col_of(t), new_col, line));
        self.last_ws = None;
        self.started = true;
    }

    /// The token's text, or its heredoc rewrite (`--fix`). A continued
    /// string is recorded once, at its first part.
    fn push_text(&mut self, t: &SyntaxToken) {
        let Some(lit) = continued_literal(t) else {
            self.out.push_str(t.text());
            return;
        };
        let in_interpolation = lit.ancestors().skip(1).any(|n| n.kind() == INTERP_STRING);
        let fixed = self.fix && !in_interpolation;
        if matches!(t.kind(), STRING | STR_BEGIN) {
            self.continued.push(Continued { range: lit.text_range(), in_interpolation, fixed });
        }
        if fixed {
            let part = heredoc_part(t, self.newline);
            self.out.push_str(&part);
        } else {
            self.out.push_str(t.text());
        }
    }

    // ---- Indentation ---------------------------------------------------------

    /// The indentation of the line of a construct: the line where its first
    /// token stands.
    fn anchor(&self, node: &SyntaxNode) -> usize {
        first_significant(node).map_or(0, |t| self.line_indent_of(&t))
    }

    /// The indentation of the statements of a block.
    fn block_indent(&self, block: &SyntaxNode) -> usize {
        match block.parent() {
            Some(p) if p.kind() != SOURCE_FILE => self.anchor(&p) + INDENT,
            _ => 0,
        }
    }

    /// The indentation of a statement or a member (see [`is_statement`]).
    fn statement_indent(&self, node: &SyntaxNode) -> usize {
        match node.parent() {
            Some(p) if p.kind() == BLOCK => self.block_indent(&p),
            Some(p) if p.kind() == SOURCE_FILE => 0,
            Some(p) => self.anchor(&p) + INDENT,
            None => 0,
        }
    }

    /// The indentation of a line that continues a statement at `node`
    /// (which holds, or is, the token the line starts with): one level
    /// deeper than the innermost open bracket, or than the statement.
    fn continuation_indent(&self, node: &SyntaxNode) -> usize {
        // The innermost open bracket belongs to one of the ancestors; the
        // statement wins when it lies inside that bracket.
        let opener = self.brackets.last();
        let opener_node = opener.and_then(SyntaxToken::parent);
        let mut node = node.clone();
        loop {
            if opener_node.as_ref() == Some(&node) {
                return opener.map_or(0, |o| self.line_indent_of(o)) + INDENT;
            }
            if is_statement(&node) {
                return first_significant(&node).map_or(0, |t| self.line_indent_of(&t)) + INDENT;
            }
            match node.parent() {
                Some(p) => node = p,
                None => return 0,
            }
        }
    }

    /// The indentation of a line starting with the token `t`.
    fn indent_for(&self, t: &SyntaxToken) -> usize {
        let Some(parent) = t.parent() else { return 0 };
        // A closer stands where its construct or bracket opened.
        match t.kind() {
            END_KW => return self.anchor(&parent),
            ELSE_KW | ELSEIF_KW | WHEN_KW | CATCH_KW | FINALLY_KW if is_clause(parent.kind()) => {
                if let Some(construct) = parent.parent() {
                    return self.anchor(&construct);
                }
            }
            R_PAREN | R_BRACK | R_CURLY | GT | PIPE | STR_MID | STR_END => {
                if let Some(opener) = self.matching_opener(t) {
                    return self.line_indent_of(&opener);
                }
            }
            _ => {}
        }
        // The first token of a statement.
        let mut node = parent.clone();
        while first_significant(&node).as_ref() == Some(t) {
            if is_statement(&node) {
                return self.statement_indent(&node);
            }
            match node.parent() {
                Some(p) => node = p,
                None => break,
            }
        }
        self.continuation_indent(&parent)
    }

    /// The opener the closing token `t` matches, if it is one: the innermost
    /// open bracket, when it is of the pairing kind and in the same node.
    fn matching_opener(&self, t: &SyntaxToken) -> Option<SyntaxToken> {
        let opener = self.brackets.last()?;
        let pair = matches!(
            (opener.kind(), t.kind()),
            (L_PAREN, R_PAREN) | (L_BRACK, R_BRACK) | (L_CURLY, R_CURLY) | (LT, GT) | (PIPE, PIPE) | (STR_BEGIN | STR_MID, STR_MID | STR_END)
        );
        if pair && opener.parent() == t.parent() { Some(opener.clone()) } else { None }
    }

    /// The innermost open bracket, when it is a direct child of `node`.
    fn open_bracket_of(&self, node: &SyntaxNode) -> Option<SyntaxToken> {
        self.brackets.last().filter(|o| o.parent().as_ref() == Some(node)).cloned()
    }

    /// The indentation of a comment on a line of its own.
    fn comment_indent(&self, t: &SyntaxToken) -> usize {
        let line = self.line_of(t);
        // Aligned under the comment of the line before: stays aligned.
        if let Some((src_col, new_col, last_line)) = self.last_comment {
            if last_line + 1 == line && src_col == self.col_of(t) {
                return new_col;
            }
        }
        let Some(parent) = t.parent() else { return 0 };
        let at = t.text_range().start();
        let leads = first_significant(&parent).is_some_and(|f| f.text_range().start() > at);
        match parent.kind() {
            BLOCK => self.block_indent(&parent),
            SOURCE_FILE => 0,
            k if is_member_holder(k) => {
                if leads {
                    first_significant(&parent).map_or(0, |f| self.indent_for(&f))
                } else {
                    self.anchor(&parent) + INDENT
                }
            }
            k if is_clause_holder(k) => self.anchor(&parent),
            _ => {
                if let Some(opener) = self.open_bracket_of(&parent) {
                    self.line_indent_of(&opener) + INDENT
                } else if leads {
                    // A doc comment, or a comment before a declaration's
                    // first token: the declaration's own indentation.
                    first_significant(&parent).map_or(0, |f| self.indent_for(&f))
                } else {
                    self.continuation_indent(&parent)
                }
            }
        }
    }
}

// ---- Tree queries -------------------------------------------------------------

/// The first non-trivia token inside the node.
fn first_significant(node: &SyntaxNode) -> Option<SyntaxToken> {
    node.descendants_with_tokens().filter_map(|e| e.into_token()).find(|t| !t.kind().is_trivia())
}

/// A statement of a block or a declaration file, or a member of a
/// `struct`, `interface`, `enum` or declaration body: what starts a line
/// of its own at the body's indentation.
fn is_statement(node: &SyntaxNode) -> bool {
    let Some(parent) = node.parent() else { return false };
    match parent.kind() {
        BLOCK => true,
        SOURCE_FILE => node.kind() != BLOCK,
        INTERFACE_IMPL => node.kind() == FN_DECL,
        _ => matches!(
            node.kind(),
            FIELD_DECL
                | IFACE_METHOD
                | IFACE_FIELD
                | VARIANT
                | DECL_MODULE
                | DECL_STRUCT
                | DECL_TYPE
                | DECL_FN
                | DECL_VALUE
                | DECL_FIELD
        ),
    }
}

/// A construct whose members are not in a `BLOCK`.
fn is_member_holder(kind: SyntaxKind) -> bool {
    matches!(kind, STRUCT_DECL | INTERFACE_DECL | INTERFACE_IMPL | ENUM_DECL | DECL_MODULE | DECL_STRUCT | DECL_TYPE)
}

/// A construct with clauses (`else`, `when`, `catch`, …) at its own level.
fn is_clause_holder(kind: SyntaxKind) -> bool {
    matches!(kind, IF_EXPR | UNLESS_EXPR | CASE_EXPR | COND_EXPR | TRY_EXPR)
}

fn is_clause(kind: SyntaxKind) -> bool {
    matches!(kind, ELSEIF_CLAUSE | ELSE_CLAUSE | WHEN_CLAUSE | COND_CLAUSE | CATCH_CLAUSE | FINALLY_CLAUSE)
}

/// The `|` that opens block parameters (the first `|` of a
/// `BLOCK_PARAMS`), as opposed to the one that closes them.
fn is_opening_pipe(t: &SyntaxToken) -> bool {
    t.kind() == PIPE
        && t.parent().is_some_and(|p| {
            p.kind() == BLOCK_PARAMS
                && !p
                    .children_with_tokens()
                    .filter_map(|e| e.into_token())
                    .any(|s| s.kind() == PIPE && s.text_range().start() < t.text_range().start())
        })
}

/// Tracks the open brackets as the tokens are printed in source order: an
/// opener is pushed, its closer pops it. `<` and `>` count only in type
/// argument and parameter lists, `|` only in block parameters (where the
/// second one closes the first). Brackets are balanced inside their node
/// (the tree has no errors), so the stack holds exactly the open brackets of
/// the ancestors of the current position, innermost last.
fn bracket_step(stack: &mut Vec<SyntaxToken>, tok: &SyntaxToken, parent: SyntaxKind) {
    match tok.kind() {
        L_PAREN | L_BRACK | L_CURLY | STR_BEGIN => stack.push(tok.clone()),
        LT if matches!(parent, TYPE_ARGS | TYPE_PARAMS) => stack.push(tok.clone()),
        STR_MID => {
            stack.pop();
            stack.push(tok.clone());
        }
        R_PAREN | R_BRACK | R_CURLY | STR_END => {
            stack.pop();
        }
        GT if matches!(parent, TYPE_ARGS | TYPE_PARAMS) => {
            stack.pop();
        }
        PIPE if parent == BLOCK_PARAMS => {
            if stack.last().is_some_and(|s| s.kind() == PIPE) {
                stack.pop();
            } else {
                stack.push(tok.clone());
            }
        }
        _ => {}
    }
}

/// The token lies inside an interpolation of a heredoc (not counting the
/// heredoc's own opening part).
fn in_heredoc_interpolation(t: &SyntaxToken) -> bool {
    let Some(parent) = t.parent() else { return false };
    let skip = usize::from(t.kind() == STR_BEGIN);
    parent.ancestors().skip(skip).any(|n| {
        n.kind() == INTERP_STRING && n.first_token().is_some_and(|f| f.text().starts_with("\"\"\""))
    })
}

// ---- Spacing ------------------------------------------------------------------

/// Whether one space separates two tokens printed on the same line (the
/// module docs). Where the parser reads the spacing (postfix
/// `!`, the kinds of `:`, `Function(`), the node a token is in says which reading it had.
fn space_between(prev: &SyntaxToken, cur: &SyntaxToken) -> bool {
    let (pk, ck) = (prev.kind(), cur.kind());
    let pp = prev.parent().map(|n| n.kind());
    let cp = cur.parent().map(|n| n.kind());
    // `#{x}` is tight.
    if matches!(pk, STR_BEGIN | STR_MID) || matches!(ck, STR_MID | STR_END) {
        return false;
    }
    // After an opener.
    match pk {
        L_PAREN | L_BRACK => return false,
        L_CURLY => return ck != R_CURLY && matches!(pp, Some(STRUCT_LIT | IMPORT_LIST)),
        LT if matches!(pp, Some(TYPE_ARGS | TYPE_PARAMS)) => return false,
        // `| |` must not become the one token `||`.
        PIPE if is_opening_pipe(prev) => return ck == PIPE,
        _ => {}
    }
    // After a comma, also before a closer: `{a: 1, }` rather than `{a: 1,}`.
    if matches!(pk, COMMA | SEMI) {
        return !matches!(ck, COMMA | SEMI);
    }
    // Before a closer, `,` or `;`.
    match ck {
        R_PAREN | R_BRACK | COMMA | SEMI => return false,
        R_CURLY => return matches!(cp, Some(STRUCT_LIT | IMPORT_LIST)),
        GT if matches!(cp, Some(TYPE_ARGS | TYPE_PARAMS)) => return false,
        PIPE if cp == Some(BLOCK_PARAMS) && !is_opening_pipe(cur) => return false,
        _ => {}
    }
    // Members, Lua steps and ranges: `a.b`, `:a.b`, `1..10`.
    if matches!(pk, DOT | DOT2 | DOT3) || matches!(ck, DOT | DOT2 | DOT3) {
        return false;
    }
    // Nil-safe steps: `a?.b`, `a?.[k]`, `f?.(x)`, `o?.:m()`.
    if pk == QUESTION_DOT || ck == QUESTION_DOT {
        return false;
    }
    // The colons. A method call's touches both sides, a Lua path's
    // the word after it, an annotation's or key's only the name before it;
    // `struct A : B` keeps both spaces.
    if ck == COLON && cp != Some(LUA_PATH) {
        return cp == Some(STRUCT_PARENT);
    }
    if pk == COLON {
        return !matches!(pp, Some(METHOD_CALL | LUA_METHOD | LUA_PATH));
    }
    // Prefix operators and spreads touch their operand; two minus signs
    // stay apart (`- -a`), since `--a` reads as something else.
    if pk == MINUS && ck == MINUS {
        return true;
    }
    if matches!(pk, MINUS | TILDE | BANG | BANG_POSTFIX) && matches!(pp, Some(PREFIX_EXPR | NOT_TYPE | LITERAL_PAT)) {
        return false;
    }
    if matches!(pk, STAR | STAR2)
        && matches!(pp, Some(SPREAD_ARG | KEYED_SPREAD_ARG | REST_PARAM | KEYED_PARAM | REST_PAT | FN_TYPE_PARAM))
    {
        return false;
    }
    // Postfix `!` and `?` touch what they follow.
    if matches!(ck, BANG | BANG_POSTFIX) && cp == Some(BANG_EXPR) {
        return false;
    }
    if ck == QUESTION && cp == Some(OPTIONAL_TYPE) {
        return false;
    }
    // Calls, parameter lists, indexes, type arguments and operator names.
    if ck == L_PAREN && matches!(cp, Some(ARG_LIST | PARAM_LIST | FN_TYPE_PARAMS)) {
        return false;
    }
    if ck == L_BRACK && matches!(cp, Some(INDEX_EXPR | LUA_INDEX)) {
        return false;
    }
    if ck == LT && matches!(cp, Some(TYPE_ARGS | TYPE_PARAMS)) {
        return false;
    }
    if cp == Some(OP_NAME) && pp == Some(OP_NAME) {
        return false;
    }
    true
}

// ---- Continued strings --------------------------------------------------------

/// The literal node (`LITERAL` or `INTERP_STRING`) of a string part that
/// belongs to a `"…"` string continued with a backslash; `None` for any
/// other token.
fn continued_literal(t: &SyntaxToken) -> Option<SyntaxNode> {
    if !t.kind().is_string_part() {
        return None;
    }
    let lit = t.parent()?;
    let parts: Vec<SyntaxToken> = lit.children_with_tokens().filter_map(|e| e.into_token()).filter(|t| t.kind().is_string_part()).collect();
    let first = parts.first()?;
    if string_form(first.text()) != Some(StringForm::Quoted) {
        return None;
    }
    if parts.iter().any(|p| has_continuation(p.text())) { Some(lit) } else { None }
}

/// A backslash directly before a line break in the text of a `"…"` part
/// (an escaped backslash before a line break is not one).
fn has_continuation(text: &str) -> bool {
    let b = text.as_bytes();
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        if c == b'\\' {
            if matches!(b.get(i + 1), Some(b'\n' | b'\r')) {
                return true;
            }
            i += 2;
        } else {
            i += 1;
        }
    }
    false
}

/// The heredoc text of one part of a continued `"…"` string (the module
/// docs). The part's delimiters become the heredoc's; the line
/// continuations become line breaks.
fn heredoc_part(t: &SyntaxToken, newline: &str) -> String {
    let text = t.text();
    let (opens, body, closes) = match t.kind() {
        STRING => (true, text.get(1..text.len().saturating_sub(1)), true),
        STR_BEGIN => (true, text.get(1..), false),
        STR_END => (false, text.get(..text.len().saturating_sub(1)), true),
        _ => (false, Some(text), false),
    };
    let body = body.unwrap_or("");
    let mut out = String::with_capacity(body.len() + 8);
    if opens {
        out.push_str("\"\"\"");
        out.push_str(newline);
    }
    out.push_str(&content_lines(body, opens));
    if closes {
        out.push_str(newline);
        out.push_str("\"\"\"");
    }
    out
}

/// `body` with each `\` + line break replaced by the line break, and a
/// `\` before a `"""` that would start a content line. `at_line_start`:
/// the body begins a content line (rather than continuing one after `}`).
fn content_lines(body: &str, at_line_start: bool) -> String {
    let b = body.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(b.len());
    let mut line_start = at_line_start;
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        if line_start {
            // A safeguard only: a `"…"` string holds no unescaped `"`, so no
            // content line of the rewrite can start with `"""`.
            line_start = false;
            let rest = b.get(i..).unwrap_or(&[]);
            let blanks = rest.iter().take_while(|&&x| x == b' ' || x == b'\t').count();
            if rest.get(blanks..).is_some_and(|r| r.starts_with(b"\"\"\"")) {
                out.extend_from_slice(rest.get(..blanks).unwrap_or(&[]));
                out.push(b'\\');
                i += blanks;
                continue;
            }
        }
        if c == b'\\' {
            match b.get(i + 1) {
                Some(b'\r') if b.get(i + 2) == Some(&b'\n') => {
                    out.extend_from_slice(b"\r\n");
                    i += 3;
                    line_start = true;
                }
                Some(&lb @ (b'\n' | b'\r')) => {
                    out.push(lb);
                    i += 2;
                    line_start = true;
                }
                Some(&e) => {
                    out.push(c);
                    out.push(e);
                    i += 2;
                }
                None => {
                    out.push(c);
                    i += 1;
                }
            }
        } else {
            out.push(c);
            i += 1;
        }
    }
    // Only ASCII bytes were inserted or dropped, so the text is still UTF-8;
    // should that ever fail, the string is left as written.
    String::from_utf8(out).unwrap_or_else(|_| body.to_string())
}
