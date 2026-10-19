//! `nl-fuzz`, the differential and robustness fuzzer of `nlc` (the crate
//! README has more).
//!
//! ```text
//! nl-fuzz [--nlc <path>] [--seed N] [--count N] [--mutate] [--mutants N]
//!         [--keep-passing] [--jobs N] [--timeout SECONDS] [--out DIR]
//!         [--no-minimize]
//! nl-fuzz --print SEED          print the program of a seed
//! nl-fuzz --print-mutant SEED K print mutant K of that program
//! ```
//!
//! By default `nlc` is the one next to this binary (`target/debug/nlc`
//! beside `target/debug/nl-fuzz`), findings go to `target/fuzz/findings/`,
//! the first seed comes from the clock and is printed, 200 programs run
//! on every core, and each finding is minimized. The exit status is 0
//! when nothing was found, 1 when something was, and 2 for a usage error.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use nl_fuzz::campaign::{self, Options};

const USAGE: &str = "usage: nl-fuzz [--nlc <path>] [--seed N] [--count N] [--mutate] [--mutants N] [--keep-passing]
               [--jobs N] [--timeout SECONDS] [--out DIR] [--no-minimize]
       nl-fuzz --print SEED
       nl-fuzz --print-mutant SEED K

  --nlc <path>       the nlc binary (default: next to nl-fuzz)
  --seed N           the seed of the first program (default: from the clock)
  --count N          how many programs (default: 200)
  --mutate           also fuzz nlc parse/check/build with mutants of each program
  --mutants N        mutants per program with --mutate (default: 4)
  --keep-passing     save every program under <out>/passing/, not only findings
  --jobs N           parallel workers (default: the number of cores)
  --timeout SECONDS  per nlc run (default: 20)
  --out DIR          where findings go (default: target/fuzz)
  --no-minimize      save findings as found, without shrinking them";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse(&args) {
        Ok(Cmd::Print(seed)) => {
            print!("{}", nl_fuzz::generator::generate(seed).source);
            ExitCode::SUCCESS
        }
        Ok(Cmd::PrintMutant(seed, k)) => {
            let src = nl_fuzz::generator::generate(seed).source;
            let m = nl_fuzz::mutate::mutate(&src, campaign::mutant_seed(seed, k));
            use std::io::Write as _;
            let _ = std::io::stdout().write_all(&m);
            ExitCode::SUCCESS
        }
        Ok(Cmd::Run(opts)) => run(&opts),
        Err(msg) => {
            eprintln!("nl-fuzz: {msg}\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

enum Cmd {
    Print(u64),
    PrintMutant(u64, u64),
    Run(Options),
}

fn parse(args: &[String]) -> Result<Cmd, String> {
    let exe_dir = std::env::current_exe().ok().and_then(|p| p.parent().map(PathBuf::from));
    let mut opts = Options {
        nlc: exe_dir.as_ref().map(|d| d.join(format!("nlc{}", std::env::consts::EXE_SUFFIX))).unwrap_or_else(|| PathBuf::from("nlc")),
        seed: clock_seed(),
        count: 200,
        mutate: false,
        mutants: 4,
        keep_passing: false,
        out: default_out(exe_dir.as_deref()),
        jobs: std::thread::available_parallelism().map_or(4, |n| n.get()),
        timeout: Duration::from_secs(20),
        minimize: true,
    };
    let mut it = args.iter();
    let num = |v: Option<&String>, what: &str| -> Result<u64, String> { v.ok_or(format!("{what} takes a number"))?.parse::<u64>().map_err(|_| format!("{what} takes a number")) };
    while let Some(a) = it.next() {
        match a.as_str() {
            "--nlc" => opts.nlc = PathBuf::from(it.next().ok_or("--nlc takes a path")?),
            "--seed" => opts.seed = num(it.next(), "--seed")?,
            "--count" => opts.count = num(it.next(), "--count")?,
            "--mutate" => opts.mutate = true,
            "--mutants" => opts.mutants = num(it.next(), "--mutants")?,
            "--keep-passing" => opts.keep_passing = true,
            "--jobs" => opts.jobs = usize::try_from(num(it.next(), "--jobs")?).unwrap_or(1).max(1),
            "--timeout" => opts.timeout = Duration::from_secs(num(it.next(), "--timeout")?.max(1)),
            "--out" => opts.out = PathBuf::from(it.next().ok_or("--out takes a directory")?),
            "--no-minimize" => opts.minimize = false,
            "--print" => return Ok(Cmd::Print(num(it.next(), "--print")?)),
            "--print-mutant" => {
                let s = num(it.next(), "--print-mutant")?;
                let k = num(it.next(), "--print-mutant")?;
                return Ok(Cmd::PrintMutant(s, k));
            }
            "-h" | "--help" => return Err("help".into()),
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    Ok(Cmd::Run(opts))
}

/// A seed from the clock, so that every campaign explores new programs;
/// it is printed, so that the campaign can be repeated.
fn clock_seed() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_secs() % 1_000_000 * 1000)
}

/// `target/fuzz` beside the profile directory the binary is in, or in
/// the working directory.
fn default_out(exe_dir: Option<&std::path::Path>) -> PathBuf {
    match exe_dir {
        Some(d) if d.file_name().is_some_and(|n| n == "debug" || n == "release") => d.parent().map_or_else(|| PathBuf::from("target/fuzz"), |t| t.join("fuzz")),
        _ => PathBuf::from("target/fuzz"),
    }
}

fn run(opts: &Options) -> ExitCode {
    if !opts.nlc.exists() {
        eprintln!("nl-fuzz: no nlc at {}; build it (`cargo build -p nlc`) or pass --nlc", opts.nlc.display());
        return ExitCode::from(2);
    }
    eprintln!(
        "nl-fuzz: seeds {}..{}{}, {} jobs, nlc {}, findings in {}",
        opts.seed,
        opts.seed.wrapping_add(opts.count),
        if opts.mutate { format!(", {} mutants each", opts.mutants) } else { String::new() },
        opts.jobs,
        opts.nlc.display(),
        opts.out.join("findings").display()
    );
    let log = |line: &str| println!("{line}");
    let s = campaign::run(opts, &log);
    eprintln!(
        "nl-fuzz: {} programs, {} mutants, {} nlc runs in {:.1}s; {} with findings",
        s.programs,
        s.mutants,
        s.runs,
        s.elapsed.as_secs_f64(),
        s.records.len()
    );
    if s.records.is_empty() { ExitCode::SUCCESS } else { ExitCode::FAILURE }
}
