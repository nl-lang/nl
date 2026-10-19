//! Regression programs of the compiler, in `tests/cases/` beside this file.
//!
//! The mechanics are those of the conformance suite (`conformance.rs`,
//! tests/run/README.md): every entry program (a `.nl` with a `.out`
//! beside it) runs as `nlc run <file>` in a fresh process, with the
//! working directory set to `tests/cases/`, `NL_PATH` unset and a timeout;
//! stdout must equal the `.out` file byte for byte and the exit status
//! must be 0. Every program in `reject/` must fail to compile (`nlc build
//! --emit=lua`) with an error on the line after its `# expect-error:`
//! comment. Files without a `.out` (such as those in `lib/`) are only
//! imported.
//!
//! Both build modes are exercised: an entry program also runs
//! with `--unchecked` and must print the same, unless it has a line
//! `# checked-only` (its output depends on the runtime checks); a rejected
//! program must be rejected in both modes.
//!
//! An entry program runs on the default backend (Lua source). A line
//! `# backends: lua,bc` names the backends it runs on instead (`lua`,
//! `bc`, as `nlc run --backend=`), each in both build modes, and the
//! output must be the same `.out` on every one (the trace programs use
//! it).
//!
//! `NL_CASES_FILTER=a,b` runs only the programs whose name contains one of
//! these substrings.

use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn cases_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/cases")
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

fn first_line(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    text.lines().find(|l| !l.trim().is_empty()).unwrap_or("").chars().take(200).collect()
}

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

fn nl_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut paths: Vec<PathBuf> = rd.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "nl")).collect();
    paths.sort();
    paths
}

/// The backends of a `# backends: a,b` line; `[None]` without one (the
/// default backend).
fn backends(src: &str) -> Vec<Option<String>> {
    match src.lines().find_map(|l| l.trim().strip_prefix("# backends:")) {
        Some(list) => list.split(',').map(str::trim).filter(|b| !b.is_empty()).map(|b| Some(b.to_string())).collect(),
        None => vec![None],
    }
}

/// Runs one entry program in one mode, on one backend (`None`: the
/// default); `None` when it passes.
fn check_run(nlc: &Path, dir: &Path, rel: &Path, expected: &[u8], unchecked: bool, backend: Option<&str>) -> Option<String> {
    let mut cmd = Command::new(nlc);
    cmd.current_dir(dir).env_remove("NL_PATH").arg("run");
    if unchecked {
        cmd.arg("--unchecked");
    }
    if let Some(b) = backend {
        cmd.arg(format!("--backend={b}"));
    }
    cmd.arg(rel);
    let mode = match (unchecked, backend) {
        (false, None) => String::new(),
        (true, None) => " (--unchecked)".to_string(),
        (false, Some(b)) => format!(" (--backend={b})"),
        (true, Some(b)) => format!(" (--unchecked --backend={b})"),
    };
    match run_with_timeout(cmd, Duration::from_secs(30)) {
        Err(e) => Some(format!("{e}{mode}")),
        Ok((code, stdout, stderr)) => {
            if stdout == expected && code == Some(0) {
                None
            } else if code != Some(0) {
                let diff = if stdout != expected { format!(" [{}]", first_difference(expected, &stdout)) } else { String::new() };
                Some(format!("exit {:?}{mode}: {}{diff}", code, first_line(&stderr)))
            } else {
                Some(format!("{}{mode}", first_difference(expected, &stdout)))
            }
        }
    }
}

/// Compiles one rejected program in one mode; `None` when it is rejected
/// with an error on line `want`.
fn check_reject(nlc: &Path, dir: &Path, rel: &Path, want: usize, unchecked: bool) -> Option<String> {
    let mut cmd = Command::new(nlc);
    cmd.current_dir(dir).env_remove("NL_PATH").arg("build").arg("--emit=lua");
    if unchecked {
        cmd.arg("--unchecked");
    }
    cmd.arg(rel);
    let mode = if unchecked { " (--unchecked)" } else { "" };
    match run_with_timeout(cmd, Duration::from_secs(30)) {
        Err(e) => Some(format!("{e}{mode}")),
        Ok((code, _stdout, stderr)) => {
            let text = String::from_utf8_lossy(&stderr);
            let lines: Vec<(usize, String)> = text
                .lines()
                .filter(|l| l.contains(": error: "))
                .filter_map(|l| {
                    let mut parts = l.splitn(4, ':');
                    let _file = parts.next()?;
                    let line = parts.next()?.trim().parse::<usize>().ok()?;
                    Some((line, l.to_string()))
                })
                .collect();
            if code == Some(0) {
                Some(format!("compiled without error{mode} (expected one on line {want})"))
            } else if lines.iter().any(|(l, _)| *l == want) {
                None
            } else {
                let got = lines.first().map_or_else(|| first_line(&stderr), |(_, l)| l.clone());
                Some(format!("expected an error on line {want}{mode}; got: {got}"))
            }
        }
    }
}

#[test]
fn regression_cases() {
    let dir = cases_dir();
    let dir = std::fs::canonicalize(&dir).unwrap_or(dir);
    let nlc = PathBuf::from(env!("CARGO_BIN_EXE_nlc"));
    let filters: Vec<String> = std::env::var("NL_CASES_FILTER")
        .ok()
        .map(|f| f.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();
    let selected = |name: &str| filters.is_empty() || filters.iter().any(|f| name.contains(f.as_str()));

    let mut failures = Vec::new();
    let mut count = 0;
    for path in nl_files(&dir) {
        let out = path.with_extension("out");
        let Ok(expected) = std::fs::read(&out) else { continue };
        let rel = path.strip_prefix(&dir).unwrap_or(&path).to_path_buf();
        let name = rel.display().to_string();
        if !selected(&name) {
            continue;
        }
        count += 1;
        let src = std::fs::read_to_string(&path).unwrap_or_default();
        let checked_only = src.lines().any(|l| l.trim() == "# checked-only");
        for backend in backends(&src) {
            if let Some(why) = check_run(&nlc, &dir, &rel, &expected, false, backend.as_deref()) {
                failures.push(format!("{name}: {why}"));
            }
            if !checked_only {
                if let Some(why) = check_run(&nlc, &dir, &rel, &expected, true, backend.as_deref()) {
                    failures.push(format!("{name}: {why}"));
                }
            }
        }
    }
    for path in nl_files(&dir.join("reject")) {
        let rel = path.strip_prefix(&dir).unwrap_or(&path).to_path_buf();
        let name = rel.display().to_string();
        if !selected(&name) {
            continue;
        }
        count += 1;
        let src = std::fs::read_to_string(&path).unwrap_or_default();
        let Some(marker) = src.lines().position(|l| l.trim_start().starts_with("# expect-error:")) else {
            failures.push(format!("{name}: no `# expect-error:` comment"));
            continue;
        };
        for unchecked in [false, true] {
            if let Some(why) = check_reject(&nlc, &dir, &rel, marker + 2, unchecked) {
                failures.push(format!("{name}: {why}"));
            }
        }
    }
    assert!(count > 0 || !filters.is_empty(), "no regression programs found in {}", dir.display());
    assert!(failures.is_empty(), "{} of {count} regression programs failed:\n{}", failures.len(), failures.join("\n"));
}

/// The forms `try` compiles to (crates/nl_ir/src/lower/error.rs and
/// lift.rs): a `try` with `finally`, used as a value or left by `return`,
/// runs its closures with `rt.xpcall` and keeps the outcome in locals, and
/// no closure is made where a `try` runs. The program is
/// `tests/cases/lib/try_forms.nl`.
#[test]
fn try_forms() {
    let dir = cases_dir();
    let nlc = PathBuf::from(env!("CARGO_BIN_EXE_nlc"));
    let mut cmd = Command::new(&nlc);
    cmd.current_dir(&dir).env_remove("NL_PATH").args(["build", "--emit=lua", "lib/try_forms.nl"]);
    let result = run_with_timeout(cmd, Duration::from_secs(30));
    assert!(result.is_ok(), "{result:?}");
    let (code, stdout, stderr) = result.unwrap_or_default();
    assert_eq!(code, Some(0), "{}", String::from_utf8_lossy(&stderr));
    let lua = String::from_utf8_lossy(&stdout);
    assert_eq!(lua.matches("__rt.xpcall(").count(), 7, "{lua}");
    assert!(!lua.contains("__rt.try("), "{lua}");
    assert!(!lua.contains("xpcall(function"), "{lua}");
}

/// A short function compiles to the same code as
/// the `fn` it stands for: `tests/cases/lib/lambda_forms.nl` and
/// `lib/lambda_forms_fn.nl` are one program written both ways, line for
/// line, and their Lua differs only in the file name. Both backends
/// generate from the same IR, so the bytecode is the same too.
#[test]
fn lambda_same_code() {
    let dir = cases_dir();
    let nlc = PathBuf::from(env!("CARGO_BIN_EXE_nlc"));
    let mut outputs = Vec::new();
    for file in ["lib/lambda_forms.nl", "lib/lambda_forms_fn.nl"] {
        let mut cmd = Command::new(&nlc);
        cmd.current_dir(&dir).env_remove("NL_PATH").args(["build", "--emit=lua", file]);
        let result = run_with_timeout(cmd, Duration::from_secs(30));
        assert!(result.is_ok(), "{result:?}");
        let (code, stdout, stderr) = result.unwrap_or_default();
        assert_eq!(code, Some(0), "{file}: {}", String::from_utf8_lossy(&stderr));
        outputs.push(String::from_utf8_lossy(&stdout).replace(file, "FILE"));
        let mut cmd = Command::new(&nlc);
        cmd.current_dir(&dir).env_remove("NL_PATH").args(["run", "--backend=bc", file]);
        let result = run_with_timeout(cmd, Duration::from_secs(30));
        assert!(result.is_ok(), "{result:?}");
        let (code, stdout, stderr) = result.unwrap_or_default();
        assert_eq!(code, Some(0), "{file}: {}", String::from_utf8_lossy(&stderr));
        assert_eq!(String::from_utf8_lossy(&stdout), "2\t6\ttrue\t1\t66\t1\n", "{file}");
    }
    assert_eq!(outputs.first(), outputs.get(1));
}

/// The nil-safe operators (nl_ir lower/nilsafe.rs): a chain
/// is a local, a test and a jump, with the steps after a test lowered as
/// they are after `.` (a declared struct's field read directly); `??` and
/// `??=` likewise; no closure and no runtime call of their own. A member
/// a `?.(…)` tests is read once and that value is called (`rt.mapply`
/// applies the member-call rule to it; with a declared struct the call is
/// direct). The program is `tests/cases/lib/nil_safe_forms.nl`; it also
/// runs on both backends.
#[test]
fn nil_safe_lowering() {
    let dir = cases_dir();
    let nlc = PathBuf::from(env!("CARGO_BIN_EXE_nlc"));
    let file = "lib/nil_safe_forms.nl";
    let mut cmd = Command::new(&nlc);
    cmd.current_dir(&dir).env_remove("NL_PATH").args(["build", "--emit=lua", file]);
    let result = run_with_timeout(cmd, Duration::from_secs(30));
    assert!(result.is_ok(), "{result:?}");
    let (code, stdout, stderr) = result.unwrap_or_default();
    assert_eq!(code, Some(0), "{}", String::from_utf8_lossy(&stderr));
    let lua = String::from_utf8_lossy(&stdout);
    // The main chunk, the type of `Point.x`, the ten `fn` declarations
    // and the `fn` literal of the last line make a function, and nothing
    // else does.
    assert_eq!(lua.matches("function(").count(), 13, "{lua}");
    for wanted in [
        "if a ~= nil then local s = (__rt.member.c((__rt.mget(a, \"b\")), d)); if s ~= nil then",
        "if p ~= nil then v_2 = p.x; end",
        "if f ~= nil then v_3 = (f(x)); end",
        "if c == nil then c = b; end",
        "if a_3 ~= nil then a_3.b = v_6; end",
        "if a_3 ~= nil then if a_3.n == nil then a_3.n = v_6; end end",
        "local s_4 = __rt.index(t, \"m\"); if s_4 ~= nil then v_7 = (__rt.mapply(s_4, t, \"x\")); end",
        "local s_5 = p_2.scaled; if s_5 ~= nil then v_8 = (s_5(p_2, 3)); end",
    ] {
        assert!(lua.contains(wanted), "missing {wanted:?} in {lua}");
    }
    for backend in ["lua", "bc"] {
        let mut cmd = Command::new(&nlc);
        cmd.current_dir(&dir).env_remove("NL_PATH").arg("run").arg(format!("--backend={backend}")).arg(file);
        let result = run_with_timeout(cmd, Duration::from_secs(30));
        assert!(result.is_ok(), "{result:?}");
        let (code, stdout, stderr) = result.unwrap_or_default();
        assert_eq!(code, Some(0), "--backend={backend}: {}", String::from_utf8_lossy(&stderr));
        assert_eq!(String::from_utf8_lossy(&stdout), "nil\t2\tnil\tnil\tnil\t4\nx\t6\n", "--backend={backend}");
    }
}

/// The locals that cache runtime values for the inline checks do not
/// count against LuaJIT's limits (nl_ir, lower/limits.rs): a function
/// that uses 56 file-level names leaves the file-level locals in place, as
/// before the cache, and one that uses 57 moves them to the hidden table.
/// The programs are `tests/cases/limits_rt_cache.nl` (whose output the
/// regression cases check) and `tests/cases/lib/limits_rt_cache57.nl`.
#[test]
fn limits_rt_cache() {
    let dir = cases_dir();
    let nlc = PathBuf::from(env!("CARGO_BIN_EXE_nlc"));
    for (file, moved, output) in [("limits_rt_cache.nl", false, "1595\n1652\n"), ("lib/limits_rt_cache57.nl", true, "1710\n")] {
        let mut cmd = Command::new(&nlc);
        cmd.current_dir(&dir).env_remove("NL_PATH").args(["build", "--emit=lua", file]);
        let result = run_with_timeout(cmd, Duration::from_secs(30));
        assert!(result.is_ok(), "{result:?}");
        let (code, stdout, stderr) = result.unwrap_or_default();
        assert_eq!(code, Some(0), "{file}: {}", String::from_utf8_lossy(&stderr));
        let lua = String::from_utf8_lossy(&stdout);
        assert_eq!(lua.contains("local file = {}"), moved, "{file}: {lua}");
        for backend in ["lua", "bc"] {
            let mut cmd = Command::new(&nlc);
            cmd.current_dir(&dir).env_remove("NL_PATH").arg("run").arg(format!("--backend={backend}")).arg(file);
            let result = run_with_timeout(cmd, Duration::from_secs(30));
            assert!(result.is_ok(), "{result:?}");
            let (code, stdout, stderr) = result.unwrap_or_default();
            assert_eq!(code, Some(0), "{file} --backend={backend}: {}", String::from_utf8_lossy(&stderr));
            assert_eq!(String::from_utf8_lossy(&stdout), output, "{file} --backend={backend}");
        }
    }
}

/// `nlc run`'s report of an uncaught error: the
/// message after `nlc: `, then the trace, with the NL line of every frame
/// of the program and no frame of the runtime library, once, the same on
/// both backends. The program is `tests/cases/lib/uncaught_trace.nl`.
#[test]
fn uncaught_error_trace() {
    let dir = cases_dir();
    let nlc = PathBuf::from(env!("CARGO_BIN_EXE_nlc"));
    let expected = "nlc: lib/uncaught_trace.nl: uncaught Exception: no value for 3\n\
                    stack traceback:\n\
                    \tlib/uncaught_trace.nl:4: in function 'lookup'\n\
                    \tlib/uncaught_trace.nl:15: in function <lib/uncaught_trace.nl:14>\n\
                    \tlib/uncaught_trace.nl:10: in function 'visit'\n\
                    \tlib/uncaught_trace.nl:14: in main chunk\n";
    for backend in ["lua", "bc"] {
        let mut cmd = Command::new(&nlc);
        cmd.current_dir(&dir).env_remove("NL_PATH").arg("run").arg(format!("--backend={backend}")).arg("lib/uncaught_trace.nl");
        let result = run_with_timeout(cmd, Duration::from_secs(30));
        assert!(result.is_ok(), "{result:?}");
        let (code, stdout, stderr) = result.unwrap_or_default();
        let stderr = String::from_utf8_lossy(&stderr);
        assert_eq!(code, Some(1), "--backend={backend}: {stderr}");
        assert_eq!(String::from_utf8_lossy(&stdout), "visiting\n", "--backend={backend}");
        assert_eq!(stderr, expected, "--backend={backend}");
    }
}

/// Runs `nlc` with `args` in `dir`; its stdout, once it exited with 0.
fn nlc_stdout(dir: &Path, args: &[&str]) -> String {
    let mut cmd = Command::new(PathBuf::from(env!("CARGO_BIN_EXE_nlc")));
    cmd.current_dir(dir).env_remove("NL_PATH").args(args);
    let result = run_with_timeout(cmd, Duration::from_secs(30));
    assert!(result.is_ok(), "{args:?}: {result:?}");
    let (code, stdout, stderr) = result.unwrap_or_default();
    assert_eq!(code, Some(0), "{args:?}: {}", String::from_utf8_lossy(&stderr));
    String::from_utf8_lossy(&stdout).into_owned()
}

/// Localized Lua paths (nl_ir,
/// lower/localize.rs): what `lib/localize_paths.nl` compiles to with each
/// `--localize` setting, and that it prints the same with each, on both
/// backends.
#[test]
fn localize_generated_code() {
    let dir = cases_dir();
    let file = "lib/localize_paths.nl";
    let none = nlc_stdout(&dir, &["build", "--emit=lua", "--localize=none", file]);
    let std = nlc_stdout(&dir, &["build", "--emit=lua", "--localize=std", file]);
    let default = nlc_stdout(&dir, &["build", "--emit=lua", file]);
    let all = nlc_stdout(&dir, &["build", "--emit=lua", "--localize=all", file]);
    assert_eq!(std, default, "std is the default");

    // none: every path is read where it stands.
    assert!(!none.contains("string_format") && !none.contains("math_pi"), "{none}");
    assert!(none.contains("return math.pi * r * r") && none.contains("string.format(\"%.1f\""), "{none}");

    // std: the fields of the libraries the file reads and does not assign,
    // read once after the file's declarations, in the order of first use;
    // `os` guarded.
    assert!(
        std.contains("local math_pi, string_upper, string_format, os_getenv = math.pi, string.upper, string.format, os and os.getenv;"),
        "{std}"
    );
    assert_eq!(std.matches("math_pi").count(), 2, "{std}");
    assert!(std.contains("return math_pi * r * r") && std.contains("return string_upper(s)"), "{std}");
    assert!(std.contains("string_format(\"%.1f\"") && std.contains("os_getenv(\"NL_SURELY_UNSET_VARIABLE\")"), "{std}");
    // Assigned by the file (`:math.floor = …`), its root assigned
    // (`:_G.table = …`): read where they stand.
    assert!(std.contains("print(math.floor(2.5))") && std.contains("math.floor = math.floor") && !std.contains("math_floor"), "{std}");
    assert!(std.contains("table.concat(") && !std.contains("table_concat"), "{std}");
    // A root alone, an index step: left alone.
    assert!(std.contains("print(\"printed\")") && std.contains("string.len(\"abc\")"), "{std}");

    // all: roots alone too; the file's `global` is a variable.
    assert!(
        all.contains(
            "local math_pi, string_upper, string_format, _G_math, os_getenv, _G_print, _G_string = math.pi, string.upper, string.format, math, os and os.getenv, print, string;"
        ),
        "{all}"
    );
    assert!(all.contains("print(_G_math.floor(2.5))") && all.contains("math.floor = _G_math.floor"), "{all}");
    assert!(all.contains("_G_print(\"printed\")") && all.contains("_G_string.len(\"abc\")"), "{all}");
    assert!(all.contains("Score = Score + 1") && all.contains("print(Score)"), "{all}");

    let expected = "12.6\tA\n2\na,b\nnil\nprinted\n3\n2\n";
    for setting in ["none", "std", "all"] {
        for backend in ["lua", "bc"] {
            let out = nlc_stdout(&dir, &["run", &format!("--backend={backend}"), &format!("--localize={setting}"), file]);
            assert_eq!(out, expected, "--localize={setting} --backend={backend}");
        }
    }
}

/// The trade of localized paths: a value is read when the file
/// loads, so a library function that a Lua file replaces later
/// (`lib/localize_late.lua`) is not seen with `std` and `all`, and a
/// global it creates later is not seen with `all`.
#[test]
fn localize_trade() {
    let dir = cases_dir();
    for (setting, expected) in [("none", "A\tnil\nlate A\t5\n"), ("std", "A\tnil\nA\t5\n"), ("all", "A\tnil\nA\tnil\n")] {
        for backend in ["lua", "bc"] {
            let out = nlc_stdout(&dir, &["run", &format!("--backend={backend}"), &format!("--localize={setting}"), "lib/localize_trade.nl"]);
            assert_eq!(out, expected, "--localize={setting} --backend={backend}");
        }
    }
}

/// Localized paths within LuaJIT's limits (nl_ir, lower/limits.rs): a
/// function that would use more than 56 upvalues with them gets back the
/// ordinary read of as many paths as it is over by, and so does a main
/// function with more than 180 active locals; nothing else moves, and the
/// program prints the same. The programs are written to a temporary
/// directory.
#[test]
fn localize_limits() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("localize_limits");
    let _ = std::fs::create_dir_all(&dir);
    let paths = ["abs", "ceil", "floor", "sqrt", "max", "min", "exp", "log", "sin", "cos"];
    let use_paths: Vec<String> = paths.iter().map(|p| format!(":math.{p}(1)")).collect();
    // 50 file-level functions, the 2 values of the keyed-arguments check
    // and 10 paths: 62 upvalues, 6 over.
    let mut upvalues = String::new();
    for i in 1..=50 {
        upvalues.push_str(&format!("fn f{i}(x) x + {i} end\n"));
    }
    let calls: Vec<String> = (1..=50).map(|i| format!("f{i}(n)")).collect();
    upvalues.push_str(&format!("fn g(n)\n  {} + {}\nend\nprint g(1)\n", calls.join(" + "), use_paths.join(" + ")));
    // 175 top-level variables and `h`, the 2 values of the inline checks
    // and 10 paths: 188 active locals, 8 over.
    let mut locals = String::new();
    for i in 1..=175 {
        locals.push_str(&format!("let v{i} = {i}\n"));
    }
    locals.push_str(&format!("fn h(x)\n  x + v1 + v175\nend\nprint h({})\n", use_paths.join(" + ")));
    for (name, src, dropped) in [("upvalues.nl", &upvalues, 6), ("locals.nl", &locals, 8)] {
        let _ = std::fs::write(dir.join(name), src);
        let lua = nlc_stdout(&dir, &["build", "--emit=lua", name]);
        assert!(!lua.contains("local file = {}"), "{name}: {lua}");
        let read = paths.iter().filter(|p| lua.contains(&format!("math.{p}(1)"))).count();
        assert_eq!(read, dropped, "{name}: {lua}");
        assert_eq!(lua.matches("math_").count(), (paths.len() - dropped) * 2, "{name}: {lua}");
        let expected = nlc_stdout(&dir, &["run", "--localize=none", name]);
        for backend in ["lua", "bc"] {
            let out = nlc_stdout(&dir, &["run", &format!("--backend={backend}"), name]);
            assert_eq!(out, expected, "{name} --backend={backend}");
        }
    }
}
