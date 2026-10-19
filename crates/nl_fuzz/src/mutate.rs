//! Mutations of generated sources for robustness fuzzing.
//!
//! A mutant starts from a well-formed program, so it reaches deep into
//! the parser, the resolver, the analyzer and the backends before it goes
//! wrong, which random bytes never do. One to four edits are applied:
//! byte flips (which may leave invalid UTF-8), truncation, token-level
//! deletion, duplication, swaps and replacements, line deletion and
//! duplication, and stray operators, brackets and keywords. Tokens are
//! found by a crude scanner (words, numbers, quoted strings, single
//! punctuation characters); it only has to cut the text in plausible
//! places, not to agree with the real lexer.

use crate::rng::Rng;

/// Things to insert: operators, brackets, keywords and fragments that
/// open or close constructs.
const JUNK: &[&str] = &[
    "(", ")", "[", "]", "{", "}", ",", ".", ":", ";", "!", "@", "*", "**", "|>", "<>", "..", "...", "=", "==", "+=", "<<=", "#{", "\"", "'", "\"\"\"", "\\", "\n", " end", " do", " do |x|", " fn", " fn(", " if", " else", " elseif", " when", " case", " try", " catch", " finally", " return", " break", " continue", " let", " const", " struct", " enum", " interface", " for", " in", " step", " is", " as", " not", " and", " or", " Self", " self", " nil", " true", " ~", " -", " //", " %", " >>>", " :", " :x.y", " 0x", " 1e", " 1.", " _", " é", "\t", "#", "##", "?.", " ??", " ??=", "?",
    " ->", " x ->", " |a| ->", " || ->",
];

/// A mutant of `source`, determined by `seed`.
pub fn mutate(source: &str, seed: u64) -> Vec<u8> {
    let mut rng = Rng::new(seed);
    let mut bytes = source.as_bytes().to_vec();
    let n = 1 + rng.below(4);
    for _ in 0..n {
        bytes = mutate_once(&bytes, &mut rng);
    }
    bytes
}

fn mutate_once(src: &[u8], rng: &mut Rng) -> Vec<u8> {
    if src.is_empty() {
        return rng.pick(JUNK).as_bytes().to_vec();
    }
    let tokens = tokens(src);
    match rng.weighted(&[10, 4, 14, 10, 6, 6, 18, 8, 6, 6]) {
        0 => {
            // A byte replaced, or one bit flipped.
            let mut out = src.to_vec();
            let i = rng.below(out.len());
            if let Some(b) = out.get_mut(i) {
                *b = if rng.chance(50) { *b ^ (1 << rng.below(8)) } else { rng.below(256) as u8 };
            }
            out
        }
        1 => src.get(..rng.below(src.len())).unwrap_or(src).to_vec(),
        2 => {
            let (s, e) = rng.pick(&tokens);
            splice(src, s, e, b"")
        }
        3 => {
            let (s, e) = rng.pick(&tokens);
            let tok = src.get(s..e).unwrap_or(b"").to_vec();
            splice(src, e, e, &tok)
        }
        4 => {
            // Two tokens swapped.
            let (a, b) = (rng.pick(&tokens), rng.pick(&tokens));
            let ((s1, e1), (s2, e2)) = if a.0 <= b.0 { (a, b) } else { (b, a) };
            if e1 > s2 {
                return src.to_vec();
            }
            let mut out = Vec::with_capacity(src.len());
            out.extend_from_slice(src.get(..s1).unwrap_or(b""));
            out.extend_from_slice(src.get(s2..e2).unwrap_or(b""));
            out.extend_from_slice(src.get(e1..s2).unwrap_or(b""));
            out.extend_from_slice(src.get(s1..e1).unwrap_or(b""));
            out.extend_from_slice(src.get(e2..).unwrap_or(b""));
            out
        }
        5 => {
            // A token replaced by another token of the source.
            let (s, e) = rng.pick(&tokens);
            let (s2, e2) = rng.pick(&tokens);
            let tok = src.get(s2..e2).unwrap_or(b"").to_vec();
            splice(src, s, e, &tok)
        }
        6 => {
            let at = rng.pick(&tokens).0;
            splice(src, at, at, rng.pick(JUNK).as_bytes())
        }
        7 => {
            // A whole line deleted.
            let lines = line_spans(src);
            let (s, e) = rng.pick(&lines);
            splice(src, s, e, b"")
        }
        8 => {
            let lines = line_spans(src);
            let (s, e) = rng.pick(&lines);
            let line = src.get(s..e).unwrap_or(b"").to_vec();
            splice(src, e, e, &line)
        }
        _ => {
            // A run of the same bracket: deep nesting.
            let at = rng.pick(&tokens).0;
            let b = rng.pick(&["(", "[", "{", "-", "not ", "fn() "]);
            splice(src, at, at, b.repeat(1 + rng.below(60)).as_bytes())
        }
    }
}

fn splice(src: &[u8], start: usize, end: usize, with: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(src.len() + with.len());
    out.extend_from_slice(src.get(..start).unwrap_or(src));
    out.extend_from_slice(with);
    out.extend_from_slice(src.get(end..).unwrap_or(b""));
    out
}

/// Byte spans of the tokens: words and numbers, quoted strings, and each
/// other non-space byte on its own. Never empty for a non-empty input.
fn tokens(src: &[u8]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < src.len() {
        let c = src.get(i).copied().unwrap_or(b' ');
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        if c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80 {
            while src.get(i).is_some_and(|&b| b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80) {
                i += 1;
            }
        } else if c == b'"' || c == b'\'' {
            i += 1;
            while let Some(&b) = src.get(i) {
                i += 1;
                if b == b'\\' {
                    i += 1;
                } else if b == c || b == b'\n' {
                    break;
                }
            }
        } else {
            i += 1;
        }
        out.push((start, i.min(src.len())));
    }
    if out.is_empty() {
        out.push((0, src.len()));
    }
    out
}

/// Byte spans of the lines, each with its line break.
fn line_spans(src: &[u8]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, &b) in src.iter().enumerate() {
        if b == b'\n' {
            out.push((start, i + 1));
            start = i + 1;
        }
    }
    if start < src.len() || out.is_empty() {
        out.push((start, src.len()));
    }
    out
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_different() {
        let src = "let x = 1\nprint(\"a b\", x + 2)\n";
        assert_eq!(mutate(src, 3), mutate(src, 3));
        let distinct: std::collections::HashSet<Vec<u8>> = (0..50).map(|s| mutate(src, s)).collect();
        assert!(distinct.len() > 30);
    }

    #[test]
    fn token_spans() {
        let t = tokens(b"let x = \"a b\"(1)");
        let words: Vec<&[u8]> = t.iter().map(|&(s, e)| &b"let x = \"a b\"(1)"[s..e]).collect();
        assert_eq!(words, vec![&b"let"[..], b"x", b"=", b"\"a b\"", b"(", b"1", b")"]);
    }

    #[test]
    fn empty_input() {
        // Nothing panics on an empty source, whatever the edits.
        for seed in 0..100 {
            let _ = mutate("", seed);
        }
        assert_eq!(line_spans(b""), vec![(0, 0)]);
        assert_eq!(tokens(b""), vec![(0, 0)]);
    }
}
