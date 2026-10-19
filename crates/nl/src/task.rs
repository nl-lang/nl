//! Events, tasks, the timer, the unhandled hook, the inbox.

use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use mlua::{Error, FromLua, FromLuaMulti, IntoLua, IntoLuaMulti, Lua, RegistryKey, Result, Table, Value};

use crate::exception::{self, Exception, values};
use crate::raw::{self, Shared};
use crate::state::{nl_state, scoped};
use crate::types::lua_of;

// -- Events --------------------------------------------------------------------

pub(crate) fn run_event<R: FromLuaMulti>(lua: &Lua, name: &str, args: impl IntoLuaMulti) -> Result<R> {
    let args = args.into_lua_multi(lua)?;
    let v = values(lua, || raw::prunevent(lua, name, args))?;
    R::from_lua_multi(v, lua)
}

// -- Tasks ---------------------------------------------------------------------

/// A `Task`. NL code awaits it; the host finishes
/// it with [`resolve`](Task::resolve) or [`reject`](Task::reject) on the
/// state's thread, or through a [`Resolver`] from anywhere.
#[derive(Clone, Debug)]
pub struct Task(pub(crate) Table);

/// What [`Task::state`] finds.
#[derive(Debug)]
pub enum TaskState<R> {
    Pending,
    Done(R),
    Failed(Exception),
}

impl Task {
    /// Finishes the task with `value` now; its waiters run before this
    /// returns. Finishing a task twice does nothing the second time.
    pub fn resolve(&self, value: impl IntoLua) -> Result<()> {
        let lua = lua_of(self.0.weak_lua())?;
        let v = value.into_lua(&lua)?;
        values(&lua, || raw::resolve(&lua, &self.0, v)).map(drop)
    }

    /// Fails the task now. An error holding an [`Exception`] fails it with
    /// that exception; any other error with a `LuaError` of its text.
    pub fn reject(&self, error: impl Into<Error>) -> Result<()> {
        let lua = lua_of(self.0.weak_lua())?;
        let v = exception::error_value(&lua, error.into())?;
        values(&lua, || raw::reject(&lua, &self.0, v)).map(drop)
    }

    /// The task's state, with its value or exception (`nl_taskstate`).
    pub fn state<R: FromLua>(&self) -> Result<TaskState<R>> {
        let lua = lua_of(self.0.weak_lua())?;
        let (st, v) = raw::taskstate(&lua, &self.0)?;
        match st {
            nl_sys::NL_TASK_PENDING => Ok(TaskState::Pending),
            nl_sys::NL_TASK_DONE => Ok(TaskState::Done(R::from_lua(v, &lua)?)),
            nl_sys::NL_TASK_FAILED => Ok(TaskState::Failed(exception::exception_of(&lua, v))),
            _ => Err(Error::runtime("nl_taskstate: not a task")),
        }
    }

    /// A handle that finishes the task from any thread.
    pub fn resolver(&self) -> Resolver {
        let made = lua_of(self.0.weak_lua()).and_then(|lua| {
            let shared = nl_state(&lua)?.shared.clone();
            Ok(Resolver { task: Some(lua.create_registry_value(self.0.clone())?), shared })
        });
        made.unwrap_or_else(|_| Resolver { task: None, shared: Arc::new(Shared::closed()) })
    }

    /// The task as NL code holds it.
    pub fn as_table(&self) -> &Table {
        &self.0
    }
}

impl FromLua for Task {
    fn from_lua(value: Value, lua: &Lua) -> Result<Self> {
        if let Value::Table(t) = &value {
            if raw::taskstate(lua, t)?.0 >= 0 {
                return Ok(Task(t.clone()));
            }
        }
        Err(Error::FromLuaConversionError { from: value.type_name(), to: "Task".to_owned(), message: None })
    }
}

impl IntoLua for Task {
    fn into_lua(self, _lua: &Lua) -> Result<Value> {
        Ok(Value::Table(self.0))
    }
}

pub(crate) fn create_task(lua: &Lua) -> Result<Task> {
    raw::newtask(lua).map(Task)
}

/// Finishes a task from any thread: it posts, and the task
/// finishes at the next pump. Dropped unused, it rejects the task, so an
/// abandoned operation does not leave script code waiting forever. After
/// the state has closed it does nothing.
pub struct Resolver {
    task: Option<RegistryKey>,
    shared: Arc<Shared>,
}

impl Resolver {
    /// Resolves the task with `value` at the next pump.
    pub fn resolve<T: IntoLua + Send + 'static>(mut self, value: T) {
        if let Some(key) = self.task.take() {
            raw::post(&self.shared, Box::new(move |lua| finish(lua, key).and_then(|t| t.resolve(value))));
        }
    }

    /// Rejects the task with `error` at the next pump.
    pub fn reject(mut self, error: impl Into<Error> + Send + 'static) {
        if let Some(key) = self.task.take() {
            raw::post(&self.shared, Box::new(move |lua| finish(lua, key).and_then(|t| t.reject(error))));
        }
    }
}

fn finish(lua: &Lua, key: RegistryKey) -> Result<Task> {
    let t: Table = lua.registry_value(&key)?;
    lua.remove_registry_value(key)?;
    Ok(Task(t))
}

impl Drop for Resolver {
    fn drop(&mut self) {
        if let Some(key) = self.task.take() {
            raw::post(
                &self.shared,
                Box::new(move |lua| {
                    finish(lua, key)?.reject(Exception::new(
                        "Exception",
                        "the task's Resolver was dropped without resolving or rejecting it",
                    ))
                }),
            );
        }
    }
}

impl fmt::Debug for Resolver {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Resolver").field("pending", &self.task.is_some()).finish()
    }
}

#[cfg(feature = "send")]
pub(crate) type UnhandledFn = dyn Fn(&Lua, &Exception) + Send;
#[cfg(not(feature = "send"))]
pub(crate) type UnhandledFn = dyn Fn(&Lua, &Exception);

/// `nl_setunhandled` with a Rust hook. A panic in it is parked and resumed
/// when the nl call that ran it returns (a pump, usually).
pub(crate) fn set_unhandled(lua: &Lua, f: Box<UnhandledFn>) -> Result<()> {
    let shared = nl_state(lua)?.shared.clone();
    let func = lua.create_function(move |lua, v: Value| {
        let e = exception::exception_of(lua, v);
        if let Err(p) = catch_unwind(AssertUnwindSafe(|| f(lua, &e))) {
            shared.park(p);
        }
        Ok(())
    })?;
    raw::set_unhandled(lua, func)
}

// -- The inbox -----------------------------------------------------------------

/// Posts closures to the state's thread from any thread. It
/// holds a counted reference to the state's inbox guard: after the state
/// has closed, [`post`](Remote::post) returns false and drops the closure.
#[derive(Clone)]
pub struct Remote(pub(crate) Arc<Shared>);

impl Remote {
    /// Queues `f` for the next pump, where it runs on the state's thread in
    /// a protected call. An error it returns goes to the unhandled hook; a
    /// panic in it is resumed when that pump returns. False when the state
    /// has closed.
    pub fn post(&self, f: impl FnOnce(&Lua) -> Result<()> + Send + 'static) -> bool {
        raw::post(&self.0, Box::new(f))
    }
}

impl fmt::Debug for Remote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Remote")
    }
}

pub(crate) fn remote(lua: &Lua) -> Remote {
    Remote(nl_state(lua).map(|s| s.shared.clone()).unwrap_or_else(|_| Arc::new(Shared::closed())))
}

/// `nl_spawn`: `Worker.run(f, args...)`.
pub(crate) fn spawn(lua: &Lua, f: &mlua::Function, args: impl IntoLuaMulti) -> Result<Task> {
    let args = args.into_lua_multi(lua)?;
    let v = values(lua, || raw::spawn(lua, f, args))?;
    Task::from_lua(v.into_iter().next().unwrap_or(Value::Nil), lua)
}

pub(crate) fn drain(lua: &Lua) -> usize {
    scoped(lua, || raw::drain(lua).unwrap_or(0))
}

pub(crate) fn pump(lua: &Lua) -> usize {
    scoped(lua, || raw::pump(lua).unwrap_or(0))
}
