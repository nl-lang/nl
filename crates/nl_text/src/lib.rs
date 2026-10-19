//! NL's UTF-8 string operations.
//!
//! A `String` holds bytes; NL's library reads them as UTF-8 and counts in
//! characters (code points). This crate is the part of that library that
//! needs to be fast or needs Unicode tables: validity, counting, the scan
//! from a character position to a byte offset (with an ASCII fast path),
//! substrings by character range, case mapping with Rust's Unicode tables,
//! and decoding for `chars`, `codepoints` and the string iterator. The
//! rest of the library (search, split, padding, replacement) is Lua in
//! `runtime/nl_80_string.lua`, built on these.
//!
//! Every function here takes bytes and is total: given a string that is
//! not valid UTF-8 it returns something harmless (the VM checks validity
//! with [`invalid_at`] once per string, remembers the answer in the string
//! object, and raises `EncodingError` before calling a character
//! operation). Positions in the safe API are 0-based byte offsets and
//! 0-based character indexes, except where a function takes NL's 1-based,
//! negative-from-the-end positions and says so.
//!
//! The C ABI over it is in [`abi`] (`nltext.h` in the VM). Nothing here
//! calls into Lua or unwinds across the boundary.

/// The C ABI (`nltext.h`). The only module allowed unsafe code.
#[allow(unsafe_code)]
pub mod abi;
/// The case mappings as text, for a string library written in Lua.
pub mod case_tables;

/// The byte index of the first invalid UTF-8 sequence, or `None` when the
/// bytes are valid (Rust's rules: no overlong forms, no surrogates, nothing
/// above U+10FFFF, no truncated sequences).
pub fn invalid_at(bytes: &[u8]) -> Option<usize> {
    std::str::from_utf8(bytes).err().map(|e| e.valid_up_to())
}

/// True when `b` starts a character: it is not a continuation byte.
#[inline]
fn is_start(b: u8) -> bool {
    b & 0xC0 != 0x80
}

/// The length of the leading run of ASCII bytes, found 8 bytes at a time.
fn ascii_prefix(bytes: &[u8]) -> usize {
    let mut n = 0;
    let mut chunks = bytes.chunks_exact(8);
    for chunk in &mut chunks {
        let word = chunk.iter().fold(0u64, |acc, &b| (acc << 8) | u64::from(b));
        if word & 0x8080_8080_8080_8080 != 0 {
            break;
        }
        n += 8;
    }
    n + bytes.get(n..).unwrap_or(&[]).iter().take_while(|&&b| b < 0x80).count()
}

/// The number of characters: the bytes that start one.
pub fn count(bytes: &[u8]) -> usize {
    let ascii = ascii_prefix(bytes);
    if ascii == bytes.len() {
        return ascii;
    }
    ascii + bytes.get(ascii..).unwrap_or(&[]).iter().filter(|&&b| is_start(b)).count()
}

/// The byte offset where the character with 0-based index `pos` starts;
/// the length of the string when `pos` is the character count or more.
pub fn offset(bytes: &[u8], pos: usize) -> usize {
    let ascii = ascii_prefix(bytes);
    if pos <= ascii {
        return pos;
    }
    let mut n = ascii;
    for (i, &b) in bytes.iter().enumerate().skip(ascii) {
        if is_start(b) {
            if n == pos {
                return i;
            }
            n += 1;
        }
    }
    bytes.len()
}

/// The number of characters that start before byte offset `byte` (clamped
/// to the length): the 0-based character index of the character at a byte
/// offset that starts one.
pub fn charpos(bytes: &[u8], byte: usize) -> usize {
    let end = byte.min(bytes.len());
    count(bytes.get(..end).unwrap_or(&[]))
}

/// The length of the character that starts with byte `b` (1 for a
/// continuation byte, so that a scan over bad input still moves).
#[inline]
pub fn char_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b < 0xE0 {
        2
    } else if b < 0xF0 {
        3
    } else {
        4
    }
}

/// The code point that starts at byte offset `byte` and its byte length;
/// `None` at the end of the string or when no valid character starts
/// there.
pub fn decode(bytes: &[u8], byte: usize) -> Option<(u32, usize)> {
    let rest = bytes.get(byte..)?;
    let &first = rest.first()?;
    let len = char_len(first).min(rest.len());
    let text = std::str::from_utf8(rest.get(..len)?).ok()?;
    let c = text.chars().next()?;
    Some((u32::from(c), c.len_utf8()))
}

/// The UTF-8 bytes of code point `cp`, or `None` when it is not a Unicode
/// scalar value (a surrogate or above U+10FFFF).
pub fn encode(cp: u32) -> Option<([u8; 4], usize)> {
    let c = char::from_u32(cp)?;
    let mut buf = [0u8; 4];
    let n = c.encode_utf8(&mut buf).len();
    Some((buf, n))
}

/// NL's positions: 1-based, inclusive, a negative one counts
/// from the end, and a range is clamped to the string as Lua's `string.sub`
/// clamps. Turns `first` and `last` (with `last` exclusive when
/// `exclusive`) into 1-based inclusive bounds over `n` items, or `None`
/// when nothing is selected.
pub fn clamp(first: i64, last: i64, n: usize, exclusive: bool) -> Option<(usize, usize)> {
    let n_i = i64::try_from(n).unwrap_or(i64::MAX);
    let mut first = if first < 0 { n_i.saturating_add(first).saturating_add(1) } else { first };
    let mut last = if last < 0 { n_i.saturating_add(last).saturating_add(1) } else { last };
    if exclusive {
        last = last.saturating_sub(1);
    }
    if first < 1 {
        first = 1;
    }
    if last > n_i {
        last = n_i;
    }
    if first > last {
        return None;
    }
    Some((usize::try_from(first).unwrap_or(0), usize::try_from(last).unwrap_or(0)))
}

/// `s[first..last]` and `s[first...last]`: the byte range of the characters
/// between two NL positions (see [`clamp`]), or `None` when it is empty.
pub fn slice(bytes: &[u8], first: i64, last: i64, exclusive: bool) -> Option<(usize, usize)> {
    let (a, b) = clamp(first, last, count(bytes), exclusive)?;
    let start = offset(bytes, a - 1);
    let end = start + offset(bytes.get(start..).unwrap_or(&[]), b - a + 1);
    Some((start, end))
}

/// `s[pos]`: the byte range of the character at NL position `pos`
/// (1-based, negative from the end), or `None` when it is out of range.
pub fn char_at(bytes: &[u8], pos: i64) -> Option<(usize, usize)> {
    let n = count(bytes);
    let n_i = i64::try_from(n).unwrap_or(i64::MAX);
    let pos = if pos < 0 { n_i.saturating_add(pos).saturating_add(1) } else { pos };
    if pos < 1 || pos > n_i {
        return None;
    }
    let start = offset(bytes, usize::try_from(pos).unwrap_or(0) - 1);
    let len = bytes.get(start).map_or(0, |&b| char_len(b)).min(bytes.len() - start);
    Some((start, start + len))
}

/// A case mapping of NL's strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseMode {
    /// `upcase`.
    Up,
    /// `downcase`.
    Down,
    /// `capitalize`: the first character in upper case, the rest in lower
    /// case. Rust's tables have no title case, so a digraph such as `ǆ`
    /// becomes `Ǆ` and not `ǅ`.
    Capitalize,
    /// `swapcase`: upper to lower and lower to upper, the rest unchanged.
    Swap,
}

impl CaseMode {
    /// The mode for the number of `nltext.h` (0 up, 1 down, 2 capitalize,
    /// 3 swap).
    pub fn from_code(code: i32) -> Option<CaseMode> {
        match code {
            0 => Some(CaseMode::Up),
            1 => Some(CaseMode::Down),
            2 => Some(CaseMode::Capitalize),
            3 => Some(CaseMode::Swap),
            _ => None,
        }
    }
}

/// Unicode case mapping with Rust's tables; one character may become
/// several (`ß` upcases to `SS`). ASCII strings take a fast path.
pub fn map_case(text: &str, mode: CaseMode) -> String {
    if text.is_ascii() {
        return match mode {
            CaseMode::Up => text.to_ascii_uppercase(),
            CaseMode::Down => text.to_ascii_lowercase(),
            CaseMode::Capitalize => {
                let mut out = text.to_ascii_lowercase();
                out.get_mut(..1).map(str::make_ascii_uppercase);
                out
            }
            CaseMode::Swap => text.chars().map(|c| if c.is_ascii_uppercase() { c.to_ascii_lowercase() } else { c.to_ascii_uppercase() }).collect(),
        };
    }
    match mode {
        CaseMode::Up => text.to_uppercase(),
        CaseMode::Down => text.to_lowercase(),
        CaseMode::Capitalize => {
            let mut chars = text.chars();
            let mut out = String::with_capacity(text.len());
            if let Some(first) = chars.next() {
                out.extend(first.to_uppercase());
            }
            out.push_str(&chars.as_str().to_lowercase());
            out
        }
        CaseMode::Swap => {
            let mut out = String::with_capacity(text.len());
            for c in text.chars() {
                if c.is_uppercase() {
                    out.extend(c.to_lowercase());
                } else if c.is_lowercase() {
                    out.extend(c.to_uppercase());
                } else {
                    out.push(c);
                }
            }
            out
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn validity() {
        assert_eq!(invalid_at(b""), None);
        assert_eq!(invalid_at("héllo".as_bytes()), None);
        assert_eq!(invalid_at(b"ab\xff"), Some(2));
        assert_eq!(invalid_at(b"\xc3"), Some(0), "truncated");
        assert_eq!(invalid_at(b"\xc0\x80"), Some(0), "overlong");
        assert_eq!(invalid_at(b"\xed\xa0\x80"), Some(0), "surrogate");
        assert_eq!(invalid_at(b"\xf4\x90\x80\x80"), Some(0), "above U+10FFFF");
        assert_eq!(invalid_at(b"a\x80"), Some(1), "stray continuation");
    }

    #[test]
    fn counting_and_offsets() {
        let s = "héllo wörld, ascii tail that is longer than eight bytes".as_bytes();
        assert_eq!(count(s), 55);
        assert_eq!(count(b""), 0);
        assert_eq!(count(b"plain ascii string"), 18);
        assert_eq!(offset(s, 0), 0);
        assert_eq!(offset(s, 1), 1);
        assert_eq!(offset(s, 2), 3);
        assert_eq!(offset(s, 5), 6);
        assert_eq!(offset(s, 55), s.len());
        assert_eq!(offset(s, 99), s.len());
        let a = b"0123456789abcdef";
        assert_eq!(offset(a, 12), 12);
        assert_eq!(offset(a, 16), 16);
        assert_eq!(offset(a, 17), 16);
        assert_eq!(charpos(s, 0), 0);
        assert_eq!(charpos(s, 3), 2);
        assert_eq!(charpos(s, 6), 5);
        assert_eq!(charpos(s, 1000), 55);
        assert_eq!(ascii_prefix(b"0123456789abcdef"), 16);
        assert_eq!(ascii_prefix("0123456789abcdéf".as_bytes()), 14);
        assert_eq!(ascii_prefix("é".as_bytes()), 0);
    }

    #[test]
    fn decoding() {
        let s = "aé€😀".as_bytes();
        assert_eq!(decode(s, 0), Some((0x61, 1)));
        assert_eq!(decode(s, 1), Some((0xE9, 2)));
        assert_eq!(decode(s, 3), Some((0x20AC, 3)));
        assert_eq!(decode(s, 6), Some((0x1F600, 4)));
        assert_eq!(decode(s, 10), None);
        assert_eq!(decode(s, 2), None, "inside a character");
        assert_eq!(decode(b"\xff", 0), None);
        assert_eq!(encode(0x1F600), Some((*b"\xf0\x9f\x98\x80", 4)));
        assert_eq!(encode(0x41).map(|(b, n)| b[..n].to_vec()), Some(b"A".to_vec()));
        assert_eq!(encode(0xD800), None);
        assert_eq!(encode(0x110000), None);
    }

    #[test]
    fn slicing() {
        let s = "héllo".as_bytes();
        let text = |r: Option<(usize, usize)>| r.map(|(a, b)| std::str::from_utf8(&s[a..b]).unwrap().to_owned());
        assert_eq!(text(slice(s, 1, 2, false)), Some("hé".into()));
        assert_eq!(text(slice(s, 1, 2, true)), Some("h".into()));
        assert_eq!(text(slice(s, -3, -1, false)), Some("llo".into()));
        assert_eq!(text(slice(s, 2, 2, false)), Some("é".into()));
        assert_eq!(text(slice(s, 0, 2, false)), Some("hé".into()));
        assert_eq!(text(slice(s, 4, 100, false)), Some("lo".into()));
        assert_eq!(slice(s, 3, 2, false), None);
        assert_eq!(slice(s, 6, 9, false), None);
        assert_eq!(slice(b"", 1, 1, false), None);
        assert_eq!(text(char_at(s, 2)), Some("é".into()));
        assert_eq!(text(char_at(s, -1)), Some("o".into()));
        assert_eq!(char_at(s, 0), None);
        assert_eq!(char_at(s, 6), None);
        assert_eq!(char_at(s, -6), None);
        assert_eq!(clamp(i64::MIN, i64::MAX, 3, false), Some((1, 3)));
    }

    #[test]
    fn case_mapping() {
        assert_eq!(map_case("héllo", CaseMode::Up), "HÉLLO");
        assert_eq!(map_case("ÉCOLE", CaseMode::Down), "école");
        assert_eq!(map_case("éa", CaseMode::Capitalize), "Éa");
        assert_eq!(map_case("Éa", CaseMode::Swap), "éA");
        assert_eq!(map_case("straße", CaseMode::Up), "STRASSE");
        assert_eq!(map_case("hello World", CaseMode::Up), "HELLO WORLD");
        assert_eq!(map_case("Hello World", CaseMode::Down), "hello world");
        assert_eq!(map_case("hELLO", CaseMode::Capitalize), "Hello");
        assert_eq!(map_case("", CaseMode::Capitalize), "");
        assert_eq!(map_case("Hello, World!", CaseMode::Swap), "hELLO, wORLD!");
        assert_eq!(map_case("ǆ 1", CaseMode::Swap), "Ǆ 1");
        assert_eq!(CaseMode::from_code(3), Some(CaseMode::Swap));
        assert_eq!(CaseMode::from_code(4), None);
    }
}
