//! Rust futures as NL tasks and NL tasks as Rust futures (feature
//! `async`).

use std::future::Future;
use std::marker::PhantomData;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::task::{Context, Poll, Waker};

use mlua::{Error, FromLua, FromLuaMulti, Function, IntoLua, Lua, MaybeSend, MultiValue, ObjectLike, Result, Value};

use crate::exception::{Exception, NlErrorExt, message_of};
use crate::state::nl_state;
use crate::task::{Task, TaskState, create_task};
use crate::types::lua_of;

/// A future the spawner runs on the host's executor.
pub type BoxFuture = Pin<Box<dyn Future<Output = ()> + Send>>;

#[cfg(feature = "send")]
pub(crate) type SpawnFn = dyn Fn(BoxFuture) + Send;
#[cfg(not(feature = "send"))]
pub(crate) type SpawnFn = dyn Fn(BoxFuture);

pub(crate) fn set_spawner(lua: &Lua, f: Box<SpawnFn>) -> Result<()> {
    nl_state(lua)?.spawner.replace(Some(f));
    Ok(())
}

/// A Rust error that can cross threads whatever mlua's features: the
/// exception it holds, or its text.
enum SendableError {
    Exception(Exception),
    Message(String),
}

impl From<Error> for SendableError {
    fn from(err: Error) -> SendableError {
        match err.exception() {
            Some(e) => SendableError::Exception(e.clone()),
            None => SendableError::Message(message_of(&err)),
        }
    }
}

impl From<SendableError> for Error {
    fn from(e: SendableError) -> Error {
        match e {
            SendableError::Exception(e) => e.into(),
            SendableError::Message(m) => Error::RuntimeError(m),
        }
    }
}

/// A function NL code calls and awaits: each call converts its arguments,
/// starts `f(args)` on the spawner, and returns a `Task` that the future's
/// output finishes through a `Resolver`. The future never touches the state.
pub(crate) fn create_task_function<A, R, F, Fut>(lua: &Lua, f: F) -> Result<Function>
where
    A: FromLuaMulti + Send + 'static,
    R: IntoLua + Send + 'static,
    F: Fn(A) -> Fut + MaybeSend + 'static,
    Fut: Future<Output = Result<R>> + Send + 'static,
{
    lua.create_function(move |lua, args: MultiValue| {
        let args = A::from_lua_multi(args, lua)?;
        if nl_state(lua)?.spawner.borrow().is_none() {
            return Err(Error::runtime("nl: create_task_function needs a spawner (set_spawner)"));
        }
        let task = create_task(lua)?;
        let resolver = task.resolver();
        let fut = f(args);
        // The spawner is taken out while it runs, so that no borrow of the
        // state's data is held across the host's code (which may call
        // set_spawner, or anything else). A guard puts it back when the call
        // ends, also when it panics, unless the host set another one
        // meanwhile.
        let Some(spawn) = nl_state(lua)?.spawner.borrow_mut().take() else {
            return Err(Error::runtime("nl: the spawner was replaced while a task function started"));
        };
        struct PutBack<'a> {
            lua: &'a Lua,
            spawn: Option<Box<SpawnFn>>,
        }
        impl Drop for PutBack<'_> {
            fn drop(&mut self) {
                if let Ok(st) = nl_state(self.lua) {
                    let mut slot = st.spawner.borrow_mut();
                    if slot.is_none() {
                        *slot = self.spawn.take();
                    }
                }
            }
        }
        let guard = PutBack { lua, spawn: Some(spawn) };
        if let Some(spawn) = guard.spawn.as_ref() {
            spawn(Box::pin(async move {
                match fut.await {
                    Ok(v) => resolver.resolve(v),
                    Err(e) => resolver.reject(SendableError::from(e)),
                }
            }));
        }
        drop(guard);
        Ok(task)
    })
}

impl Task {
    /// The task as a Rust future, polled on the state's thread. It
    /// completes when the task finishes, which happens during a pump when a
    /// `Resolver` or a posted closure finishes it. Not `Send`.
    pub fn into_future<R: FromLua>(self) -> impl Future<Output = Result<R>> {
        TaskFuture { task: self, waker: Arc::new(Mutex::new(None)), attached: false, _r: PhantomData }
    }
}

struct TaskFuture<R> {
    task: Task,
    waker: Arc<Mutex<Option<Waker>>>,
    attached: bool,
    _r: PhantomData<fn() -> R>,
}

impl<R> TaskFuture<R> {
    /// `task.on_done` and `task.on_error` with a function that wakes the
    /// future; the failure counts as handled.
    fn attach(&self) -> Result<()> {
        let lua = lua_of(self.task.0.weak_lua())?;
        let waker = self.waker.clone();
        let wake = lua.create_function(move |_, _: MultiValue| {
            if let Some(w) = waker.lock().unwrap_or_else(PoisonError::into_inner).take() {
                w.wake();
            }
            Ok(())
        })?;
        self.task.0.call_method::<()>("on_done", wake.clone())?;
        self.task.0.call_method::<()>("on_error", wake)
    }
}

impl<R: FromLua> Future for TaskFuture<R> {
    type Output = Result<R>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<R>> {
        let this = self.get_mut();
        // Attached on the first poll whatever the state, so that a failure
        // counts as handled even when the task had already failed.
        if !this.attached {
            if let Err(e) = this.attach() {
                return Poll::Ready(Err(e));
            }
            this.attached = true;
        }
        match this.task.state::<Value>() {
            Err(e) => Poll::Ready(Err(e)),
            Ok(TaskState::Done(v)) => Poll::Ready(lua_of(this.task.0.weak_lua()).and_then(|lua| R::from_lua(v, &lua))),
            Ok(TaskState::Failed(e)) => Poll::Ready(Err(e.into())),
            Ok(TaskState::Pending) => {
                *this.waker.lock().unwrap_or_else(PoisonError::into_inner) = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}
