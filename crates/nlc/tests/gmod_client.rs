//! The gmod-client profile end to end, on the stock LuaJIT that `nlc`
//! embeds (tests/gmod/run.sh runs bundles on gmod's own LuaJIT builds):
//!
//! - `nlc check` reports what gmod's Lua state and the client realm lack,
//!   all in one run, and nothing under `if SERVER`;
//! - a bundle leaves the code under `if SERVER` out;
//! - a bundle split into parts runs once, whatever order the parts load
//!   in, and its sourcemap, through `nlc remap`, takes every position of
//!   a traceback back to its source: the NL file's line, and the runtime
//!   file's line, checked by what the line says.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// A fresh scratch directory under the target directory.
fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("gmod_client_{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn nlc(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_nlc")).current_dir(dir).env_remove("NL_PATH").args(args).output().expect("cannot start nlc")
}

#[test]
fn check_reports_what_the_client_lacks() {
    let dir = scratch("check");
    let src = "let me = local_player()
me.kick(\"bye\")
let home = OS.getenv(\"HOME\")
:net.Broadcast()
if SERVER
  me.kick(\"fine here\")
  IO.write(\"and here\")
end
print(home)
";
    std::fs::write(dir.join("c.nl"), src).unwrap();
    let out = nlc(&dir, &["check", "--profile=gmod-client", "c.nl"]);
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{err}");
    let found: Vec<(u32, &str)> = err
        .lines()
        .filter(|l| l.starts_with("c.nl:"))
        .map(|l| (l.split(':').nth(1).unwrap().parse().unwrap(), l.rsplit_once(" [").unwrap().1.trim_end_matches(']')))
        .collect();
    assert_eq!(found, [(2, "wrong-realm"), (3, "not-in-gmod"), (4, "wrong-realm")], "{err}");
    // Without the profile, the same file is fine (with gmod's declarations).
    let out = nlc(&dir, &["check", "--decl", repo_root().join("decl/glua.d.nl").to_str().unwrap(), "c.nl"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
}

/// Runs the files of a bundle in a fresh state of the embedded LuaJIT, in
/// the given order, as gmod's client would (`CompileString`, `SERVER`),
/// and returns what the last one returned.
fn run_parts(lua: &mlua::Lua, paths: &[PathBuf]) -> mlua::Value {
    lua.load("SERVER, CLIENT = false, true; CompileString = function(s, id) return loadstring(s, '@' .. id) end; AddCSLuaFile = function() end").exec().unwrap();
    let mut last = mlua::Value::Nil;
    for p in paths {
        let text = std::fs::read_to_string(p).unwrap();
        last = lua.load(text.as_str()).set_name(format!("@{}", p.file_name().unwrap().to_string_lossy())).eval().unwrap();
    }
    last
}

#[test]
fn parts_sourcemap_and_server_code() {
    let dir = scratch("parts");
    std::fs::create_dir_all(dir.join("addon")).unwrap();
    let main = "if SERVER
  :util.AddNetworkString(\"SERVER_ONLY_MARKER\")
end

pub fn explode(t)
  # a member read of nil fails in the runtime's lookup
  t.missing.field
end
";
    std::fs::write(dir.join("addon/main.nl"), main).unwrap();
    let bundle = |limit: &str| nlc(&dir, &["bundle", "--profile=gmod-client", limit, "-o", "out/client.lua", "--map", "client.nlmap.json", "addon/main.nl"]);
    let list = || {
        let mut parts: Vec<PathBuf> = std::fs::read_dir(dir.join("out")).unwrap().map(|e| e.unwrap().path()).collect();
        parts.sort();
        parts
    };
    // `-o` into a folder that does not exist fails cleanly.
    assert!(!bundle("--part-limit=10000").status.success());
    std::fs::create_dir_all(dir.join("out")).unwrap();
    // A build in more parts, and a file after them that is not a part.
    let out = bundle("--part-limit=6000");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let before = list().len();
    std::fs::write(dir.join(format!("out/client_{}.lua", before + 1)), "-- not a part\n").unwrap();
    // The parts beyond the new count go; the other file stays.
    let out = bundle("--part-limit=10000");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let mut parts = list();
    assert!(parts.len() < before + 1 && parts.len() >= 3, "{before} then {parts:?}");
    assert!(parts.pop().unwrap().ends_with(format!("client_{}.lua", before + 1)), "{parts:?}");
    parts.retain(|p| p.file_name().unwrap().to_string_lossy().starts_with("client"));
    for p in &parts {
        let text = std::fs::read_to_string(p).unwrap();
        assert!(!text.contains("SERVER_ONLY_MARKER") && !text.contains("AddNetworkString"), "{}", p.display());
        assert!(text.starts_with("-- Built by `nlc bundle` (gmod-client): part "), "{}", p.display());
    }

    // Last part first; the bundle runs when the set is complete.
    parts.reverse();
    let lua = mlua::Lua::new();
    let exports = run_parts(&lua, &parts);
    let exports = exports.as_table().expect("the bundle returns the entry's exports").clone();
    let explode: mlua::Function = exports.get("explode").unwrap();
    // A traceback as LuaJIT's, from mlua's stack inspection (the safe
    // state has no `debug`).
    let handler = lua
        .create_function(|lua, msg: String| {
            let mut text = format!("{msg}\nstack traceback:");
            for level in 1..40 {
                let frame = lua.inspect_stack(level, |d| {
                    let short = d.source().short_src.map(|s| s.to_string()).unwrap_or_default();
                    let name = d.names().name.map(|n| format!("function '{n}'")).unwrap_or_else(|| "?".to_string());
                    format!("\n\t{short}:{}: in {name}", d.current_line().unwrap_or(0))
                });
                match frame {
                    Some(f) => text.push_str(&f),
                    None => break,
                }
            }
            Ok(text)
        })
        .unwrap();
    let call: mlua::Function = lua.load("return function(f, handler) local ok, e = xpcall(function() f({}) end, handler) return e end").eval().unwrap();
    let trace: String = call.call((explode, handler)).unwrap();
    assert!(trace.contains("nl runtime:") && trace.contains("addon/main.nl:7:"), "{trace}");

    // Remapped: the NL line, and the runtime file and line of the frame.
    let mut child = Command::new(env!("CARGO_BIN_EXE_nlc")).current_dir(&dir).args(["remap", "client.nlmap.json"]).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(trace.as_bytes()).unwrap();
    let remapped = String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap();
    assert!(!remapped.contains("nl runtime:"), "{remapped}");
    let lookup = remapped.lines().find(|l| l.contains("in function 'lookup'")).unwrap_or_else(|| panic!("{remapped}"));
    let (file, line) = lookup.trim().split_once(": in").unwrap().0.rsplit_once(':').unwrap();
    let text = std::fs::read_to_string(repo_root().join(file)).unwrap();
    let at = text.lines().nth(line.parse::<usize>().unwrap() - 1).unwrap();
    assert!(at.contains("error(\"attempt to read member"), "{file}:{line} is `{at}`, not the error call\n{remapped}");
    let nl = remapped.lines().find(|l| l.contains("addon/main.nl:")).unwrap();
    assert!(nl.contains("addon/main.nl:7:"), "{remapped}");
}
