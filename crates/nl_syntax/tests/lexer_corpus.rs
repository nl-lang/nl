//! The lexer over real and hostile input: the conformance programs of
//! `tests/run/`, every prefix of some of them, and pseudo-random text.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::path::{Path, PathBuf};

use nl_syntax::{Lexed, SyntaxKind, decode_string, lex};

/// Asserts the guarantees of `Lexed` for `src` and returns the result.
fn check(src: &str) -> Lexed<'_> {
    let lexed = lex(src);
    let mut joined = String::with_capacity(src.len());
    let mut pos = 0u32;
    let mut open: Vec<Vec<&str>> = Vec::new();
    let n = lexed.tokens.len();
    for (i, t) in lexed.tokens.iter().enumerate() {
        assert_eq!(u32::from(t.range.start()), pos, "gap before token {i} in {src:?}");
        pos = t.range.end().into();
        let text = t.text(src);
        joined.push_str(text);
        assert_eq!(t.kind == SyntaxKind::EOF, i + 1 == n, "EOF placement in {src:?}");
        if t.range.is_empty() {
            assert!(matches!(t.kind, SyntaxKind::EOF | SyntaxKind::STR_END), "empty {:?} in {src:?}", t.kind);
        }
        match t.kind {
            SyntaxKind::STRING => assert_eq!(decode_string(&[text]).len(), 1),
            SyntaxKind::STR_BEGIN => open.push(vec![text]),
            SyntaxKind::STR_MID => open.last_mut().expect("STR_MID outside a string").push(text),
            SyntaxKind::STR_END => {
                let mut parts = open.pop().expect("unbalanced STR_END");
                parts.push(text);
                assert_eq!(decode_string(&parts).len(), parts.len());
            }
            _ => {}
        }
    }
    assert!(open.is_empty(), "unclosed STR_BEGIN in {src:?}");
    assert_eq!(joined, src);
    for d in &lexed.diagnostics {
        let r = std::ops::Range::<usize>::from(d.range);
        assert!(src.get(r).is_some(), "diagnostic range off a char boundary in {src:?}");
    }
    lexed
}

fn corpus_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/run")
}

fn nl_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            nl_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "nl") {
            out.push(path);
        }
    }
}

fn corpus() -> Vec<(PathBuf, String)> {
    let mut files = Vec::new();
    nl_files(&corpus_dir(), &mut files);
    files
        .into_iter()
        .map(|p| {
            let bytes = std::fs::read(&p).unwrap();
            let text = nl_syntax::source_from_bytes(&bytes).unwrap().to_string();
            (p, text)
        })
        .collect()
}

#[test]
fn conformance_programs_lex_cleanly() {
    let files = corpus();
    assert!(files.len() >= 158, "expected the conformance corpus, found {} files", files.len());
    let mut failures = Vec::new();
    for (path, src) in &files {
        let lexed = check(src);
        for d in &lexed.diagnostics {
            let r = std::ops::Range::<usize>::from(d.range);
            failures.push(format!("{}: {:?} {:?}: {}", path.display(), r.clone(), &src[r], d.message));
        }
    }
    assert!(failures.is_empty(), "lexical diagnostics in the corpus:\n{}", failures.join("\n"));
}

#[test]
fn every_prefix_of_some_programs() {
    let files = corpus();
    let wanted = ["lex_heredoc.nl", "lex_string_interp.nl", "lex_numbers.nl", "call_block.nl"];
    let mut done = 0;
    for (path, src) in &files {
        let name = path.file_name().unwrap().to_str().unwrap();
        if !wanted.contains(&name) {
            continue;
        }
        done += 1;
        for i in 0..=src.len() {
            if let Some(prefix) = src.get(..i) {
                check(prefix);
            }
            // Also each suffix, which starts in the middle of constructs.
            if let Some(suffix) = src.get(i..) {
                check(suffix);
            }
        }
    }
    assert_eq!(done, wanted.len());
}

/// xorshift64*: small, deterministic, good enough for test input.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

#[test]
fn random_bytes() {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    for _ in 0..3000 {
        let len = rng.below(64);
        let bytes: Vec<u8> = (0..len)
            .map(|_| match rng.below(4) {
                // Bias towards ASCII, which reaches more lexer states.
                0..=2 => (rng.below(0x80)) as u8,
                _ => rng.next() as u8,
            })
            .collect();
        let text = String::from_utf8_lossy(&bytes);
        check(&text);
        // And the strict entry point never panics either.
        let _ = nl_syntax::source_from_bytes(&bytes);
    }
}

#[test]
fn random_token_soup() {
    const PIECES: &[&str] = &[
        "\"", "'", "\"\"\"", "\"\"\"-", "\"\"\"=", "\"\"\"-=", "\"\"\"=-", "#{", "}", "{", "\\", "\\#", "\\u{",
        "\\x4", "\\256", "#", "##", "\n", "\r\n", "\r", " ", "\t", "a", "Foo", "getX", "end", "@", "@x",
        "!", "!=", "=", ":", ".", "..", "...", "..<", "1", "0x", "0b1", "1_", "1e", "2.5", "e5", "_",
        ">>>=", "<>", "|>", "||=", "(", ")", "[", "]", ",", ";", "é", "😀", "$", "\u{FEFF}", "“",
        "-", "+", "*", "**=", "?", "~", "->",
    ];
    let mut rng = Rng(42);
    for _ in 0..4000 {
        let count = rng.below(40);
        let text: String = (0..count).map(|_| PIECES[rng.below(PIECES.len())]).collect();
        check(&text);
    }
}

#[test]
fn deep_nesting() {
    // Deeply nested interpolations and braces: no recursion, no panic.
    let mut src = String::new();
    for _ in 0..2000 {
        src.push_str("\"#{");
    }
    src.push('x');
    check(&src);
    for _ in 0..2000 {
        src.push_str("}\"");
    }
    let lexed = check(&src);
    assert!(lexed.diagnostics.is_empty());
    let braces = "{".repeat(5000) + &"}".repeat(5000);
    check(&format!("\"#{{{braces}}}\""));
}
