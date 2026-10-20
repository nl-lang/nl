//! Every conformance program under `tests/run/`
//! (programs, helpers and the rejected programs) parses with no diagnostic,
//! and its tree holds exactly the source text; a declaration file there
//! (`.d.nl`) as a declaration file.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::path::{Path, PathBuf};

use nl_syntax::{SyntaxKind, dump_diagnostics, parse, parse_decl_file};

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

#[test]
fn conformance_programs_parse_cleanly() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/run");
    let mut files = Vec::new();
    nl_files(&dir, &mut files);
    assert!(files.len() >= 158, "expected the conformance corpus, found {} files", files.len());
    let mut failures = Vec::new();
    for path in &files {
        let bytes = std::fs::read(path).unwrap();
        let src = nl_syntax::source_from_bytes(&bytes).unwrap();
        let parse = if path.to_string_lossy().ends_with(".d.nl") { parse_decl_file(src) } else { parse(src) };
        let root = parse.syntax();
        assert_eq!(root.kind(), SyntaxKind::SOURCE_FILE);
        if root.to_string() != src {
            failures.push(format!("{}: the tree text differs from the source", path.display()));
        }
        if root.descendants().any(|n| n.kind() == SyntaxKind::ERROR_NODE) {
            failures.push(format!("{}: the tree has an ERROR_NODE", path.display()));
        }
        if !parse.diagnostics().is_empty() {
            failures.push(format!("{}:\n{}", path.display(), dump_diagnostics(src, parse.diagnostics())));
        }
    }
    assert!(failures.is_empty(), "{} problem(s):\n{}", failures.len(), failures.join("\n"));
}
