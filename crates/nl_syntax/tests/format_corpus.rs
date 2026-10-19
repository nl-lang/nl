//! The invariants of the formatter (`nl_syntax::format`) on every NL file of the repository: the conformance programs and
//! their helpers under `tests/run/`, the parser snapshots under
//! `tests/syntax/`, the regression programs under `crates/nlc/tests/cases/`,
//! `example.nl` and the declaration files under `decl/`.
//!
//! For a file that parses without errors, with and without `--fix`:
//!
//! - **parse equivalence**: the formatted text parses to the same tree,
//!   trivia aside: the same kinds and the same token texts (with `--fix`,
//!   the same decoded value for each string literal, since the continued
//!   strings are rewritten);
//! - **idempotence**: formatting the formatted text changes nothing;
//! - **no diagnostics change**: the same diagnostic messages.
//!
//! A file with syntax errors (`tests/syntax/err/`) must be refused.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::path::{Path, PathBuf};

use nl_syntax::{DumpOptions, FormatOptions, Formatted, Parse, SyntaxKind, decode_string, dump_tree, format, format_decl, parse, parse_decl_file};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn files_with_ext(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<PathBuf> = read.map(|e| e.unwrap().path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            files_with_ext(&path, ext, out);
        } else if path.extension().is_some_and(|e| e == ext) {
            out.push(path);
        }
    }
}

fn corpus() -> Vec<PathBuf> {
    let root = repo_root();
    let mut files = Vec::new();
    for dir in ["tests/run", "tests/syntax", "crates/nlc/tests/cases", "decl"] {
        files_with_ext(&root.join(dir), "nl", &mut files);
    }
    files.push(root.join("example.nl"));
    files
}

fn is_decl(path: &Path) -> bool {
    path.to_string_lossy().ends_with(".d.nl")
}

fn parse_any(decl: bool, src: &str) -> Parse {
    if decl { parse_decl_file(src) } else { parse(src) }
}

fn format_any(decl: bool, src: &str, fix: bool) -> Result<Formatted, String> {
    let options = FormatOptions { fix };
    let r = if decl { format_decl(src, options) } else { format(src, options) };
    r.map_err(|d| d.iter().map(|d| d.message.clone()).collect::<Vec<_>>().join("; "))
}

/// A tree dump without its byte offsets.
fn strip_offsets(dump: &str) -> String {
    let mut out = String::new();
    for line in dump.lines() {
        match line.find('@') {
            Some(at) => {
                let end = line[at..].find(' ').map_or(line.len(), |i| at + i);
                out.push_str(&line[..at]);
                out.push_str(&line[end..]);
            }
            None => out.push_str(line),
        }
        out.push('\n');
    }
    out
}

fn shape(p: &Parse) -> String {
    strip_offsets(&dump_tree(&p.syntax(), DumpOptions { skip_trivia: true }))
}

/// The shape with the text of every string part left out, and the decoded
/// value of every string literal: what `--fix` must keep.
fn shape_with_values(p: &Parse) -> (String, Vec<Vec<Vec<u8>>>) {
    let root = p.syntax();
    let mut kinds = String::new();
    for element in root.descendants_with_tokens() {
        match element {
            rowan::NodeOrToken::Node(n) => kinds.push_str(&format!("{:?}\n", n.kind())),
            rowan::NodeOrToken::Token(t) => {
                if t.kind().is_trivia() && t.kind() != SyntaxKind::DOC_COMMENT {
                    continue;
                }
                if t.kind().is_string_part() {
                    kinds.push_str(&format!("{:?}\n", t.kind()));
                } else {
                    kinds.push_str(&format!("{:?} {:?}\n", t.kind(), t.text()));
                }
            }
        }
    }
    let values = root
        .descendants()
        .filter(|n| matches!(n.kind(), SyntaxKind::LITERAL | SyntaxKind::INTERP_STRING))
        .filter_map(|n| {
            let parts: Vec<String> =
                n.children_with_tokens().filter_map(|e| e.into_token()).filter(|t| t.kind().is_string_part()).map(|t| t.text().to_string()).collect();
            if parts.is_empty() {
                return None;
            }
            let refs: Vec<&str> = parts.iter().map(String::as_str).collect();
            Some(decode_string(&refs))
        })
        .collect();
    (kinds, values)
}

fn messages(p: &Parse) -> Vec<String> {
    p.diagnostics().iter().map(|d| d.message.clone()).collect()
}

fn rel(path: &Path) -> String {
    let root = repo_root().canonicalize().unwrap();
    path.canonicalize().ok().and_then(|p| p.strip_prefix(&root).ok().map(|p| p.display().to_string())).unwrap_or_else(|| path.display().to_string())
}

#[test]
fn formatter_invariants_on_every_corpus_file() {
    let files = corpus();
    assert!(files.len() > 400, "expected the whole corpus, found {} files", files.len());
    let mut problems = Vec::new();
    let mut formatted = 0usize;
    let mut refused = 0usize;
    for path in &files {
        let name = rel(path);
        let bytes = std::fs::read(path).unwrap();
        let Ok(src) = nl_syntax::source_from_bytes(&bytes) else {
            // A file that is not UTF-8 is a lexer corpus case; the driver
            // refuses it before the formatter.
            continue;
        };
        let decl = is_decl(path);
        let first = parse_any(decl, src);
        if first.has_errors() {
            refused += 1;
            if format_any(decl, src, false).is_ok() {
                problems.push(format!("{name}: a file with syntax errors was formatted"));
            }
            continue;
        }
        formatted += 1;
        for fix in [false, true] {
            let mode = if fix { " (--fix)" } else { "" };
            let out = match format_any(decl, src, fix) {
                Ok(out) => out,
                Err(e) => {
                    problems.push(format!("{name}{mode}: refused: {e}"));
                    continue;
                }
            };
            let second = parse_any(decl, &out.text);
            if fix {
                if shape_with_values(&first) != shape_with_values(&second) {
                    problems.push(format!("{name}{mode}: the tree or a string value changed"));
                }
            } else if shape(&first) != shape(&second) {
                problems.push(format!("{name}{mode}: the tree changed:\n{}", first_difference(&shape(&first), &shape(&second))));
            }
            if messages(&first) != messages(&second) {
                problems.push(format!("{name}{mode}: the diagnostics changed: {:?} -> {:?}", messages(&first), messages(&second)));
            }
            match format_any(decl, &out.text, fix) {
                Ok(again) if again.text == out.text => {}
                Ok(again) => problems.push(format!("{name}{mode}: not idempotent:\n{}", first_difference(&out.text, &again.text))),
                Err(e) => problems.push(format!("{name}{mode}: the formatted text is refused: {e}")),
            }
            if fix && out.continued.iter().any(|c| !c.fixed && !c.in_interpolation) {
                problems.push(format!("{name}{mode}: a continued string outside an interpolation was not fixed"));
            }
        }
    }
    assert!(formatted > 350, "only {formatted} files formatted");
    assert!(refused > 100, "only {refused} files refused (tests/syntax/err)");
    assert!(problems.is_empty(), "{} problem(s):\n{}", problems.len(), problems.join("\n"));
}

/// The first differing line of two texts, for a message.
fn first_difference(a: &str, b: &str) -> String {
    let al: Vec<&str> = a.lines().collect();
    let bl: Vec<&str> = b.lines().collect();
    for i in 0..al.len().max(bl.len()) {
        let (x, y) = (al.get(i).copied(), bl.get(i).copied());
        if x != y {
            return format!("line {}: {:?} vs {:?}", i + 1, x.unwrap_or("<end>"), y.unwrap_or("<end>"));
        }
    }
    "no line differs".to_string()
}
