//! The host side of `nlc run` on stock LuaJIT:
//! the file resolver of `import`, the host table the runtime is opened
//! with (runtime/nl.lua, "HOST TABLE"), and a stand-in for Lua's `debug`
//! library.

use std::cell::RefCell;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;

use mlua::{Function, Lua, MultiValue, Table, Value};
use nl_capi::{CompileOptions, Loader, Severity, search};

/// The runtime library, `runtime/*.lua` concatenated (build.rs).
pub const RUNTIME: &str = include_str!(concat!(env!("OUT_DIR"), "/runtime.lua"));

/// Finds files as `import` does: relative to the importing file's
/// directory, then the working directory, then each search-path entry
/// (`nl_capi::search`), on disk and with names from `fs::canonicalize`.
pub struct Files {
    search: RefCell<Vec<PathBuf>>,
    cwd: PathBuf,
    /// `--decl` paths: declaration files, or directories of them.
    decls: RefCell<Vec<PathBuf>>,
}

impl Files {
    pub fn new() -> Self {
        let search = std::env::var_os("NL_PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Files { search: RefCell::new(search), cwd, decls: RefCell::new(Vec::new()) }
    }

    /// A `--decl` path: a `.d.nl` file, or a directory whose
    /// `.d.nl` files are all read.
    pub fn add_decl(&self, path: &str) {
        self.decls.borrow_mut().push(PathBuf::from(path));
    }

    /// The declaration files in scope: every `.d.nl` in
    /// the directories of the import search path and in the `--decl`
    /// paths, in that order, each read once.
    pub fn declarations(&self) -> Vec<(String, Vec<u8>)> {
        let (decl_dirs, files) = search::split_decl_paths(&self.decls.borrow());
        let mut dirs: Vec<PathBuf> = self.search.borrow().clone();
        dirs.extend(decl_dirs);
        let names = search::decl_files(&dirs, &files, &search::dir_files, &|p| self.canonical(p));
        names.into_iter().filter_map(|name| std::fs::read(&name).ok().map(|bytes| (name, bytes))).collect()
    }

    /// A relative directory is taken relative to the working
    /// directory, like every relative path of the process.
    pub fn add_path(&self, dir: &str) {
        self.search.borrow_mut().push(PathBuf::from(dir));
    }

    /// The canonical name of a file: relative to the working directory
    /// when it is inside it, absolute otherwise.
    pub fn canonical(&self, path: &Path) -> String {
        let abs = std::fs::canonicalize(path).unwrap_or_else(|_| normalize(&self.cwd.join(path)));
        let cwd = std::fs::canonicalize(&self.cwd).unwrap_or_else(|_| self.cwd.clone());
        let shown = abs.strip_prefix(&cwd).map(Path::to_path_buf).unwrap_or(abs);
        shown.to_string_lossy().replace('\\', "/")
    }

    pub fn resolve(&self, from: Option<&str>, path: &str) -> Option<String> {
        search::candidates(from, path, &self.cwd, &self.search.borrow()).into_iter().find(|c| c.is_file()).map(|c| self.canonical(&c))
    }

    pub fn read(&self, name: &str) -> Option<Vec<u8>> {
        std::fs::read(name).ok()
    }

    /// `host.canonical`: the canonical name of the file at `path`, taken as
    /// given (no search, no extension added); `None` when there is no such
    /// file. The same name `resolve` gives for that file.
    pub fn canonical_file(&self, path: &str) -> Option<String> {
        let p = Path::new(path);
        p.is_file().then(|| self.canonical(p))
    }
}

/// Lexical normalization: removes `.` and resolves `..` where possible.
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push("..");
                }
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

struct FilesLoader<'a>(&'a Files);

impl Loader for FilesLoader<'_> {
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

/// Compiles NL source with the files' search order; the chunk, or the
/// first error as `file:line:column: message`, followed by how many more
/// errors there are.
pub fn compile(files: &Files, source: &[u8], chunk_name: &str, options: &CompileOptions) -> Result<Vec<u8>, String> {
    let out = nl_capi::compile(source, chunk_name, options, &FilesLoader(files));
    out.chunk.ok_or_else(|| error_message(&out.diagnostics))
}

/// The message of a failed compile: the first error as
/// `file:line:column: message`, and how many more there are.
pub fn error_message(diagnostics: &[nl_capi::Diagnostic]) -> String {
    let mut errors = diagnostics.iter().filter(|d| d.severity == Severity::Error);
    match errors.next() {
        Some(d) => {
            let first = format!("{}:{}:{}: {}", d.file, d.line, d.column, d.message);
            match errors.count() {
                0 => first,
                1 => format!("{first} (and 1 more error)"),
                n => format!("{first} (and {n} more errors)"),
            }
        }
        None => "compilation failed".to_string(),
    }
}

/// Builds the host table of runtime/nl.lua.
pub fn host_table(lua: &Lua, files: Rc<Files>, options: CompileOptions) -> mlua::Result<Table> {
    let host = lua.create_table()?;
    let f = files.clone();
    host.set(
        "compile",
        lua.create_function(move |lua, (source, name): (mlua::LuaString, String)| {
            let mut out = MultiValue::new();
            match compile(&f, &source.as_bytes(), &name, &options) {
                Ok(chunk) => out.push_back(Value::String(lua.create_string(&chunk)?)),
                Err(msg) => {
                    out.push_back(Value::Nil);
                    out.push_back(Value::String(lua.create_string(&msg)?));
                }
            }
            Ok(out)
        })?,
    )?;
    let f = files.clone();
    host.set(
        "resolve",
        lua.create_function(move |_, (from, path): (Option<String>, String)| Ok(f.resolve(from.as_deref(), &path)))?,
    )?;
    let f = files.clone();
    host.set(
        "read",
        lua.create_function(move |lua, name: String| match f.read(&name) {
            Some(bytes) => Ok(Value::String(lua.create_string(&bytes)?)),
            None => Ok(Value::Nil),
        })?,
    )?;
    let f = files.clone();
    host.set("canonical", lua.create_function(move |_, path: String| Ok(f.canonical_file(&path)))?)?;
    let f = files;
    host.set(
        "add_path",
        lua.create_function(move |_, dir: String| {
            f.add_path(&dir);
            Ok(())
        })?,
    )?;
    Ok(host)
}

/// mlua opens only the safe standard libraries, and loading `debug` needs
/// `unsafe`, which the workspace forbids. `nlc run` therefore provides a
/// small `debug` table of its own, built from what mlua offers safely; the
/// fork's embedding has the real library.
///
/// - `traceback([thread,] [message [, level]])`: Lua's arguments. The
///   frames of a coroutine other than the running one are not available.
/// - `getinfo(level | function [, what])`: the fields of the options in
///   `what` (default all): `S` (`source`, `short_src`, `what`,
///   `linedefined`), `l` (`currentline`), `n` (`name`, `namewhat`), `f`
///   (`func`); for a function, those of its definition. No thread
///   argument. The runtime builds the `trace` of an exception from it
///   (runtime/nl_50_error.lua).
/// - `setmetatable(value, mt)`: for tables, and for the types whose values
///   share one metatable (numbers, booleans, strings, functions,
///   coroutines, light userdata), which the runtime needs for `Number` and
///   `Bool` methods and operators. Not for full
///   userdata.
/// - `getmetatable(value)`: the raw metatable of a table, ignoring
///   `__metatable`; for other values Lua's `getmetatable`, which a
///   `__metatable` field still hides (mlua has no safe raw access there).
///   A table locked with a `__metatable` that is itself a table without
///   that field is not told from an unlocked one.
pub fn install_debug_standin(lua: &Lua) -> mlua::Result<()> {
    let globals = lua.globals();
    if !matches!(globals.get::<Value>("debug")?, Value::Nil) {
        return Ok(());
    }
    let debug = lua.create_table()?;
    debug.set("traceback", lua.create_function(traceback)?)?;
    debug.set("getinfo", lua.create_function(getinfo)?)?;
    // Lua's `getmetatable` answers for every value whose metatable has no
    // `__metatable` field; only for a table whose answer may be such a
    // field's value is the raw metatable asked of Rust (the runtime calls
    // this often, and a Rust callback costs more than a Lua call).
    let raw_table_metatable = lua.create_function(|_, t: Table| Ok(t.metatable()))?;
    let getmetatable: Function = lua
        .load(
            "local raw, getmetatable, rawget, type = ...
             return function(v)
               local m = getmetatable(v)
               if m == nil or type(v) ~= 'table' then return m end
               if type(m) == 'table' and rawget(m, '__metatable') == nil then return m end
               return raw(v)
             end",
        )
        .set_name("=nl host debug stand-in")
        .call((raw_table_metatable, globals.get::<Function>("getmetatable")?, globals.get::<Function>("rawget")?, globals.get::<Function>("type")?))?;
    debug.set("getmetatable", getmetatable)?;
    debug.set(
        "setmetatable",
        lua.create_function(|lua, (v, mt): (Value, Option<Table>)| {
            match &v {
                Value::Table(t) => t.set_metatable(mt)?,
                Value::Integer(_) | Value::Number(_) => lua.set_type_metatable::<mlua::Number>(mt),
                Value::Boolean(_) => lua.set_type_metatable::<bool>(mt),
                Value::String(_) => lua.set_type_metatable::<mlua::LuaString>(mt),
                Value::Function(_) => lua.set_type_metatable::<Function>(mt),
                Value::Thread(_) => lua.set_type_metatable::<mlua::Thread>(mt),
                Value::LightUserData(_) => lua.set_type_metatable::<mlua::LightUserData>(mt),
                other => {
                    return Err(mlua::Error::runtime(format!(
                        "debug.setmetatable: a {} value is not supported by the debug stand-in of `nlc run`",
                        other.type_name()
                    )));
                }
            }
            Ok(v)
        })?,
    )?;
    globals.set("debug", debug)?;
    Ok(())
}

/// `debug.getinfo(level | function [, what])`; nil for a level beyond the
/// stack. Level 0 is `getinfo` itself, as in Lua.
fn getinfo(lua: &Lua, (target, what): (Value, Option<mlua::LuaString>)) -> mlua::Result<Value> {
    let what = what.as_ref().map(|w| w.as_bytes());
    let has = |c: u8| what.as_ref().is_none_or(|w| w.contains(&c));
    let t = lua.create_table_with_capacity(0, 6)?;
    let set_opt = |t: &Table, k: &str, v: Option<usize>| match v {
        Some(n) => t.set(k, i64::try_from(n).unwrap_or(-1)),
        None => t.set(k, -1),
    };
    match target {
        Value::Function(f) => {
            let info = f.info();
            t.set("source", info.source)?;
            t.set("short_src", info.short_src)?;
            t.set("what", info.what)?;
            set_opt(&t, "linedefined", info.line_defined)?;
            t.set("currentline", -1)?;
            t.set("func", f)?;
        }
        Value::Integer(_) | Value::Number(_) => {
            let level = match target {
                Value::Integer(i) => usize::try_from(i).ok(),
                Value::Number(n) if n >= 0.0 => Some(n as usize),
                _ => None,
            };
            let Some(level) = level else { return Ok(Value::Nil) };
            let found = lua.inspect_stack(level, |d| -> mlua::Result<()> {
                if has(b'S') {
                    let src = d.source();
                    t.set("source", src.source.as_deref())?;
                    t.set("short_src", src.short_src.as_deref())?;
                    t.set("what", src.what)?;
                    set_opt(&t, "linedefined", src.line_defined)?;
                }
                if has(b'l') {
                    set_opt(&t, "currentline", d.current_line())?;
                }
                if has(b'n') {
                    let names = d.names();
                    t.set("name", names.name.as_deref())?;
                    t.set("namewhat", names.name_what.unwrap_or(""))?;
                }
                if has(b'f') {
                    t.set("func", d.function())?;
                }
                Ok(())
            });
            match found {
                Some(r) => r?,
                None => return Ok(Value::Nil),
            }
        }
        other => return Err(mlua::Error::runtime(format!("bad argument #1 to 'getinfo' (number or function expected, got {})", other.type_name()))),
    }
    Ok(Value::Table(t))
}

/// The most frames a traceback lists.
const TRACEBACK_FRAMES: usize = 64;

/// `debug.traceback([thread,] [message [, level]])`. As in Lua, a message
/// that is neither a string, a number nor nil is returned as it is, and
/// `level` (default 1, the caller) is where the listing starts.
fn traceback(lua: &Lua, args: MultiValue) -> mlua::Result<Value> {
    let mut args: Vec<Value> = args.into_iter().collect();
    let mut other_thread = false;
    if let Some(Value::Thread(t)) = args.first() {
        other_thread = t.to_pointer() != lua.current_thread().to_pointer();
        args.remove(0);
    }
    let mut it = args.into_iter();
    let message = it.next().unwrap_or(Value::Nil);
    let level = match it.next() {
        Some(Value::Integer(i)) => usize::try_from(i).unwrap_or(0),
        Some(Value::Number(n)) if n >= 0.0 => n as usize,
        _ if other_thread => 0,
        _ => 1,
    };
    let mut text = match message {
        Value::Nil => String::new(),
        Value::String(_) | Value::Integer(_) | Value::Number(_) => match lua.coerce_string(message)? {
            Some(s) => format!("{}\n", s.to_string_lossy()),
            None => String::new(),
        },
        other => return Ok(other),
    };
    text.push_str("stack traceback:");
    if other_thread {
        text.push_str("\n\t(the frames of another coroutine are not available in `nlc run`)");
        return Ok(Value::String(lua.create_string(&text)?));
    }
    for level in level..level + TRACEBACK_FRAMES {
        let line = lua.inspect_stack(level, |d| {
            let src = d.source();
            let short = src.short_src.map(|s| s.to_string()).unwrap_or_else(|| "?".to_string());
            let name = d.names().name.map(|n| format!("function '{n}'")).unwrap_or_else(|| "?".to_string());
            match d.current_line() {
                Some(l) => format!("\n\t{short}:{l}: in {name}"),
                None => format!("\n\t{short}: in {name}"),
            }
        });
        match line {
            Some(line) => text.push_str(&line),
            None => break,
        }
    }
    Ok(Value::String(lua.create_string(&text)?))
}
