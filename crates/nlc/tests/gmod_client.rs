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
        // By part number, since `client_12.lua` sorts before `client_2.lua` as text.
        let number = |p: &PathBuf| p.file_stem().unwrap().to_string_lossy().rsplit('_').next().and_then(|n| n.parse::<u32>().ok()).unwrap_or(1);
        parts.sort_by_key(number);
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

/// A runtime of its own (`nlc runtime`, `--runtime-include`): bundles carry
/// no runtime and include its file when the state has none; two bundles
/// share it, and it runs once. A runtime in parts made with
/// `--include-parts` runs from its first file alone.
#[test]
fn runtime_of_its_own_and_included_parts() {
    let dir = scratch("runtime");
    std::fs::create_dir_all(dir.join("nl")).unwrap();
    std::fs::write(dir.join("a.nl"), "print \"a ran\"\n").unwrap();
    std::fs::write(dir.join("b.nl"), "print \"b ran\"\n").unwrap();
    let out = nlc(&dir, &["runtime", "--profile=gmod-client", "--part-limit=12000", "--include-parts", "-o", "nl/runtime.lua"]);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(dir.join("nl/runtime_2.lua").is_file());
    let first = std::fs::read_to_string(dir.join("nl/runtime.lua")).unwrap();
    assert!(first.contains("include(\"nl/runtime_2.lua\")"), "{first}");
    for name in ["a", "b"] {
        let out = nlc(&dir, &["bundle", "--profile=gmod-client", "--runtime-include", "nl/runtime.lua", "-o", &format!("{name}.lua"), &format!("{name}.nl")]);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let text = std::fs::read_to_string(dir.join(format!("{name}.lua"))).unwrap();
        assert!(!text.contains("nl runtime"), "the bundle carries the runtime");
        assert!(text.len() < 20_000, "{}", text.len());
    }

    let lua = mlua::Lua::new();
    let files = lua.create_table().unwrap();
    for entry in std::fs::read_dir(dir.join("nl")).unwrap() {
        let path = entry.unwrap().path();
        let key = format!("nl/{}", path.file_name().unwrap().to_string_lossy());
        files.set(key, std::fs::read_to_string(&path).unwrap()).unwrap();
    }
    lua.globals().set("files", files).unwrap();
    lua.load(
        "printed = {} print = function(s) printed[#printed + 1] = s end
         includes = 0
         include = function(p) includes = includes + 1 return assert(loadstring(assert(files[p], p), '@' .. p))() end",
    )
    .exec()
    .unwrap();
    run_parts(&lua, &[dir.join("a.lua"), dir.join("b.lua")]);
    let printed: Vec<String> = lua.load("printed").eval::<Vec<String>>().unwrap();
    assert_eq!(printed, ["a ran", "b ran"]);
    // The runtime's file and its parts, once: the second bundle found it.
    let includes: i64 = lua.load("includes").eval().unwrap();
    let parts = std::fs::read_dir(dir.join("nl")).unwrap().count() as i64;
    assert_eq!(includes, parts);
}

/// gmod's autorefresh sends a client the new bundle of a changed file. A
/// bundle that comes again runs again the files whose source changed, and
/// the files that import them; the others, as before, run once.
#[test]
fn a_bundle_that_comes_again_reruns_what_changed() {
    let dir = scratch("reload");
    let entry = "import { value } from \"./m\"\nprint \"a sees #{value()}\"\n";
    std::fs::write(dir.join("a.nl"), entry).unwrap();
    std::fs::write(dir.join("b.nl"), "print \"b ran\"\n").unwrap();
    let bundle = |name: &str, entry: &str| {
        let out = nlc(&dir, &["bundle", "--profile=gmod-client", "-o", &format!("{name}.lua"), entry]);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        dir.join(format!("{name}.lua"))
    };
    std::fs::write(dir.join("m.nl"), "pub fn value: Number\n  1\nend\n").unwrap();
    let a1 = bundle("a1", "a.nl");
    let b = bundle("b", "b.nl");
    std::fs::write(dir.join("m.nl"), "pub fn value: Number\n  2\nend\n").unwrap();
    let a2 = bundle("a2", "a.nl");

    let lua = mlua::Lua::new();
    lua.load("printed = {} print = function(s) printed[#printed + 1] = s end").exec().unwrap();
    run_parts(&lua, &[a1.clone(), b.clone(), a1, b, a2]);
    let printed: Vec<String> = lua.load("printed").eval::<Vec<String>>().unwrap();
    assert_eq!(printed, ["a sees 1", "b ran", "a sees 2"]);
}
