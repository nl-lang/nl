//! Snapshot tests of the parser: `tests/syntax/` at the repository root
//! (see its README.md).
//!
//! Every `<name>.nl` under `tests/syntax/ok/` and `tests/syntax/err/` has a
//! `<name>.tree` beside it holding the expected `dump_parse` output: the tree
//! with trivia, then `---` and the diagnostics when there are any. The test
//! compares all of them and reports every mismatch with a line diff.
//!
//! Set `UPDATE_SNAPSHOTS=1` to write the `.tree` files from the current
//! parser instead (creating missing ones and removing `.tree` files whose
//! `.nl` is gone). Read the diff of the result before committing it: a
//! snapshot locks in whatever the parser does.
//!
//! Besides the comparison, the suite checks that:
//!
//! - every `.nl` file starts with a `#` comment saying what it checks (after
//!   a byte-order mark, for the one case that tests it);
//! - a file under `ok/` has no diagnostic and no `ERROR_NODE`;
//! - a file under `err/` has at least one diagnostic;
//! - for every file under `tests/syntax/` and `tests/run/`, the tree's text
//!   is the source, every node's text is the source at its range, and
//!   parsing the tree's text again gives the same dump (parse, print, parse).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use nl_syntax::{DumpOptions, Parse, SyntaxKind, dump_diagnostics, dump_parse, dump_parse_decl, dump_tree, parse, parse_decl_file};

/// A `.d.nl` file is a declaration file and has a parser of
/// its own.
fn is_decl(path: &Path) -> bool {
    path.to_string_lossy().ends_with(".d.nl")
}

fn parse_any(path: &Path, src: &str) -> Parse {
    if is_decl(path) { parse_decl_file(src) } else { parse(src) }
}

fn dump_any(path: &Path, src: &str) -> String {
    if is_decl(path) { dump_parse_decl(src) } else { dump_parse(src) }
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn syntax_dir() -> PathBuf {
    repo_root().join("tests/syntax")
}

/// Every file under `dir` (recursively) with extension `ext`, sorted.
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

fn read_source(path: &Path) -> String {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    match nl_syntax::source_from_bytes(&bytes) {
        Ok(s) => s.to_string(),
        Err(d) => panic!("{}: not UTF-8: {}", path.display(), d.message),
    }
}

fn rel(path: &Path) -> String {
    let root = repo_root().canonicalize().unwrap();
    let full = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    full.strip_prefix(&root).map_or_else(|_| path.display().to_string(), |p| p.display().to_string())
}

fn updating() -> bool {
    std::env::var_os("UPDATE_SNAPSHOTS").is_some_and(|v| v != "0" && !v.is_empty())
}

#[test]
fn syntax_snapshots() {
    let update = updating();
    let mut problems: Vec<String> = Vec::new();
    let mut updated: Vec<String> = Vec::new();
    let mut count = 0usize;

    for (sub, expect_diagnostics) in [("ok", false), ("err", true)] {
        let dir = syntax_dir().join(sub);
        assert!(dir.is_dir(), "{} is missing", dir.display());
        let mut sources = Vec::new();
        files_with_ext(&dir, "nl", &mut sources);
        let mut trees = Vec::new();
        files_with_ext(&dir, "tree", &mut trees);

        // A `.tree` without its `.nl`.
        for tree in &trees {
            if !tree.with_extension("nl").exists() {
                if update {
                    std::fs::remove_file(tree).unwrap();
                    updated.push(format!("removed {}", rel(tree)));
                } else {
                    problems.push(format!("{}: no matching .nl file", rel(tree)));
                }
            }
        }

        for source_path in &sources {
            count += 1;
            let name = rel(source_path);
            let src = read_source(source_path);
            if !src.trim_start_matches('\u{feff}').starts_with('#') {
                problems.push(format!("{name}: must start with a `#` comment saying what it checks"));
            }

            let p = parse_any(source_path, &src);
            if expect_diagnostics && p.diagnostics().is_empty() {
                problems.push(format!("{name}: a case under err/ must produce a diagnostic, and has none"));
            }
            if !expect_diagnostics {
                if !p.diagnostics().is_empty() {
                    problems.push(format!(
                        "{name}: a case under ok/ must parse without diagnostics, and has:\n{}",
                        indent(&dump_diagnostics(&src, p.diagnostics()))
                    ));
                }
                if p.syntax().descendants().any(|n| n.kind() == SyntaxKind::ERROR_NODE) {
                    problems.push(format!("{name}: a case under ok/ has an ERROR_NODE"));
                }
            }

            let actual = dump_any(source_path, &src);
            let tree_path = source_path.with_extension("tree");
            let expected = std::fs::read_to_string(&tree_path).ok();
            match expected {
                Some(expected) if expected == actual => {}
                _ if update => {
                    std::fs::write(&tree_path, &actual).unwrap();
                    updated.push(format!("wrote {}", rel(&tree_path)));
                }
                None => problems.push(format!(
                    "{name}: no {} (run with UPDATE_SNAPSHOTS=1 to create it, then read it)",
                    tree_path.file_name().unwrap().to_string_lossy()
                )),
                Some(expected) => {
                    problems.push(format!("{name}: snapshot differs (- expected, + actual)\n{}", diff(&expected, &actual)));
                }
            }
        }
    }

    for line in &updated {
        eprintln!("{line}");
    }
    assert!(count > 0, "no cases found under {}", syntax_dir().display());
    assert!(
        problems.is_empty(),
        "{} problem(s) in {count} syntax snapshot cases:\n\n{}",
        problems.len(),
        problems.join("\n\n")
    );
}

/// Parse, print, parse: the tree's text is the source, every node covers the
/// source text at its range, and the printed text parses to the same tree
/// and diagnostics.
#[test]
fn round_trip() {
    let mut files = Vec::new();
    files_with_ext(&syntax_dir(), "nl", &mut files);
    let run_start = files.len();
    files_with_ext(&repo_root().join("tests/run"), "nl", &mut files);
    assert!(run_start > 0, "no files under tests/syntax");
    assert!(files.len() - run_start >= 158, "expected the conformance corpus under tests/run");

    let mut problems = Vec::new();
    for path in &files {
        let src = read_source(path);
        let first = parse(&src);
        let root = first.syntax();
        let printed = root.to_string();
        if printed != src {
            problems.push(format!("{}: the tree's text differs from the source", rel(path)));
            continue;
        }
        for node in root.descendants() {
            let range = std::ops::Range::<usize>::from(node.text_range());
            if src.get(range).is_none_or(|s| s != node.to_string()) {
                problems.push(format!("{}: {:?}@{:?} does not cover its source text", rel(path), node.kind(), node.text_range()));
                break;
            }
        }
        let second = parse(&printed);
        let options = DumpOptions::default();
        if dump_tree(&root, options) != dump_tree(&second.syntax(), options)
            || first.diagnostics() != second.diagnostics()
        {
            problems.push(format!("{}: parsing the printed tree gives a different result", rel(path)));
        }
    }
    assert!(problems.is_empty(), "{} round-trip problem(s):\n{}", problems.len(), problems.join("\n"));
}

fn indent(text: &str) -> String {
    text.lines().map(|l| format!("    {l}\n")).collect()
}

/// A line diff of `expected` and `actual` with three lines of context, from a
/// longest common subsequence (snapshots are a few hundred lines at most).
fn diff(expected: &str, actual: &str) -> String {
    let a: Vec<&str> = expected.lines().collect();
    let b: Vec<&str> = actual.lines().collect();
    let (n, m) = (a.len(), b.len());
    // lcs[i][j]: length of the LCS of a[i..] and b[j..].
    let mut lcs = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i] == b[j] { lcs[i + 1][j + 1] + 1 } else { lcs[i + 1][j].max(lcs[i][j + 1]) };
        }
    }
    // Edit script: (' ' | '-' | '+', line).
    let mut ops: Vec<(char, &str)> = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            ops.push((' ', a[i]));
            i += 1;
            j += 1;
        } else if i < n && (j == m || lcs[i + 1][j] >= lcs[i][j + 1]) {
            ops.push(('-', a[i]));
            i += 1;
        } else {
            ops.push(('+', b[j]));
            j += 1;
        }
    }
    const CONTEXT: usize = 3;
    let changed: Vec<usize> = ops.iter().enumerate().filter(|(_, (c, _))| *c != ' ').map(|(k, _)| k).collect();
    let mut out = String::new();
    let mut last_printed: Option<usize> = None;
    for (k, (c, line)) in ops.iter().enumerate() {
        let near = changed.iter().any(|&x| x.abs_diff(k) <= CONTEXT);
        if !near {
            continue;
        }
        if last_printed.is_some_and(|l| l + 1 != k) || (last_printed.is_none() && k > 0) {
            out.push_str("    ...\n");
        }
        let _ = writeln!(out, "  {c} {line}");
        last_printed = Some(k);
    }
    if expected.ends_with('\n') != actual.ends_with('\n') {
        out.push_str("    (the final line break differs)\n");
    }
    out
}
