//! Every call into `nl.h` and every callback the VM calls.
//!
//! This is the only module with unsafe code. The rules it keeps:
//!
//! - A call that can raise runs inside `Lua::exec_raw`, which makes it in a
//!   protected call. Calls that run script code go further: they use
//!   `nl_pcall` (or `nl_prunevent`, which is the same), so that a failure
//!   leaves the exception itself on the stack, not mlua's text of it. Where
//!   the `nl.h` function raises on its own (`nl_import`, `nl_new`,
//!   `nlL_callmember`, ...), a small C function below makes the call and
//!   `nl_pcall` calls it.
//! - Inside an `exec_raw` closure no value that needs dropping is alive when
//!   a function that can raise is called: a Lua error unwinds through the
//!   closure's frame. Strings are converted to `CString`s before.
//! - A callback the VM calls from inside a Lua call (the timer, a posted
//!   closure) catches panics; the payload is parked in [`Shared`] and
//!   resumed when the nl call that ran it returns (`crate::resume_parked`).
//!   A callback with no Rust caller to return to (loader, reporter,
//!   wakeup) aborts on a panic.
#![allow(unsafe_code)]

use std::any::Any;
use std::borrow::Cow;
use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_double, c_int, c_void};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicPtr, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use mlua::ffi as lua;
use mlua::{Error, Function, Lua, MultiValue, Result, Table, Value};
use nl_sys as nl;

use crate::state::{Diagnostic, Loader};

// -- Shared state: the inbox guard, parked panics --------------------------

/// A `lua_State` pointer that may cross threads. Only `nl_post` uses it on
/// another thread, which nl.h allows.
#[derive(Clone, Copy)]
struct StatePtr(*mut lua::lua_State);
// SAFETY: the pointer is only dereferenced by nl_post (callable from any
// thread) and, on the state's own thread, by the functions below.
unsafe impl Send for StatePtr {}
// SAFETY: as for Send; the pointer itself is never written after creation.
unsafe impl Sync for StatePtr {}

#[derive(Default)]
struct Parked {
    depth: usize,
    panics: Vec<Box<dyn Any + Send>>,
}

struct InboxState {
    closed: bool,
    active: usize,
}

/// What the state shares with other threads: the right to post to its
/// inbox while it is open (`Remote`), and the panics of
/// callbacks that no Rust frame could receive when they happened.
pub(crate) struct Shared {
    /// A thread of the state, anchored in its registry, so alive until
    /// `lua_close`; `nl_post` takes any live thread.
    state: StatePtr,
    inbox: Mutex<InboxState>,
    idle: Condvar,
    /// Per thread (several, with mlua's `send` feature: mlua's lock lets
    /// threads take turns inside one state, between which their calls of
    /// the crate interleave): the panics parked there, and how many calls of
    /// the crate that run script code are in progress there
    /// (`crate::state::Scope`).
    parked: Mutex<HashMap<std::thread::ThreadId, Parked>>,
    /// A worker's state: no Rust frame can receive a panic there, so a
    /// parked one is dropped.
    worker: AtomicBool,
    /// The posted closure that the runner (`post_runner`) takes next.
    current_post: AtomicPtr<c_void>,
    /// The registry reference of the runner.
    runner_ref: AtomicI32,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Shared {
    /// For a state that is not open: posting always fails.
    pub(crate) fn closed() -> Shared {
        Shared {
            state: StatePtr(ptr::null_mut()),
            inbox: Mutex::new(InboxState { closed: true, active: 0 }),
            idle: Condvar::new(),
            parked: Mutex::new(HashMap::new()),
            worker: AtomicBool::new(false),
            current_post: AtomicPtr::new(ptr::null_mut()),
            runner_ref: AtomicI32::new(lua::LUA_NOREF),
        }
    }

    /// Keeps the payload of a panic that no Rust frame could receive when
    /// it happened, for the call of the crate that runs. In a
    /// worker's state it is dropped: the worker's loop is C, and nothing
    /// there takes it back.
    pub(crate) fn park(&self, payload: Box<dyn Any + Send>) {
        if self.worker.load(Ordering::SeqCst) {
            drop(payload);
            return;
        }
        lock(&self.parked).entry(std::thread::current().id()).or_default().panics.push(payload);
    }

    /// A call of the crate begins on this thread: returns how many panics
    /// are parked for this thread. The outermost call of the thread first
    /// drops what earlier code on it parked and nobody took (under mlua's own
    /// `Function::call`, for one); the old payloads are dropped outside the
    /// lock.
    pub(crate) fn enter(&self) -> usize {
        let id = std::thread::current().id();
        let (old, mark) = {
            let mut parked = lock(&self.parked);
            let p = parked.entry(id).or_default();
            p.depth += 1;
            let old = if p.depth == 1 { std::mem::take(&mut p.panics) } else { Vec::new() };
            (old, p.panics.len())
        };
        drop(old);
        mark
    }

    /// The call of [`enter`](Shared::enter) ends: the panics this thread
    /// parked since `mark`. A thread with no call in progress and nothing
    /// parked is forgotten.
    pub(crate) fn leave(&self, mark: usize) -> Vec<Box<dyn Any + Send>> {
        let mut parked = lock(&self.parked);
        let id = std::thread::current().id();
        let Some(p) = parked.get_mut(&id) else { return Vec::new() };
        p.depth = p.depth.saturating_sub(1);
        let mine = p.panics.split_off(mark.min(p.panics.len()));
        if p.depth == 0 && p.panics.is_empty() {
            parked.remove(&id);
        }
        mine
    }

    /// Marks the state closed and waits for posts in progress. Called while
    /// `lua_close` runs the finalizers, before the inbox is freed.
    pub(crate) fn close(&self) {
        let mut g = lock(&self.inbox);
        g.closed = true;
        while g.active > 0 {
            g = self.idle.wait(g).unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// `nl_post(L, f, data)` unless the state has closed. On false `data`
    /// was not queued: the caller still owns it.
    fn post_raw(&self, f: nl::nl_Callback, data: *mut c_void) -> bool {
        {
            let mut g = lock(&self.inbox);
            if g.closed {
                return false;
            }
            g.active += 1;
        }
        // SAFETY: the state is open (closed is false and close() waits for
        // `active` to drop to zero before lua_close frees the inbox), and
        // nl_post may be called from any thread.
        let ok = unsafe { nl::nl_post(self.state.0, f, data) } != 0;
        let mut g = lock(&self.inbox);
        g.active -= 1;
        if g.active == 0 {
            self.idle.notify_all();
        }
        ok
    }
}

/// A closure posted with `Remote::post`.
pub(crate) type PostFn = Box<dyn FnOnce(&Lua) -> Result<()> + Send>;

struct PostBox {
    f: PostFn,
    shared: Arc<Shared>,
}

/// Queues `f` for the next pump. False when the state has closed; `f` is
/// dropped then.
pub(crate) fn post(shared: &Arc<Shared>, f: PostFn) -> bool {
    let data = Box::into_raw(Box::new(PostBox { f, shared: shared.clone() }));
    if shared.post_raw(post_cb, data.cast()) {
        true
    } else {
        // SAFETY: not queued, so this is the only owner of the box.
        drop(unsafe { Box::from_raw(data) });
        false
    }
}

/// The posted callback (`nl_Callback`): runs the closure through the runner,
/// an mlua function, so that mlua turns its error or panic into a Lua error;
/// the pump's protected call gives that to the unhandled hook, and the
/// translator parks a panic.
unsafe extern "C-unwind" fn post_cb(l: *mut lua::lua_State, data: *mut c_void) {
    let data = data.cast::<PostBox>();
    if l.is_null() {
        // The state closed first: free the closure. This runs
        // inside lua_close, with no Rust caller to give a panic to: a Drop of
        // what the closure captured that panics aborts (as mlua's own
        // finalizers do).
        // SAFETY: the box was made by `post` and is passed here once.
        drop_or_abort(unsafe { Box::from_raw(data) });
        return;
    }
    let (runner, slot) = {
        // SAFETY: data is a live PostBox until the runner takes it; the
        // reference ends before the runner runs. The slot belongs to the
        // Shared that the PostBox holds, and that a Remote or the state keeps
        // alive past this call.
        let pb = unsafe { &*data };
        pb.shared.current_post.store(data.cast(), Ordering::SeqCst);
        (pb.shared.runner_ref.load(Ordering::SeqCst), &pb.shared.current_post as *const AtomicPtr<c_void>)
    };
    // SAFETY: called by nl_pump inside a protected call on the state's
    // thread. The runner runs under a protected call of its own, so that a
    // failure before it took the closure (the stack cannot grow) still
    // frees it; its error is raised again for the pump's handler, with
    // nothing in this frame that needs dropping.
    unsafe {
        lua::lua_rawgeti(l, lua::LUA_REGISTRYINDEX, runner as lua::lua_Integer);
        let st = lua::lua_pcall(l, 0, 0, 0);
        let left = (*slot).swap(ptr::null_mut(), Ordering::SeqCst);
        if !left.is_null() {
            drop_or_abort(Box::from_raw(left.cast::<PostBox>()));
        }
        if st != 0 {
            lua::lua_error(l);
        }
    }
}

/// The function that runs posted closures; its registry reference goes to
/// `Shared::runner_ref`. It takes the closure from the shared slot, never
/// from its arguments, so a script that finds it can only make it fail.
pub(crate) fn post_runner(lua: &Lua, shared: &Arc<Shared>) -> Result<Function> {
    let shared = shared.clone();
    lua.create_function(move |lua, ()| {
        let p = shared.current_post.swap(ptr::null_mut(), Ordering::SeqCst);
        if p.is_null() {
            return Err(Error::runtime("an internal function of nl was called"));
        }
        // SAFETY: post_cb stored a PostBox made by `post`, and swapping the
        // slot to null makes this the one owner.
        let b = unsafe { Box::from_raw(p.cast::<PostBox>()) };
        (b.f)(lua)
    })
}

/// Anchors a new thread in the registry and returns the shared state for
/// it, with the runner installed.
pub(crate) fn new_shared(lua: &Lua) -> Result<Arc<Shared>> {
    let mut thread = ptr::null_mut();
    // SAFETY: lua_newthread and luaL_ref raise only memory errors, which
    // exec_raw catches.
    unsafe {
        lua.exec_raw::<()>((), |l| {
            thread = lua::lua_newthread(l);
            lua::luaL_ref(l, lua::LUA_REGISTRYINDEX);
        })?;
    }
    let shared = Arc::new(Shared {
        state: StatePtr(thread),
        inbox: Mutex::new(InboxState { closed: false, active: 0 }),
        idle: Condvar::new(),
        parked: Mutex::new(HashMap::new()),
        worker: AtomicBool::new(isworker(lua)?),
        current_post: AtomicPtr::new(ptr::null_mut()),
        runner_ref: AtomicI32::new(lua::LUA_NOREF),
    });
    let runner = post_runner(lua, &shared)?;
    let mut r = lua::LUA_NOREF;
    // SAFETY: as above.
    unsafe {
        lua.exec_raw::<()>(runner, |l| {
            r = lua::luaL_ref(l, lua::LUA_REGISTRYINDEX);
        })?;
    }
    shared.runner_ref.store(r, Ordering::SeqCst);
    Ok(shared)
}

// -- Opening ----------------------------------------------------------------

/// `luaopen_nl`, with the debug library visible to the runtime while it
/// opens and removed afterwards when the state did not have it. The runtime
/// takes `debug.getmetatable` and `debug.getinfo` once, when it opens
/// (runtime/nl.lua): mlua's userdata metatables have `__metatable = false`,
/// so host types need the raw `getmetatable`, and traces need `getinfo`.
/// Returns whether the debug library was opened here.
pub(crate) fn open_nl(lua: &Lua) -> Result<bool> {
    let mut opened_debug = false;
    // SAFETY: luaopen_debug and luaopen_nl raise Lua errors, which exec_raw
    // catches; nothing here needs dropping.
    unsafe {
        lua.exec_raw::<()>((), |l| {
            lua::lua_getfield(l, lua::LUA_GLOBALSINDEX, c"debug".as_ptr());
            let has = lua::lua_istable(l, -1) != 0;
            lua::lua_settop(l, 0);
            if !has {
                opened_debug = true;
                lua::lua_pushcfunction(l, lua::luaopen_debug);
                lua::lua_call(l, 0, 0);
            }
            nl::luaopen_nl(l);
        })?;
    }
    Ok(opened_debug)
}

/// Whether `luaopen_nl` has run in this state: only then does
/// `nl_gettype` know the built-in types.
pub(crate) fn nl_is_open(lua: &Lua) -> bool {
    gettype(lua, "Exception").map(|v| !v.is_nil()).unwrap_or(false)
}

/// `nl_settranslator`: pops the function.
pub(crate) fn set_translator(lua: &Lua, f: Function) -> Result<()> {
    // SAFETY: nl_settranslator raises nothing.
    unsafe { lua.exec_raw::<()>(f, |l| nl::nl_settranslator(l)) }
}

/// `nl_setunhandled`: pops the function.
pub(crate) fn set_unhandled(lua: &Lua, f: Function) -> Result<()> {
    // SAFETY: nl_setunhandled raises nothing.
    unsafe { lua.exec_raw::<()>(f, |l| nl::nl_setunhandled(l)) }
}

// -- Protected calls ---------------------------------------------------------

/// The result of a protected call: the values, or the status and the error
/// value (an exception, or Lua's message for `LUA_ERRMEM`).
pub(crate) enum Outcome {
    Ok(MultiValue),
    Failed(c_int, Value),
}

fn outcome(status: c_int, out: MultiValue) -> Outcome {
    if status == 0 {
        Outcome::Ok(out)
    } else {
        Outcome::Failed(status, out.into_iter().next().unwrap_or(Value::Nil))
    }
}

/// `f(args...)` under `nl_pcall`.
pub(crate) fn pcall(lua: &Lua, f: &Function, args: MultiValue) -> Result<Outcome> {
    let mut all = args;
    all.push_front(Value::Function(f.clone()));
    let mut status = 0;
    // SAFETY: nl_pcall raises nothing; the stack holds the function and its
    // arguments.
    let out: MultiValue = unsafe {
        lua.exec_raw(all, |l| {
            status = nl::nl_pcall(l, lua::lua_gettop(l) - 1, lua::LUA_MULTRET);
        })?
    };
    Ok(outcome(status, out))
}

/// The C function `f` called with `args` under `nl_pcall`.
fn pcall_c(lua: &Lua, f: lua::lua_CFunction, args: MultiValue) -> Result<Outcome> {
    let mut status = 0;
    // SAFETY: lua_pushcfunction raises only a memory error, which exec_raw
    // catches; nl_pcall raises nothing.
    let out: MultiValue = unsafe {
        lua.exec_raw(args, |l| {
            lua::lua_pushcfunction(l, f);
            lua::lua_insert(l, 1);
            status = nl::nl_pcall(l, lua::lua_gettop(l) - 1, lua::LUA_MULTRET);
        })?
    };
    Ok(outcome(status, out))
}

/// Reads the string argument `idx` of a C function below.
///
/// # Safety
///
/// Called by a C function with a valid state; the string stays on the stack
/// while the pointer is used.
unsafe fn arg_str(l: *mut lua::lua_State, idx: c_int) -> *const c_char {
    // SAFETY: the caller's guarantees; luaL_checkstring raises on a value
    // that is not a string.
    unsafe { lua::luaL_checklstring(l, idx, ptr::null_mut()) }
}

unsafe extern "C-unwind" fn c_import(l: *mut lua::lua_State) -> c_int {
    // SAFETY: called by nl_pcall with the path at 1.
    unsafe {
        let path = arg_str(l, 1);
        nl::nl_import(l, path);
    }
    1
}

unsafe extern "C-unwind" fn c_callmember(l: *mut lua::lua_State) -> c_int {
    // SAFETY: called by nl_pcall with the name at 1, then the receiver and
    // the arguments.
    unsafe {
        let name = arg_str(l, 1);
        nl::nlL_callmember(l, name, lua::lua_gettop(l) - 2, lua::LUA_MULTRET);
        lua::lua_gettop(l) - 1
    }
}

unsafe extern "C-unwind" fn c_new(l: *mut lua::lua_State) -> c_int {
    // SAFETY: called by nl_pcall with the type at 1 and the fields at 2.
    unsafe {
        lua::lua_settop(l, 2);
        nl::nl_new(l, 1);
    }
    1
}

unsafe extern "C-unwind" fn c_keyed(l: *mut lua::lua_State) -> c_int {
    // SAFETY: called by nl_pcall with the table at 1.
    unsafe {
        lua::lua_settop(l, 1);
        nl::nl_keyed(l, 1);
    }
    1
}

unsafe extern "C-unwind" fn c_settype(l: *mut lua::lua_State) -> c_int {
    // SAFETY: called by nl_pcall with the name at 1 and a userdata at 2.
    unsafe {
        let name = arg_str(l, 1);
        lua::lua_settop(l, 2);
        if lua::lua_getmetatable(l, 2) == 0 {
            return lua::luaL_error(l, c"register_type: the userdata has no metatable".as_ptr());
        }
        nl::nl_settype(l, 3, name, 0);
        lua::lua_pushboolean(l, 1);
        lua::lua_setfield(l, 3, c"__methods".as_ptr());
    }
    0
}

unsafe extern "C-unwind" fn c_resolve(l: *mut lua::lua_State) -> c_int {
    // SAFETY: called by nl_pcall with the task at 1 and the value at 2.
    unsafe {
        lua::lua_settop(l, 2);
        nl::nl_resolve(l, 1);
    }
    0
}

unsafe extern "C-unwind" fn c_reject(l: *mut lua::lua_State) -> c_int {
    // SAFETY: called by nl_pcall with the task at 1 and the error at 2.
    unsafe {
        lua::lua_settop(l, 2);
        nl::nl_reject(l, 1);
    }
    0
}

pub(crate) fn import(lua: &Lua, path: &str) -> Result<Outcome> {
    pcall_c(lua, c_import, MultiValue::from_vec(vec![Value::String(lua.create_string(path)?)]))
}

pub(crate) fn callmember(lua: &Lua, name: &str, receiver: Value, args: MultiValue) -> Result<Outcome> {
    let mut all = args;
    all.push_front(receiver);
    all.push_front(Value::String(lua.create_string(name)?));
    pcall_c(lua, c_callmember, all)
}

pub(crate) fn new_instance(lua: &Lua, ty: &Table, fields: Value) -> Result<Outcome> {
    pcall_c(lua, c_new, MultiValue::from_vec(vec![Value::Table(ty.clone()), fields]))
}

pub(crate) fn keyed(lua: &Lua, opts: Table) -> Result<Outcome> {
    pcall_c(lua, c_keyed, MultiValue::from_vec(vec![Value::Table(opts)]))
}

pub(crate) fn settype(lua: &Lua, name: &str, ud: Value) -> Result<Outcome> {
    let name = Value::String(lua.create_string(name)?);
    pcall_c(lua, c_settype, MultiValue::from_vec(vec![name, ud]))
}

pub(crate) fn resolve(lua: &Lua, task: &Table, value: Value) -> Result<Outcome> {
    pcall_c(lua, c_resolve, MultiValue::from_vec(vec![Value::Table(task.clone()), value]))
}

pub(crate) fn reject(lua: &Lua, task: &Table, error: Value) -> Result<Outcome> {
    pcall_c(lua, c_reject, MultiValue::from_vec(vec![Value::Table(task.clone()), error]))
}

// -- Calls that raise nothing, or only memory errors -------------------------

fn cstring(s: &str) -> Result<CString> {
    CString::new(s).map_err(|_| Error::runtime(format!("nl: a name contains a NUL byte: {s:?}")))
}

pub(crate) fn path_cstring(p: &std::path::Path) -> Result<CString> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        CString::new(p.as_os_str().as_bytes())
            .map_err(|_| Error::runtime(format!("nl: a path contains a NUL byte: {}", p.display())))
    }
    #[cfg(not(unix))]
    {
        match p.to_str() {
            Some(s) => cstring(s),
            None => Err(Error::runtime(format!("nl: a path is not UTF-8: {}", p.display()))),
        }
    }
}

/// The status and the value `nl_loadbuffer` or `nl_loadfile` left.
fn load_result(status: c_int, v: Value) -> Result<Function> {
    match (status, v) {
        (0, Value::Function(f)) => Ok(f),
        (0, _) => Err(Error::runtime("nl: the loader returned no function")),
        (st, v) => {
            let message = match v {
                Value::String(s) => s.to_string_lossy(),
                other => format!("{other:?}"),
            };
            Err(match st {
                lua::LUA_ERRSYNTAX => Error::SyntaxError { message, incomplete_input: false },
                lua::LUA_ERRMEM => Error::MemoryError(message),
                _ => Error::RuntimeError(message),
            })
        }
    }
}

pub(crate) fn loadbuffer(lua: &Lua, source: &[u8], name: &str) -> Result<Function> {
    let name = cstring(name)?;
    let mut status = 0;
    // SAFETY: nl_loadbuffer raises only memory errors; source and name
    // outlive the call.
    let v: Value = unsafe {
        lua.exec_raw((), |l| {
            status = nl::nl_loadbuffer(l, source.as_ptr().cast(), source.len(), name.as_ptr());
        })?
    };
    load_result(status, v)
}

pub(crate) fn loadfile(lua: &Lua, path: &std::path::Path) -> Result<Function> {
    let path = path_cstring(path)?;
    let mut status = 0;
    // SAFETY: nl_loadfile raises only memory errors; path outlives the call.
    let v: Value = unsafe {
        lua.exec_raw((), |l| {
            status = nl::nl_loadfile(l, path.as_ptr());
        })?
    };
    load_result(status, v)
}

pub(crate) fn addpath(lua: &Lua, dir: &std::path::Path) -> Result<()> {
    let dir = path_cstring(dir)?;
    // SAFETY: nl_addpath raises only memory errors; dir outlives the call.
    unsafe { lua.exec_raw::<()>((), |l| nl::nl_addpath(l, dir.as_ptr())) }
}

pub(crate) fn adddecl(lua: &Lua, name: &str) -> Result<()> {
    let name = cstring(name)?;
    // SAFETY: nl_adddecl raises only memory errors.
    unsafe { lua.exec_raw::<()>((), |l| nl::nl_adddecl(l, name.as_ptr())) }
}

pub(crate) fn setoption(lua: &Lua, opt: c_int, value: i64) -> Result<bool> {
    let mut ok = 0;
    // SAFETY: nl_setoption raises nothing.
    unsafe {
        lua.exec_raw::<()>((), |l| {
            ok = nl::nl_setoption(l, opt, value as lua::lua_Integer);
        })?;
    }
    Ok(ok != 0)
}

/// Whether the main state's workers' settings are fixed because a worker
/// exists. False in a worker itself, whose own search may still widen
/// (`nl.h`: `nl_addpath` and `nl_adddecl` have no effect exactly when
/// `nl_workersstarted(L) && !nl_isworker(L)`).
pub(crate) fn workers_frozen(lua: &Lua) -> Result<bool> {
    let mut frozen = false;
    // SAFETY: nl_isworker and nl_workersstarted raise nothing.
    unsafe {
        lua.exec_raw::<()>((), |l| {
            frozen = nl::nl_workersstarted(l) != 0 && nl::nl_isworker(l) == 0;
        })?;
    }
    Ok(frozen)
}

pub(crate) fn sandboxenv(lua: &Lua) -> Result<Table> {
    // SAFETY: nl_sandboxenv raises only memory errors.
    unsafe { lua.exec_raw((), |l| nl::nl_sandboxenv(l)) }
}

pub(crate) fn gettype(lua: &Lua, name: &str) -> Result<Value> {
    let name = cstring(name)?;
    // SAFETY: nl_gettype raises nothing and pushes one value.
    unsafe { lua.exec_raw((), |l| nl::nl_gettype(l, name.as_ptr())) }
}

pub(crate) fn typeof_(lua: &Lua, v: Value) -> Result<Option<String>> {
    let mut name = None;
    // SAFETY: nl_typeof raises nothing; the string belongs to the state and
    // is copied before the stack is restored.
    unsafe {
        lua.exec_raw::<()>(v, |l| {
            let s = nl::nl_typeof(l, 1);
            if !s.is_null() {
                name = Some(CStr::from_ptr(s).to_string_lossy().into_owned());
            }
        })?;
    }
    Ok(name)
}

pub(crate) fn typename(lua: &Lua, ty: &Table) -> Result<Option<String>> {
    let mut name = None;
    // SAFETY: nl_typename raises nothing; the string belongs to the state
    // and is copied before the stack is restored.
    unsafe {
        lua.exec_raw::<()>(ty.clone(), |l| {
            let s = nl::nl_typename(l, 1);
            if !s.is_null() {
                name = Some(CStr::from_ptr(s).to_string_lossy().into_owned());
            }
        })?;
    }
    Ok(name)
}

pub(crate) fn isa(lua: &Lua, v: Value, ty: &Table) -> Result<bool> {
    let mut r = 0;
    // SAFETY: nl_isa raises nothing.
    unsafe {
        lua.exec_raw::<()>((v, ty.clone()), |l| {
            r = nl::nl_isa(l, 1, 2);
        })?;
    }
    Ok(r != 0)
}

pub(crate) fn pushrange(lua: &Lua, first: f64, last: f64, exclusive: bool) -> Result<Value> {
    // SAFETY: nl_pushrange raises only memory errors (the runtime's range
    // constructor checks nothing that a number fails).
    unsafe { lua.exec_raw((), |l| nl::nl_pushrange(l, first, last, exclusive as c_int)) }
}

pub(crate) fn torange(lua: &Lua, v: Value) -> Result<Option<(f64, f64, bool)>> {
    let (mut first, mut last, mut exclusive) = (0.0, 0.0, 0);
    let mut ok = 0;
    // SAFETY: nl_torange raises nothing.
    unsafe {
        lua.exec_raw::<()>(v, |l| {
            ok = nl::nl_torange(l, 1, &mut first, &mut last, &mut exclusive);
        })?;
    }
    Ok((ok != 0).then_some((first, last, exclusive != 0)))
}

pub(crate) fn markmethod(lua: &Lua, f: &Function) -> Result<()> {
    // SAFETY: nl_markmethod raises nothing.
    unsafe { lua.exec_raw::<()>(f.clone(), |l| nl::nl_markmethod(l, 1)) }
}

pub(crate) fn ismethod(lua: &Lua, f: &Function) -> Result<bool> {
    let mut r = 0;
    // SAFETY: nl_ismethod raises nothing.
    unsafe {
        lua.exec_raw::<()>(f.clone(), |l| {
            r = nl::nl_ismethod(l, 1);
        })?;
    }
    Ok(r != 0)
}

// -- Events -------------------------------------------------------------------

pub(crate) fn prunevent(lua: &Lua, name: &str, args: MultiValue) -> Result<Outcome> {
    let name = cstring(name)?;
    let mut status = 0;
    // SAFETY: nl_prunevent raises nothing; name outlives the call.
    let out: MultiValue = unsafe {
        lua.exec_raw(args, |l| {
            status = nl::nl_prunevent(l, name.as_ptr(), lua::lua_gettop(l), lua::LUA_MULTRET);
        })?
    };
    Ok(outcome(status, out))
}

pub(crate) fn onevent(lua: &Lua, name: &str, handler: &str, f: Function) -> Result<()> {
    let (name, handler) = (cstring(name)?, cstring(handler)?);
    // SAFETY: nl_onevent pops the function; it raises a memory error, or an
    // error for a value that is not a function, which exec_raw catches.
    unsafe { lua.exec_raw::<()>(f, |l| nl::nl_onevent(l, name.as_ptr(), handler.as_ptr())) }
}

pub(crate) fn offevent(lua: &Lua, name: &str, handler: &str) -> Result<()> {
    let (name, handler) = (cstring(name)?, cstring(handler)?);
    // SAFETY: nl_offevent raises nothing.
    unsafe { lua.exec_raw::<()>((), |l| nl::nl_offevent(l, name.as_ptr(), handler.as_ptr())) }
}

// -- Tasks ---------------------------------------------------------------------

pub(crate) fn newtask(lua: &Lua) -> Result<Table> {
    // SAFETY: nl_newtask raises only memory errors.
    unsafe { lua.exec_raw((), |l| nl::nl_newtask(l)) }
}

/// `nl_taskstate`: the state, and the value or exception.
pub(crate) fn taskstate(lua: &Lua, task: &Table) -> Result<(c_int, Value)> {
    let mut st = -1;
    // SAFETY: nl_taskstate raises nothing; it pushes the value or exception
    // for a finished task, which is returned with the task below it.
    let out: MultiValue = unsafe {
        lua.exec_raw(task.clone(), |l| {
            st = nl::nl_taskstate(l, 1);
        })?
    };
    Ok((st, out.into_iter().nth(1).unwrap_or(Value::Nil)))
}

/// The posted wake of a `Task.sleep`: the VM's `wake` and
/// `arg`, called once, on the state's thread through the inbox, or with
/// `NULL` to free `arg` when the state has closed.
pub struct Wake {
    wake: nl::nl_Callback,
    arg: *mut c_void,
    shared: Arc<Shared>,
}

// SAFETY: `arg` is only ever handed to `wake`: posted to the state's inbox
// (nl_post is callable from any thread), or called with L == NULL, which only
// frees it (lj_nlapi.c, timer_wake) and is safe on any thread.
unsafe impl Send for Wake {}

impl Wake {
    /// Ends the `Task.sleep` this wake belongs to. It is posted: the
    /// sleeping task resumes at the next pump.
    pub fn wake(self) {
        drop(self);
    }
}

impl Drop for Wake {
    /// A wake dropped unused ends its sleep as `wake` does, so script code
    /// is not left waiting forever (as a dropped `Resolver` rejects).
    fn drop(&mut self) {
        if !self.shared.post_raw(self.wake, self.arg) {
            // SAFETY: the state has closed; with L == NULL the VM's wake only
            // frees arg.
            unsafe { (self.wake)(ptr::null_mut(), self.arg) };
        }
    }
}

impl std::fmt::Debug for Wake {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Wake")
    }
}

#[cfg(feature = "send")]
pub(crate) type TimerFn = dyn Fn(Duration, Wake) + Send;
#[cfg(not(feature = "send"))]
pub(crate) type TimerFn = dyn Fn(Duration, Wake);

pub(crate) struct TimerBox {
    pub(crate) f: Box<TimerFn>,
    pub(crate) shared: Arc<Shared>,
}

/// `nl_Timer`: called by `Task.sleep`, inside a Lua call. A panic is parked.
unsafe extern "C-unwind" fn timer_cb(
    data: *mut c_void,
    _l: *mut lua::lua_State,
    seconds: c_double,
    wake: nl::nl_Callback,
    arg: *mut c_void,
) {
    // SAFETY: data is the TimerBox given to nl_settimer, kept alive until the
    // state closes (crate::state::Kept).
    let tb = unsafe { &*data.cast::<TimerBox>() };
    let delay = Duration::try_from_secs_f64(seconds).unwrap_or(Duration::ZERO);
    let w = Wake { wake, arg, shared: tb.shared.clone() };
    if let Err(p) = catch_unwind(AssertUnwindSafe(|| (tb.f)(delay, w))) {
        // Parking may drop the payload (in a worker): its Drop runs here.
        abort_on_panic(|| tb.shared.park(p));
    }
}

pub(crate) fn settimer(lua: &Lua, tb: *const TimerBox) -> Result<()> {
    // SAFETY: nl_settimer raises nothing; tb stays alive until the state
    // closes.
    unsafe { lua.exec_raw::<()>((), |l| nl::nl_settimer(l, Some(timer_cb), tb as *mut c_void)) }
}

// -- The pump and the wakeup ---------------------------------------------------

pub(crate) fn pump(lua: &Lua) -> Result<usize> {
    let mut n = 0;
    // SAFETY: nl_pump raises nothing.
    unsafe {
        lua.exec_raw::<()>((), |l| {
            n = nl::nl_pump(l);
        })?;
    }
    Ok(usize::try_from(n).unwrap_or(0))
}

pub(crate) type WakeupFn = dyn Fn() + Send + Sync;

/// `nl_Wakeup`: called on a posting thread. A panic aborts.
unsafe extern "C" fn wakeup_cb(data: *mut c_void) {
    // SAFETY: data is the Box<WakeupFn> given to nl_setwakeup; nl_setwakeup
    // waits for calls in progress before the old one is freed.
    let f = unsafe { &*data.cast::<Box<WakeupFn>>() };
    if catch_unwind(AssertUnwindSafe(f)).is_err() {
        std::process::abort();
    }
}

/// `nl_setwakeup`. When it returns, the previous wakeup is no longer called
/// and may be freed.
pub(crate) fn setwakeup(lua: &Lua, f: *const Box<WakeupFn>) -> Result<()> {
    // SAFETY: nl_setwakeup raises nothing; f stays alive until replaced or
    // the state closes.
    unsafe { lua.exec_raw::<()>((), |l| nl::nl_setwakeup(l, Some(wakeup_cb), f as *mut c_void)) }
}

// -- Loader and reporter ---------------------------------------------------------

pub(crate) struct LoaderBox {
    pub(crate) loader: Box<dyn Loader>,
    /// Owned contents handed to the compiler, by address, until release.
    pub(crate) owned: Mutex<HashMap<usize, Box<[u8]>>>,
}

/// Drops a value of the host's inside a callback of the VM, where a panic
/// may not unwind: a `Drop` that panics aborts the process.
fn drop_or_abort<T>(v: T) {
    abort_on_panic(move || drop(v));
}

fn abort_on_panic<R>(f: impl FnOnce() -> R) -> R {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(r) => r,
        Err(_) => std::process::abort(),
    }
}

/// # Safety
///
/// `s` is NULL or a NUL-terminated string.
unsafe fn opt_str<'a>(s: *const c_char) -> Option<Cow<'a, str>> {
    // SAFETY: the caller's guarantee.
    (!s.is_null()).then(|| unsafe { CStr::from_ptr(s) }.to_string_lossy())
}

unsafe extern "C" fn loader_resolve(
    data: *mut c_void,
    from: *const c_char,
    path: *const c_char,
    out: *mut c_char,
    size: usize,
) -> c_int {
    abort_on_panic(|| {
        // SAFETY: data is the LoaderBox given to nl_setloader; the strings
        // are NULL or NUL-terminated (nl.h, nl_Loader).
        let (lb, from, path) = unsafe { (&*data.cast::<LoaderBox>(), opt_str(from), opt_str(path)) };
        let Some(path) = path else { return 0 };
        let Some(name) = lb.loader.resolve(from.as_deref(), &path) else { return 0 };
        let bytes = name.as_bytes();
        if bytes.contains(&0) || bytes.len() + 1 > size {
            return 0;
        }
        // SAFETY: out has room for size bytes, and bytes.len() + 1 <= size.
        unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr(), out.cast::<u8>(), bytes.len());
            *out.add(bytes.len()) = 0;
        }
        1
    })
}

unsafe extern "C" fn loader_read(data: *mut c_void, name: *const c_char, size: *mut usize) -> *const c_char {
    abort_on_panic(|| {
        // SAFETY: as in loader_resolve.
        let (lb, name) = unsafe { (&*data.cast::<LoaderBox>(), opt_str(name)) };
        let Some(name) = name else { return ptr::null() };
        let (p, len) = match lb.loader.read(&name) {
            None => return ptr::null(),
            Some(Cow::Borrowed(b)) => (b.as_ptr(), b.len()),
            Some(Cow::Owned(v)) if v.is_empty() => (c"".as_ptr().cast::<u8>(), 0),
            Some(Cow::Owned(v)) => {
                let b = v.into_boxed_slice();
                let (p, len) = (b.as_ptr(), b.len());
                lock(&lb.owned).insert(p as usize, b);
                (p, len)
            }
        };
        // SAFETY: size points to a size_t (nl_Loader.read).
        unsafe { *size = len };
        p.cast()
    })
}

unsafe extern "C" fn loader_release(data: *mut c_void, _name: *const c_char, buf: *const c_char) {
    abort_on_panic(|| {
        // SAFETY: as in loader_resolve.
        let lb = unsafe { &*data.cast::<LoaderBox>() };
        lock(&lb.owned).remove(&(buf as usize));
    })
}

/// `nl_setloader`, or the disk loader again for `None`.
pub(crate) fn setloader(lua: &Lua, lb: Option<*const LoaderBox>) -> Result<()> {
    let l = nl::nl_Loader {
        resolve: Some(loader_resolve),
        read: Some(loader_read),
        release: Some(loader_release),
        data: lb.unwrap_or(ptr::null()) as *mut c_void,
    };
    // SAFETY: nl_setloader copies the struct and raises nothing; the box
    // stays alive until the state closes.
    unsafe { lua.exec_raw::<()>((), |s| nl::nl_setloader(s, if lb.is_some() { &l } else { ptr::null() })) }
}

pub(crate) type ReporterFn = dyn Fn(&Diagnostic) + Send + Sync;

unsafe extern "C" fn reporter_cb(data: *mut c_void, d: *const nl::nl_Diagnostic) {
    abort_on_panic(|| {
        // SAFETY: data is the Box<ReporterFn> given to nl_setreporter, d a
        // diagnostic valid for the call, with NUL-terminated strings.
        let (f, d) = unsafe { (&*data.cast::<Box<ReporterFn>>(), &*d) };
        // SAFETY: as above.
        let text = |s| unsafe { opt_str(s) }.map(Cow::into_owned).unwrap_or_default();
        let diag = Diagnostic {
            severity: match d.severity {
                nl::NL_DIAG_WARNING => crate::Severity::Warning,
                nl::NL_DIAG_LINT => crate::Severity::Lint,
                _ => crate::Severity::Error,
            },
            code: crate::state::intern(&text(d.code)),
            message: text(d.message),
            file: text(d.file),
            line: u32::try_from(d.line).unwrap_or(0),
            column: u32::try_from(d.column).unwrap_or(0),
            end_line: u32::try_from(d.endline).unwrap_or(0),
            end_column: u32::try_from(d.endcolumn).unwrap_or(0),
        };
        f(&diag);
    })
}

pub(crate) fn setreporter(lua: &Lua, f: *const Box<ReporterFn>) -> Result<()> {
    // SAFETY: nl_setreporter raises nothing; f stays alive until the state
    // closes.
    unsafe { lua.exec_raw::<()>((), |l| nl::nl_setreporter(l, Some(reporter_cb), f as *mut c_void)) }
}

// -- Workers -------------------------------------------------------------------

pub(crate) fn drain(lua: &Lua) -> Result<usize> {
    let mut n = 0;
    // SAFETY: nl_drain raises nothing.
    unsafe {
        lua.exec_raw::<()>((), |l| {
            n = nl::nl_drain(l);
        })?;
    }
    Ok(usize::try_from(n).unwrap_or(0))
}

pub(crate) fn setworkers(lua: &Lua, n: usize) -> Result<()> {
    let n = c_int::try_from(n).unwrap_or(c_int::MAX);
    // SAFETY: nl_setworkers raises nothing.
    unsafe { lua.exec_raw::<()>((), |l| nl::nl_setworkers(l, n)) }
}

pub(crate) fn isworker(lua: &Lua) -> Result<bool> {
    let mut r = 0;
    // SAFETY: nl_isworker raises nothing.
    unsafe {
        lua.exec_raw::<()>((), |l| {
            r = nl::nl_isworker(l);
        })?;
    }
    Ok(r != 0)
}

pub(crate) type WorkerInitFn = dyn Fn(&crate::WorkerInit<'_>) -> Result<()> + Send + Sync;

/// The text a failing init function leaves for the worker's `WorkerError`.
fn init_failure(err: &Error) -> String {
    use crate::exception::NlErrorExt;
    match err.exception() {
        Some(e) => e.to_string(),
        None => crate::exception::message_of(err),
    }
}

/// The worker-init function (`lua_CFunction`), run by the VM as
/// `lua_cpcall(W, init, data)` in each new worker state, on the worker's
/// thread. It gives the state a `Lua` of mlua's (`get_or_init_from_ptr`; the
/// state is not mlua's, so mlua neither owns nor closes it), opens nl in it
/// (the translator, a `Remote`), and calls the host's closure. An error
/// fails the worker with `WorkerError`; a panic aborts.
unsafe extern "C-unwind" fn worker_init_cb(l: *mut lua::lua_State) -> c_int {
    let failure: Option<String> = abort_on_panic(|| {
        // SAFETY: index 1 holds the data given to nl_setworkerinit, a
        // Box<WorkerInitFn> the main state keeps until it closes, which is
        // after its workers have stopped (lj_nl_freestate).
        let f = unsafe { &*lua::lua_touserdata(l, 1).cast::<Box<WorkerInitFn>>() };
        // SAFETY: l is the worker's main state, alive for this call; the Lua
        // reaches the closure only inside a WorkerInit, which hands out no
        // handle that could outlive it. The VM lifts the worker's memory
        // limit while this function runs, so the attachment, which allocates
        // outside any protected call, cannot fail on it.
        let lua = unsafe { Lua::get_or_init_from_ptr(l) };
        match crate::worker::init(lua, f) {
            Ok(()) => None,
            Err(e) => Some(init_failure(&e)),
        }
    });
    let Some(msg) = failure else { return 0 };
    // SAFETY: inside the VM's lua_cpcall; the message is copied into the
    // state and dropped before lua_error unwinds this frame.
    unsafe {
        lua::lua_pushlstring(l, msg.as_ptr().cast(), msg.len());
        drop(msg);
        lua::lua_error(l)
    }
}

pub(crate) fn setworkerinit(lua: &Lua, f: *const Box<WorkerInitFn>) -> Result<()> {
    // SAFETY: nl_setworkerinit raises nothing; f stays alive until the main
    // state closes, after its workers.
    unsafe { lua.exec_raw::<()>((), |l| nl::nl_setworkerinit(l, Some(worker_init_cb), f as *mut c_void)) }
}

unsafe extern "C-unwind" fn c_spawn(l: *mut lua::lua_State) -> c_int {
    // SAFETY: called by nl_pcall with the function at 1 and its arguments.
    unsafe {
        nl::nl_spawn(l, lua::lua_gettop(l) - 1);
    }
    1
}

unsafe extern "C-unwind" fn c_settransfer(l: *mut lua::lua_State) -> c_int {
    // SAFETY: called by nl_pcall with the name at 1, encode at 2, decode at 3.
    unsafe {
        let name = arg_str(l, 1);
        lua::lua_settop(l, 3);
        nl::nl_settransfer(l, name);
    }
    0
}

pub(crate) fn spawn(lua: &Lua, f: &Function, args: MultiValue) -> Result<Outcome> {
    let mut all = args;
    all.push_front(Value::Function(f.clone()));
    pcall_c(lua, c_spawn, all)
}

pub(crate) fn settransfer(lua: &Lua, name: &str, encode: Function, decode: Function) -> Result<Outcome> {
    let name = Value::String(lua.create_string(name)?);
    pcall_c(lua, c_settransfer, MultiValue::from_vec(vec![name, Value::Function(encode), Value::Function(decode)]))
}
