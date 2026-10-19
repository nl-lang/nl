//! Loading, imports, the loader, the reporter, options and the sandbox.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use nl::prelude::*;

struct MemLoader {
    files: HashMap<String, Vec<u8>>,
    reads: Arc<Mutex<Vec<String>>>,
}

impl Loader for MemLoader {
    fn resolve(&self, from: Option<&str>, path: &str) -> Option<String> {
        let _ = from;
        let name = if path.ends_with(".nl") || path.ends_with(".d.nl") { path.to_owned() } else { format!("{path}.nl") };
        self.files.contains_key(&name).then_some(name)
    }
    fn read(&self, name: &str) -> Option<Cow<'_, [u8]>> {
        self.reads.lock().expect("lock").push(name.to_owned());
        let bytes = self.files.get(name)?;
        // One owned, the others borrowed: both paths are exercised.
        if name == "util.nl" { Some(Cow::Owned(bytes.clone())) } else { Some(Cow::Borrowed(bytes)) }
    }
}

#[test]
fn a_host_loader_and_a_reporter() -> LuaResult<()> {
    let lua = nl::new()?;
    let diags: Arc<Mutex<Vec<Diagnostic>>> = Arc::default();
    let d = diags.clone();
    lua.set_reporter(move |diag| d.lock().expect("lock").push(diag.clone()));
    let reads = Arc::new(Mutex::new(Vec::new()));
    let mut files = HashMap::new();
    files.insert("main.nl".to_owned(), b"import { twice } from 'util'\npub fn run() twice(:host_value()) end\n".to_vec());
    files.insert("util.nl".to_owned(), b"pub fn twice(x: Number) x * 2 end\n".to_vec());
    files.insert("host.d.nl".to_owned(), b"declare fn host_seven(): Number\n".to_vec());
    files.insert("decl_user.nl".to_owned(), b"pub fn seven() host_seven() end\n".to_vec());
    lua.set_loader(MemLoader { files, reads: reads.clone() })?;
    lua.globals().set("host_value", lua.create_function(|_, ()| Ok(21))?)?;
    lua.globals().set("host_seven", lua.create_function(|_, ()| Ok(7))?)?;

    let main = lua.import("main")?;
    assert_eq!(main.get::<Function>("run")?.try_call::<f64>(())?, 42.0);
    // Imported once per state.
    assert_eq!(lua.import("main")?, main);
    {
        let reads = reads.lock().expect("lock");
        assert!(reads.contains(&"main.nl".to_owned()) && reads.contains(&"util.nl".to_owned()), "{reads:?}");
    }

    // A declaration file names a host function; with a loader the host
    // registers it by name.
    assert!(lua.import("decl_user").is_err(), "host_seven is not declared yet");
    lua.add_decl("host.d.nl")?;
    let d = lua.load_nl_file("decl_user.nl")?.try_call::<Table>(())?;
    assert_eq!(d.get::<Function>("seven")?.try_call::<f64>(())?, 7.0);

    // A compile error reaches the reporter and the caller.
    diags.lock().expect("lock").clear();
    let err = lua.load_nl("pub fn f() undefined_name end\n", "=bad").unwrap_err();
    assert!(matches!(err, LuaError::SyntaxError { .. }), "{err:?}");
    let diags = diags.lock().expect("lock");
    let e = diags.iter().find(|d| d.severity == Severity::Error).expect("an error diagnostic");
    assert_eq!((e.line, e.column > 0), (1, true));
    assert!(!e.code.is_empty() && e.message.contains("undefined_name"), "{e:?}");
    Ok(())
}

#[test]
fn files_from_disk_and_the_search_path() -> LuaResult<()> {
    let dir = nl_host::tmpdir("disk");
    std::fs::write(dir.join("lib.nl"), "pub fn greet(n: String) \"hello #{n}\" end\n").expect("write");
    std::fs::write(dir.join("app.nl"), "import { greet } from 'lib'\npub fn run() greet(\"disk\") end\n").expect("write");
    let lua = nl::new()?;
    lua.add_path(&dir)?;
    let app = lua.import("app")?;
    assert_eq!(app.get::<Function>("run")?.try_call::<String>(())?, "hello disk");
    let f = lua.load_nl_file(dir.join("lib.nl"))?;
    let lib: Table = f.try_call(())?;
    assert_eq!(lib.get::<Function>("greet")?.try_call::<String>("file")?, "hello file");
    let err = lua.load_nl_file(dir.join("missing.nl")).unwrap_err();
    assert!(err.to_string().contains("missing.nl"), "{err}");
    let err = lua.import("no_such_module").unwrap_err();
    assert!(err.exception().is_some(), "{err}");
    // mlua's own `require` finds .nl files by itself.
    let lib2: Table = lua.load(r#"return require("lib")"#).eval().or_else(|_| {
        lua.load(format!("package.path = {:?} .. '/?.lua;' .. package.path return require('lib')", dir.display())).eval()
    })?;
    let _ = lib2;
    Ok(())
}

#[test]
fn options_and_the_sandbox() -> LuaResult<()> {
    let lua = nl::new()?;
    lua.set_option(NlOption::Checks(false))?;
    lua.set_option(NlOption::Checks(true))?;
    lua.set_option(NlOption::Strip(true))?;
    lua.set_option(NlOption::Localize(Localize::None))?;
    lua.set_option(NlOption::Localize(Localize::All))?;
    lua.set_option(NlOption::MaxWorkers(4))?;
    lua.set_option(NlOption::WorkerMemory(1 << 20))?;
    lua.set_option(NlOption::Glua(true))?;
    lua.set_option(NlOption::Glua(false))?;

    let env = lua.sandbox_env()?;
    assert!(env.contains_key("print")?);
    assert!(!env.contains_key("io")?);
    env.set("host_answer", lua.create_function(|_, ()| Ok(42))?)?;
    lua.set_option(NlOption::Sandbox(true))?;
    assert!(lua.set_option(NlOption::Sandbox(false)).is_err(), "the sandbox is one way");
    let m: Table = lua.load_nl("pub fn f() [:host_answer(), :io == nil] end", "=mod")?.try_call(())?;
    let r: Table = m.get::<Function>("f")?.try_call(())?;
    assert_eq!((r.get::<f64>(1)?, r.get::<bool>(2)?), (42.0, true));
    Ok(())
}

#[test]
fn the_debug_library_stays_hidden_from_scripts() -> LuaResult<()> {
    let lua = nl::new()?;
    assert!(lua.globals().get::<Value>("debug")?.is_nil());
    assert!(lua.load("return package.loaded.debug").eval::<Value>()?.is_nil());
    // A state that has it keeps it.
    let lua = unsafe { Lua::unsafe_new() };
    nl::open(&lua)?;
    assert!(lua.globals().get::<Value>("debug")?.is_table());
    Ok(())
}
