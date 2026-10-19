//! A small fixed differential-fuzzing campaign, with the generator and
//! runner of `crates/nl_fuzz`.
//!
//! Forty generated programs, from a fixed seed, run through `nlc run`
//! under every combination of options that must agree: both backends,
//! checked and `--unchecked`, static resolution on and off, and the JIT
//! on, off and forced (crates/nl_fuzz/src/runner.rs). Their stdout and exit
//! status must equal the Lua backend's, `nlc check` must accept them, and
//! nothing may crash (a LuaJIT assertion included) or hang. One mutant of
//! each program then goes through `nlc parse`, `nlc check` and
//! `nlc build`, and through `nlc run` under every configuration when it
//! compiles, which must answer without crashing or hanging (as
//! `compile_robustness.rs` checks the compile through the library).
//!
//! The programs are the same on every machine: a seed fixes the text. A
//! failure prints one line per finding; the program and every output are
//! saved under `$CARGO_TARGET_TMPDIR/differential/findings/<seed>/`, and
//! `nl-fuzz --seed <seed> --count 1` reproduces it (crates/nl_fuzz/README.md).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::path::PathBuf;
use std::time::Duration;

use nl_fuzz::campaign::{self, Options};

/// The first seed. Changing it changes every program of the test.
const SEED: u64 = 20_261_012;

#[test]
fn generated_programs_agree() {
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("differential");
    let _ = std::fs::remove_dir_all(&out);
    let opts = Options {
        nlc: PathBuf::from(env!("CARGO_BIN_EXE_nlc")),
        seed: SEED,
        count: 40,
        mutate: true,
        mutants: 1,
        keep_passing: false,
        out: out.clone(),
        jobs: std::thread::available_parallelism().map_or(4, |n| n.get()),
        // Generous: a debug build on a loaded machine is slow, and a
        // timeout is a finding.
        timeout: Duration::from_secs(60),
        // A failure should be reported quickly; `nl-fuzz` minimizes.
        minimize: false,
    };
    let summary = campaign::run(&opts, &|line| eprintln!("{line}"));
    assert_eq!(summary.programs, 40);
    assert_eq!(summary.mutants, 40);
    let lines: Vec<String> = summary.records.iter().map(|r| r.line()).collect();
    assert!(lines.is_empty(), "{} finding(s), saved under {}:\n{}", lines.len(), out.join("findings").display(), lines.join("\n"));
}
