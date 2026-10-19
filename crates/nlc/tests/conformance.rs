//! The conformance suite of `tests/run/` (see its README).
//!
//! Every entry program (a `.nl` with a `.out` beside it) runs as
//! `nlc run <file>` in a fresh process, with the working directory set to
//! `tests/run/`, `NL_PATH` unset and a timeout; stdout must equal the `.out`
//! file byte for byte and the exit status must be 0. It runs eight times:
//! through the Lua backend, and through the bytecode backend with
//! the JIT on, off and forced (`--backend=bc --jit=on|off|hot`), each with
//! the analyzer's static resolution on (the default) and off
//! (`--no-static-resolution`), which must agree.
//! Four more runs change which Lua paths are read once into hidden locals
//! (the default is `std`): the Lua
//! backend and the bytecode backend with the JIT forced, each with
//! `--localize=none` and `--localize=all`, which must print the same too.
//! The table has one row per run, with its mode. Every program in `reject/`
//! must fail to compile (`nlc build --emit=lua`, and `--emit=bc`) with an
//! error on the line after its `# expect-error:` comment, in that file
//! itself (an error on the same line of a file it imports does not count).
//!
//! The exclusions are the README's ("Which programs need what") for stock
//! LuaJIT: the `regex_*` programs and the two worker programs that the same-state stand-in of `spawn` cannot run
//! (`worker_dedicated`, which needs `Worker.new`, and `worker_spawn_block`,
//! which reads a file-level variable that each worker has a copy of); the
//! other `worker_*` programs run on the stand-in. Every rejected
//! program runs: `reject/regex_*` and `reject/type_*` are caught by the
//! compiler.
//!
//! Environment:
//! - `NL_CONFORMANCE_FILTER=a,b`: run only programs whose name contains one
//!   of these substrings (`reject/` included in the name for rejected
//!   programs);
//! - `NL_CONFORMANCE_TIMEOUT`: seconds per program (default 20).
//!
//! The run fails when any selected program fails; nothing is hidden
//! behind an allow-list.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Run,
    Reject,
}

/// A backend and, for the bytecode backend, the JIT setting of a run.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Lua,
    BcJitOn,
    BcJitOff,
    BcJitHot,
}

impl Mode {
    const RUN: [Mode; 4] = [Mode::Lua, Mode::BcJitOn, Mode::BcJitOff, Mode::BcJitHot];
    const REJECT: [Mode; 2] = [Mode::Lua, Mode::BcJitOn];

    fn label(self) -> &'static str {
        match self {
            Mode::Lua => "lua",
            Mode::BcJitOn => "bc jit=on",
            Mode::BcJitOff => "bc jit=off",
            Mode::BcJitHot => "bc jit=hot",
        }
    }

    /// The `nlc run` options of the mode.
    fn run_args(self) -> &'static [&'static str] {
        match self {
            Mode::Lua => &["--backend=lua"],
            Mode::BcJitOn => &["--backend=bc", "--jit=on"],
            Mode::BcJitOff => &["--backend=bc", "--jit=off"],
            Mode::BcJitHot => &["--backend=bc", "--jit=hot"],
        }
    }

    /// The `nlc build` option of the mode's backend.
    fn emit_arg(self) -> &'static str {
        match self {
            Mode::Lua => "--emit=lua",
            _ => "--emit=bc",
        }
    }
}

struct Case {
    name: String,
    path: PathBuf,
    kind: Kind,
    mode: Mode,
    /// Run with the analyzer's static resolution; `false` adds
    /// `--no-static-resolution`.
    sr: bool,
    /// `--localize=` this setting; `None`: the default.
    localize: Option<&'static str>,
}

struct Outcome {
    name: String,
    kind: Kind,
    mode: Mode,
    sr: bool,
    localize: Option<&'static str>,
    pass: bool,
    detail: String,
}

/// The mode column: the backend and JIT mode, `-sr` without static
/// resolution, and the localize setting when it is not the default.
fn mode_label(mode: Mode, sr: bool, localize: Option<&str>) -> String {
    let mut label = if sr { mode.label().to_owned() } else { format!("{} -sr", mode.label()) };
    if let Some(l) = localize {
        label.push_str(&format!(" loc={l}"));
    }
    label
}

/// The localize settings other than the default, and the modes each runs
/// in (see the module documentation).
const LOCALIZE_RUNS: [(&str, Mode); 4] = [("none", Mode::Lua), ("none", Mode::BcJitHot), ("all", Mode::Lua), ("all", Mode::BcJitHot)];

fn run_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/run")
}

fn excluded_run(name: &str) -> bool {
    name.starts_with("regex_") || name == "worker_dedicated" || name == "worker_spawn_block"
}

fn excluded_reject(_name: &str) -> bool {
    false
}

fn cases(dir: &Path) -> Vec<Case> {
    let mut out = Vec::new();
    let mut add = |sub: &Path, kind: Kind| {
        let Ok(rd) = std::fs::read_dir(sub) else { return };
        let mut paths: Vec<PathBuf> = rd.filter_map(Result::ok).map(|e| e.path()).collect();
        paths.sort();
        for p in paths {
            if p.extension().is_none_or(|e| e != "nl") {
                continue;
            }
            let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            match kind {
                Kind::Run => {
                    if !p.with_extension("out").exists() || excluded_run(&stem) {
                        continue;
                    }
                    for mode in Mode::RUN {
                        for sr in [true, false] {
                            out.push(Case { name: stem.clone(), path: p.clone(), kind: Kind::Run, mode, sr, localize: None });
                        }
                    }
                    for (localize, mode) in LOCALIZE_RUNS {
                        out.push(Case { name: stem.clone(), path: p.clone(), kind: Kind::Run, mode, sr: true, localize: Some(localize) });
                    }
                }
                Kind::Reject => {
                    if excluded_reject(&stem) {
                        continue;
                    }
                    for mode in Mode::REJECT {
                        out.push(Case { name: format!("reject/{stem}"), path: p.clone(), kind: Kind::Reject, mode, sr: true, localize: None });
                    }
                }
            }
        }
    };
    add(dir, Kind::Run);
    add(&dir.join("reject"), Kind::Reject);
    out
}

/// Runs a command with a timeout; returns (status code, stdout, stderr),
/// or an error text.
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

fn first_line(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("").to_string();
    if line.len() > 160 { format!("{}…", &line[..line.char_indices().nth(160).map_or(line.len(), |(i, _)| i)]) } else { line }
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

fn run_case(nlc: &Path, dir: &Path, case: &Case, timeout: Duration) -> Outcome {
    let rel = case.path.strip_prefix(dir).unwrap_or(&case.path).to_path_buf();
    let mut cmd = Command::new(nlc);
    cmd.current_dir(dir).env_remove("NL_PATH");
    let fail = |detail: String| Outcome { name: case.name.clone(), kind: case.kind, mode: case.mode, sr: case.sr, localize: case.localize, pass: false, detail };
    let pass = || Outcome { name: case.name.clone(), kind: case.kind, mode: case.mode, sr: case.sr, localize: case.localize, pass: true, detail: String::new() };
    match case.kind {
        Kind::Run => {
            cmd.arg("run").args(case.mode.run_args());
            if !case.sr {
                cmd.arg("--no-static-resolution");
            }
            if let Some(l) = case.localize {
                cmd.arg(format!("--localize={l}"));
            }
            cmd.arg(&rel);
            let expected = std::fs::read(case.path.with_extension("out")).unwrap_or_default();
            match run_with_timeout(cmd, timeout) {
                Err(e) => fail(e),
                Ok((code, stdout, stderr)) => {
                    if stdout == expected && code == Some(0) {
                        pass()
                    } else if code != Some(0) {
                        let why = first_line(&stderr);
                        let diff = if stdout != expected { format!(" [{}]", first_difference(&expected, &stdout)) } else { String::new() };
                        fail(format!("exit {}: {why}{diff}", code.map_or("signal".to_string(), |c| c.to_string())))
                    } else {
                        fail(first_difference(&expected, &stdout))
                    }
                }
            }
        }
        Kind::Reject => {
            let src = std::fs::read_to_string(&case.path).unwrap_or_default();
            let Some(marker) = src.lines().position(|l| l.trim_start().starts_with("# expect-error:")) else {
                return fail("no `# expect-error:` comment".to_string());
            };
            let want = marker + 2;
            cmd.arg("build").arg(case.mode.emit_arg()).arg(&rel);
            match run_with_timeout(cmd, timeout) {
                Err(e) => fail(e),
                Ok((code, _stdout, stderr)) => {
                    let text = String::from_utf8_lossy(&stderr);
                    // The file of a diagnostic is its canonical name, relative
                    // to the working directory: `reject/name.nl`.
                    let own = rel.to_string_lossy().replace('\\', "/");
                    let lines: Vec<(String, usize, String)> = text
                        .lines()
                        .filter(|l| l.contains(": error: "))
                        .filter_map(|l| {
                            let mut parts = l.splitn(4, ':');
                            let file = parts.next()?.trim().replace('\\', "/");
                            let line = parts.next()?.trim().parse::<usize>().ok()?;
                            Some((file, line, l.to_string()))
                        })
                        .collect();
                    if code == Some(0) {
                        fail(format!("compiled without error (expected one on line {want})"))
                    } else if lines.iter().any(|(f, l, _)| *l == want && *f == own) {
                        pass()
                    } else {
                        let got = lines.first().map_or_else(|| first_line(&stderr), |(_, _, l)| l.clone());
                        fail(format!("expected an error in {own} on line {want}; got: {got}"))
                    }
                }
            }
        }
    }
}

fn main() {
    let dir = run_dir();
    let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
    let nlc = PathBuf::from(env!("CARGO_BIN_EXE_nlc"));
    let timeout = Duration::from_secs(std::env::var("NL_CONFORMANCE_TIMEOUT").ok().and_then(|s| s.parse().ok()).unwrap_or(20));
    let filters: Vec<String> = std::env::var("NL_CONFORMANCE_FILTER")
        .ok()
        .map(|f| f.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();
    let all = cases(&dir);
    let selected: Vec<&Case> = all.iter().filter(|c| filters.is_empty() || filters.iter().any(|f| c.name.contains(f.as_str()))).collect();

    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<Outcome>>> = Mutex::new((0..selected.len()).map(|_| None).collect());
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get()).min(8);
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some(case) = selected.get(i) else { break };
                let o = run_case(&nlc, &dir, case, timeout);
                if let Ok(mut r) = results.lock() {
                    if let Some(slot) = r.get_mut(i) {
                        *slot = Some(o);
                    }
                }
            });
        }
    });
    let results: Vec<Outcome> = results.into_inner().unwrap_or_default().into_iter().flatten().collect();

    let width = results.iter().map(|r| r.name.len()).max().unwrap_or(10).max(10);
    println!("{:<width$}  {:<19}  result  detail", "program", "mode");
    println!("{:-<width$}  -------------------  ------  ------", "");
    let mut failed = 0;
    for r in &results {
        if !r.pass {
            failed += 1;
        }
        println!("{:<width$}  {:<19}  {}  {}", r.name, mode_label(r.mode, r.sr, r.localize), if r.pass { "pass  " } else { "FAIL  " }, r.detail);
    }
    let passed = results.len() - failed;
    let programs: std::collections::BTreeSet<&str> = selected.iter().map(|c| c.name.as_str()).collect();
    let all_programs: std::collections::BTreeSet<&str> = all.iter().map(|c| c.name.as_str()).collect();
    println!(
        "\n{passed} passed, {failed} failed: {} runs of {} programs selected of {} ({} excluded by the README)",
        results.len(),
        programs.len(),
        all_programs.len(),
        count_excluded(&dir)
    );
    let summarize = |label: &str, runs: Vec<&Outcome>| {
        let f = runs.iter().filter(|r| !r.pass).count();
        if !runs.is_empty() {
            println!("  {label:<19}  {} passed, {f} failed", runs.len() - f);
        }
    };
    for mode in Mode::RUN {
        for sr in [true, false] {
            summarize(&mode_label(mode, sr, None), results.iter().filter(|r| r.kind == Kind::Run && r.mode == mode && r.sr == sr && r.localize.is_none()).collect());
        }
    }
    for (localize, mode) in LOCALIZE_RUNS {
        let runs = results.iter().filter(|r| r.kind == Kind::Run && r.mode == mode && r.localize == Some(localize)).collect();
        summarize(&mode_label(mode, true, Some(localize)), runs);
    }
    summarize("reject", results.iter().filter(|r| r.kind == Kind::Reject).collect());
    if failed > 0 {
        std::process::exit(1);
    }
}

fn count_excluded(dir: &Path) -> usize {
    let mut n = 0;
    for (sub, reject) in [(dir.to_path_buf(), false), (dir.join("reject"), true)] {
        let Ok(rd) = std::fs::read_dir(&sub) else { continue };
        for e in rd.filter_map(Result::ok) {
            let p = e.path();
            if p.extension().is_none_or(|x| x != "nl") {
                continue;
            }
            let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            if reject {
                n += usize::from(excluded_reject(&stem));
            } else if p.with_extension("out").exists() {
                n += usize::from(excluded_run(&stem));
            }
        }
    }
    n
}
