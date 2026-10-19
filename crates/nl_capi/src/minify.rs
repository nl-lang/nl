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

/// `src` minified (module docs), or why it does not lex.
pub fn minify(src: &str, options: MinifyOptions) -> Result<String, String> {
    let src = normalize_breaks(src);
    let tokens = lex(&src, options.glua)?;
    let mut out = String::with_capacity(src.len() / 2);
    // The last token on the current output line, if any.
    let mut prev: Option<&Token> = None;
    for t in &tokens {
        match t.kind {
            Kind::Break => {
                out.push('\n');
                prev = None;
            }
            Kind::Comment => {
                // A comment is the line breaks it spans.
                for _ in t.text.matches('\n') {
                    out.push('\n');
                }
                if t.text.contains('\n') {
                    prev = None;
                }
            }
            _ => {
                if let Some(p) = prev {
                    if needs_space(p, t) {
                        out.push(' ');
                    }
                }
                if options.escape_gmod_macro && t.kind == Kind::Quoted && t.text.contains(GMOD_MACRO) {
                    out.push_str(&t.text.replace(GMOD_MACRO, "DEFINE_\\066ASECLASS"));
                } else {
                    out.push_str(t.text);
                }
                prev = Some(t);
                if t.text.contains('\n') {
                    // A long string spanning lines: what follows it is on
                    // its last line, after `]]`, which joins nothing.
                    prev = None;
                }
            }
        }
    }
    Ok(out)
}

fn normalize_breaks(src: &str) -> String {
    if !src.contains('\r') {
        return src.to_owned();
    }
    let mut out = String::with_capacity(src.len());
    let mut chars = src.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                chars.next_if_eq(&'\n');
                out.push('\n');
            }
            '\n' => {
                chars.next_if_eq(&'\r');
                out.push('\n');
            }
            c => out.push(c),
        }
    }
    out
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
    text: &'a str,
}

/// A byte that continues a name or a number: LuaJIT takes every byte
/// from 0x80 up as part of a name.
fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
}

/// Whether two tokens written together would lex as something else.
fn needs_space(p: &Token, n: &Token) -> bool {
    let (Some(&last), Some(&first)) = (p.text.as_bytes().last(), n.text.as_bytes().first()) else { return false };
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
fn long_end(src: &str, from: usize, level: usize) -> Option<usize> {
    let close = format!("]{}]", "=".repeat(level));
    src.get(from..)?.find(&close).map(|k| from + k + close.len())
}

fn lex(src: &str, glua: bool) -> Result<Vec<Token<'_>>, String> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let line_at = |i: usize| src.get(..i).map_or(0, |s| s.matches('\n').count()) + 1;
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
                    None => i = src.get(i..).and_then(|r| r.find('\n')).map_or(b.len(), |k| i + k),
                }
                Kind::Comment
            }
            // GLua's comments: `//` to the end of the line, `/* … */` (to
            // the end of the file when unterminated).
            b'/' if glua && b.get(i + 1) == Some(&b'/') => {
                i = src.get(i..).and_then(|r| r.find('\n')).map_or(b.len(), |k| i + k);
                Kind::Comment
            }
            b'/' if glua && b.get(i + 1) == Some(&b'*') => {
                i = src.get(i + 2..).and_then(|r| r.find("*/")).map_or(b.len(), |k| i + 2 + k + 2);
                Kind::Comment
            }
            b'"' | b'\'' => {
                i += 1;
                loop {
                    match b.get(i) {
                        None | Some(b'\n') => return Err(format!("line {}: unfinished string", line_at(start))),
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
        // A quoted string or a long string never ends inside a UTF-8
        // sequence, and every other token is ASCII.
        let text = src.get(start..i).ok_or_else(|| format!("line {}: a token ends inside a character", line_at(start)))?;
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
    fn unfinished_tokens_are_errors() {
        assert!(minify("x = 'abc\n'", LUA).is_err());
        assert!(minify("x = [[abc", LUA).is_err());
        assert!(minify("--[==[ abc ]]", LUA).is_err());
    }
}
