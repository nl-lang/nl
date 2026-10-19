//! The parser: tokens to a lossless syntax tree.
//!
//! # Structure
//!
//! - this module: the [`Parser`] state, token access with the newline
//!   contexts, diagnostics, the nesting limit and the
//!   indentation-guided recovery for a missing `end`;
//! - [`event`]: events and markers, from which [`build`] makes the tree;
//! - `grammar/`: one module per part of the grammar, with functions named
//!   after the productions (`stmt`, `decl`, `control`, `expr`, `postfix`,
//!   `primary`, `lua_path`, `types`, `patterns`).
//!
//! # Robustness
//!
//! The parser never panics, and it always terminates:
//!
//! - every loop either consumes a token or exits; as a backstop, a "fuel"
//!   counter is spent by every look at the current token and refilled by
//!   every consumed token. If it runs out, the parser stops looking at the
//!   input (every token then reads as `EOF`), reports one internal error, and
//!   puts the rest of the tokens into an `ERROR_NODE`;
//! - nesting is bounded (see [`NESTING_LIMIT`], [`RECURSION_LIMIT`],
//!   [`TREE_DEPTH_LIMIT`]). Past a limit, the parser reports one error and
//!   skips the rest of the construct it was in, as a balanced group of tokens
//!   (brackets and block keywords matched with their closers), into one
//!   `ERROR_NODE`; then it goes on with the enclosing construct. At most one
//!   such error is reported per statement of the file's top level. Long
//!   left-associative chains, right-associative chains and runs of prefix
//!   operators are parsed by loops, not recursion.

mod build;
mod event;
mod grammar;

use std::cell::Cell;

use rowan::{GreenNode, TextRange, TextSize};

use crate::{Diagnostic, Lexed, SyntaxKind, lex};
use event::{CompletedMarker, Event, Marker};

use SyntaxKind::*;

/// The nesting limit users see: open brackets and block constructs, each
/// one level (the newline context stack without the file's
/// own context). A bracket is `(`, `[`, `{`, `<…>` of type arguments or
/// parameters, `|…|` of block parameters or `#{…}`; a block construct is
/// anything closed by `end`. So 128 nested `if`s or parentheses parse, and 64
/// nested `f(fn() … end)` (two levels each).
pub(crate) const NESTING_LIMIT: usize = 128;
/// Recursion of the parser's functions, a safety net for the stack. Ordinary
/// nesting costs at most about 3 per level of [`NESTING_LIMIT`] (a block, a
/// statement, an expression), so only pathological input reaches it (for
/// example thousands of prefix operators mixed with junk tokens).
pub(crate) const RECURSION_LIMIT: u32 = 448;
/// Depth of the tree being built: recursion plus operator and postfix chains
/// (`a + b + …` or `x.a.b.…` is one level per link). `rowan` frees a tree
/// recursively, so this bounds the stack used then; an expression with more
/// than about this many chained operators reaches it.
pub(crate) const TREE_DEPTH_LIMIT: u32 = 1000;
/// Looks at the input without consuming a token before the parser decides it
/// is stuck.
const FUEL: u32 = 100_000;

/// Closer keywords a block construct accepts, as bits.
pub(crate) mod closer {
    pub const END: u8 = 1;
    pub const ELSE: u8 = 2;
    pub const ELSEIF: u8 = 4;
    pub const WHEN: u8 = 8;
    pub const CATCH: u8 = 16;
    pub const FINALLY: u8 = 32;
}

/// The closer bit of a keyword, 0 for other tokens.
fn closer_bit(kind: SyntaxKind) -> u8 {
    match kind {
        END_KW => closer::END,
        ELSE_KW => closer::ELSE,
        ELSEIF_KW => closer::ELSEIF,
        WHEN_KW => closer::WHEN,
        CATCH_KW => closer::CATCH,
        FINALLY_KW => closer::FINALLY,
        _ => 0,
    }
}

/// One significant token as the parser sees it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Tok {
    pub kind: SyntaxKind,
    pub range: TextRange,
    /// A line break lies before it. Lexer `ERROR` tokens that the
    /// parser skips pass their flags on to the next token.
    pub nl: bool,
    /// Whitespace, a comment or a line break lies before it.
    pub sp: bool,
    /// Column (in bytes; indentation is ASCII) of the first token of its line.
    pub indent: u32,
    /// Its own column.
    pub col: u32,
    /// Its 1-based line, for messages.
    pub line: u32,
    /// A lexer `ERROR` token (already reported) lies directly before it, or
    /// it is the empty `STR_END` with which the lexer cuts off an unclosed
    /// interpolation (also reported). A missing operand here is not reported
    /// again.
    pub after_error: bool,
}

/// One entry of the newline context stack, with the
/// restrictions that hold for expressions parsed directly in it.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Ctx {
    /// A statement context (else a bracket context, where line breaks are
    /// ignored).
    pub stmt: bool,
    /// Parsing the arguments of a command call: a trailing block belongs to
    /// the command call, and a command call here is nested.
    pub cmd_args: bool,
    /// Parsing a header expression: `then` never starts a command argument,
    /// and a `do` at the end of the line is not a trailing block.
    pub header: bool,
    /// Parsing the iterable of a `for`: `step` never starts a command
    /// argument.
    pub for_iter: bool,
    /// In block parameters: a top-level `|` closes the list.
    pub no_union: bool,
}

/// An open block construct, for matching `end` and the closer keywords.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Construct {
    /// Token index of the opening keyword.
    pub opener: usize,
    /// Indentation of the opener's line.
    pub indent: u32,
    /// Its header holds a `do … end` without parameters, all on the
    /// header's line: Lua's `while x do y end`. Used for the message when
    /// the construct then has no `end`.
    pub header_do: Option<TextRange>,
}

pub(crate) struct Parser<'a> {
    src: &'a str,
    toks: Vec<Tok>,
    pos: usize,
    pub(crate) events: Vec<Event>,
    diags: Vec<Diagnostic>,
    ctx: Vec<Ctx>,
    /// Open bracket contexts by closer: `)`, `]`, `}`, interpolation, `|`, `>`.
    open_brackets: [u32; 6],
    constructs: Vec<Construct>,
    /// How many open constructs accept each closer keyword, by bit index.
    open_closers: [u32; 6],
    rec_depth: u32,
    tree_depth: u32,
    fuel: Cell<u32>,
    stuck: Cell<bool>,
    /// A nesting or length limit was reported in the current top-level
    /// statement (one error per statement).
    depth_error_reported: bool,
    /// A statement other than `import` was seen at the file's top level:
    /// an `import` from here on is misplaced.
    pub(crate) past_imports: bool,
    /// Token indices where a postfix chain or Lua path that holds a `?.`
    /// step starts: such a chain is an assignment target only
    /// in an assignment of its own.
    pub(crate) safe_chain_starts: std::collections::HashSet<usize>,
    /// The misplaced `import` was reported (once per file).
    pub(crate) import_order_reported: bool,
    /// Start offsets of the errors reported, for "one error per position".
    error_starts: std::collections::HashSet<TextSize>,
    /// Positions where an indentation hint closes constructs (token index,
    /// column); sorted.
    hints: Vec<(usize, u32)>,
    /// Openers of constructs whose `end` was missing at the end of the file.
    pub(crate) unclosed_at_eof: Vec<usize>,
    /// After an error about a missing statement separator, further ones are
    /// not reported until the next line.
    pub(crate) sep_error_reported: bool,
    /// Missing-`end` errors reported (to compare runs).
    pub(crate) missing_end_errors: usize,
    /// Token index where the expression body of the innermost short
    /// function being parsed starts: a command call there gets a
    /// targeted error.
    pub(crate) lambda_body: Option<usize>,
}

/// Index into `open_brackets`.
fn bracket_slot(closer: SyntaxKind) -> usize {
    match closer {
        R_PAREN => 0,
        R_BRACK => 1,
        R_CURLY => 2,
        STR_END | STR_MID => 3,
        PIPE => 4,
        _ => 5,
    }
}

/// The result of one parser run, before choosing between runs.
pub(crate) struct RunOutput {
    pub green: GreenNode,
    pub diags: Vec<Diagnostic>,
    pub unclosed_at_eof: Vec<usize>,
    pub stuck: bool,
    /// Errors other than a missing `end`.
    pub other_errors: usize,
}

/// Parses `source` into a green tree and the sorted diagnostics (lexer and
/// parser).
pub(crate) fn parse_source(source: &str) -> (GreenNode, Vec<Diagnostic>) {
    let lexed = lex(source);
    let toks = significant_tokens(&lexed);
    let mut best = run(&lexed, toks.clone(), Vec::new());
    // A missing `end` is only certain at the end of the file. Then the
    // indentation shows where it was probably missing: parse again, closing
    // the constructs there, and keep that parse if it is not worse (see
    // `indentation_hints`).
    let mut hints: Vec<(usize, u32)> = Vec::new();
    for _ in 0..2 {
        if best.stuck || best.unclosed_at_eof.is_empty() {
            break;
        }
        let new = indentation_hints(&toks, &best.unclosed_at_eof, &hints);
        if new.is_empty() {
            break;
        }
        hints.extend(new);
        hints.sort_unstable();
        hints.dedup_by_key(|h| h.0);
        let candidate = run(&lexed, toks.clone(), hints.clone());
        // Missing `end`s are expected to move, and to be reported one by one
        // instead of together at the end of the file; any other error counts.
        if candidate.other_errors <= best.other_errors {
            best = candidate;
        } else {
            break;
        }
    }
    let mut diags = lexed.diagnostics.clone();
    diags.extend(best.diags);
    diags.sort_by_key(|d| (d.range.start(), d.range.end()));
    (best.green, diags)
}

/// Parses a declaration file into a green tree and the sorted
/// diagnostics. One run: the indentation hints of [`parse_source`] serve
/// nested blocks, which a declaration file has few of.
pub(crate) fn parse_decl_source(source: &str) -> (GreenNode, Vec<Diagnostic>) {
    let lexed = lex(source);
    let toks = significant_tokens(&lexed);
    let mut p = Parser::new(lexed.source, toks, Vec::new());
    grammar::decl_file(&mut p);
    let green = build::build_tree(&lexed, std::mem::take(&mut p.events));
    let mut diags = lexed.diagnostics.clone();
    diags.extend(p.diags);
    diags.sort_by_key(|d| (d.range.start(), d.range.end()));
    (green, diags)
}

fn run(lexed: &Lexed<'_>, toks: Vec<Tok>, hints: Vec<(usize, u32)>) -> RunOutput {
    let mut p = Parser::new(lexed.source, toks, hints);
    grammar::source_file(&mut p);
    let green = build::build_tree(lexed, std::mem::take(&mut p.events));
    let errors = p.diags.iter().filter(|d| d.is_error()).count();
    let other_errors = errors.saturating_sub(p.missing_end_errors);
    let stuck = p.stuck.get();
    RunOutput { green, diags: p.diags, unclosed_at_eof: p.unclosed_at_eof, stuck, other_errors }
}

/// The parser's token stream: the non-trivia tokens without the lexer's
/// `ERROR` tokens (already reported; the tree builder puts them back as they
/// are), with columns. Always ends with `EOF`.
fn significant_tokens(lexed: &Lexed<'_>) -> Vec<Tok> {
    let mut out = Vec::with_capacity(lexed.tokens.len() / 2 + 1);
    let mut line_start: u32 = 0;
    let mut line_indent: Option<u32> = None;
    let mut carry_nl = false;
    let mut carry_sp = false;
    let mut carry_err = false;
    let mut line: u32 = 1;
    // A byte-order mark is not part of the first line's indentation (S7).
    if lexed.source.starts_with('\u{FEFF}') {
        line_start = 3;
    }
    for t in &lexed.tokens {
        if t.kind == NEWLINE {
            line_start = t.range.end().into();
            line_indent = None;
            line = line.saturating_add(1);
            continue;
        }
        if t.kind.is_trivia() {
            continue;
        }
        let token_line = line;
        if t.kind.is_string_part() {
            // A heredoc spans lines inside one token.
            line = line.saturating_add(count_line_breaks(t.text(lexed.source)));
        }
        let start: u32 = t.range.start().into();
        let col = start.saturating_sub(line_start);
        let indent = *line_indent.get_or_insert(col);
        if t.kind == ERROR {
            carry_nl |= t.nl_before;
            carry_sp = true;
            carry_err = true;
            continue;
        }
        out.push(Tok {
            kind: t.kind,
            range: t.range,
            nl: t.nl_before || carry_nl,
            sp: t.sp_before || carry_sp,
            indent,
            col,
            line: token_line,
            after_error: carry_err || (t.kind == STR_END && t.range.is_empty()),
        });
        carry_nl = false;
        carry_sp = false;
        carry_err = false;
    }
    if out.last().is_none_or(|t| t.kind != EOF) {
        let end = TextSize::of(lexed.source);
        let end = if lexed.tokens.is_empty() { TextSize::new(0) } else { end };
        out.push(Tok {
            kind: EOF,
            range: TextRange::empty(end),
            nl: true,
            sp: true,
            indent: 0,
            col: 0,
            line,
            after_error: false,
        });
    }
    out
}

/// Line breaks (LF, CRLF or CR) in `text`.
fn count_line_breaks(text: &str) -> u32 {
    let b = text.as_bytes();
    let mut n: u32 = 0;
    for (i, &c) in b.iter().enumerate() {
        if c == b'\n' || (c == b'\r' && b.get(i + 1) != Some(&b'\n')) {
            n = n.saturating_add(1);
        }
    }
    n
}

/// Where a missing `end` probably was, from indentation (only used once the
/// file is known to lack an `end`).
///
/// For each construct still open at the end of the file, outermost first: the
/// first later line whose first token is at most as indented as the
/// construct's opening line, and which starts a statement or is a closer
/// keyword. Parsing again, the constructs at least that indented are closed
/// there (for an `end`, those more indented, so that the `end` closes its own
/// construct).
fn indentation_hints(toks: &[Tok], unclosed: &[usize], existing: &[(usize, u32)]) -> Vec<(usize, u32)> {
    let mut out: Vec<(usize, u32)> = Vec::new();
    let mut openers = unclosed.to_vec();
    openers.sort_unstable();
    for opener in openers {
        let Some(open) = toks.get(opener) else { continue };
        let indent = open.indent;
        // Skip to the line after the opener.
        let mut i = opener + 1;
        while toks.get(i).is_some_and(|t| !t.nl) {
            i += 1;
        }
        while let Some(t) = toks.get(i) {
            if t.kind == EOF {
                break;
            }
            if t.nl
                && t.col <= indent
                && (closer_bit(t.kind) != 0 || grammar::can_start_statement(t.kind))
            {
                if !existing.iter().chain(out.iter()).any(|h| h.0 == i) {
                    out.push((i, t.col));
                }
                break;
            }
            i += 1;
        }
    }
    out
}

impl<'a> Parser<'a> {
    fn new(src: &'a str, toks: Vec<Tok>, hints: Vec<(usize, u32)>) -> Parser<'a> {
        Parser {
            src,
            toks,
            pos: 0,
            events: Vec::new(),
            diags: Vec::new(),
            ctx: vec![Ctx { stmt: true, ..Ctx::default() }],
            open_brackets: [0; 6],
            constructs: Vec::new(),
            open_closers: [0; 6],
            rec_depth: 0,
            tree_depth: 0,
            fuel: Cell::new(FUEL),
            stuck: Cell::new(false),
            depth_error_reported: false,
            past_imports: false,
            safe_chain_starts: std::collections::HashSet::new(),
            import_order_reported: false,
            error_starts: std::collections::HashSet::new(),
            hints,
            unclosed_at_eof: Vec::new(),
            sep_error_reported: false,
            missing_end_errors: 0,
            lambda_body: None,
        }
    }

    // ---- Token access --------------------------------------------------------

    fn halted(&self) -> bool {
        self.stuck.get()
    }

    fn spend_fuel(&self) {
        let fuel = self.fuel.get();
        if fuel == 0 {
            self.stuck.set(true);
        } else {
            self.fuel.set(fuel - 1);
        }
    }

    fn eof_tok(&self) -> Tok {
        self.toks.last().copied().unwrap_or(Tok {
            kind: EOF,
            range: TextRange::default(),
            nl: true,
            sp: true,
            indent: 0,
            col: 0,
            line: 1,
            after_error: false,
        })
    }

    /// The token `n` positions ahead, for lookahead scans: spends no fuel, so
    /// a long scan is not taken for a parser that is stuck.
    pub(crate) fn peek(&self, n: usize) -> Tok {
        if self.halted() {
            return self.eof_tok();
        }
        self.toks.get(self.pos.saturating_add(n)).copied().unwrap_or_else(|| self.eof_tok())
    }

    /// The token `n` positions ahead (`EOF` past the end, or once halted).
    pub(crate) fn tok(&self, n: usize) -> Tok {
        self.spend_fuel();
        if self.halted() {
            return self.eof_tok();
        }
        self.toks.get(self.pos + n).copied().unwrap_or_else(|| self.eof_tok())
    }

    pub(crate) fn nth(&self, n: usize) -> SyntaxKind {
        self.tok(n).kind
    }

    pub(crate) fn cur(&self) -> SyntaxKind {
        self.nth(0)
    }

    pub(crate) fn at(&self, kind: SyntaxKind) -> bool {
        self.cur() == kind
    }

    pub(crate) fn nth_at(&self, n: usize, kind: SyntaxKind) -> bool {
        self.nth(n) == kind
    }

    /// The current token's index in the token stream.
    pub(crate) fn pos(&self) -> usize {
        self.pos
    }

    /// The text of the token `n` ahead.
    pub(crate) fn nth_text(&self, n: usize) -> &'a str {
        let t = self.tok(n);
        self.src.get(std::ops::Range::<usize>::from(t.range)).unwrap_or("")
    }

    /// The current token is `LNAME` with this text (a contextual word).
    pub(crate) fn at_word(&self, word: &str) -> bool {
        self.at(LNAME) && self.nth_text(0) == word
    }

    pub(crate) fn cur_range(&self) -> TextRange {
        self.tok(0).range
    }

    /// A significant line break lies before the current token: it has
    /// `nl_before` and the innermost context is a statement context.
    pub(crate) fn nl(&self) -> bool {
        self.tok(0).nl && self.ctx_top().stmt
    }

    /// The token `n` ahead has a line break before it, whatever the context.
    pub(crate) fn nth_raw_nl(&self, n: usize) -> bool {
        self.tok(n).nl
    }

    /// The token `n` ahead touches the token before it.
    pub(crate) fn nth_touches(&self, n: usize) -> bool {
        !self.tok(n).sp
    }

    /// Indentation of the line of the token at absolute index `idx`.
    pub(crate) fn indent_of(&self, idx: usize) -> u32 {
        self.toks.get(idx).map_or(0, |t| t.indent)
    }

    /// The 1-based line of the token at absolute index `idx`, for messages.
    pub(crate) fn line_of(&self, idx: usize) -> u32 {
        self.toks.get(idx).map_or(1, |t| t.line)
    }

    /// Source text of a range of tokens `[from, to)`, for messages: at most
    /// 40 characters, longer text is cut with `…`.
    pub(crate) fn text_between(&self, from: usize, to: usize) -> String {
        let (Some(a), Some(b)) = (self.toks.get(from), self.toks.get(to.saturating_sub(1))) else {
            return String::new();
        };
        if to <= from {
            return String::new();
        }
        let text = self.src.get(usize::from(a.range.start())..usize::from(b.range.end())).unwrap_or("");
        if text.chars().count() > 40 {
            let mut short: String = text.chars().take(39).collect();
            short.push('…');
            short
        } else {
            text.to_string()
        }
    }

    /// The range from the token at `from` to the last consumed token.
    pub(crate) fn range_from(&self, from: usize) -> TextRange {
        let start = self.toks.get(from).map_or(TextSize::new(0), |t| t.range.start());
        let end = if self.pos > from {
            self.toks.get(self.pos - 1).map_or(start, |t| t.range.end())
        } else {
            start
        };
        TextRange::new(start, end.max(start))
    }

    // ---- Events --------------------------------------------------------------

    fn start_event(&mut self) -> u32 {
        let pos = u32::try_from(self.events.len()).unwrap_or(u32::MAX);
        self.events.push(Event::Start { kind: None, forward_parent: None });
        pos
    }

    pub(crate) fn start(&mut self) -> Marker {
        Marker::new(self.start_event())
    }

    /// Consumes the current token. Never moves past `EOF`, and does nothing
    /// once the parser has halted.
    pub(crate) fn bump(&mut self) {
        let kind = self.cur();
        self.bump_as(kind);
    }

    /// Consumes the current token, giving it `kind` in the tree.
    pub(crate) fn bump_as(&mut self, kind: SyntaxKind) {
        if self.halted() {
            return;
        }
        let Some(t) = self.toks.get(self.pos).copied() else { return };
        if t.kind == EOF {
            return;
        }
        self.events.push(Event::Token { kind, range: t.range });
        self.pos += 1;
        self.fuel.set(FUEL);
    }

    /// Consumes the token if it has this kind.
    pub(crate) fn eat(&mut self, kind: SyntaxKind) -> bool {
        if self.at(kind) {
            self.bump();
            true
        } else {
            false
        }
    }

    /// Consumes one `>` of the current token, which is `>`, `>>`, `>>>`,
    /// `>=`, `>>=` or `>>>=`. The rest stays as a token of its own.
    pub(crate) fn bump_split_gt(&mut self) -> bool {
        let rest = match self.cur() {
            GT => {
                self.bump();
                return true;
            }
            SHR => GT,
            USHR => SHR,
            GT_EQ => EQ,
            SHR_EQ => GT_EQ,
            USHR_EQ => SHR_EQ,
            _ => return false,
        };
        let Some(t) = self.toks.get(self.pos).copied() else { return false };
        let mid = t.range.start() + TextSize::new(1);
        self.events.push(Event::Token { kind: GT, range: TextRange::new(t.range.start(), mid) });
        if let Some(slot) = self.toks.get_mut(self.pos) {
            *slot = Tok { kind: rest, range: TextRange::new(mid, t.range.end()), nl: false, sp: false, ..t };
        }
        self.fuel.set(FUEL);
        true
    }

    // ---- Diagnostics ---------------------------------------------------------

    /// Reports an error at `range`. Suppressed once the parser has halted,
    /// and when an error already starts at the same offset: exactly one
    /// error per position (one mistake, one message; the first one wins).
    pub(crate) fn error_at(&mut self, range: TextRange, message: impl Into<String>) {
        if self.halted() || !self.error_starts.insert(range.start()) {
            return;
        }
        self.diags.push(Diagnostic::error(range, message));
    }

    /// The number of diagnostics so far.
    pub(crate) fn diag_count(&self) -> usize {
        self.diags.len()
    }

    /// Takes the diagnostics reported since `diag_count()` was `from`, to
    /// report them later with [`Parser::restore_diags`] or drop them.
    pub(crate) fn take_diags(&mut self, from: usize) -> Vec<Diagnostic> {
        let taken: Vec<Diagnostic> = self.diags.drain(from.min(self.diags.len())..).collect();
        for d in &taken {
            if d.is_error() {
                self.error_starts.remove(&d.range.start());
            }
        }
        taken
    }

    /// Reports diagnostics taken with [`Parser::take_diags`].
    pub(crate) fn restore_diags(&mut self, diags: Vec<Diagnostic>) {
        for d in diags {
            if d.is_error() {
                self.error_at(d.range, d.message);
            } else {
                self.diags.push(d);
            }
        }
    }

    /// Reports an error at the current token.
    pub(crate) fn error(&mut self, message: impl Into<String>) {
        let range = self.cur_range();
        self.error_at(range, message);
    }

    pub(crate) fn warning_at(&mut self, range: TextRange, message: impl Into<String>) {
        if !self.halted() {
            self.diags.push(Diagnostic::warning(range, message));
        }
    }

    /// Reports an error and wraps the current token in an `ERROR_NODE`.
    pub(crate) fn err_bump(&mut self, message: impl Into<String>) {
        self.error(message);
        let m = self.start();
        self.bump();
        m.complete(self, ERROR_NODE);
    }

    /// Consumes `kind` or reports "expected `kind`".
    pub(crate) fn expect(&mut self, kind: SyntaxKind) -> bool {
        if self.eat(kind) {
            return true;
        }
        let what = kind.fixed_text().unwrap_or("?");
        let found = self.describe_cur();
        self.error(format!("expected `{what}`, found {found}"));
        false
    }

    /// The current token, described for a message: "`end`", "the end of the
    /// file", "a line break".
    pub(crate) fn describe_cur(&self) -> String {
        if self.nl() && !self.at(EOF) {
            return format!("a line break (then {})", self.describe_nth(0));
        }
        self.describe_nth(0)
    }

    pub(crate) fn describe_nth(&self, n: usize) -> String {
        match self.nth(n) {
            EOF => "the end of the file".to_string(),
            STRING | STR_BEGIN => "a string".to_string(),
            STR_MID | STR_END => "the end of the interpolation".to_string(),
            _ => {
                let text = self.nth_text(n);
                if text.chars().count() > 24 {
                    let short: String = text.chars().take(24).collect();
                    format!("`{short}…`")
                } else {
                    format!("`{text}`")
                }
            }
        }
    }

    // ---- Contexts ------------------------------------------------------------

    pub(crate) fn ctx_top(&self) -> Ctx {
        self.ctx.last().copied().unwrap_or(Ctx { stmt: true, ..Ctx::default() })
    }

    pub(crate) fn ctx_top_mut(&mut self) -> Option<&mut Ctx> {
        self.ctx.last_mut()
    }

    /// Runs `f` in a new bracket context closed by `closer`.
    pub(crate) fn in_bracket<R>(&mut self, closer: SyntaxKind, f: impl FnOnce(&mut Self) -> R) -> R {
        self.in_bracket_with(closer, false, f)
    }

    /// As [`Parser::in_bracket`]; `no_union` is set for block parameters.
    pub(crate) fn in_bracket_with<R>(
        &mut self,
        closer: SyntaxKind,
        no_union: bool,
        f: impl FnOnce(&mut Self) -> R,
    ) -> R {
        self.ctx.push(Ctx { stmt: false, no_union, ..Ctx::default() });
        let slot = bracket_slot(closer);
        if let Some(n) = self.open_brackets.get_mut(slot) {
            *n += 1;
        }
        let r = f(self);
        if let Some(n) = self.open_brackets.get_mut(slot) {
            *n = n.saturating_sub(1);
        }
        self.ctx.pop();
        r
    }

    /// Runs `f` with the flags of the current context changed by `set`, and
    /// restores them afterwards.
    pub(crate) fn with_flags<R>(&mut self, set: impl FnOnce(&mut Ctx), f: impl FnOnce(&mut Self) -> R) -> R {
        let saved = self.ctx_top();
        if let Some(top) = self.ctx_top_mut() {
            set(top);
        }
        let r = f(self);
        if let Some(top) = self.ctx_top_mut() {
            *top = saved;
        }
        r
    }

    /// Runs `f` for a block construct opened by the current token: a
    /// statement context, and the closer keywords it accepts.
    pub(crate) fn in_construct<R>(&mut self, closers: u8, f: impl FnOnce(&mut Self) -> R) -> R {
        let opener = self.pos;
        let indent = self.indent_of(opener);
        self.constructs.push(Construct { opener, indent, header_do: None });
        for bit in 0..6 {
            if closers & (1 << bit) != 0 {
                if let Some(n) = self.open_closers.get_mut(bit) {
                    *n += 1;
                }
            }
        }
        self.ctx.push(Ctx { stmt: true, ..Ctx::default() });
        let r = f(self);
        self.ctx.pop();
        for bit in 0..6 {
            if closers & (1 << bit) != 0 {
                if let Some(n) = self.open_closers.get_mut(bit) {
                    *n = n.saturating_sub(1);
                }
            }
        }
        self.constructs.pop();
        r
    }

    /// Marks the innermost open construct as having a one-line `do … end` in
    /// its header (see [`Construct::header_do`]).
    pub(crate) fn mark_header_do(&mut self, do_range: TextRange) {
        if let Some(c) = self.constructs.last_mut() {
            c.header_do = Some(do_range);
        }
    }

    /// The innermost open construct.
    pub(crate) fn construct(&self) -> Option<Construct> {
        self.constructs.last().copied()
    }

    /// No construct is open: the statements of the file's top-level block.
    pub(crate) fn at_file_level(&self) -> bool {
        self.constructs.is_empty()
    }

    /// Some open construct accepts this closer keyword.
    pub(crate) fn closer_accepted(&self, kind: SyntaxKind) -> bool {
        let bit = closer_bit(kind);
        bit != 0 && self.open_closers.get(bit.trailing_zeros() as usize).is_some_and(|&n| n > 0)
    }

    /// The token closes an enclosing bracket context: `)`, `]`, `}`, or the
    /// end of an interpolation.
    pub(crate) fn closes_bracket(&self, kind: SyntaxKind) -> bool {
        matches!(kind, R_PAREN | R_BRACK | R_CURLY | STR_MID | STR_END)
            && self.open_brackets.get(bracket_slot(kind)).is_some_and(|&n| n > 0)
    }

    /// The current token ends a block: `EOF`, a closer keyword some construct
    /// accepts, or the closer of an enclosing bracket.
    pub(crate) fn at_block_end(&self) -> bool {
        let k = self.cur();
        k == EOF || self.closer_accepted(k) || self.closes_bracket(k)
    }

    // ---- Indentation hints ---------------------------------------------------

    /// The column of a hint at the current token, if any.
    pub(crate) fn hint_here(&self) -> Option<u32> {
        if self.hints.is_empty() || self.halted() {
            return None;
        }
        self.hints.binary_search_by_key(&self.pos, |h| h.0).ok().and_then(|i| self.hints.get(i)).map(|h| h.1)
    }

    /// A hint says the innermost construct ends before the current token,
    /// which starts a statement.
    pub(crate) fn hint_closes_construct(&self) -> bool {
        match (self.hint_here(), self.construct()) {
            (Some(col), Some(c)) => c.indent >= col,
            _ => false,
        }
    }

    /// The current token is the closer keyword `kind`, and it belongs to the
    /// innermost construct: no indentation hint puts it in an enclosing one
    /// (an `else` less indented than the `if` inside it, T5).
    pub(crate) fn takes_closer(&self, kind: SyntaxKind) -> bool {
        self.at(kind) && !self.hint_end_belongs_outside()
    }

    /// A hint says the `end` at the current token belongs to an enclosing
    /// construct, not to the innermost one.
    pub(crate) fn hint_end_belongs_outside(&self) -> bool {
        match (self.hint_here(), self.construct()) {
            (Some(col), Some(c)) => c.indent > col,
            _ => false,
        }
    }

    // ---- Nesting limit -------------------------------------------------------

    /// Open brackets and block constructs (the context stack without the
    /// file's own context).
    pub(crate) fn nesting(&self) -> usize {
        self.ctx.len().saturating_sub(1)
    }

    /// Runs `f` one recursion level deeper. When a limit is reached, reports
    /// it (once per top-level statement), skips the rest of the construct
    /// (see [`Parser::skip_deep`]) and returns `Err` with the `ERROR_NODE` of
    /// the skipped tokens, if any.
    pub(crate) fn guarded<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> Result<R, Option<CompletedMarker>> {
        if self.nesting() > NESTING_LIMIT
            || self.rec_depth >= RECURSION_LIMIT
            || self.tree_depth >= TREE_DEPTH_LIMIT
        {
            return Err(self.too_deep());
        }
        self.rec_depth += 1;
        self.tree_depth += 1;
        let r = f(self);
        self.rec_depth -= 1;
        self.tree_depth -= 1;
        Ok(r)
    }

    /// Adds one link of an operator or postfix chain to the tree depth. At
    /// the limit, reports it, skips the rest of the expression and returns
    /// false; the caller then stops the chain. The caller calls
    /// [`Parser::chain_done`] with the number of links at the end.
    pub(crate) fn chain_link(&mut self) -> bool {
        if self.tree_depth >= TREE_DEPTH_LIMIT {
            self.too_deep();
            return false;
        }
        self.tree_depth += 1;
        true
    }

    pub(crate) fn chain_done(&mut self, links: u32) {
        self.tree_depth = self.tree_depth.saturating_sub(links);
    }

    /// A new statement of the file's top level: a nesting error may be
    /// reported again.
    pub(crate) fn top_level_statement(&mut self) {
        self.depth_error_reported = false;
        if !self.at(IMPORT_KW) {
            self.past_imports = true;
        }
    }

    fn too_deep(&mut self) -> Option<CompletedMarker> {
        if !self.depth_error_reported {
            self.depth_error_reported = true;
            // The chain limit, when that is the one reached: the tree is deep
            // although the nesting is not.
            let chain = self.nesting() <= NESTING_LIMIT && self.tree_depth >= TREE_DEPTH_LIMIT;
            let msg = if chain {
                format!(
                    "this expression is too long: more than {TREE_DEPTH_LIMIT} operators or postfix \
                     steps in a chain. The parser skips the rest of it; split it into parts with \
                     `let`"
                )
            } else {
                format!(
                    "this is nested too deeply: more than {NESTING_LIMIT} levels of brackets and \
                     blocks. The parser skips the rest of it; split it into smaller parts, for \
                     example with `let` or helper functions"
                )
            };
            self.error(msg);
        }
        self.skip_deep()
    }

    /// Skips the rest of the construct the parser is in, into an
    /// `ERROR_NODE`: tokens up to the end of the statement, a `,`, or a
    /// closer of an enclosing bracket or block, where brackets and block
    /// keywords inside the skipped part are matched with their closers. No
    /// recursion, so it is safe at any depth.
    fn skip_deep(&mut self) -> Option<CompletedMarker> {
        let m = self.start();
        let mut depth: u32 = 0;
        let mut skipped = false;
        loop {
            let t = self.tok(0);
            let k = t.kind;
            if k == EOF {
                break;
            }
            if depth == 0 {
                if skipped && (k == SEMI || k == COMMA || (t.nl && self.ctx_top().stmt)) {
                    break;
                }
                if self.closes_bracket(k) || (closer_bit(k) != 0 && self.closer_accepted(k)) {
                    break;
                }
            }
            match k {
                L_PAREN | L_BRACK | L_CURLY | STR_BEGIN => depth += 1,
                R_PAREN | R_BRACK | R_CURLY | STR_END | END_KW => depth = depth.saturating_sub(1),
                FN_KW | DO_KW | LOOP_KW | FOR_KW | CASE_KW | COND_KW | TRY_KW | STRUCT_KW | INTERFACE_KW
                | ENUM_KW | MODULE_KW => depth += 1,
                IF_KW | UNLESS_KW | WHILE_KW | UNTIL_KW if !self.at_modifier_position() => depth += 1,
                _ => {}
            }
            self.bump();
            skipped = true;
        }
        if skipped {
            Some(m.complete(self, ERROR_NODE))
        } else {
            m.abandon(self);
            None
        }
    }

    /// An `if`, `unless`, `while` or `until` here follows a complete operand
    /// on its line, so it is a modifier and has no `end`, as far as a
    /// look at the previous token tells.
    fn at_modifier_position(&self) -> bool {
        if self.tok(0).nl {
            return false;
        }
        let Some(prev) = self.pos.checked_sub(1).and_then(|i| self.toks.get(i)) else { return false };
        matches!(
            prev.kind,
            LNAME | CNAME | ONAME | AT_NAME | NUMBER | STRING | STR_END | R_PAREN | R_BRACK | R_CURLY
                | END_KW | BANG_POSTFIX | SELF_KW | SELF_TYPE_KW | TRUE_KW | FALSE_KW | NIL_KW
                | RETURN_KW | BREAK_KW | CONTINUE_KW
        )
    }

    /// After the file's block: reports an internal error if the parser got
    /// stuck, and puts every token not yet consumed into an `ERROR_NODE`, so
    /// that the tree still holds the whole source. Then the `EOF` token.
    pub(crate) fn finish_file(&mut self) {
        if self.stuck.get() {
            let range = self.toks.get(self.pos).map_or_else(TextRange::default, |t| t.range);
            self.diags.push(Diagnostic::error(
                range,
                "internal parser error: the parser stopped making progress here; the rest of the \
                 file was not parsed",
            ));
        }
        let last = self.toks.len().saturating_sub(1);
        if self.pos < last {
            let m = self.start();
            while self.pos < last {
                if let Some(t) = self.toks.get(self.pos) {
                    self.events.push(Event::Token { kind: t.kind, range: t.range });
                }
                self.pos += 1;
            }
            m.complete(self, ERROR_NODE);
        }
        if let Some(t) = self.toks.get(self.pos) {
            self.events.push(Event::Token { kind: t.kind, range: t.range });
            self.pos += 1;
        }
    }
}

/// Parsed expression with what the grammar needs to know about its shape.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Expr {
    pub cm: CompletedMarker,
    pub shape: Shape,
    /// Index of its first token.
    pub start: usize,
}

/// The last step of a Lua path, which decides whether the path is a command
/// callee (`Name`, `Field`, `Method`) or an assignment target (`Field`,
/// `Index`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LuaLast {
    /// The first name alone: `:print`.
    Name,
    /// A `.name` step.
    Field,
    /// An `[expr]` step.
    Index,
    /// A call step, a method step with arguments or a block, or a block.
    Call,
    /// A `:name` step without arguments.
    Method,
}

/// The shape of a parsed expression, as far as the grammar's decisions
/// depend on it: callees, capitalized name paths, postfix `!`,
/// assignment targets, modifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Shape {
    /// An `LNAME`.
    Name,
    /// A `CNAME`.
    CapName,
    /// `self`.
    SelfValue,
    /// `Self`.
    SelfType,
    /// `@name`.
    At,
    /// `x.name`: `lower` when the name is a lowercase word (callee);
    /// `cap_path` when the whole chain is a capitalized name path.
    Member { lower: bool, cap_path: bool },
    /// A Lua path, by its last step.
    LuaPath { last: LuaLast },
    /// `x:m` (`args`: with an argument list or block).
    Method { args: bool },
    /// A call with parentheses and no block.
    Call,
    /// A call with a trailing block.
    BlockCall,
    /// A command call.
    Command,
    Index,
    /// `x!`.
    Bang,
    /// A literal (number, string, `true`, …).
    Literal,
    /// A table constructor.
    Table,
    /// A range `a..b` or `a...b`, possibly in parentheses (for `step`).
    Range,
    /// A construct closed by `end`: `if`, `unless`, `case`, `cond`, `try`,
    /// `fn`, `on`, `spawn do`.
    Construct,
    Other,
}

impl Shape {
    /// May be the callee of a command call.
    pub(crate) fn is_callee(self) -> bool {
        matches!(
            self,
            Shape::Name
                | Shape::At
                | Shape::Member { lower: true, .. }
                | Shape::LuaPath { last: LuaLast::Name | LuaLast::Field | LuaLast::Method }
                | Shape::Method { args: false }
        )
    }

    /// A capitalized name path, which a `{` makes a struct literal.
    pub(crate) fn is_cap_path(self) -> bool {
        matches!(self, Shape::CapName | Shape::SelfType | Shape::Member { cap_path: true, .. })
    }

    /// May be followed by a postfix `!`.
    pub(crate) fn bang_ok(self) -> bool {
        matches!(self, Shape::Name | Shape::CapName | Shape::At | Shape::Member { .. })
    }
}
