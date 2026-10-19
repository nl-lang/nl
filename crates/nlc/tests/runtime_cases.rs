//! Regression tests of the runtime library and of `nlc run`, in
//! `tests/runtime_cases/`.
//!
//! Every `.nl` file at the top of that directory with a `.out` beside it is
//! a case. It runs as `nlc run <file>` in a fresh process, with the working
//! directory set to `tests/runtime_cases/`, `NL_PATH` unset and a timeout;
//! stdout must equal the `.out` file byte for byte and the exit status must
//! be 0, as in the conformance suite (`conformance.rs`). Helpers (`.nl` and
//! `.lua` files that the cases `include` or import) live in `lib/`.
//!
//! The cases also run with `NLC_TEST_RT_GLOBAL=NL_RT`, the test hook of
//! `nlc run` (crates/nlc/src/run.rs) that stores the runtime table in the
//! global `NL_RT`, so that a Lua helper can call the host interface of
//! runtime/nl.lua, which NL code cannot reach.
//!
//! A libtest test (`cargo test -p nlc --test runtime_cases`); the table of
//! results is printed on failure, or always with `-- --nocapture`.
//! Environment: `NL_RUNTIME_CASES_FILTER=a,b` runs only the cases whose
//! name contains one of these substrings.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn cases_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/runtime_cases")
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
    let stdout = out_t.join().unwrap_or_default();
    let stderr = err_t.join().unwrap_or_default();
    Ok((status.code(), stdout, stderr))
}

/// Where the expected output and the actual one first differ.
fn first_difference(expected: &[u8], actual: &[u8]) -> String {
    let e = String::from_utf8_lossy(expected);
    let a = String::from_utf8_lossy(actual);
    let el: Vec<&str> = e.split('\n').collect();
    let al: Vec<&str> = a.split('\n').collect();
    for i in 0..el.len().max(al.len()) {
        let x = el.get(i).copied();
        let y = al.get(i).copied();
        if x != y {
            return format!("line {}: expected {:?}, got {:?}", i + 1, x.unwrap_or("<end>"), y.unwrap_or("<end>"));
        }
    }
    "output differs".to_string()
}

#[test]
fn runtime_cases() {
    let dir = cases_dir();
    let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
    let nlc = PathBuf::from(env!("CARGO_BIN_EXE_nlc"));
    let filters: Vec<String> = std::env::var("NL_RUNTIME_CASES_FILTER")
        .ok()
        .map(|f| f.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|rd| rd.filter_map(Result::ok).map(|e| e.path()).collect())
        .unwrap_or_default();
    paths.sort();
    let cases: Vec<(String, PathBuf)> = paths
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "nl") && p.with_extension("out").exists())
        .map(|p| (p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(), p))
        .filter(|(name, _)| filters.is_empty() || filters.iter().any(|f| name.contains(f.as_str())))
        .collect();

    let width = cases.iter().map(|(n, _)| n.len()).max().unwrap_or(10).max(10);
    println!("{:<width$}  result  detail", "case");
    println!("{:-<width$}  ------  ------", "");
    let mut failed = 0;
    for (name, path) in &cases {
        let rel = path.strip_prefix(&dir).unwrap_or(path).to_path_buf();
        let mut cmd = Command::new(&nlc);
        cmd.current_dir(&dir).env_remove("NL_PATH").env("NLC_TEST_RT_GLOBAL", "NL_RT").arg("run").arg(&rel);
        let expected = std::fs::read(path.with_extension("out")).unwrap_or_default();
        let detail = match run_with_timeout(cmd, Duration::from_secs(30)) {
            Err(e) => Some(e),
            Ok((code, stdout, stderr)) => {
                if stdout == expected && code == Some(0) {
                    None
                } else {
                    let why = String::from_utf8_lossy(&stderr).lines().find(|l| !l.trim().is_empty()).unwrap_or("").to_string();
                    let diff = if stdout != expected { first_difference(&expected, &stdout) } else { String::new() };
                    Some(format!("exit {}: {why} [{diff}]", code.map_or("signal".to_string(), |c| c.to_string())))
                }
            }
        };
        match detail {
            None => println!("{name:<width$}  pass"),
            Some(d) => {
                failed += 1;
                println!("{name:<width$}  FAIL    {d}");
            }
        }
    }
    println!("\n{} passed, {failed} failed", cases.len() - failed);
    assert!(!cases.is_empty(), "no case selected in {}", dir.display());
    assert_eq!(failed, 0, "{failed} runtime case(s) failed; see the table above");
}

/// `nlc run` reports an uncaught error once, with one traceback that has
/// the line of the `raise`, also when the `raise` is a function's last
/// statement. The program is
/// `tests/runtime_cases/lib/uncaught.nl`.
#[test]
fn uncaught_error_report() {
    let dir = cases_dir();
    let nlc = PathBuf::from(env!("CARGO_BIN_EXE_nlc"));
    let mut cmd = Command::new(&nlc);
    cmd.current_dir(&dir).env_remove("NL_PATH").arg("run").arg("lib/uncaught.nl");
    let result = run_with_timeout(cmd, Duration::from_secs(30));
    assert!(result.is_ok(), "{result:?}");
    let (code, _, stderr) = result.unwrap_or_default();
    let stderr = String::from_utf8_lossy(&stderr);
    assert_eq!(code, Some(1), "{stderr}");
    assert!(stderr.starts_with("nlc: lib/uncaught.nl: uncaught Exception: left uncaught\n"), "{stderr}");
    assert_eq!(stderr.matches("stack traceback:").count(), 1, "{stderr}");
    assert!(stderr.contains("lib/uncaught.nl:4:"), "{stderr}");
}
