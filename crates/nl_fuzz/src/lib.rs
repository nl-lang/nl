//! Differential and robustness fuzzing of `nlc`: generated programs
//! through both backends, and broken programs for panics.
//!
//! - [`generator`] turns a seed into a well-formed NL program whose output
//!   the language defines exactly, and which prints a trace of what it
//!   computes.
//! - [`runner`] runs such a program through the `nlc` binary under every
//!   combination of options that must agree (`--backend=lua|bc`, checked
//!   and `--unchecked`, static resolution on and off, `--jit=on|off|hot`)
//!   and compares stdout and the exit status with the Lua backend's, the
//!   oracle.
//! - [`mutate`] breaks generated sources (byte flips, truncation, token
//!   deletions and duplications, stray operators and brackets) for
//!   `nlc parse`, `nlc check` and `nlc build`, and for those that compile
//!   `nlc run` under every combination, where nothing may crash or hang.
//! - [`campaign`] drives both over a range of seeds on several threads,
//!   saves every finding under `target/fuzz/findings/<seed>/` and
//!   minimizes it ([`minimize`]).
//!
//! The binary `nl-fuzz` is the command-line front end (crate README);
//! `crates/nlc/tests/differential.rs` runs a small fixed campaign as a
//! test. A program is reproduced from its seed alone: the generator uses
//! its own SplitMix64 ([`rng`]), so the text does not depend on a crate
//! version.

pub mod campaign;
pub mod generator;
pub mod minimize;
pub mod mutate;
pub mod rng;
pub mod runner;
