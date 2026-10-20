//! `nlc check` on the conformance suite (exit
//! criterion): every entry program of `tests/run/` reports no error, and
//! exactly the warnings and lints the table below expects, by diagnostic
//! code. tests/run/README.md names the kinds of diagnostic that are
//! expected (shadowing in pattern tests, unused bindings, the use of a
//! `global`, a file-level variable read in a `spawn do` block); this test
//! pins them per file, so that a new false positive or a lost diagnostic
//! fails it.
//!
//! Each program is checked as `nlc check --decl decl <file>` in a fresh
//! process, with the working directory set to `tests/run/` and `NL_PATH`
//! unset. The
//! regex and worker programs are included: `nlc check` compiles without
//! running. A diagnostic line ends in its code in brackets
//! (`crates/nlc/src/run.rs`); the lines of its source snippet are skipped.
//!
//! The rejected programs are covered by `conformance.rs` and by the
//! resolver's and analyzer's own tests (`crates/nl_sema/src/tests.rs`).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

fn run_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/run")
}

/// The diagnostics each entry program is expected to produce: the codes
/// in sorted order, with repeats. A program not listed is expected to
/// produce none.
fn expected() -> BTreeMap<&'static str, Vec<&'static str>> {
    let table: &[(&str, &[&str])] = &[
        // `typeof(label)` calls `label` with nothing, on purpose there.
        ("call_autocall_argument", &["autocall-argument"]),
        ("conv_builtin", &["unused-binding", "unused-binding", "unused-binding"]),
        ("conv_custom", &["unused-binding"]),
        ("ctrl_if_let", &["unused-binding", "unused-binding", "unused-binding"]),
        ("err_builtin", &["unused-binding", "unused-binding", "unused-binding", "unused-binding", "unused-binding"]),
        ("expr_compare_logic", &["unused-binding", "unused-binding"]),
        ("expr_ranges", &["unused-binding", "unused-binding"]),
        ("fn_return", &["unused-binding"]),
        ("lex_string_raw", &["raw-escape"]),
        ("lua_path_own_function", &["lua-path-own-function"]),
        ("op_arith", &["unused-binding"]),
        ("op_coerce", &["unused-binding"]),
        ("pattern_bind_rules", &["shadowing"]),
        ("pattern_let_mismatch", &["unused-binding"; 9]),
        ("pattern_struct", &["shadowing"]),
        ("string_encoding", &["unused-binding", "unused-binding"]),
        // A member call of a Table function on an untyped value, caught.
        ("table_errors", &["table-method-untyped"]),
        ("struct_basic", &["unused-binding"]),
        ("type_checks", &["unused-binding"]),
        ("var_const_hoist", &["shadowing"]),
        ("var_global", &["global-use", "global-use", "global-use"]),
        ("var_scope", &["shadowing"]),
        ("worker_copy", &["unused-binding", "unused-binding"]),
        ("worker_send_error", &["unused-binding"; 6]),
        ("worker_spawn_block", &["spawn-file-variable"]),
        // A top-level `try` is a statement the worker runs again.
        ("worker_try_decls", &["spawn-loads-file"; 3]),
    ];
    table.iter().map(|(name, codes)| (*name, codes.to_vec())).collect()
}

/// The diagnostics `nlc check` printed for one file: (severity, code).
fn check(nlc: &Path, dir: &Path, rel: &Path) -> Vec<(String, String)> {
    let out = Command::new(nlc).current_dir(dir).env_remove("NL_PATH").args(["check", "--decl", "decl"]).arg(rel).output().expect("cannot start nlc");
    let text = String::from_utf8_lossy(&out.stderr);
    text.lines()
        // The snippet under each diagnostic (`  |`, `12 | source`, `   |   ^^^`)
        // is not a diagnostic: its gutter is a line number or nothing.
        .filter(|l| !l.split_once(" |").is_some_and(|(gutter, _)| gutter.trim().chars().all(|c| c.is_ascii_digit())))
        .filter_map(|l| {
            // `file:line:col: severity: message [code]`
            let rest = l.splitn(4, ':').nth(3)?.trim_start();
            let severity = rest.split(':').next()?.to_string();
            let code = l.rsplit_once(" [")?.1.strip_suffix(']')?.to_string();
            Some((severity, code))
        })
        .collect()
}

#[test]
fn conformance_programs_check_as_expected() {
    let dir = std::fs::canonicalize(run_dir()).unwrap();
    let nlc = PathBuf::from(env!("CARGO_BIN_EXE_nlc"));
    let expected = expected();
    let mut programs: Vec<PathBuf> = std::fs::read_dir(&dir).unwrap().filter_map(Result::ok).map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "nl") && p.with_extension("out").exists()).collect();
    programs.sort();
    assert!(programs.len() >= 100, "expected the conformance corpus under {}", dir.display());
    let mut failures = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for path in &programs {
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        seen.insert(stem.clone());
        let rel = path.strip_prefix(&dir).unwrap();
        let diags = check(&nlc, &dir, rel);
        let errors: Vec<&(String, String)> = diags.iter().filter(|(s, _)| s == "error").collect();
        if !errors.is_empty() {
            failures.push(format!("{stem}: errors {errors:?}"));
            continue;
        }
        let mut codes: Vec<&str> = diags.iter().map(|(_, c)| c.as_str()).collect();
        codes.sort();
        let want: Vec<&str> = expected.get(stem.as_str()).cloned().unwrap_or_default();
        let mut want_sorted = want.clone();
        want_sorted.sort();
        if codes != want_sorted {
            failures.push(format!("{stem}: expected {want_sorted:?}, got {codes:?}"));
        }
    }
    for name in expected.keys() {
        if !seen.contains(*name) {
            failures.push(format!("{name}: listed as expected to warn, but there is no such program"));
        }
    }
    assert!(failures.is_empty(), "{} program(s) differ from the expected diagnostics:\n{}", failures.len(), failures.join("\n"));
}

/// Each diagnostic of `nlc check` is its one-line form, then the source
/// line with carets under the range: a tab is
/// expanded and a two-byte character is one column, so the carets stay
/// under the name. `nlc run` prints its first error with the same snippet.
/// The program is `tests/cases/lib/snippet_errors.nl`.
#[test]
fn diagnostics_show_snippets() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cases");
    let nlc = PathBuf::from(env!("CARGO_BIN_EXE_nlc"));
    let out = Command::new(&nlc).current_dir(&dir).env_remove("NL_PATH").args(["check", "lib/snippet_errors.nl"]).output().expect("cannot start nlc");
    let text = String::from_utf8_lossy(&out.stderr);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 8, "{text}");
    assert!(lines[0].starts_with("lib/snippet_errors.nl:4:13: error: `missing` is not declared") && lines[0].ends_with(" [undeclared-name]"), "{text}");
    assert_eq!(lines[1..4], ["  |", "4 | print(  \"é\", missing)", "  |              ^^^^^^^"], "{text}");
    assert!(lines[4].starts_with("lib/snippet_errors.nl:6:3: error: `also_missing` is not declared"), "{text}");
    assert_eq!(lines[5..8], ["  |", "6 |   also_missing", "  |   ^^^^^^^^^^^^"], "{text}");

    let out = Command::new(&nlc).current_dir(&dir).env_remove("NL_PATH").args(["run", "lib/snippet_errors.nl"]).output().expect("cannot start nlc");
    let text = String::from_utf8_lossy(&out.stderr);
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].starts_with("lib/snippet_errors.nl:4:13: `missing` is not declared") && lines[0].ends_with("(and 1 more error)"), "{text}");
    assert_eq!(lines[1..], ["  |", "4 | print(  \"é\", missing)", "  |              ^^^^^^^"], "{text}");
}
