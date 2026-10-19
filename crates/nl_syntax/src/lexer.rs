//! The lexer: source text to tokens.

use rowan::{TextRange, TextSize};

use crate::literal::{EscError, escape_message, read_escape, utf8_len};
use crate::{Diagnostic, SyntaxKind, keyword_kind, name_case};

/// One token: its kind, its byte range in the source, and the two layout
/// layout flags.
///
/// The layout flags are meaningful on non-trivia tokens only, and always
/// `false` on trivia:
///
/// - `nl_before`: at least one line break lies between this token and the
///   previous non-trivia token. Line breaks inside a token (a heredoc) do not
///   count; line breaks inside a multi-line interpolation of a heredoc do
///   (they lie between tokens).
/// - `sp_before`: any whitespace, comment or line break lies between this
///   token and the previous non-trivia token. `nl_before` implies it. "Touches
///   the previous token" is `!sp_before`.
///
/// `EOF` always has both flags set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Token {
    pub kind: SyntaxKind,
    pub range: TextRange,
    pub nl_before: bool,
    pub sp_before: bool,
}

impl Token {
    /// The token's text in `source` (the source it was lexed from); empty if
    /// the range does not fit `source`.
    pub fn text<'a>(&self, source: &'a str) -> &'a str {
        source.get(std::ops::Range::<usize>::from(self.range)).unwrap_or("")
    }

    /// The length in bytes.
    pub fn len(&self) -> TextSize {
        self.range.len()
    }

    /// True for the empty tokens: `EOF`, and the empty `STR_END` the lexer
    /// emits when it recovers from an unclosed interpolation.
    pub fn is_empty(&self) -> bool {
        self.range.is_empty()
    }
}

/// The result of [`lex`]: every token of the source, trivia included, and
/// the lexical diagnostics.
///
/// Guarantees, for every input:
///
/// - the token ranges are contiguous and in order, starting at 0; their texts
///   concatenate to exactly the source;
/// - the last token is `EOF` (empty, at the end of the source) and no other
///   token is `EOF`;
/// - string parts are balanced: every `STR_BEGIN` is followed, after the
///   tokens of the embedded expression, by zero or more `STR_MID` and exactly
///   one `STR_END` of the same string; strings in interpolations nest
///   properly. The parser can therefore rely on the shape
///   `STR_BEGIN expr { STR_MID expr } STR_END`, with possibly malformed
///   expressions in between;
/// - a token is never split by the lexer, and only `EOF` and recovery
///   `STR_END` tokens are empty.
#[derive(Clone, Debug)]
pub struct Lexed<'a> {
    /// The source text.
    pub source: &'a str,
    /// All tokens, trivia included, ending with `EOF`.
    pub tokens: Vec<Token>,
    /// Lexical errors, in source order of detection.
    pub diagnostics: Vec<Diagnostic>,
}

impl<'a> Lexed<'a> {
    /// The text of token `index` (empty if out of range).
    pub fn text(&self, index: usize) -> &'a str {
        self.tokens.get(index).map_or("", |t| t.text(self.source))
    }

    /// The kind of token `index` (`EOF` if out of range).
    pub fn kind(&self, index: usize) -> SyntaxKind {
        self.tokens.get(index).map_or(SyntaxKind::EOF, |t| t.kind)
    }

    /// The indices of the non-trivia tokens, in order, ending with `EOF`.
    /// This is the token stream the parser reads; trivia between two of
    /// these indices goes into the tree before the second one.
    pub fn significant(&self) -> impl Iterator<Item = usize> + '_ {
        self.tokens.iter().enumerate().filter(|(_, t)| !t.kind.is_trivia()).map(|(i, _)| i)
    }
}

/// Checks that `bytes` is UTF-8 and returns it as text.
///
/// [`lex`] takes `&str`, so invalid UTF-8 never reaches the lexer. The
/// driver reads a file as bytes and calls this first; on invalid UTF-8 it
/// reports the returned diagnostic, which points at the first offending
/// byte, and does not lex the file.
pub fn source_from_bytes(bytes: &[u8]) -> Result<&str, Diagnostic> {
    std::str::from_utf8(bytes).map_err(|e| {
        let start = e.valid_up_to();
        let len = e.error_len().unwrap_or(bytes.len().saturating_sub(start)).max(1);
        Diagnostic::error(
            make_range(start, start.saturating_add(len)),
            format!(
                "this file is not valid UTF-8 (invalid byte at offset {start}); NL source must \
                 be UTF-8"
            ),
        )
    })
}

/// Lexes `source` into tokens and diagnostics. Never panics, never stops
/// early; see [`Lexed`] for the guarantees.
///
/// A source longer than `u32::MAX` bytes (4 GiB) cannot be addressed by a
/// `TextRange`; it is rejected with one diagnostic and no tokens.
pub fn lex(source: &str) -> Lexed<'_> {
    if u32::try_from(source.len()).is_err() {
        return Lexed {
            source,
            tokens: Vec::new(),
            diagnostics: vec![Diagnostic::error(
                TextRange::default(),
                "this file is larger than 4 GiB, which NL does not support",
            )],
        };
    }
    let mut lexer = Lexer {
        src: source,
        b: source.as_bytes(),
        pos: 0,
        tokens: Vec::with_capacity(source.len() / 3 + 1),
        diags: Vec::new(),
        frames: Vec::new(),
        pend_nl: false,
        pend_sp: false,
        prev: None,
        prev2: None,
    };
    lexer.run();
    Lexed { source, tokens: lexer.tokens, diagnostics: lexer.diags }
}

fn size(n: usize) -> TextSize {
    TextSize::new(u32::try_from(n).unwrap_or(u32::MAX))
}

fn make_range(a: usize, b: usize) -> TextRange {
    let (a, b) = if a <= b { (a, b) } else { (b, a) };
    TextRange::new(size(a), size(b))
}

fn is_word_start(b: u8) -> bool {
    b.is_ascii_alphabetic() || b == b'_'
}

fn is_word_char(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn is_blank(b: u8) -> bool {
    b == b' ' || b == b'\t'
}

fn is_line_break(b: u8) -> bool {
    b == b'\n' || b == b'\r'
}

/// Operators and punctuation, longest first. `!`, `{` and
/// `}` are handled separately.
const OPERATORS: &[(&str, SyntaxKind)] = {
    use SyntaxKind::*;
    &[
        (">>>=", USHR_EQ),
        (">>>", USHR),
        (">>=", SHR_EQ),
        ("<<=", SHL_EQ),
        ("**=", STAR2_EQ),
        ("//=", SLASH2_EQ),
        ("<>=", CONCAT_EQ),
        ("||=", PIPE2_EQ),
        ("&&=", AMP2_EQ),
        ("??=", QUESTION2_EQ),
        ("...", DOT3),
        ("..<", DOT2_LT),
        ("**", STAR2),
        ("//", SLASH2),
        ("<>", CONCAT),
        ("<<", SHL),
        (">>", SHR),
        ("==", EQ2),
        ("<=", LT_EQ),
        (">=", GT_EQ),
        ("&&", AMP2),
        ("||", PIPE2),
        ("|>", PIPE_GT),
        ("..", DOT2),
        ("??", QUESTION2),
        ("?.", QUESTION_DOT),
        ("+=", PLUS_EQ),
        ("-=", MINUS_EQ),
        ("->", ARROW),
        ("*=", STAR_EQ),
        ("/=", SLASH_EQ),
        ("%=", PERCENT_EQ),
        ("|=", PIPE_EQ),
        ("&=", AMP_EQ),
        ("^=", CARET_EQ),
        ("+", PLUS),
        ("-", MINUS),
        ("*", STAR),
        ("/", SLASH),
        ("%", PERCENT),
        ("|", PIPE),
        ("&", AMP),
        ("^", CARET),
        ("~", TILDE),
        ("<", LT),
        (">", GT),
        ("=", EQ),
        ("(", L_PAREN),
        (")", R_PAREN),
        ("[", L_BRACK),
        ("]", R_BRACK),
        (",", COMMA),
        (";", SEMI),
        (".", DOT),
        (":", COLON),
        ("?", QUESTION),
    ]
};

/// An open interpolation `#{ … }`.
#[derive(Clone, Copy, Debug)]
struct Frame {
    /// The string is a heredoc (else a one-line `"…"` string).
    heredoc: bool,
    /// Depth of `{` opened inside the interpolation and not yet closed.
    depth: u32,
    /// Offset of the `#{`.
    open: usize,
    /// Offset of the string's opening quote.
    string_start: usize,
    /// A line break inside this interpolation of a one-line string was
    /// already reported (the interpolation went on, see
    /// `line_break_in_interpolation`).
    reported: bool,
}

struct Lexer<'a> {
    src: &'a str,
    b: &'a [u8],
    pos: usize,
    tokens: Vec<Token>,
    diags: Vec<Diagnostic>,
    /// Open interpolations, innermost last.
    frames: Vec<Frame>,
    /// A line break / any trivia since the last non-trivia token.
    pend_nl: bool,
    pend_sp: bool,
    /// The last two non-trivia token kinds, most recent first.
    prev: Option<SyntaxKind>,
    prev2: Option<SyntaxKind>,
}

impl Lexer<'_> {
    fn at(&self, i: usize) -> Option<u8> {
        self.b.get(i).copied()
    }

    fn cur(&self) -> Option<u8> {
        self.at(self.pos)
    }

    fn peek(&self, n: usize) -> Option<u8> {
        self.at(self.pos + n)
    }

    fn text(&self, a: usize, b: usize) -> &str {
        self.src.get(a..b).unwrap_or("")
    }

    /// Length of the line break at `i` (2 for CRLF), 0 if none.
    fn line_break_len(&self, i: usize) -> usize {
        match (self.at(i), self.at(i + 1)) {
            (Some(b'\r'), Some(b'\n')) => 2,
            (Some(b'\r' | b'\n'), _) => 1,
            _ => 0,
        }
    }

    /// Length of the character at `i` (0 at the end).
    fn char_len(&self, i: usize) -> usize {
        self.at(i).map_or(0, |b| utf8_len(b).min(self.b.len() - i))
    }

    fn error(&mut self, a: usize, b: usize, message: impl Into<String>) {
        self.diags.push(Diagnostic::error(make_range(a, b), message));
    }

    fn push(&mut self, kind: SyntaxKind, start: usize, end: usize) {
        let (nl_before, sp_before) = if kind.is_trivia() {
            if kind == SyntaxKind::NEWLINE {
                self.pend_nl = true;
            }
            self.pend_sp = true;
            (false, false)
        } else if kind == SyntaxKind::EOF {
            (true, true)
        } else {
            let flags = (self.pend_nl, self.pend_sp);
            self.pend_nl = false;
            self.pend_sp = false;
            self.prev2 = self.prev;
            self.prev = Some(kind);
            flags
        };
        self.tokens.push(Token { kind, range: make_range(start, end), nl_before, sp_before });
    }

    fn run(&mut self) {
        if self.src.starts_with('\u{FEFF}') {
            self.pos = 3;
            self.push(SyntaxKind::WHITESPACE, 0, 3);
        }
        while let Some(c) = self.cur() {
            let before = self.pos;
            self.step(c);
            if self.pos <= before {
                // Cannot happen; guards against an endless loop all the same.
                self.pos = before + self.char_len(before).max(1);
                self.push(SyntaxKind::ERROR, before, self.pos);
            }
        }
        let end = self.b.len();
        while let Some(frame) = self.frames.pop() {
            self.error(
                frame.open,
                frame.open + 2,
                "this interpolation is not closed: the file ends before its `}`",
            );
            self.push(SyntaxKind::STR_END, end, end);
        }
        self.push(SyntaxKind::EOF, end, end);
    }

    fn step(&mut self, c: u8) {
        use SyntaxKind::*;
        let start = self.pos;
        match c {
            b' ' | b'\t' => {
                while self.cur().is_some_and(is_blank) {
                    self.pos += 1;
                }
                self.push(WHITESPACE, start, self.pos);
            }
            b'\n' | b'\r' => {
                self.line_break_in_interpolation();
                self.pos += self.line_break_len(start);
                self.push(NEWLINE, start, self.pos);
            }
            b'#' => {
                // Exactly two `#` start a doc comment; `###`
                // and longer runs are plain comments, as `////` is in Rust.
                let kind = if self.peek(1) == Some(b'#') && self.peek(2) != Some(b'#') { DOC_COMMENT } else { COMMENT };
                while self.cur().is_some_and(|b| !is_line_break(b)) {
                    self.pos += 1;
                }
                self.push(kind, start, self.pos);
            }
            b'"' => {
                if self.peek(1) == Some(b'"') && self.peek(2) == Some(b'"') {
                    self.triple_quote();
                } else {
                    self.pos += 1;
                    self.quoted_body(start, true, start);
                }
            }
            b'\'' => self.raw_string(),
            b'0'..=b'9' => self.number(),
            _ if is_word_start(c) => {
                while self.cur().is_some_and(is_word_char) {
                    self.pos += 1;
                }
                let word = self.text(start, self.pos);
                let kind = keyword_kind(word).unwrap_or_else(|| name_case(word).token_kind());
                self.push(kind, start, self.pos);
            }
            b'@' => {
                self.pos += 1;
                if self.cur().is_some_and(is_word_start) {
                    while self.cur().is_some_and(is_word_char) {
                        self.pos += 1;
                    }
                    self.push(AT_NAME, start, self.pos);
                } else {
                    self.push(ERROR, start, self.pos);
                    self.error(
                        start,
                        self.pos,
                        "`@` must be directly followed by a field name, as in `@health`",
                    );
                }
            }
            b'{' => {
                if let Some(frame) = self.frames.last_mut() {
                    frame.depth = frame.depth.saturating_add(1);
                }
                self.pos += 1;
                self.push(L_CURLY, start, self.pos);
            }
            b'}' => match self.frames.last_mut() {
                Some(frame) if frame.depth == 0 => self.close_interpolation(),
                frame => {
                    if let Some(frame) = frame {
                        frame.depth -= 1;
                    }
                    self.pos += 1;
                    self.push(R_CURLY, start, self.pos);
                }
            },
            b'!' => self.bang(),
            _ => {
                let rest = self.b.get(start..).unwrap_or(&[]);
                if let Some(&(op, kind)) =
                    OPERATORS.iter().find(|(op, _)| rest.starts_with(op.as_bytes()))
                {
                    self.pos += op.len();
                    self.push(kind, start, self.pos);
                } else {
                    self.stray();
                }
            }
        }
    }

    /// `!`, `!=` and postfix `!`.
    fn bang(&mut self) {
        let start = self.pos;
        if self.peek(1) == Some(b'=') {
            self.pos += 2;
            self.push(SyntaxKind::NEQ, start, self.pos);
            return;
        }
        self.pos += 1;
        let touches = !self.pend_sp;
        let postfix = touches
            && self.prev.is_some_and(|k| {
                k.is_name()
                    || matches!(k, SyntaxKind::AT_NAME | SyntaxKind::R_PAREN | SyntaxKind::R_BRACK)
                    || (k.is_keyword() && matches!(self.prev2, Some(SyntaxKind::DOT | SyntaxKind::QUESTION_DOT)))
            });
        let kind = if postfix { SyntaxKind::BANG_POSTFIX } else { SyntaxKind::BANG };
        self.push(kind, start, self.pos);
    }

    /// A run of characters that start no token.
    fn stray(&mut self) {
        let start = self.pos;
        let first = self.src.get(start..).and_then(|s| s.chars().next()).unwrap_or('\u{FFFD}');
        loop {
            self.pos += self.char_len(self.pos).max(1);
            let Some(next) = self.src.get(self.pos..).and_then(|s| s.chars().next()) else {
                break;
            };
            if !is_stray(next) {
                break;
            }
        }
        self.push(SyntaxKind::ERROR, start, self.pos);
        self.error(start, self.pos, stray_message(first));
    }

    /// A line break in token mode: inside the interpolation of a one-line
    /// string it is an error, and every one-line string still open is closed
    /// with an empty `STR_END`.
    fn line_break_in_interpolation(&mut self) {
        let Some(&frame) = self.frames.last() else {
            return;
        };
        if frame.heredoc {
            return;
        }
        if !frame.reported {
            self.error(
                frame.open,
                frame.open + 2,
                "an interpolation in a one-line string must close on the line where it opens: add \
                 the missing `}`, or use a heredoc `\"\"\"` for text on several lines",
            );
        }
        // When the next line closes the interpolation and then the string
        // (`"x #{ 1 +` then `2 }"`), the expression only went on: keep lexing
        // it, so that the one mistake gives one error.
        let next = self.pos + self.line_break_len(self.pos);
        if self.next_line_closes_interpolation(next, frame.depth) {
            if let Some(f) = self.frames.last_mut() {
                f.reported = true;
            }
            return;
        }
        while self.frames.last().is_some_and(|f| !f.heredoc) {
            self.frames.pop();
            self.push(SyntaxKind::STR_END, self.pos, self.pos);
        }
    }

    /// The line starting at `i` holds the `}` that closes an interpolation
    /// (at brace depth `depth` there), then a `"` after it.
    fn next_line_closes_interpolation(&self, i: usize, depth: u32) -> bool {
        let mut depth = depth;
        let mut j = i;
        while let Some(b) = self.at(j) {
            match b {
                b'\n' | b'\r' | b'#' | b'"' | b'\'' => return false,
                b'{' => depth += 1,
                b'}' if depth == 0 => return self.quote_before_line_end(j + 1),
                b'}' => depth -= 1,
                _ => {}
            }
            j += 1;
        }
        false
    }

    /// A `"` lies between `i` and the end of its line.
    fn quote_before_line_end(&self, i: usize) -> bool {
        let mut j = i;
        while let Some(b) = self.at(j) {
            if is_line_break(b) {
                return false;
            }
            if b == b'"' {
                return true;
            }
            j += 1;
        }
        false
    }

    /// The `}` that closes an interpolation: the string continues.
    fn close_interpolation(&mut self) {
        let start = self.pos;
        let Some(frame) = self.frames.pop() else {
            return;
        };
        if matches!(self.prev, Some(SyntaxKind::STR_BEGIN | SyntaxKind::STR_MID)) {
            self.error(
                frame.open,
                start + 1,
                "empty interpolation: put an expression between `#{` and `}`, or write `\\#{` for \
                 a literal `#{`",
            );
        }
        self.pos += 1;
        if frame.heredoc {
            self.heredoc_body(start, false, false, frame.string_start);
        } else {
            self.quoted_body(start, false, frame.string_start);
        }
    }

    /// Validates the escape at `self.pos` (a backslash) and moves past it.
    fn escape(&mut self) {
        let start = self.pos;
        let rest = self.b.get(start + 1..).unwrap_or(&[]);
        let (value, len) = read_escape(rest);
        match value {
            // `AtEnd` only happens at the end of the file, where the string
            // is reported as unterminated.
            Ok(_) | Err(EscError::AtEnd) => {}
            Err(err) => {
                let end = start + 1 + len;
                let text = self.text(start, end).to_string();
                self.error(start, end, escape_message(err, &text));
            }
        }
        self.pos = start + 1 + len;
    }

    /// The body of a `"…"` string, from `self.pos` (after the opening quote,
    /// or after the `}` of an interpolation) up to and including its end.
    fn quoted_body(&mut self, tok_start: usize, first: bool, string_start: usize) {
        use SyntaxKind::*;
        let closing = if first { STRING } else { STR_END };
        loop {
            match self.cur() {
                None => {
                    self.push(closing, tok_start, self.pos);
                    self.error(
                        string_start,
                        self.pos,
                        "unterminated string: the file ends before the closing `\"`",
                    );
                    return;
                }
                Some(b'"') => {
                    self.pos += 1;
                    self.push(closing, tok_start, self.pos);
                    return;
                }
                Some(c) if is_line_break(c) => {
                    self.push(closing, tok_start, self.pos);
                    self.error(
                        string_start,
                        self.pos,
                        "unterminated string: a `\"…\"` string must close on the line where it \
                         starts; for text on several lines use a heredoc `\"\"\"`",
                    );
                    return;
                }
                Some(b'\\') if self.line_break_len(self.pos + 1) > 0 => {
                    // A backslash before a line break continues the string on
                    // the next line and writes a line break.
                    self.pos += 1 + self.line_break_len(self.pos + 1);
                }
                Some(b'\\') => self.escape(),
                Some(b'#') if self.peek(1) == Some(b'{') => {
                    let open = self.pos;
                    self.pos += 2;
                    self.push(if first { STR_BEGIN } else { STR_MID }, tok_start, self.pos);
                    self.frames.push(Frame { heredoc: false, depth: 0, open, string_start, reported: false });
                    return;
                }
                Some(_) => self.pos += 1,
            }
        }
    }

    /// A raw `'…'` string.
    fn raw_string(&mut self) {
        let start = self.pos;
        self.pos += 1;
        loop {
            match self.cur() {
                None => {
                    self.push(SyntaxKind::STRING, start, self.pos);
                    self.error(start, self.pos, "unterminated string: the file ends before the closing `'`");
                    return;
                }
                Some(b'\'') => {
                    self.pos += 1;
                    self.push(SyntaxKind::STRING, start, self.pos);
                    return;
                }
                Some(c) if is_line_break(c) => {
                    self.push(SyntaxKind::STRING, start, self.pos);
                    self.error(
                        start,
                        self.pos,
                        "unterminated string: a `'…'` string must close on the line where it \
                         starts; for raw text on several lines use a heredoc `\"\"\"=`",
                    );
                    return;
                }
                Some(b'\\') if matches!(self.peek(1), Some(b'\'' | b'\\')) => self.pos += 2,
                Some(_) => self.pos += 1,
            }
        }
    }

    /// True when the `"""` at `self.pos` is the first non-blank text of its
    /// line.
    fn at_line_start_after_blanks(&self) -> bool {
        let before = self.b.get(..self.pos).unwrap_or(&[]);
        match before.iter().rev().find(|&&c| !is_blank(c)) {
            None => true,
            Some(&c) => is_line_break(c),
        }
    }

    /// `"""` in token mode: a heredoc opener, or (inside an interpolation of
    /// a heredoc, at the start of a line) the closer of that heredoc.
    fn triple_quote(&mut self) {
        let start = self.pos;
        if let Some(&frame) = self.frames.last() {
            if frame.heredoc && self.at_line_start_after_blanks() {
                // The first line whose first non-blank text is
                // `"""` closes the heredoc. The interpolation was left open.
                self.frames.pop();
                self.error(
                    frame.open,
                    frame.open + 2,
                    "this interpolation is not closed: the heredoc ends before its `}`",
                );
                self.pos += 3;
                self.push(SyntaxKind::STR_END, start, self.pos);
                return;
            }
            self.error(
                start,
                start + 3,
                "a heredoc cannot be opened inside an interpolation `#{…}`: bind the heredoc to \
                 a variable first, and interpolate the variable",
            );
        }
        self.heredoc();
    }

    /// A heredoc, from its opener at `self.pos`.
    fn heredoc(&mut self) {
        let start = self.pos;
        self.pos += 3;
        let mut raw = false;
        if self.cur() == Some(b'-') {
            self.pos += 1;
            if self.cur() == Some(b'=') {
                self.pos += 1;
                raw = true;
            }
        } else if self.cur() == Some(b'=') {
            self.pos += 1;
            raw = true;
            if self.cur() == Some(b'-') {
                self.error(
                    start,
                    self.pos + 1,
                    "write the heredoc flags in the order `-=`, as in `\"\"\"-=`",
                );
                self.pos += 1;
            }
        }
        while self.cur().is_some_and(is_blank) {
            self.pos += 1;
        }
        match self.cur() {
            None => {
                self.push(SyntaxKind::STRING, start, self.pos);
                self.error(
                    start,
                    start + 3,
                    "unterminated heredoc: the file ends on the line of its opener. The text \
                     starts on the next line and ends at a line that starts with `\"\"\"`",
                );
                return;
            }
            Some(c) if is_line_break(c) => {}
            Some(c) => {
                let junk = self.pos;
                // One pass up to the first `"""` or the end of the line,
                // whichever comes first (a line of many `"` stays linear).
                let mut line_end = junk;
                let mut closer = None;
                while let Some(b) = self.at(line_end) {
                    if is_line_break(b) {
                        break;
                    }
                    if b == b'"' && self.at(line_end + 1) == Some(b'"') && self.at(line_end + 2) == Some(b'"') {
                        closer = Some(line_end);
                        break;
                    }
                    line_end += 1;
                }
                if let Some(k) = closer {
                    // `"""text"""` on one line.
                    self.pos = k + 3;
                    self.push(SyntaxKind::STRING, start, self.pos);
                    self.error(
                        start,
                        self.pos,
                        "a heredoc's text starts on the line after `\"\"\"`; for one line of text \
                         use `\"…\"`",
                    );
                    return;
                }
                let message = if c == b'#' {
                    "a comment cannot follow a heredoc opener: `#` there is text of the string. \
                     Put the comment on another line"
                } else {
                    "nothing may follow `\"\"\"` on its line: the heredoc's text starts on the \
                     next line"
                };
                self.error(junk, line_end, message);
                self.pos = line_end;
                if self.cur().is_none() {
                    self.push(SyntaxKind::STRING, start, self.pos);
                    return;
                }
            }
        }
        self.pos += self.line_break_len(self.pos);
        self.heredoc_body(start, true, raw, start);
    }

    /// The content of a heredoc from `self.pos`, which is at the start of a
    /// line when `first` (after the opener line) and after the `}` of an
    /// interpolation otherwise, up to and including the closer.
    fn heredoc_body(&mut self, tok_start: usize, first: bool, raw: bool, string_start: usize) {
        use SyntaxKind::*;
        let closing = if first { STRING } else { STR_END };
        let mut line_start = first;
        loop {
            if line_start {
                line_start = false;
                let mut p = self.pos;
                while self.at(p).is_some_and(is_blank) {
                    p += 1;
                }
                if self.b.get(p..).is_some_and(|r| r.starts_with(b"\"\"\"")) {
                    self.pos = p + 3;
                    self.push(closing, tok_start, self.pos);
                    return;
                }
            }
            match self.cur() {
                None => {
                    self.push(closing, tok_start, self.pos);
                    self.error(
                        string_start,
                        string_start + 3,
                        "unterminated heredoc: no line starting with `\"\"\"` closes it",
                    );
                    return;
                }
                Some(c) if is_line_break(c) => {
                    self.pos += self.line_break_len(self.pos);
                    line_start = true;
                }
                Some(b'\\') if !raw => self.escape(),
                Some(b'#') if !raw && self.peek(1) == Some(b'{') => {
                    let open = self.pos;
                    self.pos += 2;
                    self.push(if first { STR_BEGIN } else { STR_MID }, tok_start, self.pos);
                    self.frames.push(Frame { heredoc: true, depth: 0, open, string_start, reported: false });
                    return;
                }
                Some(_) => self.pos += 1,
            }
        }
    }

    /// A number. Always one `NUMBER` token; malformed numbers
    /// get a diagnostic.
    fn number(&mut self) {
        let start = self.pos;
        let mut bad_underscore: Option<usize> = None;
        let mut message: Option<String> = None;
        let radix = match (self.cur(), self.peek(1)) {
            (Some(b'0'), Some(b'x' | b'X')) => 16,
            (Some(b'0'), Some(b'b' | b'B')) => 2,
            _ => 10,
        };
        if radix != 10 {
            self.pos += 2;
            let is_digit: fn(u8) -> bool =
                if radix == 16 { |b| b.is_ascii_hexdigit() } else { |b| b == b'0' || b == b'1' };
            let digits = self.digit_run(is_digit, &mut bad_underscore);
            if digits == 0 {
                let prefix = self.text(start, start + 2).to_string();
                message = Some(if radix == 16 {
                    format!("`{prefix}` must be followed by hexadecimal digits, as in `0xff`")
                } else {
                    format!("`{prefix}` must be followed by binary digits, as in `0b1010`")
                });
            } else if self.cur() == Some(b'.') && self.peek(1).is_some_and(|b| b.is_ascii_digit()) {
                // `0xff.5`: no fraction. `0xff.abs` is a
                // member access and ends the number before the `.`.
                self.pos += 1;
                while self.cur().is_some_and(is_word_char) {
                    self.pos += 1;
                }
                let text = snippet(self.text(start, self.pos));
                let name = if radix == 16 { "hexadecimal" } else { "binary" };
                message = Some(format!("`{text}` is not a number: a {name} number has no fraction"));
            }
        } else {
            self.digit_run(|b| b.is_ascii_digit(), &mut bad_underscore);
            let digit_at = |l: &Self, n: usize| l.peek(n).is_some_and(|b| b.is_ascii_digit());
            if self.cur() == Some(b'.')
                && (digit_at(self, 1) || (self.peek(1) == Some(b'_') && digit_at(self, 2)))
            {
                // `1._5` is an error, not the member `_5`.
                self.pos += 1;
                self.digit_run(|b| b.is_ascii_digit(), &mut bad_underscore);
            }
            if matches!(self.cur(), Some(b'e' | b'E')) {
                let sign = usize::from(matches!(self.peek(1), Some(b'+' | b'-')));
                if self.peek(1 + sign).is_some_and(|b| b.is_ascii_digit() || b == b'_') {
                    self.pos += 1 + sign;
                    self.digit_run(|b| b.is_ascii_digit(), &mut bad_underscore);
                }
            }
        }
        if self.cur().is_some_and(is_word_char) {
            let junk = self.pos;
            while self.cur().is_some_and(is_word_char) {
                self.pos += 1;
            }
            let text = snippet(self.text(start, self.pos));
            let first = self.at(junk).unwrap_or(b'_');
            message = Some(if radix == 2 && first.is_ascii_digit() {
                format!("`{text}` is not a number: `{}` is not a binary digit", char::from(first))
            } else if radix == 10 && matches!(first, b'e' | b'E') {
                format!("`{text}` is not a number: an exponent needs digits, as in `1e5` or `1e-3`")
            } else if message.is_some() {
                message.unwrap_or_default()
            } else {
                format!(
                    "`{text}` is not a number: a number cannot be directly followed by a letter \
                     or `_`"
                )
            });
        }
        self.push(SyntaxKind::NUMBER, start, self.pos);
        if let Some(message) = message {
            self.error(start, self.pos, message);
        } else if let Some(u) = bad_underscore {
            self.error(
                u,
                u + 1,
                "misplaced `_` in a number: `_` may only stand between two digits, as in \
                 `1_000_000`",
            );
        }
    }

    /// Digits and underscores; returns the number of digits and records the
    /// first underscore that does not stand between two digits.
    fn digit_run(&mut self, is_digit: fn(u8) -> bool, bad: &mut Option<usize>) -> usize {
        let mut digits = 0;
        let mut prev_digit = false;
        loop {
            match self.cur() {
                Some(b) if is_digit(b) => {
                    digits += 1;
                    prev_digit = true;
                }
                Some(b'_') => {
                    if !prev_digit {
                        bad.get_or_insert(self.pos);
                    }
                    prev_digit = false;
                }
                _ => break,
            }
            self.pos += 1;
        }
        if !prev_digit && self.pos > 0 && self.at(self.pos - 1) == Some(b'_') {
            bad.get_or_insert(self.pos - 1);
        }
        digits
    }
}

/// A character that starts no token: non-ASCII, or ASCII that is neither
/// blank, a line break, nor the start of any token.
fn is_stray(c: char) -> bool {
    if !c.is_ascii() {
        return true;
    }
    let b = u8::try_from(c).unwrap_or(0);
    !(is_word_char(b)
        || is_blank(b)
        || is_line_break(b)
        || b"\"'#@{}!+-*/%<>=|&^~()[],;.:?".contains(&b))
}

fn stray_message(c: char) -> String {
    match c {
        '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{00AB}' | '\u{00BB}' => format!(
            "`{c}` is a typographic quotation mark; strings are written with a straight `\"` or `'`"
        ),
        '\u{2018}' | '\u{2019}' | '\u{201A}' => {
            format!("`{c}` is a typographic quotation mark; raw strings are written with a straight `'`")
        }
        '\u{FEFF}' => {
            "a byte-order mark (U+FEFF) may only stand at the very start of the file".to_string()
        }
        '\\' => "a backslash may only stand inside a string".to_string(),
        _ if c.is_whitespace() || c.is_control() => format!(
            "the character U+{:04X} is not allowed here: NL source uses spaces, tabs and line \
             breaks for layout",
            u32::from(c)
        ),
        _ if !c.is_ascii() => format!(
            "the character `{c}` (U+{:04X}) is not allowed outside strings and comments: NL \
             names and operators are ASCII",
            u32::from(c)
        ),
        _ => format!("the character `{c}` is not used in NL"),
    }
}

/// `text`, shortened for a message.
fn snippet(text: &str) -> String {
    if text.chars().count() > 24 {
        let short: String = text.chars().take(24).collect();
        format!("{short}…")
    } else {
        text.to_string()
    }
}
