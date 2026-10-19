//! Opening a state, options, loading, the loader and the reporter.

use std::any::{Any, TypeId};
use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};

use mlua::{Error, Function, Lua, Result, Table, Value};

use crate::raw::{self, Shared};

pub use nl_capi::{Localize, Severity};

/// Rust data of an opened state, in mlua's app data. It is dropped after
/// `lua_close`, so whatever the VM holds a pointer to lives here.
pub(crate) struct NlState {
    pub(crate) shared: Arc<Shared>,
    /// Boxes the VM holds pointers to (timer, loader, reporter), kept until
    /// the state closes: a replaced one may still be running.
    kept: RefCell<Vec<Kept>>,
    wakeup: RefCell<Option<Box<Box<raw::WakeupFn>>>>,
    /// The names `register_type` gave Rust types, for `register_transfer`.
    pub(crate) types: RefCell<HashMap<TypeId, String>>,
    /// In a worker's state: the state as mlua sees it, to find, when the
    /// state closes, a `Lua` that outlived it (`Drop`).
    pub(crate) worker_lua: RefCell<Option<mlua::WeakLua>>,
    #[cfg(feature = "async")]
    pub(crate) spawner: RefCell<Option<Box<crate::future::SpawnFn>>>,
}

/// What app data may hold: anything, and with mlua's `send` feature only
/// what is `Send`.
#[cfg(feature = "send")]
pub(crate) trait Keep: Any + Send {}
#[cfg(feature = "send")]
impl<T: Any + Send> Keep for T {}
#[cfg(not(feature = "send"))]
pub(crate) trait Keep: Any {}
#[cfg(not(feature = "send"))]
impl<T: Any> Keep for T {}

type Kept = Box<dyn Keep>;

impl NlState {
    /// Keeps `b` until the state closes and returns its address.
    pub(crate) fn keep<T: Keep>(&self, b: Box<T>) -> *const T {
        let p: *const T = &*b;
        self.kept.borrow_mut().push(b);
        p
    }
}

impl Drop for NlState {
    /// mlua drops this with its own data for the state. For a worker's state
    /// that happens when the VM closes it; mlua has dropped its own `Lua` of
    /// the state just before, so a `Lua` that is still alive is a clone the
    /// host kept (through `WorkerInit::lua`, or the `&Lua` mlua gives a
    /// `UserData` method), which now points at a closed state. Using it would
    /// read freed memory: the process stops here instead.
    fn drop(&mut self) {
        let weak = self.worker_lua.get_mut().take();
        if weak.is_some_and(|w| w.try_upgrade().is_some()) {
            eprintln!(
                "nl: a Lua of a worker's state outlived the state (a clone kept past the call that \
                 received it); aborting rather than letting it use freed memory"
            );
            std::process::abort();
        }
    }
}

/// The nl data of `lua`, once `nl::open` has run.
pub(crate) fn nl_state(lua: &Lua) -> Result<mlua::AppDataRef<'_, NlState>> {
    lua.app_data_ref::<NlState>()
        .ok_or_else(|| Error::runtime("NL is not open in this state: call nl::open (or make it with nl::new)"))
}

/// One call of the crate that runs script code: the panics
/// parked on its thread while it runs are resumed when it ends, by [`Scope::finish`]; one
/// as itself, several together as a [`Panics`](crate::Panics). Panics parked
/// before it began are not its own: a nested call leaves them to the call
/// that runs it, and the outermost call drops them.
pub(crate) struct Scope {
    shared: Option<Arc<Shared>>,
    mark: usize,
}

pub(crate) fn enter(lua: &Lua) -> Scope {
    let shared = lua.app_data_ref::<NlState>().map(|s| s.shared.clone());
    let mark = shared.as_ref().map_or(0, |s| s.enter());
    Scope { shared, mark }
}

impl Scope {
    pub(crate) fn finish(mut self) {
        let Some(shared) = self.shared.take() else { return };
        let mut parked = shared.leave(self.mark);
        drop(shared);
        match parked.len() {
            0 => {}
            1 => std::panic::resume_unwind(parked.remove(0)),
            _ => std::panic::resume_unwind(Box::new(crate::Panics(parked))),
        }
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        if let Some(shared) = self.shared.take() {
            // Unwinding: what this call parked is dropped with the frame.
            drop(shared.leave(self.mark));
        }
    }
}

/// `f` in a [`Scope`].
pub(crate) fn scoped<T>(lua: &Lua, f: impl FnOnce() -> T) -> T {
    let scope = enter(lua);
    let r = f();
    scope.finish();
    r
}

/// Closes the inbox when `lua_close` runs the finalizers, before the VM
/// frees the inbox: from then on `Remote::post` returns false.
struct CloseGuard(Arc<Shared>);

impl Drop for CloseGuard {
    fn drop(&mut self) {
        self.0.close();
    }
}

/// `Lua::new()`, then [`open`].
pub fn new() -> Result<Lua> {
    let lua = Lua::new();
    open(&lua)?;
    Ok(lua)
}

/// Opens NL in a `Lua` the host made itself (`Lua::new_with`,
/// `Lua::unsafe_new`): `luaopen_nl`, which mlua never calls (it opens the
/// standard libraries one by one and never calls `luaL_openlibs`), and the
/// exception translator. The state needs the base, string, table, math,
/// bit and coroutine libraries. Calling it again does nothing.
///
/// The runtime library takes the debug library's `getmetatable` and
/// `getinfo` when it opens: mlua's userdata metatables have
/// `__metatable = false`, so host types (`register_type`) need the raw
/// `getmetatable`, and exception traces need `getinfo`. When the state has
/// no `debug` library, it is opened for the runtime and removed again
/// (global `debug` and `package.loaded.debug`), so scripts still cannot
/// reach it.
pub fn open(lua: &Lua) -> Result<()> {
    if lua.app_data_ref::<NlState>().is_some() {
        return Ok(());
    }
    if !raw::nl_is_open(lua) && raw::open_nl(lua)? {
        hide_debug(lua)?;
    }
    let shared = raw::new_shared(lua)?;
    let guard = lua.create_any_userdata(CloseGuard(shared.clone()))?;
    lua.set_named_registry_value("nl.close_guard", guard)?;
    lua.set_app_data(NlState {
        shared: shared.clone(),
        kept: RefCell::new(Vec::new()),
        wakeup: RefCell::new(None),
        types: RefCell::new(HashMap::new()),
        worker_lua: RefCell::new(None),
        #[cfg(feature = "async")]
        spawner: RefCell::new(None),
    });
    crate::exception::install_translator(lua, &shared)
}

/// Removes the debug library from what scripts reach: the global and
/// `package.loaded.debug`.
pub(crate) fn hide_debug(lua: &Lua) -> Result<()> {
    lua.globals().raw_set("debug", Value::Nil)?;
    if let Ok(loaded) = lua.named_registry_value::<Table>("_LOADED") {
        loaded.raw_set("debug", Value::Nil)?;
    }
    Ok(())
}

// -- Options ------------------------------------------------------------------

/// The options of `nl_setoption`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NlOption {
    /// `NL_OPT_CHECKS`: runtime type checks in code compiled from now on.
    Checks(bool),
    /// `NL_OPT_STRIP`: leave debug information out of compiled code.
    Strip(bool),
    /// `NL_OPT_MAXWORKERS`: the most dedicated workers alive at once.
    MaxWorkers(usize),
    /// `NL_OPT_WORKERMEM`: memory limit of each worker state in bytes; 0 is
    /// none.
    WorkerMemory(usize),
    /// `NL_OPT_GLUA`: Lua chunks loaded from now on are GLua.
    Glua(bool),
    /// `NL_OPT_SANDBOX`: the sandbox profile; one way.
    Sandbox(bool),
    /// `NL_OPT_LOCALIZE`: which Lua paths compiled code reads once.
    Localize(Localize),
}

impl NlOption {
    fn raw(self) -> (std::ffi::c_int, i64) {
        let b = |v: bool| i64::from(v);
        let n = |v: usize| i64::try_from(v).unwrap_or(i64::MAX);
        match self {
            NlOption::Checks(v) => (nl_sys::NL_OPT_CHECKS, b(v)),
            NlOption::Strip(v) => (nl_sys::NL_OPT_STRIP, b(v)),
            NlOption::MaxWorkers(v) => (nl_sys::NL_OPT_MAXWORKERS, n(v)),
            NlOption::WorkerMemory(v) => (nl_sys::NL_OPT_WORKERMEM, n(v)),
            NlOption::Glua(v) => (nl_sys::NL_OPT_GLUA, b(v)),
            NlOption::Sandbox(v) => (nl_sys::NL_OPT_SANDBOX, b(v)),
            NlOption::Localize(v) => (nl_sys::NL_OPT_LOCALIZE, v.level()),
        }
    }
}

pub(crate) fn set_option(lua: &Lua, option: NlOption) -> Result<()> {
    let (opt, value) = option.raw();
    if raw::setoption(lua, opt, value)? {
        Ok(())
    } else {
        Err(Error::runtime(format!("nl_setoption refused {option:?}")))
    }
}

// -- Loading ------------------------------------------------------------------

pub(crate) fn load_nl(lua: &Lua, source: &[u8], name: &str) -> Result<Function> {
    raw::loadbuffer(lua, source, name)
}

pub(crate) fn load_nl_file(lua: &Lua, path: &Path) -> Result<Function> {
    raw::loadfile(lua, path)
}

pub(crate) fn import(lua: &Lua, path: &str) -> Result<Table> {
    let v = crate::exception::values(lua, || raw::import(lua, path))?;
    match v.into_iter().next() {
        Some(Value::Table(t)) => Ok(t),
        other => Err(Error::runtime(format!("import '{path}' gave {other:?}, not an export table"))),
    }
}

// -- The loader ---------------------------------------------------------------

/// How files are found and read (`nl_Loader`). Called from any
/// thread. A panic in it aborts the process.
pub trait Loader: Send + Sync + 'static {
    /// The canonical name of the file an `import` path means; `from` is the
    /// canonical name of the importing file. `None` when there is no such
    /// file.
    fn resolve(&self, from: Option<&str>, path: &str) -> Option<String>;
    /// The contents of a canonical name, or of a name `nl_loadfile`,
    /// `luaL_loadfile` or `include` was given. A borrowed slice is used as it
    /// is; an owned one is kept until the compiler is done with it (C's
    /// `release`).
    fn read(&self, name: &str) -> Option<Cow<'_, [u8]>>;
}

/// An error when workers exist and `what`, on the main state, would no
/// longer reach them (the workers' settings are fixed once the
/// first worker starts, and `nl_addpath`, `nl_adddecl` and `nl_setloader`
/// then do nothing).
pub(crate) fn unless_frozen(lua: &Lua, what: &str) -> Result<()> {
    if raw::workers_frozen(lua)? {
        return Err(Error::runtime(format!(
            "{what}: workers exist, so the main state's search path, declaration files and loader are fixed \
             (set them before the first spawn, Worker.new or nl::NlExt::spawn)"
        )));
    }
    Ok(())
}

pub(crate) fn set_loader(lua: &Lua, loader: Box<dyn Loader>) -> Result<()> {
    let st = nl_state(lua)?;
    let lb = st.keep(Box::new(raw::LoaderBox { loader, owned: Mutex::new(Default::default()) }));
    raw::setloader(lua, Some(lb))
}

// -- Diagnostics --------------------------------------------------------------

/// One diagnostic of a compile (`nl_Diagnostic`). Lines and
/// columns are 1-based.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    /// A stable identifier, such as `"undeclared-name"`.
    pub code: &'static str,
    pub message: String,
    /// The resolved name of the file.
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

/// The codes are few and stable: each is leaked once, so that
/// `Diagnostic::code` can be `&'static str`.
pub(crate) fn intern(code: &str) -> &'static str {
    static CODES: OnceLock<Mutex<BTreeSet<&'static str>>> = OnceLock::new();
    let mut set = CODES
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(c) = set.get(code) {
        return c;
    }
    let c: &'static str = Box::leak(code.to_owned().into_boxed_str());
    set.insert(c);
    c
}

pub(crate) fn set_reporter(lua: &Lua, f: Box<raw::ReporterFn>) -> Result<()> {
    let st = nl_state(lua)?;
    let p = st.keep(Box::new(f));
    raw::setreporter(lua, p)
}

// -- The wakeup -----------------------------------------------------------------

pub(crate) fn set_wakeup(lua: &Lua, f: Box<raw::WakeupFn>) -> Result<()> {
    let st = nl_state(lua)?;
    let b = Box::new(f);
    raw::setwakeup(lua, &*b)?;
    // nl_setwakeup returned: the previous function is no longer called.
    st.wakeup.replace(Some(b));
    Ok(())
}

/// `nl_setworkerinit` with a Rust closure. The closure is kept
/// until the state closes: a worker that is starting may still call an old
/// one.
pub(crate) fn set_worker_init(lua: &Lua, f: Box<raw::WorkerInitFn>) -> Result<()> {
    let st = nl_state(lua)?;
    let p = st.keep(Box::new(f));
    raw::setworkerinit(lua, p)
}

pub(crate) fn set_timer(lua: &Lua, f: Box<raw::TimerFn>) -> Result<()> {
    let st = nl_state(lua)?;
    let p = st.keep(Box::new(raw::TimerBox { f, shared: st.shared.clone() }));
    raw::settimer(lua, p)
}
