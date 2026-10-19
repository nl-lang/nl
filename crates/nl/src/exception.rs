//! Exceptions in both directions, and the translator.

use std::any::Any;
use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, OnceLock};

use mlua::{Error, Function, Lua, MultiValue, RegistryKey, Result, Table, Value};

use crate::raw::{self, Outcome, Shared};
use crate::state::scoped;

/// An NL exception, held by Rust. It is `Send` and `Sync`, and
/// an `std::error::Error`; `mlua::Error::external` carries it, so a Rust
/// function returns one with `Err(Exception::new(..).into())`.
///
/// One that came from NL code keeps the instance that was raised in the
/// registry of its state, so [`Exception::value`] returns that very table
/// and raising it again raises the same instance. One made with
/// [`Exception::new`] gets its instance the first time it is raised or
/// [`value`](Exception::value) is called.
#[derive(Clone)]
pub struct Exception(Arc<Inner>);

struct Inner {
    type_name: String,
    message: String,
    trace: Option<String>,
    instance: OnceLock<RegistryKey>,
}

impl Exception {
    /// An exception of the type named `type_name` (`TypeError`, a type of the
    /// host's, ...), built when it reaches NL code. A name that is not an
    /// exception type known to `nl_gettype` makes it reach NL as a
    /// `LuaError` whose message is this exception's text.
    pub fn new(type_name: &str, message: impl Into<String>) -> Exception {
        Exception(Arc::new(Inner {
            type_name: type_name.to_owned(),
            message: message.into(),
            trace: None,
            instance: OnceLock::new(),
        }))
    }

    /// `typeof(e)`: the exception's type name.
    pub fn type_name(&self) -> &str {
        &self.0.type_name
    }

    /// Its `message` field.
    pub fn message(&self) -> &str {
        &self.0.message
    }

    /// Its `trace` field, filled in where NL caught it. `None` for an
    /// exception made in Rust.
    pub fn trace(&self) -> Option<&str> {
        self.0.trace.as_deref()
    }

    /// The exception instance: the table that was raised, for one that came
    /// from NL code. Fails for an exception held from another state.
    pub fn value(&self, lua: &Lua) -> Result<Table> {
        scoped(lua, || self.instance(lua))
    }

    /// [`value`](Exception::value) without resuming a parked panic: for the
    /// translator, which runs while the panic is on its way out.
    fn instance(&self, lua: &Lua) -> Result<Table> {
        if let Some(k) = self.0.instance.get() {
            return lua.registry_value(k);
        }
        let Some(ty) = crate::types::nl_type(lua, &self.0.type_name)? else {
            return Err(Error::runtime(format!("no exception type named {} is known (nl_gettype)", self.0.type_name)));
        };
        let fields = lua.create_table()?;
        fields.raw_set("message", self.0.message.as_str())?;
        if let Some(t) = &self.0.trace {
            fields.raw_set("trace", t.as_str())?;
        }
        let inst = crate::types::instantiate(lua, ty.as_table(), Value::Table(fields))?;
        if !is_exception(lua, &Value::Table(inst.clone())) {
            return Err(Error::runtime(format!("{} is not an exception type", self.0.type_name)));
        }
        let _ = self.0.instance.set(lua.create_registry_value(inst.clone())?);
        Ok(inst)
    }

    /// An exception instance raised in NL code.
    pub(crate) fn from_table(lua: &Lua, t: Table) -> Result<Exception> {
        let type_name = raw::typeof_(lua, Value::Table(t.clone()))?.unwrap_or_else(|| "Exception".to_owned());
        let message = match t.raw_get::<Value>("message")? {
            Value::Nil => String::new(),
            Value::String(s) => s.to_string_lossy(),
            other => other.to_string()?,
        };
        let trace = match t.raw_get::<Value>("trace")? {
            Value::String(s) => Some(s.to_string_lossy()),
            _ => None,
        };
        let instance = OnceLock::new();
        let _ = instance.set(lua.create_registry_value(t)?);
        Ok(Exception(Arc::new(Inner { type_name, message, trace, instance })))
    }
}

impl fmt::Display for Exception {
    /// `Type: message`, as NL prints an exception.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.0.type_name, self.0.message)
    }
}

impl fmt::Debug for Exception {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Exception")
            .field("type_name", &self.0.type_name)
            .field("message", &self.0.message)
            .field("trace", &self.0.trace)
            .field("instance", &self.0.instance.get().is_some())
            .finish()
    }
}

impl std::error::Error for Exception {}

/// The payload of a panic resumed for several panics that happened during
/// one call of the crate (a pump that ran two posted closures that both
/// panicked, for one), in the order they happened.
#[derive(Debug)]
pub struct Panics(pub Vec<Box<dyn Any + Send>>);

impl From<Exception> for Error {
    fn from(e: Exception) -> Error {
        Error::external(e)
    }
}

/// Finds the [`Exception`] in an `mlua::Error`.
pub trait NlErrorExt {
    /// The exception this error holds, looking through `CallbackError` and
    /// `WithContext`.
    fn exception(&self) -> Option<&Exception>;
}

impl NlErrorExt for Error {
    fn exception(&self) -> Option<&Exception> {
        match self {
            Error::ExternalError(e) => e.downcast_ref::<Exception>(),
            Error::CallbackError { cause, .. } => cause.exception(),
            Error::WithContext { cause, .. } => cause.exception(),
            _ => None,
        }
    }
}

/// The text of a Rust error for a `LuaError`'s message: the innermost
/// cause, without mlua's "runtime error:" and callback tracebacks.
pub(crate) fn message_of(err: &Error) -> String {
    match err {
        Error::CallbackError { cause, .. } => message_of(cause),
        Error::RuntimeError(m) => m.clone(),
        Error::ExternalError(e) => e.to_string(),
        Error::WithContext { context, cause } => format!("{context}: {}", message_of(cause)),
        other => other.to_string(),
    }
}

/// Whether `v` is an NL exception (`v is Exception`).
pub(crate) fn is_exception(lua: &Lua, v: &Value) -> bool {
    match raw::gettype(lua, "Exception") {
        Ok(Value::Table(t)) => raw::isa(lua, v.clone(), &t).unwrap_or(false),
        _ => false,
    }
}

/// The values of the protected call `call` makes, or its failure as an
/// `mlua::Error`, in a scope of its own: a panic that a callback parked
/// during the call is resumed when it ends.
pub(crate) fn values(lua: &Lua, call: impl FnOnce() -> Result<Outcome>) -> Result<MultiValue> {
    scoped(lua, || call().and_then(|o| convert(lua, o)))
}

/// [`values`] without resuming a parked panic.
pub(crate) fn convert(lua: &Lua, outcome: Outcome) -> Result<MultiValue> {
    match outcome {
        Outcome::Ok(v) => Ok(v),
        Outcome::Failed(status, v) => Err(failure(lua, status, v)),
    }
}

/// The error for what `nl_pcall` left: an [`Exception`] for an exception,
/// mlua's errors for Lua's messages.
pub(crate) fn failure(lua: &Lua, status: std::ffi::c_int, v: Value) -> Error {
    let text = |v: &Value| match v {
        Value::String(s) => s.to_string_lossy(),
        other => other.to_string().unwrap_or_else(|_| format!("{other:?}")),
    };
    if status == mlua::ffi::LUA_ERRMEM {
        return Error::MemoryError(text(&v));
    }
    match v {
        Value::Table(t) if is_exception(lua, &Value::Table(t.clone())) => match Exception::from_table(lua, t) {
            Ok(e) => e.into(),
            Err(e) => e,
        },
        Value::Error(e) => *e,
        other => Error::RuntimeError(text(&other)),
    }
}

/// The exception a Lua value stands for, for the unhandled hook and a
/// failed task: the instance itself, or a `LuaError` with its text.
pub(crate) fn exception_of(lua: &Lua, v: Value) -> Exception {
    if let Value::Table(t) = &v {
        if is_exception(lua, &v) {
            if let Ok(e) = Exception::from_table(lua, t.clone()) {
                return e;
            }
        }
    }
    let text = match &v {
        Value::String(s) => s.to_string_lossy(),
        Value::Error(e) => message_of(e),
        other => other.to_string().unwrap_or_else(|_| format!("{other:?}")),
    };
    Exception::new("LuaError", text)
}

/// The Lua value to reject a task with or raise for `err`: the exception
/// instance it holds, else mlua's error value, which the translator turns
/// into a `LuaError`.
pub(crate) fn error_value(lua: &Lua, err: Error) -> Result<Value> {
    if let Some(t) = err.exception().and_then(|e| e.instance(lua).ok()) {
        return Ok(Value::Table(t));
    }
    Ok(Value::Error(Box::new(err)))
}

/// A `LuaError { message, value }`.
fn lua_error(lua: &Lua, message: String, value: Value) -> Result<Value> {
    let Some(ty) = crate::types::nl_type(lua, "LuaError")? else {
        return Ok(Value::Nil);
    };
    let fields = lua.create_table()?;
    fields.raw_set("message", message)?;
    fields.raw_set("value", value)?;
    Ok(Value::Table(crate::types::instantiate(lua, ty.as_table(), Value::Table(fields))?))
}

pub(crate) fn panic_text(p: &(dyn Any + Send)) -> String {
    if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "<panic>".to_owned()
    }
}

/// The translator's work: mlua's error userdata becomes
/// the exception it carries, or a `LuaError` with the Rust error's text; a
/// panic is parked, to be resumed when the nl call that ran it returns, and
/// NL sees a `LuaError`. Anything else is left to the default wrapping.
fn translate(lua: &Lua, holder: &Table, shared: &Shared) -> Result<Value> {
    // Reading the value is what makes mlua resume a panic it carries.
    let v = match catch_unwind(AssertUnwindSafe(|| holder.raw_get::<Value>(1))) {
        Err(p) => {
            let text = panic_text(&*p);
            shared.park(p);
            return lua_error(lua, format!("a Rust panic: {text}"), Value::Nil);
        }
        Ok(Ok(v)) => v,
        Ok(Err(_)) => return Ok(Value::Nil),
    };
    let Value::Error(err) = v else { return Ok(Value::Nil) };
    if let Some(e) = err.exception() {
        match e.instance(lua) {
            Ok(t) => return Ok(Value::Table(t)),
            Err(_) => {
                let text = e.to_string();
                return lua_error(lua, text, Value::Error(err));
            }
        }
    }
    let text = message_of(&err);
    lua_error(lua, text, Value::Error(err))
}

/// Installs the translator with `nl_settranslator`. The runtime passes it
/// every caught value that is not an exception; a Lua wrapper hands over
/// only userdata, inside a table, so that mlua converts the value (and
/// resumes a panic it carries) inside `translate`'s `catch_unwind`, not
/// while it converts the arguments.
pub(crate) fn install_translator(lua: &Lua, shared: &Arc<Shared>) -> Result<()> {
    let shared = shared.clone();
    let f = lua.create_function(move |lua, holder: Table| {
        match catch_unwind(AssertUnwindSafe(|| translate(lua, &holder, &shared))) {
            Ok(Ok(v)) => Ok(v),
            Ok(Err(_)) => Ok(Value::Nil),
            Err(p) => {
                shared.park(p);
                Ok(Value::Nil)
            }
        }
    })?;
    let wrapper: Function = lua
        .load("local f = ... local type = type return function(e) if type(e) == 'userdata' then return f({ e }) end end")
        .set_name("=[nl translator]")
        .call(f)?;
    raw::set_translator(lua, wrapper)
}
