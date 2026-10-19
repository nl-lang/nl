//! Runs programs through the `nlc` binary and decides what is a finding.
//!
//! A generated program runs under every combination of the options that
//! must not change its output (crates/nlc/src/main.rs): the backend
//! (`--backend=lua`, the oracle, and `--backend=bc`), checked and
//! `--unchecked` (skipped for checked-only programs),
//! `--no-static-resolution` and `--jit=on|off|hot`. Each run
//! is a fresh process with the working directory set to the program's
//! directory, `NL_PATH` unset and a timeout, as tests/run/README.md asks.
//! Stdout and the exit status are compared with the oracle's; stderr is
//! kept for the report but not compared.
//!
//! A finding is: a disagreement, a crash (a signal, an exit status other
//! than 0 and 1, or a Rust panic reported on stderr), a timeout, a compile
//! error from `nlc check` on a program the generator meant to be valid,
//! a failing oracle run (then the only finding: nothing is compared with
//! it), or `nlc` failing to start. Outputs are compared byte for byte. A
//! mutated source is a finding only when `nlc parse`, `nlc check`,
//! `nlc build` or, for a mutant that compiles, `nlc run` crashes, hangs or
//! cannot start on it: any answer with status 0 or 1 is fine there
//! (crates/nlc/tests/compile_robustness.rs checks the compile through the
//! library).

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Where `nlc` is and how long a run may take.
#[derive(Clone, Debug)]
pub struct Nlc {
    pub path: PathBuf,
    pub timeout: Duration,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Backend {
    Lua,
    Bc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Jit {
    On,
    Off,
    Hot,
}

/// One combination of `nlc run` options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Config {
    pub backend: Backend,
    pub checked: bool,
    pub static_resolution: bool,
    pub jit: Jit,
}

/// The oracle: the Lua backend with the defaults.
pub const ORACLE: Config = Config { backend: Backend::Lua, checked: true, static_resolution: true, jit: Jit::On };

impl Config {
    /// The arguments of `nlc run` before the file.
    pub fn args(&self) -> Vec<&'static str> {
        let mut a = vec!["run"];
        a.push(match self.backend {
            Backend::Lua => "--backend=lua",
            Backend::Bc => "--backend=bc",
        });
        if !self.checked {
            a.push("--unchecked");
        }
        if !self.static_resolution {
            a.push("--no-static-resolution");
        }
        a.push(match self.jit {
            Jit::On => "--jit=on",
            Jit::Off => "--jit=off",
            Jit::Hot => "--jit=hot",
        });
        a
    }

    /// A file-name-safe label: `bc-unchecked-nostatic-jithot`.
    pub fn label(&self) -> String {
        format!(
            "{}-{}-{}-jit{}",
            match self.backend {
                Backend::Lua => "lua",
                Backend::Bc => "bc",
            },
            if self.checked { "checked" } else { "unchecked" },
            if self.static_resolution { "static" } else { "nostatic" },
            match self.jit {
                Jit::On => "on",
                Jit::Off => "off",
                Jit::Hot => "hot",
            }
        )
    }
}

/// Every combination to run, the oracle first. `checked_only` leaves the
/// unchecked ones out.
pub fn configs(checked_only: bool) -> Vec<Config> {
    let mut out = vec![ORACLE];
    for backend in [Backend::Lua, Backend::Bc] {
        for checked in [true, false] {
            if checked_only && !checked {
                continue;
            }
            for static_resolution in [true, false] {
                for jit in [Jit::On, Jit::Off, Jit::Hot] {
                    let c = Config { backend, checked, static_resolution, jit };
                    if c != ORACLE {
                        out.push(c);
                    }
                }
            }
        }
    }
    out
}

/// How a process ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Exited(i32),
    /// Killed by a signal (no exit code).
    Signal,
    Timeout,
    /// The process could not be started.
    NotStarted(String),
}

impl std::fmt::Display for Status {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Status::Exited(c) => write!(f, "exit {c}"),
            Status::Signal => write!(f, "killed by a signal"),
            Status::Timeout => write!(f, "timeout"),
            Status::NotStarted(e) => write!(f, "not started: {e}"),
        }
    }
}

/// A finished process.
#[derive(Clone, Debug)]
pub struct Outcome {
    pub status: Status,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl Outcome {
    /// A crash of `nlc` itself rather than an answer: a signal (a LuaJIT
    /// assertion aborts), an exit status other than 0 and 1 (101 is a Rust
    /// panic), or a panic, an internal error or a LuaJIT assertion that
    /// was reported on stderr.
    pub fn crashed(&self) -> bool {
        match &self.status {
            Status::Exited(0 | 1) => {
                let err = String::from_utf8_lossy(&self.stderr);
                err.contains("panicked at") || err.contains("[internal-error]") || err.contains("LuaJIT ASSERT")
            }
            Status::Exited(_) | Status::Signal => true,
            Status::Timeout | Status::NotStarted(_) => false,
        }
    }

    /// The finding this run is on its own, whatever any other run did: a
    /// crash, a timeout, or a process that could not be started.
    pub fn own_finding(&self, run: &str, timeout: Duration) -> Option<Finding> {
        let (kind, detail) = match &self.status {
            Status::NotStarted(e) => (Kind::NotStarted, e.clone()),
            Status::Timeout => (Kind::Timeout, format!("no answer after {}s", timeout.as_secs())),
            _ if self.crashed() => (Kind::Crash, format!("{}: {}", self.status, first_error(&self.stderr))),
            _ => return None,
        };
        Some(Finding { kind, run: run.to_string(), detail })
    }
}

impl Nlc {
    /// Runs `nlc <args> <file>` in `cwd`, with `NL_PATH` unset.
    pub fn run(&self, args: &[&str], file: &Path, cwd: &Path) -> Outcome {
        let mut cmd = Command::new(&self.path);
        cmd.args(args).arg(file).current_dir(cwd).env_remove("NL_PATH");
        // A backtrace would only make stderr longer.
        cmd.env_remove("RUST_BACKTRACE");
        run_with_timeout(cmd, self.timeout)
    }
}

fn run_with_timeout(mut cmd: Command, timeout: Duration) -> Outcome {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return Outcome { status: Status::NotStarted(e.to_string()), stdout: Vec::new(), stderr: Vec::new() },
    };
    // Both pipes are drained on threads, so that a large output cannot
    // block the child while we wait for it.
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
            Ok(Some(s)) => break s.code().map_or(Status::Signal, Status::Exited),
            Ok(None) if start.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break Status::Timeout;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(2)),
            Err(e) => break Status::NotStarted(format!("wait failed: {e}")),
        }
    };
    let stdout = out_t.join().unwrap_or_default();
    let stderr = err_t.join().unwrap_or_default();
    Outcome { status, stdout, stderr }
}

/// What kind of finding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `nlc check` reported an error on a generated program.
    Rejected,
    /// The oracle run did not exit 0.
    OracleFailed,
    /// A run printed something else than the oracle, or ended otherwise.
    Mismatch,
    /// `nlc` crashed: a signal, a panic, an internal error, or a stop on
    /// one of LuaJIT's own assertions (`LuaJIT ASSERT`, which the debug
    /// build of the vendored LuaJIT checks).
    Crash,
    Timeout,
    /// `nlc` could not be started: the campaign itself is broken.
    NotStarted,
}

impl std::fmt::Display for Kind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Kind::Rejected => "rejected",
            Kind::OracleFailed => "oracle-failed",
            Kind::Mismatch => "mismatch",
            Kind::Crash => "crash",
            Kind::Timeout => "timeout",
            Kind::NotStarted => "not-started",
        })
    }
}

/// One finding: its kind, the command it came from, and a one-line
/// description.
#[derive(Clone, Debug)]
pub struct Finding {
    pub kind: Kind,
    /// The label of the run (`bc-checked-static-jithot`, `check`,
    /// `parse`, `build-bc`, ...).
    pub run: String,
    pub detail: String,
}

/// Every run of one program and the findings among them.
#[derive(Clone, Debug, Default)]
pub struct Report {
    /// `(label, outcome)`, the oracle (or `check`) first.
    pub outcomes: Vec<(String, Outcome)>,
    pub findings: Vec<Finding>,
}

/// The first line of stderr that says something, without the position
/// prefix, for summaries and for keeping a finding the same while it is
/// minimized.
pub fn first_error(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
    let mut rest = line.trim();
    if let Some(r) = rest.strip_prefix("nlc: ") {
        rest = r;
    }
    // `path.nl:line:col: message`: keep the message.
    if let Some(i) = rest.find(".nl:") {
        if rest.get(..i).is_some_and(|p| !p.contains(' ')) {
            rest = rest.get(i + 4..).unwrap_or("").trim_start_matches(|c: char| c.is_ascii_digit() || c == ':').trim();
        }
    }
    rest.chars().take(160).collect()
}

/// The first line where two outputs differ, for a summary.
pub fn first_difference(expected: &[u8], actual: &[u8]) -> String {
    let e = String::from_utf8_lossy(expected);
    let a = String::from_utf8_lossy(actual);
    let el: Vec<&str> = e.split('\n').collect();
    let al: Vec<&str> = a.split('\n').collect();
    for i in 0..el.len().max(al.len()) {
        let x = el.get(i).copied();
        let y = al.get(i).copied();
        if x != y {
            let show = |s: Option<&str>| s.map_or("<end of output>".to_string(), |s| format!("{:?}", s.chars().take(80).collect::<String>()));
            return format!("line {}: oracle {} vs {}", i + 1, show(x), show(y));
        }
    }
    "outputs are equal".into()
}

/// Checks a generated program: `nlc check`, then every configuration,
/// the oracle first. `file` is the program, already written; runs happen
/// in its directory. When the oracle run fails, that is the one finding:
/// there is nothing to compare the other runs with.
pub fn check_program(nlc: &Nlc, file: &Path, checked_only: bool, configs_to_run: &[Config]) -> Report {
    let cwd = file.parent().unwrap_or(Path::new("."));
    let name = Path::new(file.file_name().unwrap_or_default());
    let mut report = Report::default();
    let check = nlc.run(&["check"], name, cwd);
    if let Some(f) = check.own_finding("check", nlc.timeout) {
        report.findings.push(f);
    } else if check.status != Status::Exited(0) {
        report.findings.push(Finding { kind: Kind::Rejected, run: "check".into(), detail: first_error(&check.stderr) });
    }
    report.outcomes.push(("check".into(), check));
    if !report.findings.is_empty() {
        return report;
    }
    let mut oracle: Option<Outcome> = None;
    for c in configs_to_run {
        if checked_only && !c.checked {
            continue;
        }
        let out = nlc.run(&c.args(), name, cwd);
        let label = c.label();
        match &oracle {
            None => {
                let failed = match out.own_finding(&label, nlc.timeout) {
                    Some(f) => Some(f),
                    None if out.status != Status::Exited(0) => Some(Finding { kind: Kind::OracleFailed, run: label.clone(), detail: first_error(&out.stderr) }),
                    None => None,
                };
                report.outcomes.push((label, out.clone()));
                if let Some(f) = failed {
                    report.findings.push(f);
                    return report;
                }
                oracle = Some(out);
                continue;
            }
            Some(o) => {
                if let Some(f) = out.own_finding(&label, nlc.timeout) {
                    report.findings.push(f);
                } else if o.stdout != out.stdout || o.status != out.status {
                    let detail = if o.status != out.status { format!("{} vs {}: {}", o.status, out.status, first_error(&out.stderr)) } else { first_difference(&o.stdout, &out.stdout) };
                    report.findings.push(Finding { kind: Kind::Mismatch, run: label.clone(), detail });
                }
            }
        }
        report.outcomes.push((label, out));
    }
    report
}

/// How long the oracle run of a mutant may take: a mutant may loop
/// forever (a deleted counter step), which is no finding.
const MUTANT_ORACLE_TIMEOUT: Duration = Duration::from_secs(5);

/// Checks a mutated source: `nlc parse`, `nlc check`, and when it
/// compiles, `nlc build` through both backends and `nlc run` under every
/// configuration. Only crashes, timeouts and failures to start are
/// findings: what a mutant prints may be undefined, so outputs are not
/// compared. The oracle runs first with a short timeout; a mutant it
/// cannot finish loops on its own and is not run further.
pub fn check_mutant(nlc: &Nlc, file: &Path) -> Report {
    let cwd = file.parent().unwrap_or(Path::new("."));
    let name = Path::new(file.file_name().unwrap_or_default());
    let mut report = Report::default();
    let mut compiled = false;
    for (label, args) in [("parse", vec!["parse"]), ("check", vec!["check"])] {
        let out = nlc.run(&args, name, cwd);
        report.findings.extend(out.own_finding(label, nlc.timeout));
        compiled = label == "check" && out.status == Status::Exited(0);
        report.outcomes.push((label.to_string(), out));
    }
    if !compiled {
        return report;
    }
    for (l, a) in [("build-lua", vec!["build", "--emit=lua"]), ("build-bc", vec!["build", "--emit=bc", "-o", "mutant.bc"])] {
        let o = nlc.run(&a, name, cwd);
        report.findings.extend(o.own_finding(l, nlc.timeout));
        report.outcomes.push((l.to_string(), o));
    }
    let short = Nlc { path: nlc.path.clone(), timeout: nlc.timeout.min(MUTANT_ORACLE_TIMEOUT) };
    for (i, c) in configs(false).iter().enumerate() {
        let label = c.label();
        let out = if i == 0 { short.run(&c.args(), name, cwd) } else { nlc.run(&c.args(), name, cwd) };
        if i == 0 && out.status == Status::Timeout {
            report.outcomes.push((label, out));
            break;
        }
        report.findings.extend(out.own_finding(&label, nlc.timeout));
        report.outcomes.push((label, out));
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_combination_once() {
        let all = configs(false);
        assert_eq!(all.len(), 24);
        assert_eq!(all.first(), Some(&ORACLE));
        let labels: std::collections::HashSet<String> = all.iter().map(Config::label).collect();
        assert_eq!(labels.len(), 24);
        assert_eq!(configs(true).len(), 12);
        assert!(configs(true).iter().all(|c| c.checked));
    }

    #[test]
    fn error_lines() {
        assert_eq!(first_error(b"\nprog.nl:3:4: error: bad thing [code]\nmore"), "error: bad thing [code]");
        assert_eq!(first_error(b"nlc: prog.nl:9: attempt to call nil"), "attempt to call nil");
        assert_eq!(first_difference(b"a\nb\n", b"a\nc\n"), "line 2: oracle \"b\" vs \"c\"");
    }

    #[test]
    fn crash_detection() {
        let o = |status, err: &str| Outcome { status, stdout: Vec::new(), stderr: err.as_bytes().to_vec() };
        assert!(!o(Status::Exited(1), "x.nl:1:1: error: nope").crashed());
        assert!(o(Status::Exited(101), "").crashed());
        assert!(o(Status::Signal, "").crashed());
        assert!(o(Status::Exited(1), "thread 'main' panicked at src/x.rs").crashed());
        assert!(o(Status::Exited(1), "x.nl:1:1: error: boom [internal-error]").crashed());
        assert!(o(Status::Exited(1), "LuaJIT ASSERT lj_record.c:177: rec_check_slots").crashed());
        let t = Duration::from_secs(1);
        assert_eq!(o(Status::Signal, "LuaJIT ASSERT lj_record.c:177").own_finding("x", t).map(|f| f.kind), Some(Kind::Crash));
        assert_eq!(o(Status::Timeout, "").own_finding("x", t).map(|f| f.kind), Some(Kind::Timeout));
        assert_eq!(o(Status::NotStarted("no such file".into()), "").own_finding("x", t).map(|f| f.kind), Some(Kind::NotStarted));
        assert!(o(Status::Exited(1), "x.nl:2: attempt to call nil").own_finding("x", t).is_none());
    }
}
