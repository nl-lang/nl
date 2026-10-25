//! Values of literal tokens: numbers and strings.
//!
//! The lexer validates literals and reports every error. The functions here
//! turn token text into values; they are total and never report anything.
//! On text the lexer has already flagged they still return something
//! reasonable (an invalid escape is kept as written, a malformed number is
//! `None`).

/// The value of a `NUMBER` token. Underscores are ignored; `0x…` and `0b…`
/// are integers; everything is a double, as in Lua. For a malformed token
/// (which the lexer has reported) the result is `None` or a best guess
/// (`1_.5` gives 1.5); it is only meaningful when the lexer reported nothing.
pub fn number_value(text: &str) -> Option<f64> {
    let digits: String = text.chars().filter(|&c| c != '_').collect();
    let first = digits.bytes().next()?;
    if !first.is_ascii_digit() {
        return None;
    }
    let lower = digits.to_ascii_lowercase();
    if let Some(hex) = lower.strip_prefix("0x") {
        return radix_value(hex, 16);
    }
    if let Some(bin) = lower.strip_prefix("0b") {
        return radix_value(bin, 2);
    }
    let valid = lower
        .bytes()
        .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'+' | b'-'));
    if !valid || lower.ends_with('.') {
        return None;
    }
    lower.parse::<f64>().ok()
}

fn radix_value(digits: &str, radix: u32) -> Option<f64> {
    if digits.is_empty() {
        return None;
    }
    if let Ok(v) = u128::from_str_radix(digits, radix) {
        // Round to nearest, as Lua's own conversion does.
        #[allow(clippy::cast_precision_loss)]
        return Some(v as f64);
    }
    // More digits than fit in 128 bits: accumulate in floating point.
    let mut v = 0.0f64;
    for c in digits.chars() {
        v = v * f64::from(radix) + f64::from(c.to_digit(radix)?);
    }
    Some(v)
}

// ---------------------------------------------------------------------------
// Escapes
// ---------------------------------------------------------------------------

/// The value of a valid escape.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EscValue {
    Byte(u8),
    /// A code point from `\u{…}`, at most `0x10FFFF` and not a surrogate.
    Code(u32),
}

/// Why an escape is invalid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EscError {
    /// `\` followed by a character that starts no escape.
    Unknown,
    /// `\` directly before a line break.
    LineBreak,
    /// `\` at the very end of the text.
    AtEnd,
    /// `\x` without two hexadecimal digits.
    Hex,
    /// `\ddd` above 255.
    DecTooLarge,
    /// `\u` not followed by `{`.
    UnicodeBrace,
    /// `\u{}`.
    UnicodeEmpty,
    /// `\u{…}` above `10FFFF`.
    UnicodeTooLarge,
    /// `\u{…}` in `D800` to `DFFF`: a surrogate, which is not a character.
    UnicodeSurrogate,
    /// `\u{…` without `}`.
    UnicodeUnclosed,
}

/// Reads one escape. `rest` starts just after the backslash. Returns the
/// value or the error, and the number of bytes after the backslash that
/// belong to the escape (0 when the backslash stands alone before a line
/// break or at the end). The length always ends on a character boundary
/// when `rest` starts on one.
pub(crate) fn read_escape(rest: &[u8]) -> (Result<EscValue, EscError>, usize) {
    let at = |i: usize| rest.get(i).copied();
    let Some(c) = at(0) else {
        return (Err(EscError::AtEnd), 0);
    };
    let simple = match c {
        b'a' => Some(0x07),
        b'b' => Some(0x08),
        b'f' => Some(0x0c),
        b'n' => Some(b'\n'),
        b'r' => Some(b'\r'),
        b't' => Some(b'\t'),
        b'v' => Some(0x0b),
        b'\\' | b'"' | b'\'' | b'#' => Some(c),
        _ => None,
    };
    if let Some(v) = simple {
        return (Ok(EscValue::Byte(v)), 1);
    }
    match c {
        b'\n' | b'\r' => (Err(EscError::LineBreak), 0),
        b'x' => {
            let n = (1..3).take_while(|&i| at(i).is_some_and(|b| b.is_ascii_hexdigit())).count();
            if n == 2 {
                let v = hex_val(at(1)) * 16 + hex_val(at(2));
                (Ok(EscValue::Byte(v)), 3)
            } else {
                (Err(EscError::Hex), 1 + n)
            }
        }
        b'0'..=b'9' => {
            let n = (0..3).take_while(|&i| at(i).is_some_and(|b| b.is_ascii_digit())).count();
            let v = (0..n).fold(0u32, |v, i| v * 10 + u32::from(at(i).unwrap_or(b'0') - b'0'));
            match u8::try_from(v) {
                Ok(b) => (Ok(EscValue::Byte(b)), n),
                Err(_) => (Err(EscError::DecTooLarge), n),
            }
        }
        b'u' => {
            if at(1) != Some(b'{') {
                return (Err(EscError::UnicodeBrace), 1);
            }
            let n = rest
                .iter()
                .skip(2)
                .take_while(|b| b.is_ascii_hexdigit())
                .count();
            let v = rest
                .iter()
                .skip(2)
                .take(n)
                .fold(0u32, |v, &b| v.saturating_mul(16).saturating_add(u32::from(hex_val(Some(b)))));
            if at(2 + n) != Some(b'}') {
                (Err(EscError::UnicodeUnclosed), 2 + n)
            } else if n == 0 {
                (Err(EscError::UnicodeEmpty), 3)
            } else if v > 0x10FFFF {
                (Err(EscError::UnicodeTooLarge), 3 + n)
            } else if (0xD800..=0xDFFF).contains(&v) {
                // As in Rust and Ruby: a surrogate is not a code point a
                // string can hold.
                (Err(EscError::UnicodeSurrogate), 3 + n)
            } else {
                (Ok(EscValue::Code(v)), 3 + n)
            }
        }
        _ => (Err(EscError::Unknown), utf8_len(c).min(rest.len())),
    }
}

fn hex_val(b: Option<u8>) -> u8 {
    match b {
        Some(b @ b'0'..=b'9') => b - b'0',
        Some(b @ b'a'..=b'f') => b - b'a' + 10,
        Some(b @ b'A'..=b'F') => b - b'A' + 10,
        _ => 0,
    }
}

/// Length of the UTF-8 sequence that starts with byte `b`.
pub(crate) fn utf8_len(b: u8) -> usize {
    match b {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xff => 4,
        // A continuation byte: never the start of a character in a `str`.
        _ => 1,
    }
}

/// The message for an invalid escape whose source text is `text` (the
/// backslash included).
pub(crate) fn escape_message(err: EscError, text: &str) -> String {
    match err {
        EscError::Unknown => format!(
            "unknown escape `{text}`. Write `\\\\` for a backslash; for a pattern such as a \
             regex, a raw string `'…'` keeps every backslash as written"
        ),
        EscError::LineBreak | EscError::AtEnd => {
            "a backslash cannot stand directly before a line break in a heredoc, where the line \
             break is already part of the text; write `\\\\` for a backslash"
                .to_string()
        }
        EscError::Hex => {
            format!("`{text}` is not an escape: `\\x` takes exactly two hexadecimal digits, as in `\\x41`")
        }
        EscError::DecTooLarge => format!(
            "`{text}` is larger than 255: a decimal escape writes one byte. Use `\\u{{…}}` for a \
             character"
        ),
        EscError::UnicodeBrace => {
            "write a Unicode escape with braces, as in `\\u{48}` or `\\u{1F600}`".to_string()
        }
        EscError::UnicodeEmpty => {
            "`\\u{}` needs a hexadecimal code point between the braces, as in `\\u{48}`".to_string()
        }
        EscError::UnicodeTooLarge => {
            format!("`{text}` is beyond the last Unicode code point, `\\u{{10FFFF}}`")
        }
        EscError::UnicodeSurrogate => {
            format!("`{text}` is a surrogate, not a character: code points D800 to DFFF cannot be written; for the bytes use `\\xHH`")
        }
        EscError::UnicodeUnclosed => {
            format!("`{text}` is not closed: write a Unicode escape as `\\u{{48}}`, with a closing `}}`")
        }
    }
}

/// Appends the UTF-8 encoding of `c` (a surrogate never gets here; it would be encoded like any
/// other code point, as LuaJIT does).
fn push_code_point(out: &mut Vec<u8>, c: u32) {
    // Each value below is masked to fit in a byte before the conversion.
    let byte = |v: u32| u8::try_from(v & 0xff).unwrap_or(0);
    if c < 0x80 {
        out.push(byte(c));
    } else if c < 0x800 {
        out.extend([byte(0xc0 | (c >> 6)), byte(0x80 | (c & 0x3f))]);
    } else if c < 0x10000 {
        out.extend([
            byte(0xe0 | (c >> 12)),
            byte(0x80 | ((c >> 6) & 0x3f)),
            byte(0x80 | (c & 0x3f)),
        ]);
    } else {
        out.extend([
            byte(0xf0 | ((c >> 18) & 0x07)),
            byte(0x80 | ((c >> 12) & 0x3f)),
            byte(0x80 | ((c >> 6) & 0x3f)),
            byte(0x80 | (c & 0x3f)),
        ]);
    }
}

/// Decodes the escapes of `text` (no delimiters, no interpolation). An
/// invalid escape is kept as written. In a `"…"` string (`quoted`) a
/// backslash before a line break continues the string and writes one `\n`;
/// in a heredoc it is an error the lexer reported, kept as written.
fn decode_escapes(text: &[u8], quoted: bool, out: &mut Vec<u8>) {
    let mut i = 0;
    while let Some(&b) = text.get(i) {
        if b != b'\\' {
            out.push(b);
            i += 1;
            continue;
        }
        let lb = if quoted { line_break_len(text, i + 1) } else { 0 };
        if lb > 0 {
            out.push(b'\n');
            i += 1 + lb;
            continue;
        }
        let rest = text.get(i + 1..).unwrap_or(&[]);
        let (value, len) = read_escape(rest);
        match value {
            Ok(EscValue::Byte(v)) => out.push(v),
            Ok(EscValue::Code(c)) => push_code_point(out, c),
            Err(_) => out.extend_from_slice(text.get(i..i + 1 + len).unwrap_or(b"\\")),
        }
        i += 1 + len;
    }
}

// ---------------------------------------------------------------------------
// Strings
// ---------------------------------------------------------------------------

/// The six string forms, as the opener writes them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StringForm {
    /// `'…'`: only `\'` and `\\` are escapes; no interpolation.
    Raw,
    /// `"…"`: full escapes and interpolation, one line.
    Quoted,
    /// `"""` heredocs. `dedent` is the `-` flag, `raw` the `=` flag (no
    /// escapes, no interpolation).
    Heredoc { dedent: bool, raw: bool },
}

/// The form of a string literal, from the text of its first token (`STRING`
/// or `STR_BEGIN`). `None` if the text does not start with a quote.
pub fn string_form(first_part: &str) -> Option<StringForm> {
    let b = first_part.as_bytes();
    if b.starts_with(b"\"\"\"") {
        let flags = b.get(3..).unwrap_or(&[]);
        let flags: Vec<u8> = flags.iter().copied().take_while(|&c| c == b'-' || c == b'=').take(2).collect();
        Some(StringForm::Heredoc { dedent: flags.contains(&b'-'), raw: flags.contains(&b'=') })
    } else if b.starts_with(b"\"") {
        Some(StringForm::Quoted)
    } else if b.starts_with(b"'") {
        Some(StringForm::Raw)
    } else {
        None
    }
}

/// Decodes a string literal into its text pieces.
///
/// `parts` are the texts of the literal's tokens in order: a single `STRING`,
/// or `STR_BEGIN`, any number of `STR_MID`, and `STR_END`. The result has one
/// byte string per part: for an interpolated string these are the texts
/// between the embedded expressions (`"a#{x}b#{y}"` gives `a`, `b`, and the
/// empty string). Values are bytes, not `String`, because `\xHH` and `\ddd`
/// may write any byte, as in Lua.
///
/// Every string rule is applied here: escapes, `\#`, the dropped
/// line break after a heredoc opener and before its closer, `\n` for every
/// line break of a heredoc, and the `-` dedent. Dedent is computed here, not
/// in the lexer, from the texts of the parts alone (see the crate docs for
/// how a heredoc line that starts inside an interpolation is treated).
///
/// The function is total. Errors were reported by the lexer; here an invalid
/// escape is kept as written, and the empty `STR_END` the lexer emits when it
/// recovers decodes to an empty piece.
pub fn decode_string(parts: &[&str]) -> Vec<Vec<u8>> {
    let Some(first) = parts.first() else {
        return Vec::new();
    };
    match string_form(first) {
        None => vec![Vec::new(); parts.len()],
        Some(StringForm::Raw) => {
            let mut out = vec![decode_raw(first)];
            out.resize(parts.len(), Vec::new());
            out
        }
        Some(StringForm::Quoted) => {
            parts.iter().enumerate().map(|(i, p)| decode_quoted_part(p, i == 0)).collect()
        }
        Some(StringForm::Heredoc { dedent, raw }) => decode_heredoc(parts, dedent, raw),
    }
}

fn is_line_break(b: u8) -> bool {
    b == b'\n' || b == b'\r'
}

/// Length of the line break at `i` (2 for CRLF), 0 if there is none.
fn line_break_len(b: &[u8], i: usize) -> usize {
    match (b.get(i), b.get(i + 1)) {
        (Some(b'\r'), Some(b'\n')) => 2,
        (Some(b'\r' | b'\n'), _) => 1,
        _ => 0,
    }
}

fn decode_raw(text: &str) -> Vec<u8> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 1;
    let end = raw_end(b);
    while i < end {
        let Some(&c) = b.get(i) else { break };
        match c {
            b'\\' if matches!(b.get(i + 1), Some(b'\'' | b'\\')) => {
                out.extend(b.get(i + 1).copied());
                i += 2;
            }
            _ => {
                out.push(c);
                i += 1;
            }
        }
    }
    out
}

/// Where the text of a raw string `'…'` ends: its closing quote, a line
/// break, or the end.
fn raw_end(b: &[u8]) -> usize {
    let mut i = 1;
    while let Some(&c) = b.get(i) {
        match c {
            b'\'' => break,
            _ if is_line_break(c) => break,
            b'\\' if matches!(b.get(i + 1), Some(b'\'' | b'\\')) => i += 2,
            _ => i += 1,
        }
    }
    i.min(b.len())
}

/// Where the text of one part of a `"…"` string ends: its closing `"`, a
/// `#{`, a line break, or the end. A backslash before a line break does not
/// end it: the string continues on the next line.
fn quoted_end(b: &[u8]) -> usize {
    let mut end = 1;
    while let Some(&c) = b.get(end) {
        match c {
            b'"' => break,
            _ if is_line_break(c) => break,
            b'#' if b.get(end + 1) == Some(&b'{') => break,
            b'\\' if line_break_len(b, end + 1) > 0 => end += 1 + line_break_len(b, end + 1),
            b'\\' => end += 1 + read_escape(b.get(end + 1..).unwrap_or(&[])).1,
            _ => end += 1,
        }
    }
    end.min(b.len())
}

/// For each part of a literal, the offset where the decoder stops reading
/// it: the closing delimiter, the `#{` of an interpolation, or the start of
/// a heredoc's closer line; `None` when it reads to the end of the part (or
/// skips it). The lexer and the decoder scan strings separately; this lets a
/// test check that they agree on where each part's text ends.
#[cfg(test)]
pub(crate) fn content_ends(parts: &[&str]) -> Vec<Option<usize>> {
    let Some(first) = parts.first() else { return Vec::new() };
    let at = |end: usize, len: usize| if end < len { Some(end) } else { None };
    match string_form(first) {
        None => vec![None; parts.len()],
        Some(StringForm::Raw) => {
            let mut out = vec![at(raw_end(first.as_bytes()), first.len())];
            out.resize(parts.len(), None);
            out
        }
        Some(StringForm::Quoted) => parts.iter().map(|p| at(quoted_end(p.as_bytes()), p.len())).collect(),
        Some(StringForm::Heredoc { raw, .. }) => heredoc_scan(parts, raw).2,
    }
}

fn decode_quoted_part(text: &str, first: bool) -> Vec<u8> {
    let b = text.as_bytes();
    // Every part starts with `"` (first) or `}` (later); a part that does
    // not is an empty recovery token.
    let expected = if first { b'"' } else { b'}' };
    if b.first() != Some(&expected) {
        return Vec::new();
    }
    let end = quoted_end(b);
    let mut out = Vec::new();
    decode_escapes(b.get(1..end).unwrap_or(&[]), true, &mut out);
    out
}

/// One line, or piece of a line, of heredoc content.
struct HLine<'a> {
    part: usize,
    /// The text, without its line break.
    text: &'a [u8],
    /// The line starts here (as opposed to continuing after a `}`).
    starts_line: bool,
    /// The piece ends at an interpolation `#{` (so the line is not blank).
    open_end: bool,
    /// A line break follows the text.
    has_break: bool,
}

/// True when `b[i..]` is a heredoc closer: blanks, then `"""`.
fn is_closer(b: &[u8], i: usize) -> bool {
    let rest = b.get(i..).unwrap_or(&[]);
    let blanks = rest.iter().take_while(|&&c| c == b' ' || c == b'\t').count();
    rest.get(blanks..).is_some_and(|r| r.starts_with(b"\"\"\""))
}

/// The lines of a heredoc's content, whether its closer was found, and
/// where reading stopped in each part (see [`content_ends`]).
type HeredocScan<'a> = (Vec<HLine<'a>>, bool, Vec<Option<usize>>);

fn heredoc_scan<'a>(parts: &[&'a str], raw: bool) -> HeredocScan<'a> {
    let mut lines: Vec<HLine<'a>> = Vec::new();
    let mut closed = false;
    let mut stops = vec![None; parts.len()];
    for (pi, part) in parts.iter().enumerate() {
        let b = part.as_bytes();
        let (start, mut at_line_start) = if pi == 0 {
            // Skip the opener line: `"""`, flags, blanks, and the line break.
            match b.iter().position(|&c| is_line_break(c)) {
                Some(i) => (i + line_break_len(b, i), true),
                None => return (Vec::new(), false, stops),
            }
        } else if b.first() == Some(&b'}') {
            (1, false)
        } else {
            continue; // an empty recovery token
        };
        let mut pos = start;
        let mut line_start = start;
        let mut starts_line = at_line_start;
        loop {
            if at_line_start {
                at_line_start = false;
                if is_closer(b, pos) {
                    closed = true;
                    if let Some(s) = stops.get_mut(pi) {
                        *s = Some(pos);
                    }
                    break;
                }
            }
            let text = b.get(line_start..pos).unwrap_or(&[]);
            match b.get(pos) {
                None => {
                    lines.push(HLine { part: pi, text, starts_line, open_end: false, has_break: false });
                    break;
                }
                Some(b'#') if !raw && b.get(pos + 1) == Some(&b'{') => {
                    lines.push(HLine { part: pi, text, starts_line, open_end: true, has_break: false });
                    if let Some(s) = stops.get_mut(pi) {
                        *s = Some(pos);
                    }
                    break;
                }
                Some(b'\\') if !raw => pos += 1 + read_escape(b.get(pos + 1..).unwrap_or(&[])).1,
                Some(&c) if is_line_break(c) => {
                    lines.push(HLine { part: pi, text, starts_line, open_end: false, has_break: true });
                    pos += line_break_len(b, pos);
                    line_start = pos;
                    starts_line = true;
                    at_line_start = true;
                }
                Some(_) => pos += 1,
            }
        }
        if closed {
            break;
        }
    }
    (lines, closed, stops)
}

fn decode_heredoc(parts: &[&str], dedent: bool, raw: bool) -> Vec<Vec<u8>> {
    let (mut lines, closed, _) = heredoc_scan(parts, raw);
    if lines.is_empty() && !closed && parts.first().is_some_and(|p| !p.bytes().any(is_line_break)) {
        return vec![Vec::new(); parts.len()];
    }
    // The line break before the closer is not part of the string.
    if closed && let Some(last) = lines.last_mut() {
        last.has_break = false;
    }

    let mut indent: &[u8] = &[];
    if dedent {
        let mut common: Option<&[u8]> = None;
        for line in lines.iter().filter(|l| l.starts_line) {
            let ws = leading_blanks(line.text);
            let blank = !line.open_end && ws.len() == line.text.len();
            if blank {
                continue;
            }
            common = Some(match common {
                None => ws,
                Some(c) => {
                    let n = c.iter().zip(ws).take_while(|(a, b)| a == b).count();
                    c.get(..n).unwrap_or(&[])
                }
            });
        }
        indent = common.unwrap_or(&[]);
    }

    let mut raw_text: Vec<Vec<u8>> = vec![Vec::new(); parts.len()];
    for line in &lines {
        let mut text = line.text;
        if dedent && line.starts_line {
            let ws = leading_blanks(text);
            text = if !line.open_end && ws.len() == text.len() {
                &[]
            } else {
                text.get(indent.len()..).unwrap_or(&[])
            };
        }
        if let Some(out) = raw_text.get_mut(line.part) {
            out.extend_from_slice(text);
            if line.has_break {
                out.push(b'\n');
            }
        }
    }
    if raw {
        return raw_text;
    }
    raw_text
        .iter()
        .map(|t| {
            let mut out = Vec::new();
            decode_escapes(t, false, &mut out);
            out
        })
        .collect()
}

fn leading_blanks(text: &[u8]) -> &[u8] {
    let n = text.iter().take_while(|&&c| c == b' ' || c == b'\t').count();
    text.get(..n).unwrap_or(&[])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(parts: &[&str]) -> String {
        let pieces = decode_string(parts);
        assert_eq!(pieces.len(), parts.len());
        pieces.iter().map(|p| String::from_utf8_lossy(p).into_owned()).collect::<Vec<_>>().join("|")
    }

    #[test]
    fn numbers() {
        assert_eq!(number_value("1"), Some(1.0));
        assert_eq!(number_value("1.1"), Some(1.1));
        assert_eq!(number_value("0xabcdef"), Some(11259375.0));
        assert_eq!(number_value("0XFF"), Some(255.0));
        assert_eq!(number_value("0b1010"), Some(10.0));
        assert_eq!(number_value("1_000_000"), Some(1e6));
        assert_eq!(number_value("5e+20"), Some(5e20));
        assert_eq!(number_value("0.3e12"), Some(0.3e12));
        assert_eq!(number_value("4.57e-3"), Some(4.57e-3));
        assert_eq!(number_value("3px"), None);
        assert_eq!(number_value("0x"), None);
        assert_eq!(number_value("1e"), None);
        assert_eq!(number_value(""), None);
        assert!(number_value(&format!("0x{}", "f".repeat(40))).is_some());
    }

    #[test]
    fn escapes() {
        assert_eq!(decode_string(&[r#""\a\b\f\n\r\t\v\\\"\'\#""#]), vec![b"\x07\x08\x0c\n\r\t\x0b\\\"'#".to_vec()]);
        assert_eq!(decode_string(&[r#""\x41\65\0\255""#]), vec![vec![b'A', b'A', 0, 255]]);
        assert_eq!(one(&[r#""\u{48}\u{e9}\u{1F600}""#]), "Hé😀");
        assert_eq!(one(&[r#""\u{D7FF}\u{E000}""#]), "\u{D7FF}\u{E000}");
        // Invalid escapes are kept as written.
        assert_eq!(one(&[r#""\d\x4\256\u{}\u{110000}\u{D800}\u{41""#]), r"\d\x4\256\u{}\u{110000}\u{D800}\u{41");
        assert_eq!(one(&[r#""a\#{b}""#]), "a#{b}");
    }

    #[test]
    fn raw_strings() {
        assert_eq!(one(&[r"'\d+'"]), r"\d+");
        assert_eq!(one(&[r"'it\'s \\ ok'"]), r"it's \ ok");
        assert_eq!(one(&[r"'unterminated"]), "unterminated");
        assert_eq!(one(&["''"]), "");
    }

    #[test]
    fn interpolated_parts() {
        assert_eq!(one(&["\"a#{", "}b#{", "}c\""]), "a|b|c");
        assert_eq!(one(&["\"#{", "}\""]), "|");
        assert_eq!(one(&["\"x\\n#{", ""]), "x\n|");
        assert_eq!(one(&["\"é #{", "} ü\""]), "é | ü");
    }

    #[test]
    fn heredocs() {
        assert_eq!(one(&["\"\"\"\nmulti\nline\n\"\"\""]), "multi\nline");
        assert_eq!(one(&["\"\"\"\n\"\"\""]), "");
        assert_eq!(one(&["\"\"\"\n\n\"\"\""]), "");
        assert_eq!(one(&["\"\"\"\n\n\n\"\"\""]), "\n");
        assert_eq!(one(&["\"\"\"\r\na\r\nb\r\"\"\""]), "a\nb");
        assert_eq!(one(&["\"\"\"   \n  a\\tb\n  \"\"\""]), "  a\tb");
        assert_eq!(one(&["\"\"\"-\n  first\n    indented\n\n  last\n\"\"\""]), "first\n  indented\n\nlast");
        assert_eq!(one(&["\"\"\"-\n  a\n   \n  b\n    \"\"\""]), "a\n\nb");
        assert_eq!(one(&["\"\"\"=\nno #{a} and no \\n escape\n\"\"\""]), "no #{a} and no \\n escape");
        assert_eq!(one(&["\"\"\"-=\n    kept #{a}\n    as written\n\"\"\""]), "kept #{a}\nas written");
        // Tabs and spaces are compared character for character.
        assert_eq!(one(&["\"\"\"-\n\t a\n\t\tb\n\"\"\""]), " a\n\tb");
        // `\"""` is content.
        assert_eq!(one(&["\"\"\"\n\\\"\"\"\n\"\"\""]), "\"\"\"");
        // A backslash before a line break continues a `"…"` string only; in
        // a heredoc it is an error, kept as written.
        assert_eq!(one(&["\"a\\\nb\""]), "a\nb");
        assert_eq!(one(&["\"\"\"\na\\\nb\n\"\"\""]), "a\\\nb");
    }

    #[test]
    fn heredoc_with_interpolation() {
        // """-
        //   last #{a}
        //   x #{b} y
        // """
        let parts = ["\"\"\"-\n  last #{", "}\n  x #{", "} y\n\"\"\""];
        assert_eq!(one(&parts), "last |\nx | y");
        // A line that starts with an interpolation is not blank.
        let parts = ["\"\"\"-\n    a\n  #{", "}\n\"\"\""];
        assert_eq!(one(&parts), "  a\n|");
        // A line that starts inside a multi-line interpolation does not count.
        let parts = ["\"\"\"-\n    a #{", "}\n    b\n\"\"\""];
        assert_eq!(one(&parts), "a |\nb");
        // Recovery tokens.
        assert_eq!(one(&["\"\"\"\n  a #{", "\"\"\""]), "  a |");
        assert_eq!(one(&["\"\"\"abc\"\"\""]), "");
    }

    #[test]
    fn forms() {
        assert_eq!(string_form("'a'"), Some(StringForm::Raw));
        assert_eq!(string_form("\"a\""), Some(StringForm::Quoted));
        assert_eq!(string_form("\"\"\"-=\n\"\"\""), Some(StringForm::Heredoc { dedent: true, raw: true }));
        assert_eq!(string_form("\"\"\"=\n\"\"\""), Some(StringForm::Heredoc { dedent: false, raw: true }));
        assert_eq!(string_form("x"), None);
        assert!(decode_string(&[]).is_empty());
    }
}
