//! The safe Rust host API of NL, on mlua.
//!
//! A host uses `mlua::Lua`, `Value`, `Table`, `Function` and `UserData` as it
//! would with stock LuaJIT, and gets NL's features from extension traits
//! ([`NlExt`] on `Lua`, [`NlFunctionExt`], [`NlObjectExt`], [`NlErrorExt`])
//! and a few types. The VM is the fork of LuaJIT that mlua builds when the
//! host patches the `luajit-src` crate; [`ffi`] is `nl_sys`.
//!
//! ```ignore
//! use nl::prelude::*;
//!
//! let lua = nl::new()?;
//! lua.add_path("scripts")?;
//! let exports: Table = lua.load_nl_file("scripts/main.nl")?.try_call(())?;
//! ```
//!
//! Safety: every `nl.h` call that can raise is made inside
//! `Lua::exec_raw`; calls that run script code go through `nl_pcall`, so a
//! failure is an [`Exception`]. A panic in a closure that script code calls
//! is carried by mlua; one in a callback the VM calls from inside a Lua
//! call (a posted closure, the timer, the unhandled hook, the translator)
//! is resumed when the nl call that ran it returns (`pump`, `try_call`,
//! ...); one in the loader, the reporter or the wakeup aborts.
//!
//! Workers: `set_workers`, `set_worker_init`, `spawn`,
//! `drain`, `is_worker`, and `register_transfer` with the [`Transfer`]
//! trait for host types that cross between states.

use std::path::Path;
use std::time::Duration;

use mlua::{
    AnyUserData, FromLuaMulti, Function, IntoLuaMulti, Lua, MaybeSend, MaybeSync, Result, Table, UserData, Value,
};

mod exception;
#[cfg(feature = "async")]
mod future;
mod raw;
mod state;
mod task;
mod types;
mod worker;

pub use exception::{Exception, NlErrorExt, Panics};
#[cfg(feature = "async")]
pub use future::BoxFuture;
pub use mlua;
/// The unsafe declarations of `nl.h`, for what this crate does not cover.
/// Call them inside `Lua::exec_raw`.
pub use nl_sys as ffi;
pub use raw::Wake;
pub use state::{Diagnostic, Loader, Localize, NlOption, Severity, new, open};
pub use task::{Remote, Resolver, Task, TaskState};
pub use types::{NlFunctionExt, NlObjectExt, Range, Transfer, Type};
pub use worker::WorkerInit;

/// `mlua::prelude`, the unprefixed mlua names the examples use, and
/// everything this crate adds.
pub mod prelude {
    pub use crate::{
        Diagnostic, Exception, Loader, Localize, NlErrorExt, NlExt, NlFunctionExt, NlObjectExt, NlOption, Range,
        Remote, Resolver, Severity, Task, TaskState, Transfer, Type, Wake, WorkerInit,
    };
    pub use mlua::prelude::*;
    pub use mlua::{AnyUserData, Function, MultiValue, Table, UserData, UserDataFields, UserDataMethods, Value};
}

/// What NL adds to `mlua::Lua`. The methods need a
/// state opened with [`new`] or [`open`]; on another one they fail (or, for
/// those that return nothing, do nothing).
pub trait NlExt {
    // -- States and loading --

    /// Compiles NL source (`nl_loadbuffer`). `name` follows Lua's chunk
    /// names: `@path` for a file, `=text` for anything else; a name with
    /// neither is shown as `[string "..."]`. Imports are relative to a
    /// file's path. Running the chunk returns the file's export table.
    fn load_nl(&self, source: impl AsRef<[u8]>, name: &str) -> Result<Function>;
    /// Compiles an NL file, whatever its extension (`nl_loadfile`).
    fn load_nl_file(&self, path: impl AsRef<Path>) -> Result<Function>;
    /// `import from 'path' as X`: runs the file if this state has not, and
    /// returns its export table (`nl_import`).
    fn import(&self, path: &str) -> Result<Table>;
    /// `add_path(dir)` (`nl_addpath`). On the main state it fails once
    /// workers exist: their search path is fixed then, and
    /// `nl_addpath` would do nothing.
    fn add_path(&self, dir: impl AsRef<Path>) -> Result<()>;
    /// Registers a declaration file by the name the loader reads it under
    /// (`nl_adddecl`). On the main state it fails once workers exist, as
    /// `add_path` does.
    fn add_decl(&self, name: &str) -> Result<()>;
    /// `nl_setoption`; an error where it returns 0, as for `Sandbox(false)`
    /// once the sandbox is on.
    fn set_option(&self, option: NlOption) -> Result<()>;
    /// The sandbox environment (`nl_sandboxenv`), whatever the option.
    fn sandbox_env(&self) -> Result<Table>;
    /// Replaces how `.nl` files are found and read (`nl_setloader`). Fails
    /// once workers exist (or in a worker), where `nl_setloader` does
    /// nothing.
    fn set_loader(&self, loader: impl Loader) -> Result<()>;
    /// Receives every diagnostic of every compile (`nl_setreporter`).
    fn set_reporter(&self, f: impl Fn(&Diagnostic) + Send + Sync + 'static);

    // -- Types, values and userdata --

    /// The type registered under a state-wide name (`nl_gettype`): host
    /// types and the built-in ones.
    fn nl_type(&self, name: &str) -> Result<Option<Type>>;
    /// `typeof(x)` (`nl_typeof`).
    fn type_of(&self, value: &Value) -> Result<String>;
    /// Makes the metatable mlua builds for `T` the host type `name`
    /// (`nl_settype`) and sets `__methods` on it: NL code then writes
    /// `unit.damage 10`, `u is Unit` and `fn heal(u: Unit)`.
    ///
    /// mlua builds that metatable when the first value of `T` is created,
    /// and nothing public reaches it before, so this creates
    /// one `T::default()` and drops it. A type without `Default` uses
    /// [`register_type_with`](NlExt::register_type_with).
    fn register_type<T>(&self, name: &str) -> Result<()>
    where
        T: UserData + Default + MaybeSend + MaybeSync + 'static;
    /// [`register_type`](NlExt::register_type) with the first value of `T`,
    /// which it returns as userdata.
    fn register_type_with<T>(&self, name: &str, value: T) -> Result<AnyUserData>
    where
        T: UserData + MaybeSend + MaybeSync + 'static;
    /// `keyed(t)` (`nl_keyed`): passed as the last argument of a call, the
    /// keyed arguments an NL function's `**opts` parameter takes.
    fn keyed(&self, opts: Table) -> Result<Value>;
    /// Lets userdata of `T` cross between states (`nl_settransfer`), under
    /// the name `register_type` gave `T` in this state. Call it in every
    /// state that sends or receives `T`: the main state, and the worker-init
    /// closure.
    fn register_transfer<T: Transfer>(&self) -> Result<()>;

    // -- Events --

    /// `Event.run(name, args...)` under `nl_pcall` (`nl_prunevent`), with
    /// every result of the deciding handler converted to `R`.
    fn run_event<R: FromLuaMulti>(&self, name: &str, args: impl IntoLuaMulti) -> Result<R>;
    /// `Event.on_run(name, handler, f)` (`nl_onevent`). A handler ends the
    /// run by returning a value; `Ok(())` lets the next handler run.
    fn on_event<A, R, F>(&self, name: &str, handler: &str, f: F) -> Result<()>
    where
        A: FromLuaMulti,
        R: IntoLuaMulti,
        F: Fn(&Lua, A) -> Result<R> + MaybeSend + 'static;
    /// `Event.remove(name, handler)` (`nl_offevent`).
    fn off_event(&self, name: &str, handler: &str) -> Result<()>;

    // -- Tasks and futures --

    /// A pending `Task` (`nl_newtask`).
    fn create_task(&self) -> Result<Task>;
    /// The timer of `Task.sleep` (`nl_settimer`): it gets the delay and a
    /// [`Wake`] to call when it has passed.
    fn set_timer(&self, f: impl Fn(Duration, Wake) + MaybeSend + 'static);
    /// The hook for tasks that failed with nobody awaiting them, and for
    /// posted closures that failed (`nl_setunhandled`).
    fn set_unhandled(&self, f: impl Fn(&Lua, &Exception) + MaybeSend + 'static);
    /// The host's executor for [`create_task_function`](NlExt::create_task_function):
    /// `|future| { tokio::spawn(future); }`, for one.
    #[cfg(feature = "async")]
    fn set_spawner(&self, f: impl Fn(BoxFuture) + MaybeSend + 'static);
    /// A function NL code calls and awaits, backed by a Rust future that
    /// runs on the spawner's executor.
    #[cfg(feature = "async")]
    fn create_task_function<A, R, F, Fut>(&self, f: F) -> Result<Function>
    where
        A: FromLuaMulti + Send + 'static,
        R: mlua::IntoLua + Send + 'static,
        F: Fn(A) -> Fut + MaybeSend + 'static,
        Fut: std::future::Future<Output = Result<R>> + Send + 'static;

    // -- Threads --

    /// Runs what was posted (`nl_pump`): posted closures, results and events
    /// from workers, and with no worker threads a step of each worker.
    /// Returns the number of posted callbacks run, plus one when the
    /// workers or the runtime did anything. A panic in a posted closure is
    /// resumed when it returns.
    fn pump(&self) -> usize;
    /// Pumps, waiting, until every job and call this state started has been
    /// delivered (`nl_drain`); returns what the pumps returned together.
    fn drain(&self) -> usize;
    /// A handle that posts to this state's thread from any thread.
    fn remote(&self) -> Remote;
    /// Called on a posting thread when the inbox stops being empty
    /// (`nl_setwakeup`). A panic in it aborts.
    fn set_wakeup(&self, f: impl Fn() + Send + Sync + 'static);
    /// The number of pool workers (`nl_setworkers`); 0 runs jobs and
    /// dedicated workers on this thread while it pumps. Set before the first
    /// `spawn`; afterwards a pool with threads can only grow. No effect in a
    /// worker.
    fn set_workers(&self, n: usize);
    /// Runs `f` in each new worker state, on the worker's thread, before any
    /// NL file loads (`nl_setworkerinit`): it registers the host functions,
    /// types and transfers that workers may use, through a [`WorkerInit`],
    /// which hands out no handle into the worker's state (the VM closes that
    /// state when the worker ends). The state is opened with [`open`] first.
    /// An error fails the worker with `WorkerError`; a panic aborts. `f` is
    /// `Send + Sync`: it runs on every worker's thread.
    #[cfg(not(feature = "send"))]
    fn set_worker_init(&self, f: impl Fn(&WorkerInit) -> Result<()> + Send + Sync + 'static);
    /// [`set_worker_init`](NlExt::set_worker_init) with mlua's `send`
    /// feature, where it is unsafe.
    ///
    /// # Safety
    ///
    /// With `send`, every mlua value is `Send`, and the VM runs a worker's
    /// state on the worker's thread without mlua's lock. So no value of a
    /// worker's state (a `Function`, `Table`, `AnyUserData`, `LuaString`,
    /// `Thread`, or a `Lua` of it) that a host function installed by `f`, a
    /// `UserData` method or a `Transfer` receives may leave the worker's
    /// thread: another thread using it runs the state at the same time as the
    /// worker. And `f` must not capture a value of this (the main) state: a
    /// worker that calls into it waits for mlua's lock of the main state,
    /// which the main thread holds while it drains, waiting for the worker.
    #[cfg(feature = "send")]
    #[allow(unsafe_code)] // The declaration of an unsafe contract; nothing unsafe runs here.
    unsafe fn set_worker_init(&self, f: impl Fn(&WorkerInit) -> Result<()> + Send + Sync + 'static);
    /// Whether this is a worker's state (`nl_isworker`).
    fn is_worker(&self) -> bool;
    /// `Worker.run(f, args...)` (`nl_spawn`): runs the named NL function `f`
    /// in a pool worker. Fails with `SendError` as `spawn` does.
    fn spawn(&self, f: &Function, args: impl IntoLuaMulti) -> Result<Task>;
}

impl NlExt for Lua {
    fn load_nl(&self, source: impl AsRef<[u8]>, name: &str) -> Result<Function> {
        state::load_nl(self, source.as_ref(), name)
    }

    fn load_nl_file(&self, path: impl AsRef<Path>) -> Result<Function> {
        state::load_nl_file(self, path.as_ref())
    }

    fn import(&self, path: &str) -> Result<Table> {
        state::import(self, path)
    }

    fn add_path(&self, dir: impl AsRef<Path>) -> Result<()> {
        state::unless_frozen(self, "add_path")?;
        raw::addpath(self, dir.as_ref())
    }

    fn add_decl(&self, name: &str) -> Result<()> {
        state::unless_frozen(self, "add_decl")?;
        raw::adddecl(self, name)
    }

    fn set_option(&self, option: NlOption) -> Result<()> {
        state::set_option(self, option)
    }

    fn sandbox_env(&self) -> Result<Table> {
        raw::sandboxenv(self)
    }

    fn set_loader(&self, loader: impl Loader) -> Result<()> {
        if self.is_worker() {
            return Err(mlua::Error::runtime("set_loader has no effect in a worker: it uses the main state's loader"));
        }
        state::unless_frozen(self, "set_loader")?;
        state::set_loader(self, Box::new(loader))
    }

    fn set_reporter(&self, f: impl Fn(&Diagnostic) + Send + Sync + 'static) {
        let _ = state::set_reporter(self, Box::new(f));
    }

    fn nl_type(&self, name: &str) -> Result<Option<Type>> {
        types::nl_type(self, name)
    }

    fn type_of(&self, value: &Value) -> Result<String> {
        types::type_of(self, value)
    }

    fn register_type<T>(&self, name: &str) -> Result<()>
    where
        T: UserData + Default + MaybeSend + MaybeSync + 'static,
    {
        let ud = types::register_type_with(self, name, T::default())?;
        ud.destroy()
    }

    fn register_type_with<T>(&self, name: &str, value: T) -> Result<AnyUserData>
    where
        T: UserData + MaybeSend + MaybeSync + 'static,
    {
        types::register_type_with(self, name, value)
    }

    fn keyed(&self, opts: Table) -> Result<Value> {
        types::keyed(self, opts)
    }

    fn register_transfer<T: Transfer>(&self) -> Result<()> {
        types::register_transfer::<T>(self)
    }

    fn run_event<R: FromLuaMulti>(&self, name: &str, args: impl IntoLuaMulti) -> Result<R> {
        task::run_event(self, name, args)
    }

    fn on_event<A, R, F>(&self, name: &str, handler: &str, f: F) -> Result<()>
    where
        A: FromLuaMulti,
        R: IntoLuaMulti,
        F: Fn(&Lua, A) -> Result<R> + MaybeSend + 'static,
    {
        let f = self.create_function(f)?;
        raw::onevent(self, name, handler, f)
    }

    fn off_event(&self, name: &str, handler: &str) -> Result<()> {
        raw::offevent(self, name, handler)
    }

    fn create_task(&self) -> Result<Task> {
        task::create_task(self)
    }

    fn set_timer(&self, f: impl Fn(Duration, Wake) + MaybeSend + 'static) {
        let _ = state::set_timer(self, Box::new(f));
    }

    fn set_unhandled(&self, f: impl Fn(&Lua, &Exception) + MaybeSend + 'static) {
        let _ = task::set_unhandled(self, Box::new(f));
    }

    #[cfg(feature = "async")]
    fn set_spawner(&self, f: impl Fn(BoxFuture) + MaybeSend + 'static) {
        let _ = future::set_spawner(self, Box::new(f));
    }

    #[cfg(feature = "async")]
    fn create_task_function<A, R, F, Fut>(&self, f: F) -> Result<Function>
    where
        A: FromLuaMulti + Send + 'static,
        R: mlua::IntoLua + Send + 'static,
        F: Fn(A) -> Fut + MaybeSend + 'static,
        Fut: std::future::Future<Output = Result<R>> + Send + 'static,
    {
        future::create_task_function(self, f)
    }

    fn pump(&self) -> usize {
        task::pump(self)
    }

    fn remote(&self) -> Remote {
        task::remote(self)
    }

    fn set_wakeup(&self, f: impl Fn() + Send + Sync + 'static) {
        let _ = state::set_wakeup(self, Box::new(f));
    }

    fn drain(&self) -> usize {
        task::drain(self)
    }

    fn set_workers(&self, n: usize) {
        let _ = raw::setworkers(self, n);
    }

    #[cfg(not(feature = "send"))]
    fn set_worker_init(&self, f: impl Fn(&WorkerInit) -> Result<()> + Send + Sync + 'static) {
        let _ = state::set_worker_init(self, Box::new(f));
    }

    #[cfg(feature = "send")]
    #[allow(unsafe_code)] // The caller keeps the contract of the trait method.
    unsafe fn set_worker_init(&self, f: impl Fn(&WorkerInit) -> Result<()> + Send + Sync + 'static) {
        let _ = state::set_worker_init(self, Box::new(f));
    }

    fn is_worker(&self) -> bool {
        raw::isworker(self).unwrap_or(false)
    }

    fn spawn(&self, f: &Function, args: impl IntoLuaMulti) -> Result<Task> {
        task::spawn(self, f, args)
    }
}
