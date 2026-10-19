//! NL's regex engine.
//!
//! A [`Regex`] is a compiled pattern of the Rust regex crate's syntax
//! (Perl-like, Unicode classes, inline flags, no backreferences and no
//! lookaround), matched in time linear in the input. It is the engine under
//! `regex::bytes::Regex`, `regex_automata::meta::Regex`, used directly: it
//! reports the heap the compiled pattern takes ([`Regex::memory_usage`]),
//! which the VM charges to its collector, and it searches from an offset
//! without copying the subject.
//!
//! The safe API here works in bytes: positions are byte offsets, and the
//! subject is any byte string. Character positions are the
//! caller's business, converted at the API boundary in the runtime; the VM
//! only hands over subjects it has checked to be valid UTF-8, and the
//! pattern itself is compiled in UTF-8 mode, as `regex::Regex` is, so a
//! match never starts or ends inside a character.
//!
//! The C ABI over it is in [`abi`] (`nlregex.h` in the VM). Nothing in this
//! crate calls into Lua or unwinds across the boundary.

use regex_automata::meta;
use regex_automata::util::captures::Captures;
use regex_automata::{Anchored, Input, PatternID};

/// The C ABI (`nlregex.h`). The only module allowed unsafe code.
#[allow(unsafe_code)]
pub mod abi;

/// The compiled size cap when the caller gives none: the heap the NFA of a
/// pattern may take. Unicode classes such as `\w` compile to a few hundred
/// kilobytes at most.
pub const DEFAULT_SIZE_LIMIT: usize = 2 << 20;

/// Why a pattern did not compile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The pattern is not well formed; the message is the engine's.
    Syntax(String),
    /// The pattern is well formed but compiles to more than the size limit.
    TooBig(String),
}

impl Error {
    /// The message, as the `RegexError` carries it.
    pub fn message(&self) -> &str {
        match self {
            Error::Syntax(m) | Error::TooBig(m) => m,
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for Error {}

/// A compiled pattern.
#[derive(Debug)]
pub struct Regex {
    inner: meta::Regex,
    pattern: String,
    /// The name of each capture group, group 0 included (unnamed).
    names: Vec<Option<String>>,
}

/// A group that did not participate in a match, in the spans that
/// [`Regex::captures_at`] fills.
pub const NO_SPAN: usize = usize::MAX;

impl Regex {
    /// Compiles `pattern`. `size_limit` caps the heap of the compiled
    /// pattern in bytes (0 means [`DEFAULT_SIZE_LIMIT`]).
    pub fn new(pattern: &str, size_limit: usize) -> Result<Regex, Error> {
        let limit = if size_limit == 0 { DEFAULT_SIZE_LIMIT } else { size_limit };
        let config = meta::Config::new().nfa_size_limit(Some(limit)).utf8_empty(true);
        let inner = meta::Regex::builder().configure(config).build(pattern).map_err(|e| {
            if e.size_limit().is_some() {
                Error::TooBig(format!("the pattern compiles to more than {limit} bytes"))
            } else if let Some(syntax) = e.syntax_error() {
                // The parser's message spans several lines (the pattern,
                // carets and the reason), as the regex crate shows it.
                Error::Syntax(syntax.to_string())
            } else {
                Error::Syntax(e.to_string())
            }
        })?;
        let info = inner.group_info();
        let names = (0..info.group_len(PatternID::ZERO)).map(|i| info.to_name(PatternID::ZERO, i).map(str::to_owned)).collect();
        Ok(Regex { inner, pattern: pattern.to_owned(), names })
    }

    /// The pattern as given.
    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    /// The heap the compiled pattern takes, in bytes, approximately. The
    /// lazy DFA grows its cache while matching, up to its own cap, which is
    /// not included.
    pub fn memory_usage(&self) -> usize {
        self.inner.memory_usage() + self.pattern.len() + self.names.iter().map(|n| n.as_ref().map_or(0, String::len) + 8).sum::<usize>()
    }

    /// The number of capture groups, group 0 (the whole match) included.
    pub fn group_count(&self) -> usize {
        self.names.len()
    }

    /// The name of group `i`, if it has one and exists.
    pub fn group_name(&self, i: usize) -> Option<&str> {
        self.names.get(i).and_then(|n| n.as_deref())
    }

    /// Whether `subject` contains a match.
    pub fn is_match(&self, subject: &[u8]) -> bool {
        self.inner.is_match(subject)
    }

    /// The byte span `[start, end)` of the first match at or after byte
    /// offset `start`; `None` if there is none or `start` is past the end.
    pub fn find_at(&self, subject: &[u8], start: usize) -> Option<(usize, usize)> {
        if start > subject.len() {
            return None;
        }
        let input = Input::new(subject).range(start..).anchored(Anchored::No);
        self.inner.find(input).map(|m| (m.start(), m.end()))
    }

    /// The spans of every group of the first match at or after `start`:
    /// `spans[2 * i]` and `spans[2 * i + 1]` are the byte start and end of
    /// group `i`, or [`NO_SPAN`] when it did not participate. Groups past
    /// the end of `spans` are left out. Returns `false`, filling nothing,
    /// when there is no match.
    pub fn captures_at(&self, subject: &[u8], start: usize, spans: &mut [usize]) -> bool {
        if start > subject.len() {
            return false;
        }
        let input = Input::new(subject).range(start..).anchored(Anchored::No);
        let mut caps: Captures = self.inner.create_captures();
        self.inner.search_captures(&input, &mut caps);
        if !caps.is_match() {
            return false;
        }
        for (i, pair) in spans.chunks_mut(2).enumerate() {
            let (s, e) = caps.get_group(i).map_or((NO_SPAN, NO_SPAN), |sp| (sp.start, sp.end));
            if let Some(slot) = pair.first_mut() {
                *slot = s;
            }
            if let Some(slot) = pair.get_mut(1) {
                *slot = e;
            }
        }
        true
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn compiles_and_matches() {
        let re = Regex::new("^(.+)uwu$", 0).unwrap();
        assert!(re.is_match(b"hewwo uwu"));
        assert!(!re.is_match(b"hello"));
        assert_eq!(re.find_at(b"hewwo uwu", 0), Some((0, 9)));
        assert_eq!(re.pattern(), "^(.+)uwu$");
        assert_eq!(re.group_count(), 2);
        assert!(re.memory_usage() > 0);
    }

    #[test]
    fn find_at_offset_and_past_end() {
        let re = Regex::new("o", 0).unwrap();
        assert_eq!(re.find_at(b"foo boo", 3), Some((5, 6)));
        assert_eq!(re.find_at(b"foo", 3), None);
        assert_eq!(re.find_at(b"foo", 4), None);
        // An empty match at the end is a match.
        let empty = Regex::new("$", 0).unwrap();
        assert_eq!(empty.find_at(b"ab", 2), Some((2, 2)));
    }

    #[test]
    fn captures_names_and_missing_groups() {
        let re = Regex::new(r"(?<year>\d{4})-(?<month>\d{2})", 0).unwrap();
        assert_eq!(re.group_count(), 3);
        assert_eq!(re.group_name(0), None);
        assert_eq!(re.group_name(1), Some("year"));
        assert_eq!(re.group_name(2), Some("month"));
        assert_eq!(re.group_name(3), None);
        let mut spans = [0usize; 6];
        assert!(re.captures_at(b"on 2026-10", 0, &mut spans));
        assert_eq!(spans, [3, 10, 3, 7, 8, 10]);
        let alt = Regex::new("(a)|(b)", 0).unwrap();
        let mut spans = [0usize; 6];
        assert!(alt.captures_at(b"b", 0, &mut spans));
        assert_eq!(spans, [0, 1, NO_SPAN, NO_SPAN, 0, 1]);
        assert!(!alt.captures_at(b"c", 0, &mut spans));
        // A short span buffer takes what fits.
        let mut two = [9usize; 2];
        assert!(alt.captures_at(b"a", 0, &mut two));
        assert_eq!(two, [0, 1]);
    }

    #[test]
    fn unicode_and_flags() {
        let re = Regex::new("(?i)hello", 0).unwrap();
        assert!(re.is_match(b"HeLLo"));
        let re = Regex::new("l+", 0).unwrap();
        assert_eq!(re.find_at("héllo".as_bytes(), 0), Some((3, 5)));
        let w = Regex::new(r"\w+", 0).unwrap();
        assert_eq!(w.find_at("héllo wörld".as_bytes(), 0), Some((0, 6)));
    }

    #[test]
    fn errors() {
        let e = Regex::new("(", 0).unwrap_err();
        assert!(matches!(e, Error::Syntax(_)));
        assert!(e.message().contains("unclosed group"), "{e}");
        let e = Regex::new(r"\w{100}", 2000).unwrap_err();
        assert!(matches!(e, Error::TooBig(_)), "{e}");
        assert!(Regex::new("(?<=a)b", 0).is_err(), "lookaround is not supported");
        assert!(Regex::new(r"(a)\1", 0).is_err(), "backreferences are not supported");
    }

    #[test]
    fn pathological_pattern_is_linear() {
        // Catastrophic for a backtracking engine; linear here.
        let re = Regex::new("(x+x+)+y", 0).unwrap();
        let subject = "x".repeat(200_000);
        let t = Instant::now();
        assert!(!re.is_match(subject.as_bytes()));
        let mut spans = [0usize; 4];
        assert!(!re.captures_at(subject.as_bytes(), 0, &mut spans));
        assert!(t.elapsed() < Duration::from_secs(5), "took {:?}", t.elapsed());
    }
}
