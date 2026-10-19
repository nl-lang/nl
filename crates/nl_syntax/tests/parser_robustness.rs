//! The parser on hostile input: every prefix of some conformance programs,
//! the programs with single tokens deleted or duplicated, deep nesting, long
//! chains and token soup. Each parse must terminate without panicking, and
//! the tree's text must equal the input.
//!
//! The deep-input tests run each parse on a thread with exactly 2 MiB of
//! stack (the default of `std::thread::spawn`), which is what the limits are
//! chosen for, so that a stack overflow fails them as it would fail a
//! user's thread, whatever `RUST_MIN_STACK` says.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::path::Path;

use nl_syntax::{Parse, lex, parse};

/// Parses `src` and checks the guarantees of `Parse`.
fn check(src: &str) -> Parse {
    let p = parse(src);
    let root = p.syntax();
    let text = root.to_string();
    assert!(text == src, "tree text differs from the source for {:?}", short(src));
    let mut last = 0u32;
    for d in p.diagnostics() {
        let start = u32::from(d.range.start());
        assert!(start >= last, "diagnostics out of order for {:?}", short(src));
        last = start;
        assert!(src.get(std::ops::Range::<usize>::from(d.range)).is_some(), "diagnostic off a char boundary");
        assert!(!d.message.is_empty());
        // The fuel backstop must never fire: it means a loop made no progress.
        assert!(!d.message.starts_with("internal parser error"), "parser got stuck on {:?}", short(src));
    }
    p
}

fn short(src: &str) -> String {
    src.chars().take(80).collect()
}

fn program(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/run").join(name);
    std::fs::read_to_string(path).unwrap()
}

const PROGRAMS: &[&str] = &[
    "call_block.nl",
    "ctrl_case_patterns.nl",
    "pattern_when.nl",
    "lex_newlines.nl",
    "iface_basic.nl",
    "lua_paths.nl",
    "op_index_call_len.nl",
    "worker_spawn_call.nl",
    "lex_string_interp.nl",
];

#[test]
fn every_prefix() {
    for name in PROGRAMS {
        let src = program(name);
        for i in 0..=src.len() {
            if let Some(prefix) = src.get(..i) {
                check(prefix);
            }
        }
    }
}

#[test]
fn every_single_token_deleted_or_duplicated() {
    for name in PROGRAMS {
        let src = program(name);
        let lexed = lex(&src);
        for t in &lexed.tokens {
            if t.kind.is_trivia() || t.range.is_empty() {
                continue;
            }
            let r = std::ops::Range::<usize>::from(t.range);
            let deleted = format!("{}{}", &src[..r.start], &src[r.end..]);
            check(&deleted);
            let duplicated = format!("{}{}", &src[..r.end], &src[r.start..]);
            check(&duplicated);
        }
    }
}

fn assert_errors(src: &str) {
    let p = on_2mib(src, check);
    assert!(p.has_errors(), "expected a diagnostic for {:?}", short(src));
}

/// Runs `f(src)` on a thread with a 2 MiB stack.
fn on_2mib<R: Send + 'static>(src: &str, f: fn(&str) -> R) -> R {
    let src = src.to_string();
    std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(move || f(&src))
        .unwrap()
        .join()
        .unwrap_or_else(|_| panic!("the parser panicked"))
}

/// The diagnostics of `src`, parsed on a 2 MiB thread.
fn errors_2mib(src: &str) -> Vec<String> {
    on_2mib(src, |s| check(s).diagnostics().iter().map(|d| d.message.clone()).collect())
}

/// Shapes the review found to overflow the stack (B1 to B3), and inputs on
/// which lookahead used to spend the fuel of a stuck parser (S6).
#[test]
fn review_blocker_shapes() {
    // B1: a long run of `async`.
    let errs = errors_2mib(&format!("x = {}y", "async ".repeat(5000)));
    assert_eq!(errs.len(), 1, "{errs:?}");
    // B2: member chains after `spawn` and `|>`.
    let errs = errors_2mib(&format!("spawn a{}()", ".b".repeat(20000)));
    assert!(!errs.is_empty() && errs.len() <= 2, "{errs:?}");
    assert!(errs.iter().any(|e| e.contains("too long")), "{errs:?}");
    assert!(errs.iter().all(|e| e.len() < 400), "messages quote at most a short piece of source");
    let errs = errors_2mib(&format!("x |> a{}", ".b".repeat(20000)));
    assert!(!errs.is_empty() && errs.len() <= 2, "{errs:?}");
    // B3: junk and prefix operators.
    let errs = errors_2mib(&format!("x = {}y", "- * ".repeat(1000)));
    assert_eq!(errs.len(), 1, "{errs:?}");
    let errs = errors_2mib(&format!("x = {}y", "- * - async ".repeat(1000)));
    assert!(!errs.is_empty() && errs.len() <= 2, "{errs:?}");
    // S6: long lookahead is not a stuck parser.
    let src = format!("x = cond{}\nwhen x\n  y\nend", ";".repeat(100_000));
    let errs = errors_2mib(&src);
    assert!(errs.iter().all(|e| !e.contains("internal")), "{errs:?}");
    let src = format!("case x\nwhen A{} {{a}}\n  y\nend", ".A".repeat(50_000));
    let errs = errors_2mib(&src);
    assert!(errs.iter().all(|e| !e.contains("internal")), "{errs:?}");
    let src = format!("local a{} = 1", ", a".repeat(33_000));
    let errs = errors_2mib(&src);
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(errs[0].contains("let"), "{errs:?}");
}

/// Inputs that used to take quadratic time (S4, S5). Linear, they take a
/// fraction of a second; quadratic, minutes.
#[test]
fn long_lines() {
    let src = "\"".repeat(300_000);
    let p = check(&src);
    let index = nl_syntax::LineIndex::new(&src);
    for d in p.diagnostics() {
        let _ = index.line_col(&src, d.range.start());
    }
    let src = ")".repeat(40_000);
    let p = check(&src);
    assert_eq!(p.diagnostics().len(), 40_000);
    let _ = nl_syntax::dump_diagnostics(&src, p.diagnostics());
}

/// T4: a limit costs the statement it is reached in, not the rest of the
/// file, and gives one error.
#[test]
fn limits_cost_one_statement() {
    let tail = "\nlet after = 1\nfn g\n  z\nend\n";
    for deep in [
        format!("x = {}1{}", "(".repeat(300), ")".repeat(300)),
        format!("{}{}", "if a\n".repeat(300), "end\n".repeat(300)),
        format!("{}{}", "f(fn()\n".repeat(200), "end)\n".repeat(200)),
        format!("x = a{}", " + a".repeat(3000)),
        format!("x = a{}", ".b".repeat(3000)),
        format!("x = {}1{}", "{a: ".repeat(300), "}".repeat(300)),
        format!("let {}b{} = 1", "[".repeat(300), "]".repeat(300)),
        // Lua path chains count like any other chain.
        format!("x = :a{}", ".b".repeat(3000)),
        format!("x = :f{}", "(1)".repeat(3000)),
        format!("x = :t{}", ":m(1)[2]".repeat(3000)),
        format!("x = {}1{}", ":f(".repeat(300), ")".repeat(300)),
    ] {
        let src = format!("{deep}{tail}");
        let p = on_2mib(&src, check);
        let errs: Vec<_> = p.diagnostics().iter().map(|d| d.message.clone()).collect();
        assert_eq!(errs.len(), 1, "{}: {errs:?}", short(&src));
        let tree = nl_syntax::dump_tree(&p.syntax(), nl_syntax::DumpOptions { skip_trivia: true });
        assert!(tree.contains("LET_DECL") && tree.contains("FN_DECL"), "the statements after it parse");
    }
}

/// T4: 128 levels of any construct parse without a diagnostic; 64 levels of
/// `f(fn() … end)` (two levels each) too.
#[test]
fn nesting_up_to_the_limit_parses() {
    let n = 128;
    for src in [
        format!("x = {}1{}", "(".repeat(n), ")".repeat(n)),
        format!("x = {}1{}", "[".repeat(n), "]".repeat(n)),
        format!("x = {}1{}", "{a: ".repeat(n), "}".repeat(n)),
        format!("x = {}1{}", "f(".repeat(n), ")".repeat(n)),
        format!("x = {}1{}", "a[".repeat(n), "]".repeat(n)),
        format!("x = {}1{}", "fn() ".repeat(n), " end".repeat(n)),
        format!("{}x\n{}", "f(fn()\n".repeat(n / 2), "end)\n".repeat(n / 2)),
        format!("{}x\n{}", "if x\n".repeat(n), "end\n".repeat(n)),
        format!("{}x\n{}", "case x\nwhen 1\n".repeat(n), "end\n".repeat(n)),
        format!("{}x\n{}", "try\n".repeat(n), "catch\nend\n".repeat(n)),
        format!("{}x\n{}", "f do\n".repeat(n), "end\n".repeat(n)),
        format!("{}x\n{}", "while x\n".repeat(n), "end\n".repeat(n)),
        format!("x = {}y{}", "\"#{".repeat(n), "}\"".repeat(n)),
        format!("let x: {}T{}", "[".repeat(n), "]".repeat(n)),
        format!("let x: {}T{}", "Box<".repeat(n), ">".repeat(n)),
        format!("let {}b{} = 1", "{a: ".repeat(n), "}".repeat(n)),
    ] {
        let errs = errors_2mib(&src);
        assert!(errs.is_empty(), "{}: {errs:?}", short(&src));
    }
    // One level more is reported.
    let errs = errors_2mib(&format!("x = {}1{}", "(".repeat(n + 1), ")".repeat(n + 1)));
    assert_eq!(errs.len(), 1, "{errs:?}");
    assert!(errs[0].contains("more than 128 levels"));
}

#[test]
fn deep_parentheses() {
    assert_errors(&"(".repeat(10_000));
    let nested = format!("x = {}1{}", "(".repeat(10_000), ")".repeat(10_000));
    assert_errors(&nested);
    // Moderate nesting is fine.
    let ok = format!("x = {}1{}", "(".repeat(40), ")".repeat(40));
    assert!(!check(&ok).has_errors());
}

#[test]
fn deep_brackets_of_every_kind() {
    for open in ["[", "{", "f(", "{a: ", "x[", "-", "!", "not ", "~", "fn() ", "-(", "@a ** ", "a <> "] {
        assert_errors(&format!("x = {}", open.repeat(10_000)));
    }
    assert_errors("let x: ");
    assert_errors(&format!("let x: {}", "[".repeat(10_000)));
    assert_errors(&format!("let x: {}", "~".repeat(10_000)));
    assert_errors(&format!("let x: Box{}", "<Box".repeat(10_000)));
    assert_errors(&format!("let {} = 1", "{a: ".repeat(10_000)));
    assert_errors(&format!("let {} = 1", "[".repeat(10_000)));
    // Statements parsed for recovery inside bodies and operands nest too.
    assert_errors(&"struct A\n".repeat(10_000));
    assert_errors(&"enum A\n".repeat(10_000));
    assert_errors(&"interface A for B\n".repeat(10_000));
    assert_errors(&"pub ".repeat(10_000));
    assert_errors(&"x = while a\n".repeat(10_000));
    assert_errors(&format!("x = 2{}", " ** -2".repeat(10_000)));
    assert_errors(&"x = - -".repeat(10_000));
}

#[test]
fn deep_blocks() {
    let mut src = String::new();
    for _ in 0..5_000 {
        src.push_str("if x\n");
    }
    for _ in 0..5_000 {
        src.push_str("end\n");
    }
    assert_errors(&src);
    let fns = "fn() ".repeat(5_000) + &" end".repeat(5_000);
    assert_errors(&format!("x = {fns}"));
    let dos = "f do\n".repeat(5_000) + &"end\n".repeat(5_000);
    assert_errors(&dos);
    // Realistic nesting is fine.
    let mut ok = String::new();
    for _ in 0..30 {
        ok.push_str("while x\n");
    }
    for _ in 0..30 {
        ok.push_str("end\n");
    }
    assert!(!check(&ok).has_errors());
}

#[test]
fn deep_interpolations() {
    let src = format!("x = {}y{}", "\"#{".repeat(5_000), "}\"".repeat(5_000));
    assert_errors(&src);
    let src = format!("x = {}y", "\"\"\"\n#{".repeat(2_000));
    check(&src);
}

#[test]
fn long_chains() {
    for op in [" + ", " or ", " <> ", " ** ", " |> f |> ", ".", " .. ", " < "] {
        let src = format!("x = a{}", format!("{op}a").repeat(20_000));
        check(&src);
    }
    let calls = format!("x = f{}", "(1)".repeat(20_000));
    check(&calls);
    let members = format!("x = a{}", ".b".repeat(20_000));
    check(&members);
    let types = format!("let x: A{}", " | A".repeat(20_000));
    check(&types);
    // A chain of moderate length is fine.
    let ok = format!("x = a{}", " + a".repeat(500));
    assert!(!check(&ok).has_errors());
    let ok = format!("x = a{}", " <> a".repeat(500));
    assert!(!check(&ok).has_errors());
}

#[test]
fn many_statements() {
    let src = "print x\n".repeat(50_000);
    assert!(!check(&src).has_errors());
    let src = "end\n".repeat(20_000);
    assert_errors(&src);
    let src = ")".repeat(20_000);
    assert_errors(&src);
}

/// xorshift64*: small, deterministic.
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
fn token_soup() {
    soup(6000, 60, 0x9E37_79B9_7F4A_7C15);
}

/// A long run of the same: `cargo test --release --test parser_robustness --
/// --ignored`.
#[test]
#[ignore]
fn token_soup_long() {
    for seed in 1..=20 {
        soup(20_000, 200, seed);
    }
}

fn soup(rounds: usize, max_len: usize, seed: u64) {
    const PIECES: &[&str] = &[
        "if ", "unless ", "case ", "cond", "when ", "else", "elseif ", "end", "fn ", "do", "|", "||",
        "while ", "for ", " in ", "step ", "let ", "const ", "struct ", "interface ", "enum ", "type ",
        "module ", "import ", "from ", "pub ", "on ", "spawn ", "try", "catch ", "finally", "return ",
        "async ", "as ", "is ", "not ", "and ", "or ", "then ", "(", ")", "[", "]", "{", "}", ",", ";",
        "\n", " ", ":", ".", "..", "...", "=", "+=", "<", ">", ">>", "<>", "|>", "!", "-", "*", "**",
        "?", "~", "x", "Foo", "getX", "@a", "1", "'s'", "\"a#{", "}\"", "\"\"\"\n", "\n\"\"\"", "#c\n",
        "## doc\n", "$", "self", "Self", "nil", "true", "->", "x ->", "|a| ->",
    ];
    let mut rng = Rng(seed);
    for _ in 0..rounds {
        let count = rng.below(max_len);
        let text: String = (0..count).map(|_| PIECES[rng.below(PIECES.len())]).collect();
        check(&text);
    }
}

#[test]
fn random_bytes() {
    let mut rng = Rng(7);
    for _ in 0..3000 {
        let len = rng.below(80);
        let bytes: Vec<u8> = (0..len).map(|_| (rng.below(0x80)) as u8).collect();
        check(&String::from_utf8_lossy(&bytes));
    }
}

/// Corpus programs with random token-level edits (deletions, duplications,
/// swaps, insertions of tokens from elsewhere): `cargo test --release --test
/// parser_robustness -- --ignored`.
#[test]
#[ignore]
fn mutated_corpus_long() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/run");
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "nl"))
        .collect();
    files.sort();
    let mut rng = Rng(99);
    for path in files {
        let src = std::fs::read_to_string(&path).unwrap();
        let lexed = lex(&src);
        let pieces: Vec<&str> = lexed.tokens.iter().map(|t| t.text(&src)).collect();
        for _ in 0..300 {
            let mut out: Vec<&str> = pieces.clone();
            for _ in 0..1 + rng.below(4) {
                if out.is_empty() {
                    break;
                }
                let i = rng.below(out.len());
                match rng.below(4) {
                    0 => {
                        out.remove(i);
                    }
                    1 => out.insert(i, out[i]),
                    2 => {
                        let j = rng.below(out.len());
                        out.swap(i, j);
                    }
                    _ => out.insert(i, pieces[rng.below(pieces.len())]),
                }
            }
            check(&out.concat());
        }
    }
}
