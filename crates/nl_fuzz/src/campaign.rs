//! A fuzzing campaign: a range of seeds, each one a generated program run
//! through every configuration (and, with mutation, a few broken copies
//! of it run through `nlc parse`, `check` and `build`, and through every
//! configuration when they compile), on several threads.
//!
//! Program `i` of a campaign has the seed `seed + i`, so `--seed S
//! --count 1` reproduces program `S`; mutant `k` of it is
//! `<seed>-m<k>`. Every finding is saved under
//! `<out>/findings/<id>/`: the source, `summary.txt` with a one-line
//! description per finding and the commands that reproduce it, stdout
//! and stderr of every run, and, unless minimization is off, `min.nl`
//! with the outputs that show the finding on it. Only the first two
//! findings of each signature are minimized (for a mismatch, the
//! configuration that disagreed; otherwise the kind, the command and the
//! message without digits), and at most [`MAX_MINIMIZED`] per campaign;
//! the rest are saved as found.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::generator;
use crate::minimize::minimize;
use crate::mutate::mutate;
use crate::runner::{self, Config, Finding, Kind, Nlc, Outcome, Report, Status};

#[derive(Clone, Debug)]
pub struct Options {
    pub nlc: PathBuf,
    /// The seed of the first program.
    pub seed: u64,
    /// How many programs.
    pub count: u64,
    /// Also run mutants of each program.
    pub mutate: bool,
    /// Mutants per program.
    pub mutants: u64,
    /// Save every generated program, not only those with findings.
    pub keep_passing: bool,
    /// The directory for findings and work files (`target/fuzz`).
    pub out: PathBuf,
    pub jobs: usize,
    pub timeout: Duration,
    /// Minimize each finding (costs runs only when something is found).
    pub minimize: bool,
}

/// The most findings one campaign minimizes: a minimization costs up to
/// a few thousand runs.
pub const MAX_MINIMIZED: u32 = 25;

/// One saved finding.
#[derive(Clone, Debug)]
pub struct Record {
    /// The seed, or `<seed>-m<k>` for a mutant.
    pub id: String,
    pub findings: Vec<Finding>,
    pub dir: PathBuf,
}

impl Record {
    /// The one-line summary printed for it.
    pub fn line(&self) -> String {
        let first = self.findings.first();
        let more = if self.findings.len() > 1 { format!(" (+{} more)", self.findings.len() - 1) } else { String::new() };
        match first {
            Some(f) => format!("{}: {} [{}] {}{more} -> {}", self.id, f.kind, f.run, f.detail, self.dir.display()),
            None => format!("{}: no finding", self.id),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Summary {
    pub programs: u64,
    pub mutants: u64,
    pub runs: u64,
    pub records: Vec<Record>,
    pub elapsed: Duration,
}

/// Runs the campaign; `log` gets one line per finding as it is found.
pub fn run(opts: &Options, log: &(dyn Fn(&str) + Sync)) -> Summary {
    let start = Instant::now();
    let nlc = Nlc { path: opts.nlc.clone(), timeout: opts.timeout };
    let next = AtomicU64::new(0);
    let programs = AtomicU64::new(0);
    let mutants = AtomicU64::new(0);
    let runs = AtomicU64::new(0);
    let records: Mutex<Vec<Record>> = Mutex::new(Vec::new());
    let seen: Seen = Mutex::new(std::collections::HashMap::new());
    let jobs = opts.jobs.max(1);
    std::thread::scope(|scope| {
        for w in 0..jobs {
            let (nlc, next, programs, mutants, runs, records, seen) = (&nlc, &next, &programs, &mutants, &runs, &records, &seen);
            scope.spawn(move || {
                let work = opts.out.join("work").join(w.to_string());
                if std::fs::create_dir_all(&work).is_err() {
                    log(&format!("cannot create {}", work.display()));
                    return;
                }
                loop {
                    let i = next.fetch_add(1, Ordering::SeqCst);
                    if i >= opts.count {
                        break;
                    }
                    let seed = opts.seed.wrapping_add(i);
                    let counters = Counters { programs, mutants, runs, seen };
                    let found = one_seed(opts, nlc, &work, seed, &counters);
                    // Each index is claimed once, so each mark is logged once.
                    if (i + 1) % 500 == 0 {
                        log(&format!("progress: program {} of {} done, {} mutants so far", i + 1, opts.count, mutants.load(Ordering::Relaxed)));
                    }
                    for r in found {
                        log(&r.line());
                        if let Ok(mut all) = records.lock() {
                            all.push(r);
                        }
                    }
                }
            });
        }
    });
    let mut records = records.into_inner().unwrap_or_default();
    records.sort_by(|a, b| a.id.cmp(&b.id));
    Summary { programs: programs.into_inner(), mutants: mutants.into_inner(), runs: runs.into_inner(), records, elapsed: start.elapsed() }
}

/// How often each kind of finding has been minimized, by signature.
type Seen = Mutex<std::collections::HashMap<String, u32>>;

/// What the workers share.
struct Counters<'a> {
    programs: &'a AtomicU64,
    mutants: &'a AtomicU64,
    runs: &'a AtomicU64,
    seen: &'a Seen,
}

/// The same finding as one already seen. A mismatch is known by the
/// configuration that disagreed (its detail holds the values printed,
/// different every time); anything else by its kind, its run without the
/// option flags, and its detail without digits (thread ids, line numbers,
/// addresses).
fn signature(f: &Finding) -> String {
    if f.kind == Kind::Mismatch {
        return format!("{} {}", f.kind, f.run);
    }
    let run = f.run.split('-').next().unwrap_or("");
    let detail: String = f.detail.chars().filter(|c| !c.is_ascii_digit()).collect();
    format!("{} {run} {detail}", f.kind)
}

/// Whether a finding should be minimized: the first two of each
/// signature are, the rest are only saved, so that one frequent bug does
/// not keep the campaign minimizing.
fn worth_minimizing(seen: &Seen, f: Option<&Finding>) -> bool {
    let Some(f) = f else { return false };
    let Ok(mut map) = seen.lock() else { return true };
    // The total is kept under an empty key.
    if map.get("").copied().unwrap_or(0) >= MAX_MINIMIZED {
        return false;
    }
    let n = map.entry(signature(f)).or_insert(0);
    *n += 1;
    if *n > 2 {
        return false;
    }
    *map.entry(String::new()).or_insert(0) += 1;
    true
}

/// The program of one seed and its mutants. Returns the saved findings.
fn one_seed(opts: &Options, nlc: &Nlc, work: &Path, seed: u64, c: &Counters<'_>) -> Vec<Record> {
    let (programs, mutants, runs) = (c.programs, c.mutants, c.runs);
    let mut out = Vec::new();
    let program = generator::generate(seed);
    let file = work.join("program.nl");
    if std::fs::write(&file, &program.source).is_err() {
        return out;
    }
    let configs = runner::configs(program.checked_only);
    let report = runner::check_program(nlc, &file, program.checked_only, &configs);
    programs.fetch_add(1, Ordering::Relaxed);
    runs.fetch_add(report.outcomes.len() as u64, Ordering::Relaxed);
    if !report.findings.is_empty() {
        let dir = opts.out.join("findings").join(seed.to_string());
        save(&dir, "program.nl", program.source.as_bytes(), &report, &program_commands(&configs, &report));
        if opts.minimize && worth_minimizing(c.seen, report.findings.first()) {
            minimize_program(nlc, work, &dir, &program, &report);
        }
        out.push(Record { id: seed.to_string(), findings: report.findings.clone(), dir });
    } else if opts.keep_passing {
        let dir = opts.out.join("passing");
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join(format!("{seed}.nl")), &program.source);
    }
    if opts.mutate {
        for k in 0..opts.mutants {
            let m = mutate(&program.source, mutant_seed(seed, k));
            let mfile = work.join("mutant.nl");
            if std::fs::write(&mfile, &m).is_err() {
                continue;
            }
            let report = runner::check_mutant(nlc, &mfile);
            mutants.fetch_add(1, Ordering::Relaxed);
            runs.fetch_add(report.outcomes.len() as u64, Ordering::Relaxed);
            if report.findings.is_empty() {
                continue;
            }
            let id = format!("{seed}-m{k}");
            let dir = opts.out.join("findings").join(&id);
            let mut cmds = "nlc parse mutant.nl\nnlc check mutant.nl\nnlc build --emit=lua mutant.nl\nnlc build --emit=bc -o mutant.bc mutant.nl\n".to_string();
            for f in &report.findings {
                if let Some(c) = config_of(&f.run) {
                    cmds.push_str(&format!("nlc {} mutant.nl\n", c.args().join(" ")));
                }
            }
            save(&dir, "mutant.nl", &m, &report, &cmds);
            let _ = std::fs::write(dir.join("original.nl"), &program.source);
            if opts.minimize && worth_minimizing(c.seen, report.findings.first()) {
                minimize_mutant(nlc, work, &dir, &m, &report);
            }
            out.push(Record { id, findings: report.findings.clone(), dir });
        }
    }
    out
}

/// The seed of mutant `k` of a program: mixed, so that neighbouring
/// programs do not get related mutations.
pub fn mutant_seed(seed: u64, k: u64) -> u64 {
    seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ k.wrapping_add(1).wrapping_mul(0xD1B5_4A32_D192_ED03)
}

fn program_commands(configs: &[Config], report: &Report) -> String {
    let mut s = String::from("# run from this directory, with NL_PATH unset\nnlc check program.nl\n");
    for c in configs {
        if report.findings.iter().any(|f| f.run == c.label()) || *c == runner::ORACLE {
            s.push_str(&format!("nlc {} program.nl\n", c.args().join(" ")));
        }
    }
    s
}

/// Writes the source, the summary and every output into `dir`.
fn save(dir: &Path, name: &str, source: &[u8], report: &Report, commands: &str) {
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(dir.join(name), source);
    let mut summary = String::new();
    for f in &report.findings {
        summary.push_str(&format!("{} [{}] {}\n", f.kind, f.run, f.detail));
    }
    summary.push('\n');
    summary.push_str(commands);
    let _ = std::fs::write(dir.join("summary.txt"), summary);
    for (label, o) in &report.outcomes {
        write_outcome(dir, label, o);
    }
}

fn write_outcome(dir: &Path, label: &str, o: &Outcome) {
    let _ = std::fs::write(dir.join(format!("{label}.stdout")), &o.stdout);
    let mut err = o.stderr.clone();
    err.extend_from_slice(format!("\n[{}]\n", o.status).as_bytes());
    let _ = std::fs::write(dir.join(format!("{label}.stderr")), err);
}

/// The configuration of a run label.
fn config_of(label: &str) -> Option<Config> {
    runner::configs(false).into_iter().find(|c| c.label() == label)
}

/// The arguments of the command behind a run label.
fn args_of(label: &str) -> Option<Vec<&'static str>> {
    match label {
        "check" => Some(vec!["check"]),
        "parse" => Some(vec!["parse"]),
        "build-lua" => Some(vec!["build", "--emit=lua"]),
        "build-bc" => Some(vec!["build", "--emit=bc", "-o", "mutant.bc"]),
        l => config_of(l).map(|c| c.args()),
    }
}

/// Shrinks a program while its first finding stays the same, and saves
/// `min.nl` with the outputs that show it.
fn minimize_program(nlc: &Nlc, work: &Path, dir: &Path, program: &generator::Program, report: &Report) {
    let Some(f) = report.findings.first().cloned() else { return };
    // A timeout costs the whole timeout per test; a failure to start is
    // not about the program.
    if f.kind == Kind::Timeout || f.kind == Kind::NotStarted {
        return;
    }
    let file = work.join("min.nl");
    let name = Path::new("min.nl");
    let oracle_args = runner::ORACLE.args();
    let original_error = report.outcomes.iter().find(|(l, _)| *l == f.run).map(|(_, o)| runner::first_error(&o.stderr)).unwrap_or_default();
    let Some(run_args) = args_of(&f.run) else { return };
    // A compiler panic shows in `nlc build` already, which is faster than
    // a run; the options that change code generation are kept.
    let compile_panic = f.kind == Kind::Crash && f.detail.contains("panicked");
    let args: Vec<&str> = match config_of(&f.run) {
        Some(c) if compile_panic => {
            let mut a = vec!["build", if c.backend == runner::Backend::Bc { "--emit=bc" } else { "--emit=lua" }, "-o", "min.out"];
            if !c.checked {
                a.push("--unchecked");
            }
            if !c.static_resolution {
                a.push("--no-static-resolution");
            }
            a
        }
        _ => run_args.clone(),
    };
    let still = |src: &str| -> bool {
        if std::fs::write(&file, src).is_err() {
            return false;
        }
        match f.kind {
            Kind::Rejected => {
                let o = nlc.run(&args, name, work);
                o.status == Status::Exited(1) && !o.crashed() && runner::first_error(&o.stderr) == original_error
            }
            Kind::OracleFailed => {
                let o = nlc.run(&args, name, work);
                matches!(o.status, Status::Exited(1)) && !o.crashed() && runner::first_error(&o.stderr) == original_error
            }
            Kind::Mismatch => {
                let o = nlc.run(&oracle_args, name, work);
                if o.status != Status::Exited(0) {
                    return false;
                }
                let x = nlc.run(&args, name, work);
                !x.crashed() && x.status != Status::Timeout && (x.stdout != o.stdout || x.status != o.status)
            }
            Kind::Crash => nlc.run(&args, name, work).crashed(),
            Kind::Timeout | Kind::NotStarted => false,
        }
    };
    // Runs of a whole program cost more than compiles.
    let budget = if f.kind == Kind::Mismatch || f.kind == Kind::OracleFailed { 600 } else { 3000 };
    let min = minimize(&program.source, budget, still);
    let _ = std::fs::write(dir.join("min.nl"), &min);
    if std::fs::write(&file, &min).is_ok() {
        let o = nlc.run(&oracle_args, name, work);
        write_outcome(dir, &format!("min-{}", runner::ORACLE.label()), &o);
        if f.run != runner::ORACLE.label() {
            let x = nlc.run(&run_args, name, work);
            write_outcome(dir, &format!("min-{}", f.run), &x);
        }
    }
}

/// Shrinks a mutant while the same command still crashes.
fn minimize_mutant(nlc: &Nlc, work: &Path, dir: &Path, source: &[u8], report: &Report) {
    let Some(f) = report.findings.iter().find(|f| f.kind == Kind::Crash).cloned() else { return };
    let crashes = |o: &Outcome| o.crashed();
    let Some(args) = args_of(&f.run) else { return };
    // Lines of a mutant may not be valid UTF-8; such a mutant is kept
    // whole.
    let Ok(text) = std::str::from_utf8(source) else { return };
    let file = work.join("min.nl");
    let name = Path::new("min.nl");
    let still = |src: &str| -> bool { std::fs::write(&file, src).is_ok() && crashes(&nlc.run(&args, name, work)) };
    let min = minimize(text, 3000, still);
    let _ = std::fs::write(dir.join("min.nl"), &min);
}
