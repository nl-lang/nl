//! `nlc check`, `nlc build` and `nlc run`, and what `nlc bundle` shares
//! with them.
//!
//! A diagnostic is printed with the source line it points at and a caret
//! underline (`nl_capi::snippet`), under a first
//! line that keeps the one-line form `file:line:column: severity: message
//! [code]`. The source is the compiled file's, or an imported file's read
//! again by its canonical name; when it cannot be read, the first line
//! stands alone.

use std::collections::HashMap;
use std::process::ExitCode;
use std::rc::Rc;

use mlua::chunk::ChunkMode;
use mlua::{Function, Lua, Table, Value};
use nl_capi::{CompileOptions, Emit, Severity};

use crate::host::{self, Files};

/// How `nlc run` configures the JIT compiler: as it comes, off, or
/// compiling every loop and function on its first iteration or call
/// (`hotloop=1`), which exercises the trace compiler on short programs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Jit {
    On,
    Off,
    Hot,
}

struct Args {
    file: String,
    options: CompileOptions,
    /// `build`: `--emit=lua` or `--emit=bc` was given.
    emit_given: bool,
    /// `build -o path`.
    output: Option<String>,
    /// `build --minify`.
    minify: bool,
    jit: Jit,
    /// `--decl` paths: declaration files or directories of them.
    decls: Vec<String>,
}

/// The options of `check`, `build` and `run` as written.
struct Parsed {
    files: Vec<String>,
    options: CompileOptions,
    emit_given: bool,
    output: Option<String>,
    minify: bool,
    jit: Jit,
    decls: Vec<String>,
}

/// Parses the options shared by `check`, `build` and `run`; `many`: the
/// command takes several files.
fn parse_options(args: &[String], command: &str, usage: &str, many: bool) -> Result<Parsed, ExitCode> {
    let mut files = Vec::new();
    let mut options = CompileOptions::default();
    let mut emit_given = false;
    let mut output = None;
    let mut minify = false;
    let mut jit = Jit::On;
    let mut decls = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--unchecked" => options.checks = false,
            "--strip" => options.strip = true,
            "--no-static-resolution" => options.static_resolution = false,
            "--no-analyzer" => options.analyze = false,
            s if s.starts_with("--profile=") => match nl_capi::Profile::from_name(s.trim_start_matches("--profile=")) {
                Some(p) => options.profile = p,
                None => {
                    eprintln!("nlc {command}: `--profile` is `full`, `portable` or `gmod-client`\n\n{usage}");
                    return Err(ExitCode::from(2));
                }
            },
            s if s.starts_with("--localize=") => match nl_capi::Localize::from_name(s.trim_start_matches("--localize=")) {
                Some(l) => options.localize = l,
                None => {
                    eprintln!("nlc {command}: `--localize` is `none`, `std` or `all`\n\n{usage}");
                    return Err(ExitCode::from(2));
                }
            },
            "--decl" => match it.next() {
                Some(p) => decls.push(p.clone()),
                None => {
                    eprintln!("nlc {command}: `--decl` takes a path\n\n{usage}");
                    return Err(ExitCode::from(2));
                }
            },
            s if s.starts_with("--decl=") => decls.push(s.trim_start_matches("--decl=").to_string()),
            "--emit=lua" if command == "build" => {
                options.emit = Emit::Lua;
                emit_given = true;
            }
            "--minify" if command == "build" => minify = true,
            "--emit=bc" if command == "build" => {
                options.emit = Emit::Bytecode;
                emit_given = true;
            }
            "-o" if command == "build" => match it.next() {
                Some(path) => output = Some(path.clone()),
                None => {
                    eprintln!("nlc {command}: `-o` needs a path\n\n{usage}");
                    return Err(ExitCode::from(2));
                }
            },
            "--backend=lua" if command == "run" => options.emit = Emit::Lua,
            "--backend=bc" if command == "run" => options.emit = Emit::Bytecode,
            "--jit=on" if command == "run" => jit = Jit::On,
            "--jit=off" if command == "run" => jit = Jit::Off,
            "--jit=hot" if command == "run" => jit = Jit::Hot,
            s if s.starts_with('-') => {
                eprintln!("nlc {command}: unknown option `{s}`\n\n{usage}");
                return Err(ExitCode::from(2));
            }
            s if many || files.is_empty() => files.push(s.to_string()),
            _ => {
                eprintln!("nlc {command}: one input file only\n\n{usage}");
                return Err(ExitCode::from(2));
            }
        }
    }
    if files.is_empty() {
        eprintln!("nlc {command}: no input file\n\n{usage}");
        return Err(ExitCode::from(2));
    }
    // The portable profiles run Lua source on stock LuaJIT; Garry's Mod
    // refuses bytecode.
    if options.profile.is_stock() && options.emit == Emit::Bytecode {
        let what = if command == "run" { "`--backend=bc`" } else { "`--emit=bc`" };
        eprintln!("nlc {command}: the {} profile compiles to Lua source; {what} is for the VM fork\n\n{usage}", options.profile.name());
        return Err(ExitCode::from(2));
    }
    if options.profile.is_gmod() && command == "run" {
        eprintln!(
            "nlc run: a {} program runs in Garry's Mod, not here: bundle it with `nlc bundle --profile={}` (tests/gmod/mock.lua runs a bundle outside the game)\n\n{usage}",
            options.profile.name(),
            options.profile.name()
        );
        return Err(ExitCode::from(2));
    }
    if minify && options.emit != Emit::Lua {
        eprintln!("nlc {command}: `--minify` is for Lua source (`--emit=lua`)\n\n{usage}");
        return Err(ExitCode::from(2));
    }
    Ok(Parsed { files, options, emit_given, output, minify, jit, decls })
}

fn parse_args(args: &[String], command: &str, usage: &str) -> Result<Args, ExitCode> {
    let p = parse_options(args, command, usage, false)?;
    let file = p.files.into_iter().next().unwrap_or_default();
    Ok(Args { file, options: p.options, emit_given: p.emit_given, output: p.output, minify: p.minify, jit: p.jit, decls: p.decls })
}

/// The text of the files that diagnostics point at, each read once: the
/// compiled file from the bytes that were compiled, an imported one again
/// from its canonical name (relative to the working directory, or
/// absolute; host.rs).
pub(crate) struct Sources(HashMap<String, Option<String>>);

impl Sources {
    /// `name` is the canonical name of the compiled file, `bytes` its source.
    pub(crate) fn new(name: &str, bytes: &[u8]) -> Sources {
        Sources(HashMap::from([(name.to_string(), Some(String::from_utf8_lossy(bytes).into_owned()))]))
    }

    pub(crate) fn get(&mut self, file: &str) -> Option<&str> {
        self.0.entry(file.to_string()).or_insert_with(|| std::fs::read(file).ok().map(|b| String::from_utf8_lossy(&b).into_owned())).as_deref()
    }
}

/// Prints a diagnostic with its snippet (see the module doc).
pub(crate) fn report(d: &nl_capi::Diagnostic, sources: &mut Sources) {
    let text = match sources.get(&d.file) {
        Some(src) => nl_capi::snippet::render(d, src),
        None => nl_capi::snippet::header(d),
    };
    eprintln!("{text}");
}

/// `nlc check [--decl <path>]... <file>...`: every diagnostic of each file,
/// on stderr; exit status 1 when any file has an error.
pub fn cmd_check(args: &[String], usage: &str) -> ExitCode {
    let mut p = match parse_options(args, "check", usage, true) {
        Ok(p) => p,
        Err(code) => return code,
    };
    p.options.analyze = true;
    let files = files_for(&p.options, &p.decls);
    let lua = Lua::new();
    let mut failed = false;
    for path in &p.files {
        let Ok(source) = std::fs::read(path) else {
            eprintln!("{path}: error: cannot read the file");
            failed = true;
            continue;
        };
        let canonical = files.canonical(std::path::Path::new(path));
        let out = nl_capi::compile(&source, &format!("@{canonical}"), &p.options, &LoaderRef(&files));
        let mut sources = Sources::new(&canonical, &source);
        for d in out.diagnostics.iter().chain(&output_diagnostics(&lua, &p.options, &canonical, &source, out.chunk.as_deref())) {
            report(d, &mut sources);
            failed |= d.severity == Severity::Error;
        }
    }
    if failed { ExitCode::FAILURE } else { ExitCode::SUCCESS }
}

/// `nlc build --emit=lua|bc [-o path] <file>`: the backend's output on
/// stdout, or in the file.
pub fn cmd_build(args: &[String], usage: &str) -> ExitCode {
    let a = match parse_args(args, "build", usage) {
        Ok(a) => a,
        Err(code) => return code,
    };
    if !a.emit_given {
        eprintln!("nlc build: say what to emit: `--emit=lua` or `--emit=bc`\n\n{usage}");
        return ExitCode::from(2);
    }
    let files = files_for(&a.options, &a.decls);
    let Ok(source) = std::fs::read(&a.file) else {
        eprintln!("{}: error: cannot read the file", a.file);
        return ExitCode::FAILURE;
    };
    let canonical = files.canonical(std::path::Path::new(&a.file));
    let out = nl_capi::compile(&source, &format!("@{canonical}"), &a.options, &LoaderRef(&files));
    let mut sources = Sources::new(&canonical, &source);
    let checked = output_diagnostics(&Lua::new(), &a.options, &canonical, &source, out.chunk.as_deref());
    for d in out.diagnostics.iter().chain(&checked) {
        report(d, &mut sources);
    }
    let chunk = if checked.is_empty() { out.chunk } else { None };
    // `--minify`: as `nlc bundle` carries the file (nl_capi::minify).
    let chunk = match chunk {
        Some(c) if a.minify => match nl_capi::minify::minify_bytes(&c, nl_capi::minify::MinifyOptions::COMPILED) {
            Ok(m) => Some(m),
            Err(e) => {
                eprintln!("{canonical}: error: internal error: the Lua of this file does not minify: {e}");
                None
            }
        },
        other => other,
    };
    match chunk {
        Some(c) => match &a.output {
            Some(path) => match std::fs::write(path, &c) {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("{path}: error: cannot write the file: {e}");
                    ExitCode::FAILURE
                }
            },
            None => {
                use std::io::Write as _;
                let _ = std::io::stdout().write_all(&c);
                ExitCode::SUCCESS
            }
        },
        None => ExitCode::FAILURE,
    }
}

/// The files of a compile; gmod's declarations, for a gmod profile, are
/// the compiler's (nl_sema::decl::GMOD_SOURCE).
fn files_for(_options: &CompileOptions, decls: &[String]) -> Files {
    let files = Files::new();
    for d in decls {
        files.add_decl(d);
    }
    files
}

/// Whether a chunk loads on the stock LuaJIT that `nlc` embeds (the
/// limits of gmod's 2.0.4 are those of this 2.1): the packager's check
/// (`nl_capi::bundle::Check`).
pub(crate) fn lua_check(lua: &Lua) -> impl Fn(&str, &str) -> Result<(), String> + '_ {
    move |name, text| {
        lua.load(text).set_name(name).set_mode(ChunkMode::Text).into_function().map(|_| ()).map_err(|e| match e {
            mlua::Error::SyntaxError { message, .. } => message,
            other => other.to_string(),
        })
    }
}

/// For a gmod profile, the errors of the Lua a file compiled to
/// (`nl_capi::bundle::check_output`); nothing otherwise.
fn output_diagnostics(lua: &Lua, options: &CompileOptions, name: &str, source: &[u8], chunk: Option<&[u8]>) -> Vec<nl_capi::Diagnostic> {
    match chunk {
        Some(c) if options.profile.is_gmod() && options.emit == Emit::Lua => {
            let check = lua_check(lua);
            nl_capi::bundle::check_output(name, &String::from_utf8_lossy(source), &String::from_utf8_lossy(c), Some(&check))
        }
        _ => Vec::new(),
    }
}

struct LoaderRef<'a>(&'a Files);

impl nl_capi::Loader for LoaderRef<'_> {
    fn resolve(&self, from: Option<&str>, path: &str) -> Option<String> {
        self.0.resolve(from, path)
    }
    fn read(&self, name: &str) -> Option<Vec<u8>> {
        self.0.read(name)
    }
    fn declarations(&self) -> Vec<(String, Vec<u8>)> {
        self.0.declarations()
    }
}

/// `nlc run [--backend=lua|bc] [--jit=on|off|hot] <file>`.
pub fn cmd_run(args: &[String], usage: &str) -> ExitCode {
    let a = match parse_args(args, "run", usage) {
        Ok(a) => a,
        Err(code) => return code,
    };
    match run(&a) {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            eprintln!("{msg}");
            ExitCode::FAILURE
        }
    }
}

fn run(a: &Args) -> Result<(), String> {
    let files = Rc::new(Files::new());
    for d in &a.decls {
        files.add_decl(d);
    }
    let source = std::fs::read(&a.file).map_err(|e| format!("{}: error: cannot read the file: {e}", a.file))?;
    let canonical = files.canonical(std::path::Path::new(&a.file));
    let name = format!("@{canonical}");
    // Compile first, so that a compile error is reported as such: the
    // message of host.compile, then the snippet of the first error.
    let out = nl_capi::compile(&source, &name, &a.options, &LoaderRef(&files));
    let Some(chunk) = out.chunk else {
        let mut msg = host::error_message(&out.diagnostics);
        let mut sources = Sources::new(&canonical, &source);
        if let Some(d) = out.diagnostics.iter().find(|d| d.severity == Severity::Error) {
            let s = sources.get(&d.file).map(|src| nl_capi::snippet::snippet(src, d.line, d.column, d.end_line, d.end_column)).unwrap_or_default();
            if !s.is_empty() {
                msg.push('\n');
                msg.push_str(&s);
            }
        }
        return Err(msg);
    };

    let lua = Lua::new();
    let lua_err = |e: mlua::Error| format!("nlc: {}", error_text(&e));
    match a.jit {
        Jit::On => {}
        Jit::Off => lua.load("jit.off()").set_name("=nl host").exec().map_err(lua_err)?,
        Jit::Hot => lua.load("jit.opt.start('hotloop=1')").set_name("=nl host").exec().map_err(lua_err)?,
    }
    // Lua's `pcall`, taken before any script runs.
    let protected: Function = lua.load("local pcall = pcall; return function(f, ...) return pcall(f, ...) end").set_name("=nl host").call(()).map_err(lua_err)?;
    host::install_debug_standin(&lua).map_err(lua_err)?;
    let host_table = host::host_table(&lua, files.clone(), a.options).map_err(lua_err)?;
    let rt: Table = lua.load(host::RUNTIME).set_name("=nl runtime").call(host_table).map_err(lua_err)?;
    let timers = Timers::install(&lua, &rt).map_err(lua_err)?;
    expose_runtime_for_tests(&lua, &rt).map_err(lua_err)?;

    // The two-step protocol of the chunk (driver.rs): P(rt) gives main.
    let mode = match a.options.emit {
        Emit::Lua => ChunkMode::Text,
        Emit::Bytecode => ChunkMode::Binary,
    };
    let p: Function = lua.load(&chunk[..]).set_name(&name).set_mode(mode).into_function().map_err(lua_err)?;
    let main: Function = p.call(rt.clone()).map_err(lua_err)?;
    let run_main: Function = rt.get("runmain").map_err(lua_err)?;
    // `rt.runmain` raises the error with its trace already in the message;
    // called through `pcall`, it reaches here without the second traceback
    // that mlua would add to a string error.
    let (ok, err): (bool, Value) = protected.call((run_main, main, name.clone())).map_err(lua_err)?;
    if !ok {
        let text = match &err {
            Value::String(s) => s.to_string_lossy(),
            other => format!("{other:?}"),
        };
        return Err(format!("nlc: {text}"));
    }

    // Pump until no task is pending. What is queued runs even
    // when no task is pending (the check for unhandled failures, results
    // of the `spawn` stand-in); when nothing is queued, the next timer of
    // `Task.sleep` fires.
    let pending: Function = rt.get("pending").map_err(lua_err)?;
    let pump: Function = rt.get("pump").map_err(lua_err)?;
    loop {
        let progressed: bool = pump.call(()).map_err(lua_err)?;
        if progressed {
            continue;
        }
        if let Some(wake) = timers.next() {
            wake.call::<()>(()).map_err(lua_err)?;
            continue;
        }
        let n: i64 = pending.call(()).map_err(lua_err)?;
        if n <= 0 {
            break;
        }
        return Err(format!("nlc: {n} task(s) still pending and nothing left to run"));
    }
    Ok(())
}

/// Test hook, not part of the tool's interface: with `NLC_TEST_RT_GLOBAL`
/// set to a name, the runtime table `rt` is also stored in the global of
/// that name, so that a Lua file of a test can call the host interface of
/// runtime/nl.lua (`rt.settype`, `rt.pump`, …), which the C API reaches
/// and NL code cannot. Used by crates/nlc/tests/runtime_cases.rs.
fn expose_runtime_for_tests(lua: &Lua, rt: &Table) -> mlua::Result<()> {
    match std::env::var("NLC_TEST_RT_GLOBAL") {
        Ok(name) if !name.is_empty() => lua.globals().set(name, rt.clone()),
        _ => Ok(()),
    }
}

/// The host timer of `nlc run` for `Task.sleep` (`nl_settimer`):
/// real time. A sleep's `wake` is called from the pump loop once its time
/// has passed and nothing else is queued; sleeps end in the order of their
/// deadlines, and in the order they began when the deadlines are equal.
struct Timers {
    list: Rc<std::cell::RefCell<Vec<(std::time::Instant, u64, Function)>>>,
}

impl Timers {
    fn install(lua: &Lua, rt: &Table) -> mlua::Result<Timers> {
        let list: Rc<std::cell::RefCell<Vec<(std::time::Instant, u64, Function)>>> = Rc::default();
        let l = list.clone();
        let mut seq = 0u64;
        let set = lua.create_function_mut(move |_, (seconds, wake): (f64, Function)| {
            // A sleep of infinity never ends: its task stays pending, and
            // the run ends with the "still pending" report. NaN and a
            // negative time end at once; a long finite one is capped.
            if seconds == f64::INFINITY {
                return Ok(());
            }
            let secs = if seconds.is_finite() && seconds > 0.0 { seconds.min(1.0e6) } else { 0.0 };
            let at = std::time::Instant::now() + std::time::Duration::from_secs_f64(secs);
            seq += 1;
            l.borrow_mut().push((at, seq, wake));
            Ok(())
        })?;
        let settimer: Function = rt.get("settimer")?;
        settimer.call::<()>(set)?;
        Ok(Timers { list })
    }

    /// Waits for the earliest timer and returns its `wake`.
    fn next(&self) -> Option<Function> {
        let (at, wake) = {
            let mut list = self.list.borrow_mut();
            let i = list.iter().enumerate().min_by_key(|(_, (at, seq, _))| (*at, *seq)).map(|(i, _)| i)?;
            let (at, _, wake) = list.remove(i);
            (at, wake)
        };
        let now = std::time::Instant::now();
        if at > now {
            std::thread::sleep(at - now);
        }
        Some(wake)
    }
}

fn error_text(e: &mlua::Error) -> String {
    match e {
        mlua::Error::RuntimeError(s) => s.clone(),
        mlua::Error::CallbackError { traceback, cause } => format!("{}\n{traceback}", error_text(cause)),
        mlua::Error::ExternalError(x) => x.to_string(),
        other => other.to_string(),
    }
}
