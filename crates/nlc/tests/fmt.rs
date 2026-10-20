//! `nlc fmt` end to end.
//!
//! - The conformance programs still pass once formatted: `tests/run/` is
//!   copied into a scratch directory, every `.nl` file in it (helpers
//!   included, since programs import them) is formatted in place with
//!   `nlc fmt`, and each entry program runs with the mechanics of
//!   `conformance.rs` and tests/run/README.md (working directory set to the
//!   copy, `NL_PATH` unset, stdout byte-equal to the `.out`, exit status 0),
//!   through the default backend. The exclusions are those of
//!   `conformance.rs`.
//! - `nlc fmt --fix` rewrites the continued strings of
//!   `tests/cases/string_continued.nl` as heredocs, and the program prints
//!   the same output before and after.
//! - The flags: `--check`, `--stdout`, in-place rewriting, warnings, and a
//!   file with a syntax error, which is reported and left alone.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

fn nlc() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_nlc"))
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A fresh scratch directory under the target directory.
fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("fmt_{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

fn nl_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            nl_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "nl") {
            out.push(path);
        }
    }
}

/// The exit code (`None` for a signal), stdout and stderr of a process.
type Finished = (Option<i32>, Vec<u8>, Vec<u8>);

fn run_with_timeout(mut cmd: Command, timeout: Duration) -> Result<Finished, String> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());
    let mut child = cmd.spawn().map_err(|e| format!("cannot start nlc: {e}"))?;
    let mut so = child.stdout.take();
    let mut se = child.stderr.take();
    let out_t = std::thread::spawn(move || {
        let mut b = Vec::new();
        if let Some(s) = so.as_mut() {
            let _ = s.read_to_end(&mut b);
        }
        b
    });
    let err_t = std::thread::spawn(move || {
        let mut b = Vec::new();
        if let Some(s) = se.as_mut() {
            let _ = s.read_to_end(&mut b);
        }
        b
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("timed out after {}s", timeout.as_secs()));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(e) => return Err(format!("wait failed: {e}")),
        }
    };
    Ok((status.code(), out_t.join().unwrap_or_default(), err_t.join().unwrap_or_default()))
}

/// `nlc <args>` in `dir`, finished: (exit code, stdout, stderr as text).
fn nlc_in(dir: &Path, args: &[&str]) -> (Option<i32>, Vec<u8>, String) {
    let mut cmd = Command::new(nlc());
    cmd.current_dir(dir).env_remove("NL_PATH").args(args);
    let (code, stdout, stderr) = run_with_timeout(cmd, Duration::from_secs(60)).unwrap();
    (code, stdout, String::from_utf8_lossy(&stderr).into_owned())
}

fn first_line(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).lines().find(|l| !l.trim().is_empty()).unwrap_or("").chars().take(200).collect()
}

fn excluded(stem: &str) -> bool {
    stem.starts_with("regex_") || stem.starts_with("worker_")
}

#[test]
fn formatted_conformance_programs_run() {
    let dir = scratch("run");
    copy_dir(&repo_root().join("tests/run"), &dir);
    let mut files = Vec::new();
    nl_files(&dir, &mut files);
    assert!(files.len() >= 158, "expected the conformance corpus, found {} files", files.len());

    // Format every file in place, helpers and rejected programs included.
    let args: Vec<String> = std::iter::once("fmt".to_string()).chain(files.iter().map(|p| p.display().to_string())).collect();
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let (code, stdout, stderr) = nlc_in(&dir, &refs);
    assert_eq!(code, Some(0), "nlc fmt failed:\n{stderr}");
    assert!(stdout.is_empty(), "nlc fmt printed: {}", String::from_utf8_lossy(&stdout));
    let changed = files.iter().filter(|p| std::fs::read(p).unwrap() != std::fs::read(repo_root().join("tests/run").join(p.strip_prefix(&dir).unwrap())).unwrap()).count();
    assert!(changed > 0, "no conformance program was changed by the formatter");

    // Every entry program, as conformance.rs runs it.
    let entries: Vec<&PathBuf> = files
        .iter()
        .filter(|p| p.parent() == Some(dir.as_path()) && p.with_extension("out").exists())
        .filter(|p| !excluded(&p.file_stem().unwrap().to_string_lossy()))
        .collect();
    assert!(entries.len() >= 100, "only {} entry programs", entries.len());
    let next = AtomicUsize::new(0);
    let failures: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get()).min(8);
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some(path) = entries.get(i) else { break };
                let rel = path.strip_prefix(&dir).unwrap().display().to_string();
                let expected = std::fs::read(path.with_extension("out")).unwrap();
                let mut cmd = Command::new(nlc());
                cmd.current_dir(&dir).env_remove("NL_PATH").args(["run", "--decl", "decl"]).arg(&rel);
                let outcome = match run_with_timeout(cmd, Duration::from_secs(60)) {
                    Err(e) => Some(e),
                    Ok((Some(0), stdout, _)) if stdout == expected => None,
                    Ok((code, stdout, stderr)) => Some(format!("exit {code:?}: {} [stdout {}]", first_line(&stderr), first_line(&stdout))),
                };
                if let Some(why) = outcome {
                    failures.lock().unwrap().push(format!("{rel}: {why}"));
                }
            });
        }
    });
    let failures = failures.into_inner().unwrap();
    assert!(failures.is_empty(), "{} formatted program(s) failed:\n{}", failures.len(), failures.join("\n"));
}

#[test]
fn fix_rewrites_continued_strings_and_keeps_the_output() {
    let dir = scratch("fix");
    let name = "string_continued.nl";
    let cases = repo_root().join("crates/nlc/tests/cases");
    std::fs::copy(cases.join(name), dir.join(name)).unwrap();
    let expected = std::fs::read(cases.join("string_continued.out")).unwrap();
    let original = std::fs::read_to_string(dir.join(name)).unwrap();
    assert!(original.contains("\\\n"));

    // Without --fix: warnings, no change, exit 0.
    let (code, _, stderr) = nlc_in(&dir, &["fmt", name]);
    assert_eq!(code, Some(0), "{stderr}");
    let warning = "warning: a \"…\" string continued with a backslash; write a heredoc (nlc fmt --fix rewrites it)";
    assert_eq!(stderr.lines().filter(|l| l.ends_with(warning)).count(), 4, "{stderr}");
    assert!(stderr.contains(&format!("{name}:5:13: {warning}")), "{stderr}");
    assert_eq!(std::fs::read_to_string(dir.join(name)).unwrap(), original);

    // With --fix: heredocs, no warning, the same output.
    let (code, _, stderr) = nlc_in(&dir, &["fmt", "--fix", name]);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stderr.is_empty(), "{stderr}");
    let fixed = std::fs::read_to_string(dir.join(name)).unwrap();
    assert!(!fixed.contains("\\\n"), "{fixed}");
    assert_eq!(fixed.matches("\"\"\"").count(), 8, "{fixed}");
    assert!(fixed.contains("let plain = \"\"\"\naaa\nbbb\nccc\n\"\"\"\n"), "{fixed}");
    assert!(fixed.contains("let tail = \"\"\"\none\ntwo\n\"\"\".len\n"), "{fixed}");
    let (code, stdout, stderr) = nlc_in(&dir, &["run", name]);
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(stdout, expected);
    // The fixed file is formatted.
    let (code, stdout, _) = nlc_in(&dir, &["fmt", "--check", name]);
    assert_eq!(code, Some(0), "{}", String::from_utf8_lossy(&stdout));

    // A continued string inside an interpolation cannot be fixed.
    let inner = "let s = \"a #{\"b \\\nc\"} d\"\nprint s\n";
    std::fs::write(dir.join("inner.nl"), inner).unwrap();
    let (code, _, stderr) = nlc_in(&dir, &["fmt", "--fix", "inner.nl"]);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stderr.contains("inner.nl:1:14: warning: a \"…\" string continued with a backslash inside an interpolation"), "{stderr}");
    assert_eq!(std::fs::read_to_string(dir.join("inner.nl")).unwrap(), inner);
}

#[test]
fn check_stdout_and_in_place() {
    let dir = scratch("flags");
    let messy = "let  x=1\nprint( x )\n";
    let clean = "let x = 1\nprint(x)\n";
    std::fs::write(dir.join("a.nl"), messy).unwrap();
    std::fs::write(dir.join("b.nl"), clean).unwrap();

    // --check: reports the file that would change, writes nothing, exit 1.
    let (code, stdout, stderr) = nlc_in(&dir, &["fmt", "--check", "a.nl", "b.nl"]);
    assert_eq!(code, Some(1), "{stderr}");
    assert_eq!(String::from_utf8_lossy(&stdout), "a.nl: not formatted\n");
    assert_eq!(std::fs::read_to_string(dir.join("a.nl")).unwrap(), messy);
    let (code, stdout, _) = nlc_in(&dir, &["fmt", "--check", "b.nl"]);
    assert_eq!(code, Some(0));
    assert!(stdout.is_empty());

    // --stdout: the formatted text, with `# path` separators for several files.
    let (code, stdout, _) = nlc_in(&dir, &["fmt", "--stdout", "a.nl"]);
    assert_eq!(code, Some(0));
    assert_eq!(String::from_utf8_lossy(&stdout), clean);
    let (code, stdout, _) = nlc_in(&dir, &["fmt", "--stdout", "a.nl", "b.nl"]);
    assert_eq!(code, Some(0));
    assert_eq!(String::from_utf8_lossy(&stdout), format!("# a.nl\n{clean}# b.nl\n{clean}"));
    assert_eq!(std::fs::read_to_string(dir.join("a.nl")).unwrap(), messy);

    // In place: the changed file is rewritten, nothing is printed.
    let (code, stdout, stderr) = nlc_in(&dir, &["fmt", "a.nl", "b.nl"]);
    assert_eq!(code, Some(0), "{stderr}");
    assert!(stdout.is_empty() && stderr.is_empty(), "{stderr}");
    assert_eq!(std::fs::read_to_string(dir.join("a.nl")).unwrap(), clean);

    // A declaration file.
    std::fs::write(dir.join("d.d.nl"), "declare module Foo=:foo\nfn f( a : Number )\nend\n").unwrap();
    let (code, _, stderr) = nlc_in(&dir, &["fmt", "d.d.nl"]);
    assert_eq!(code, Some(0), "{stderr}");
    assert_eq!(std::fs::read_to_string(dir.join("d.d.nl")).unwrap(), "declare module Foo = :foo\n  fn f(a: Number)\nend\n");

    // A syntax error: reported as `nlc parse` does, the file left alone, exit 1.
    let broken = "let x = (1\nprint  x\n";
    std::fs::write(dir.join("bad.nl"), broken).unwrap();
    let (code, stdout, stderr) = nlc_in(&dir, &["fmt", "bad.nl", "b.nl"]);
    assert_eq!(code, Some(1));
    assert!(stdout.is_empty());
    assert!(stderr.starts_with("bad.nl:2:1: error: "), "{stderr}");
    assert!(stderr.contains(": error: "), "{stderr}");
    assert_eq!(std::fs::read_to_string(dir.join("bad.nl")).unwrap(), broken);

    // Usage errors.
    let (code, _, _) = nlc_in(&dir, &["fmt"]);
    assert_eq!(code, Some(2));
    let (code, _, _) = nlc_in(&dir, &["fmt", "--check", "--stdout", "a.nl"]);
    assert_eq!(code, Some(2));
    let (code, _, _) = nlc_in(&dir, &["fmt", "--bogus", "a.nl"]);
    assert_eq!(code, Some(2));
    let (code, _, stderr) = nlc_in(&dir, &["fmt", "missing.nl"]);
    assert_eq!(code, Some(1));
    assert!(stderr.contains("missing.nl: error: cannot read the file"), "{stderr}");
}
