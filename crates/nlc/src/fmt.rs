//! `nlc fmt`: the formatter on the command line.
//!
//! ```text
//! nlc fmt [--check] [--stdout] [--fix] <file>...
//! ```
//!
//! Each file is formatted with `nl_syntax::format` (`format_decl` for a
//! `.d.nl` file) and written back in place; a file that did not
//! change is left alone, and nothing is printed for it. `--check` writes
//! nothing and prints `path: not formatted` for each file that would change;
//! `--stdout` prints the formatted text instead of writing it, with a `#
//! path` line before each file when there are several, as `nlc parse` does.
//!
//! A `"…"` string continued on the next line with a backslash
//! is reported on stderr as a warning and left alone; `--fix`
//! rewrites it as a `"""` heredoc with the same value (the docs of
//! `nl_syntax::format`), except inside an interpolation, where a heredoc
//! cannot be written and the warning says so.
//!
//! A file with syntax errors is not formatted: its diagnostics are reported
//! as `nlc parse` reports them (`path:line:column: error: message`) and the
//! file is left alone.
//!
//! The exit status is 1 when a file could not be read, parsed or written,
//! or, with `--check`, when a file would change; warnings do not affect
//! it. A usage error exits with 2.

use std::io::Write as _;
use std::process::ExitCode;

use nl_syntax::{FormatOptions, LineIndex, Severity, TextSize, format, format_decl, source_from_bytes};

struct Args {
    check: bool,
    stdout: bool,
    fix: bool,
    files: Vec<String>,
}

fn parse_args(args: &[String], usage: &str) -> Result<Args, ExitCode> {
    let mut a = Args { check: false, stdout: false, fix: false, files: Vec::new() };
    for arg in args {
        match arg.as_str() {
            "--check" => a.check = true,
            "--stdout" => a.stdout = true,
            "--fix" => a.fix = true,
            s if s.starts_with('-') => {
                eprintln!("nlc fmt: unknown option `{s}`\n\n{usage}");
                return Err(ExitCode::from(2));
            }
            s => a.files.push(s.to_string()),
        }
    }
    if a.check && a.stdout {
        eprintln!("nlc fmt: `--check` and `--stdout` cannot be combined\n\n{usage}");
        return Err(ExitCode::from(2));
    }
    if a.files.is_empty() {
        eprintln!("nlc fmt: no input files\n\n{usage}");
        return Err(ExitCode::from(2));
    }
    Ok(a)
}

/// `nlc fmt [--check] [--stdout] [--fix] <file>...`.
pub fn cmd_fmt(args: &[String], usage: &str) -> ExitCode {
    let a = match parse_args(args, usage) {
        Ok(a) => a,
        Err(code) => return code,
    };
    let mut failed = false;
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for path in &a.files {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("{path}: error: cannot read the file: {e}");
                failed = true;
                continue;
            }
        };
        let source = match source_from_bytes(&bytes) {
            Ok(s) => s,
            Err(d) => {
                // The valid prefix locates the bad byte (as `nlc parse`).
                let start = usize::from(d.range.start());
                let prefix = String::from_utf8_lossy(bytes.get(..start).unwrap_or(&[]));
                report(path, &prefix, TextSize::of(&*prefix), Severity::Error, &d.message);
                failed = true;
                continue;
            }
        };
        let options = FormatOptions { fix: a.fix };
        let result = if path.ends_with(".d.nl") { format_decl(source, options) } else { format(source, options) };
        let formatted = match result {
            Ok(f) => f,
            Err(diagnostics) => {
                for d in &diagnostics {
                    report(path, source, d.range.start(), d.severity, &d.message);
                }
                failed = true;
                continue;
            }
        };
        for c in &formatted.continued {
            let message = if c.fixed {
                continue;
            } else if a.fix && c.in_interpolation {
                "a \"…\" string continued with a backslash inside an interpolation; a heredoc cannot be written there, so bind the string to a variable first"
            } else {
                "a \"…\" string continued with a backslash; write a heredoc (nlc fmt --fix rewrites it)"
            };
            report(path, source, c.range.start(), Severity::Warning, message);
        }
        let changed = formatted.text != source;
        if a.stdout {
            if a.files.len() > 1 {
                let _ = writeln!(out, "# {path}");
            }
            let _ = out.write_all(formatted.text.as_bytes());
        } else if a.check {
            if changed {
                let _ = writeln!(out, "{path}: not formatted");
                failed = true;
            }
        } else if changed {
            if let Err(e) = std::fs::write(path, formatted.text.as_bytes()) {
                eprintln!("{path}: error: cannot write the file: {e}");
                failed = true;
            }
        }
    }
    let _ = out.flush();
    if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}

/// Prints `path:line:col: severity: message`, as `nlc parse` does.
fn report(path: &str, text: &str, offset: TextSize, severity: Severity, message: &str) {
    let lc = LineIndex::new(text).line_col(text, offset);
    let severity = match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    };
    eprintln!("{path}:{}:{}: {severity}: {message}", lc.line, lc.col);
}
