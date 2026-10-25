//! A minifier for Lua and GLua (Garry's Mod's dialect): comments,
//! indentation and the spaces that separate nothing go; every line break
//! stays, so each line keeps its number and an error or a trace points at
//! the same line as before. The packager (bundle.rs) minifies what it
//! bundles with it; hosts reach it through `nlb_minify` (nlbundle.h).
//!
//! Lines are kept on purpose. LuaJIT reports positions as lines only, so a
//! line that held two lines' code would lose which one failed, and the
//! line structure costs little: on the runtime and on the Lua of the
//! conformance suite, joining lines and renaming locals as well would save
//! another 2–5% of the LZMA-compressed size that gmod limits, against 55%
//! (the runtime's comments) for what this does.
//!
//! Line breaks are normalized to `\n`, as Lua's lexer reads them (`\r\n`,
//! `\n\r` and a lone `\r` are one break each); inside a long string that
//! gives the same value, and a raw break inside a quoted string can only
//! follow a backslash, where it is one `\n` too. Strings keep their text.
//!
//! Lua source is bytes, and the minifier reads it as bytes: a script saved
//! as Latin-1 or Windows-1252 keeps every byte of its strings and names
//! (LuaJIT takes the bytes from 0x80 up as name bytes) as written.
//!
//! [`MinifyOptions::glua`] reads GLua: `//` and `/* … */` comments (an
//! unterminated `/*` runs to the end of the file, as in gmod) are comments,
//! and `!=`, `&&`, `||` are tokens. Plain Lua lexes the same either way,
//! as none of these is valid Lua; the output never joins two tokens into
//! one of them, in either mode.
//!
//! [`MinifyOptions::escape_gmod_macro`] spells the name of gmod's one
//! macro, `DEFINE_BASECLASS`, which gmod replaces wherever it occurs in a
//! script, strings included, with an escape inside quoted strings
//! (`DEFINE_\066ASECLASS`), so that a string keeps its text. That is for
//! code whose strings mean what they say (NL's output); a GLua script
//! written for gmod expects the replacement, and is minified without it.
//! Where the name remains (a name, a long string) the caller decides.

/// How [`minify`] reads and writes (module docs).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MinifyOptions {
    pub glua: bool,
    pub escape_gmod_macro: bool,
}

impl MinifyOptions {
    /// For the Lua the compiler writes: read as gmod reads it, strings
    /// kept as written.
    pub const COMPILED: MinifyOptions = MinifyOptions { glua: true, escape_gmod_macro: true };
}

/// The name gmod replaces in every script it loads, before compiling it
/// (with `local BaseClass = baseclass.Get`).
pub const GMOD_MACRO: &str = "DEFINE_BASECLASS";

/// `src` minified (module docs), or why it does not lex. Text in, text
/// out: [`minify_bytes`] over its bytes.
pub fn minify(src: &str, options: MinifyOptions) -> Result<String, String> {
    // Every token is copied whole and what is dropped is whole too, so the
    // minified text of UTF-8 is UTF-8.
    minify_bytes(src.as_bytes(), options).map(|out| String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned()))
}

/// `src` minified (module docs), or why it does not lex. Lua source is
/// bytes, not necessarily UTF-8 (a script saved as Latin-1 or Windows-1252):
/// every byte of a string or a name is kept as it is.
pub fn minify_bytes(src: &[u8], options: MinifyOptions) -> Result<Vec<u8>, String> {
    let src = normalize_breaks(src);
    let tokens = lex(&src, options.glua)?;
    let mut out = Vec::with_capacity(src.len() / 2);
    // The last token on the current output line, if any.
    let mut prev: Option<&Token> = None;
    for t in &tokens {
        match t.kind {
            Kind::Break => {
                out.push(b'\n');
                prev = None;
            }
            Kind::Comment => {
                // A comment is the line breaks it spans.
                let breaks = count_breaks(t.text);
                out.extend(std::iter::repeat_n(b'\n', breaks));
                if breaks > 0 {
                    prev = None;
                }
            }
            _ => {
                if let Some(p) = prev {
                    if needs_space(p, t) {
                        out.push(b' ');
                    }
                }
                if options.escape_gmod_macro && t.kind == Kind::Quoted && find(t.text, GMOD_MACRO.as_bytes(), 0).is_some() {
                    replace_into(&mut out, t.text, GMOD_MACRO.as_bytes(), b"DEFINE_\\066ASECLASS");
                } else {
                    out.extend_from_slice(t.text);
                }
                prev = Some(t);
                if count_breaks(t.text) > 0 {
                    // A long string spanning lines: what follows it is on
                    // its last line, after `]]`, which joins nothing.
                    prev = None;
                }
            }
        }
    }
    Ok(out)
}

fn normalize_breaks(src: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    if !src.contains(&b'\r') {
        return std::borrow::Cow::Borrowed(src);
    }
    let mut out = Vec::with_capacity(src.len());
    let mut bytes = src.iter().copied().peekable();
    while let Some(c) = bytes.next() {
        match c {
            b'\r' => {
                bytes.next_if_eq(&b'\n');
                out.push(b'\n');
            }
            b'\n' => {
                bytes.next_if_eq(&b'\r');
                out.push(b'\n');
            }
            c => out.push(c),
        }
    }
    std::borrow::Cow::Owned(out)
}

/// The line breaks in `text` (normalized: `\n` only).
fn count_breaks(text: &[u8]) -> usize {
    text.iter().filter(|&&b| b == b'\n').count()
}

/// The first `needle` in `hay` at or after `from`.
fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    hay.get(from..)?.windows(needle.len()).position(|w| w == needle).map(|k| from + k)
}

/// `text` with every `from` replaced by `to`, appended to `out`.
fn replace_into(out: &mut Vec<u8>, text: &[u8], from: &[u8], to: &[u8]) {
    let mut at = 0;
    while let Some(k) = find(text, from, at) {
        out.extend_from_slice(text.get(at..k).unwrap_or_default());
        out.extend_from_slice(to);
        at = k + from.len();
    }
    out.extend_from_slice(text.get(at..).unwrap_or_default());
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Name,
    Number,
    Quoted,
    Long,
    Op,
    Comment,
    Break,
}

#[derive(Debug)]
struct Token<'a> {
    kind: Kind,
    text: &'a [u8],
}

/// A byte that continues a name or a number: LuaJIT takes every byte
/// from 0x80 up as part of a name.
fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
}

/// Whether two tokens written together would lex as something else.
fn needs_space(p: &Token, n: &Token) -> bool {
    let (Some(&last), Some(&first)) = (p.text.last(), n.text.first()) else { return false };
    if is_ident(last) && is_ident(first) {
        return true;
    }
    // `1 ..x` is a malformed number `1..x`; `1 .5`, too.
    if p.kind == Kind::Number && (first == b'.' || is_ident(first)) {
        return true;
    }
    // Two characters that start a longer token or a comment: `--`, `..`,
    // `==`, `~=`, `<=`, `>=`, `::`, `[[`, `[=`; and GLua's `//`, `/*`,
    // `&&`, `||`, `!=` (gmod reads the bundles with its own lexer).
    matches!(
        (last, first),
        (b'-', b'-') | (b'.', b'.') | (b'=' | b'~' | b'<' | b'>' | b'!', b'=') | (b':', b':') | (b'[', b'[' | b'=') | (b'/', b'/' | b'*') | (b'&', b'&') | (b'|', b'|')
    )
}

/// The level of a long bracket at `i` (`[[` is 0, `[==[` is 2).
fn long_level(b: &[u8], i: usize) -> Option<usize> {
    if b.get(i) != Some(&b'[') {
        return None;
    }
    let mut j = i + 1;
    while b.get(j) == Some(&b'=') {
        j += 1;
    }
    (b.get(j) == Some(&b'[')).then_some(j - i - 1)
}

/// The end of the long bracket of `level` whose body starts at `from`.
fn long_end(src: &[u8], from: usize, level: usize) -> Option<usize> {
    let close = format!("]{}]", "=".repeat(level));
    find(src, close.as_bytes(), from).map(|k| k + close.len())
}

fn lex(src: &[u8], glua: bool) -> Result<Vec<Token<'_>>, String> {
    let b = src;
    let mut out = Vec::new();
    let mut i = 0;
    let line_at = |i: usize| count_breaks(src.get(..i).unwrap_or_default()) + 1;
    let line_end = |i: usize| find(src, b"\n", i).unwrap_or(src.len());
    while let Some(&c) = b.get(i) {
        let start = i;
        let kind = match c {
            b'\n' => {
                i += 1;
                Kind::Break
            }
            b' ' | b'\t' | 0x0b | 0x0c => {
                i += 1;
                continue;
            }
            b'-' if b.get(i + 1) == Some(&b'-') => {
                match long_level(b, i + 2) {
                    Some(level) => i = long_end(src, i + 2 + level + 2, level).ok_or_else(|| format!("line {}: unfinished long comment", line_at(start)))?,
                    None => i = line_end(i),
                }
                Kind::Comment
            }
            // GLua's comments: `//` to the end of the line, `/* … */` (to
            // the end of the file when unterminated).
            b'/' if glua && b.get(i + 1) == Some(&b'/') => {
                i = line_end(i);
                Kind::Comment
            }
            b'/' if glua && b.get(i + 1) == Some(&b'*') => {
                i = find(src, b"*/", i + 2).map_or(b.len(), |k| k + 2);
                Kind::Comment
            }
            b'"' | b'\'' => {
                i += 1;
                loop {
                    match b.get(i) {
                        None | Some(b'\n') => return Err(format!("line {}: unfinished string", line_at(start))),
                        // `\z` skips the white space after it, line
                        // breaks included, which the token keeps.
                        Some(b'\\') if b.get(i + 1) == Some(&b'z') => {
                            i += 2;
                            while b.get(i).is_some_and(|&c| matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c)) {
                                i += 1;
                            }
                        }
                        Some(b'\\') => i += 2,
                        Some(&q) if q == c => {
                            i += 1;
                            break;
                        }
                        Some(_) => i += 1,
                    }
                }
                Kind::Quoted
            }
            b'[' if long_level(b, i).is_some() => {
                let level = long_level(b, i).unwrap_or(0);
                i = long_end(src, i + level + 2, level).ok_or_else(|| format!("line {}: unfinished long string", line_at(start)))?;
                Kind::Long
            }
            b'0'..=b'9' => {
                i = number_end(b, i);
                Kind::Number
            }
            b'.' if b.get(i + 1).is_some_and(u8::is_ascii_digit) => {
                i = number_end(b, i);
                Kind::Number
            }
            c if is_ident(c) => {
                while b.get(i).is_some_and(|&c| is_ident(c)) {
                    i += 1;
                }
                Kind::Name
            }
            _ => {
                let rest = b.get(i..).unwrap_or_default();
                let ops: &[&str] = if glua { &["...", "..", "==", "~=", "<=", ">=", "::", "!=", "&&", "||"] } else { &["...", "..", "==", "~=", "<=", ">=", "::"] };
                i += ops.iter().find(|op| rest.starts_with(op.as_bytes())).map_or(1, |op| op.len());
                Kind::Op
            }
        };
        // An escape at the end of the text steps past it.
        let text = src.get(start..i.min(src.len())).unwrap_or_default();
        out.push(Token { kind, text });
    }
    Ok(out)
}

/// The end of a number at `i`, as LuaJIT's lexer reads one: name bytes
/// and dots, and a sign right after an exponent letter.
fn number_end(b: &[u8], mut i: usize) -> usize {
    let hex = b.get(i) == Some(&b'0') && matches!(b.get(i + 1), Some(b'x' | b'X'));
    while let Some(&c) = b.get(i) {
        let exponent = if hex { matches!(c, b'p' | b'P') } else { matches!(c, b'e' | b'E') };
        if exponent && matches!(b.get(i + 1), Some(b'+' | b'-')) {
            i += 2;
        } else if is_ident(c) || c == b'.' {
            i += 1;
        } else {
            break;
        }
    }
    i
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    const LUA: MinifyOptions = MinifyOptions { glua: false, escape_gmod_macro: false };

    #[test]
    fn keeps_lines_and_drops_comments() {
        let src = "-- header\nlocal  x = 1 -- one\n\n  --[[ a\nlong\ncomment ]] local y = { a = 1,\n\tb = 2 }\nreturn x+y\n";
        assert_eq!(minify(src, LUA).unwrap(), "\nlocal x=1\n\n\n\nlocal y={a=1,\nb=2}\nreturn x+y\n");
    }

    #[test]
    fn spaces_that_change_the_tokens_stay() {
        assert_eq!(minify("a = b - -c", LUA).unwrap(), "a=b- -c");
        assert_eq!(minify("x = 1 .. y .. 2 .. .5", LUA).unwrap(), "x=1 ..y..2 .. .5");
        assert_eq!(minify("x = a.b .. 3", LUA).unwrap(), "x=a.b..3");
        assert_eq!(minify("t[ [[s]] ] = t[ [=[q]=] ]", LUA).unwrap(), "t[ [[s]]]=t[ [=[q]=]]");
        assert_eq!(minify("if a == b and not c then return end", LUA).unwrap(), "if a==b and not c then return end");
        assert_eq!(minify("x = a < = b", LUA).unwrap(), "x=a< =b");
        assert_eq!(minify("x = 0x1p+4 + 1e-3 - 2", LUA).unwrap(), "x=0x1p+4+1e-3-2");
        assert_eq!(minify("goto l ::l::", LUA).unwrap(), "goto l::l::");
        assert_eq!(minify("a = b / / c", LUA).unwrap(), "a=b/ /c");
    }

    #[test]
    fn strings_keep_their_text() {
        let src = "s = 'a  -- b' .. \"c\\\"  d\" .. [==[\n x ]] y\n]==] .. \"e\\\nf\"\nz = 1";
        assert_eq!(minify(src, LUA).unwrap(), "s='a  -- b'..\"c\\\"  d\"..[==[\n x ]] y\n]==]..\"e\\\nf\"\nz=1");
    }

    #[test]
    fn line_breaks_are_normalized() {
        assert_eq!(minify("a = 1\r\nb = 2\rc = 3\n\rd = [[x\r\ny]]", LUA).unwrap(), "a=1\nb=2\nc=3\nd=[[x\ny]]");
    }

    #[test]
    fn gmods_macro_is_escaped_in_quoted_strings() {
        let o = MinifyOptions::COMPILED;
        assert_eq!(minify("print('DEFINE_BASECLASS x')", o).unwrap(), "print('DEFINE_\\066ASECLASS x')");
        // Elsewhere it stays, for the caller to report.
        assert!(minify("DEFINE_BASECLASS('x')", o).unwrap().contains(GMOD_MACRO));
        // A GLua script keeps it, as gmod's replacement is what it expects.
        assert_eq!(minify("print('DEFINE_BASECLASS x')", LUA).unwrap(), "print('DEFINE_BASECLASS x')");
    }

    #[test]
    fn glua() {
        let g = MinifyOptions { glua: true, escape_gmod_macro: false };
        let src = "local a = 1 // one\nif a != 2 && !b || c then /* x\ny */ print('//', \"/*\") end\n/* open\nto the end";
        assert_eq!(minify(src, g).unwrap(), "local a=1\nif a!=2&&!b||c then\nprint('//',\"/*\")end\n\n");
        // Division stays division.
        assert_eq!(minify("x = a / b / c", g).unwrap(), "x=a/b/c");
        // Without GLua, `//` is two operators (and no valid Lua).
        assert_eq!(minify("x = a // b", LUA).unwrap(), "x=a/ /b");
        // `!` before `=` is kept apart from it, `a ! = b` being two tokens.
        assert_eq!(minify("x = ! = y", g).unwrap(), "x=! =y");
    }

    #[test]
    fn bytes_that_are_not_utf8_are_kept() {
        // A Latin-1 script: two locals whose names differ in a byte from
        // 0x80 up, and a string with such a byte; each stays as written.
        let src: &[u8] = b"local \xe9 = 1 -- \xff\nlocal \xe8 = 2\r\nprint(\xe9, 'caf\xe9', [[\xe8]])\n";
        assert_eq!(minify_bytes(src, LUA).unwrap(), b"local \xe9=1\nlocal \xe8=2\nprint(\xe9,'caf\xe9',[[\xe8]])\n");
        let g = MinifyOptions { glua: true, escape_gmod_macro: true };
        assert_eq!(minify_bytes(b"x = '\x80DEFINE_BASECLASS\x81' // \xfe\n", g).unwrap(), b"x='\x80DEFINE_\\066ASECLASS\x81'\n");
    }

    #[test]
    fn z_escapes_skip_line_breaks() {
        // `\z` and the white space after it are part of the string, line
        // breaks included, which stay.
        assert_eq!(minify("s = \"a\\z\n   b\" .. c\nd = 1", LUA).unwrap(), "s=\"a\\z\n   b\"..c\nd=1");
        assert_eq!(minify("s = 'a\\z  b'", LUA).unwrap(), "s='a\\z  b'");
        assert!(minify("s = 'a\\z", LUA).is_err());
    }

    #[test]
    fn unfinished_tokens_are_errors() {
        assert!(minify("x = 'abc\n'", LUA).is_err());
        assert!(minify("x = [[abc", LUA).is_err());
        assert!(minify("--[==[ abc ]]", LUA).is_err());
    }
}
