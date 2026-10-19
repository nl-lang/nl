//! The worker-init closure's view of a worker's state.
//!
//! A worker's state is the VM's: it makes it with `luaL_newstate` and
//! `luaL_openlibs` and closes it when the worker ends, whatever Rust still
//! holds. mlua attaches to it with `Lua::get_or_init_from_ptr` and neither
//! owns nor closes it, and a `Lua` is `Clone + 'static`: a clone kept past
//! the closure would point at a closed state. So the closure gets a
//! [`WorkerInit`], which offers what a worker's setup needs and hands out no
//! handle into the state.

use std::marker::PhantomData;
use std::path::Path;

use mlua::{FromLuaMulti, IntoLua, IntoLuaMulti, Lua, MaybeSend, MaybeSync, Result, UserData, Value};

use crate::NlExt;
use crate::types::Transfer;

/// What the worker-init closure can do in a new worker's state: register
/// host types and their transfer pairs, install host functions and values,
/// widen the import path. It is neither `Clone`, `Send` nor `Sync`, and
/// nothing it returns refers to the state, so nothing of the state outlives
/// the closure.
pub struct WorkerInit<'a> {
    lua: &'a Lua,
    _not_send: PhantomData<*const ()>,
}

impl<'a> WorkerInit<'a> {
    pub(crate) fn new(lua: &'a Lua) -> WorkerInit<'a> {
        WorkerInit { lua, _not_send: PhantomData }
    }

    /// [`NlExt::register_type`] in the worker's state.
    pub fn register_type<T>(&self, name: &str) -> Result<()>
    where
        T: UserData + Default + MaybeSend + MaybeSync + 'static,
    {
        self.lua.register_type::<T>(name)
    }

    /// [`NlExt::register_type_with`] in the worker's state; the value is
    /// dropped, not returned.
    pub fn register_type_with<T>(&self, name: &str, first: T) -> Result<()>
    where
        T: UserData + MaybeSend + MaybeSync + 'static,
    {
        self.lua.register_type_with(name, first)?.destroy()
    }

    /// [`NlExt::register_transfer`] in the worker's state, after
    /// [`register_type`](WorkerInit::register_type).
    pub fn register_transfer<T: Transfer>(&self) -> Result<()> {
        self.lua.register_transfer::<T>()
    }

    /// Installs the global function `name`, which scripts in the worker call
    /// as `:name(args)`. It runs on the worker's thread. It gets its
    /// arguments, not the state: a table, function or userdata it receives
    /// belongs to the worker's state and must not be kept past the call.
    pub fn set_function<A, R, F>(&self, name: &str, f: F) -> Result<()>
    where
        A: FromLuaMulti,
        R: IntoLuaMulti,
        F: Fn(A) -> Result<R> + MaybeSend + 'static,
    {
        let f = self.lua.create_function(move |_, args: A| f(args))?;
        self.lua.globals().set(name, f)
    }

    /// Sets the global `name` to `value`, made in the worker's state: a
    /// number, a string, a userdata of a Rust type, and so on. A value of
    /// another state cannot be set.
    pub fn set_global(&self, name: &str, value: impl IntoLua) -> Result<()> {
        let v: Value = value.into_lua(self.lua)?;
        self.lua.globals().set(name, v)
    }

    /// [`NlExt::add_path`] in the worker's state. A worker starts with the
    /// main state's search path already.
    pub fn add_path(&self, dir: impl AsRef<Path>) -> Result<()> {
        self.lua.add_path(dir)
    }

    /// [`NlExt::add_decl`] in the worker's state. A worker starts with the
    /// main state's declaration files already.
    pub fn add_decl(&self, name: &str) -> Result<()> {
        self.lua.add_decl(name)
    }

    /// Whether the state is a worker's (`nl_isworker`): true here, unless the
    /// host's own code runs the closure on another state.
    pub fn is_worker(&self) -> bool {
        self.lua.is_worker()
    }

    /// The worker's state as mlua sees it, for what this type does not
    /// offer.
    ///
    /// # Safety
    ///
    /// No handle into the state may outlive the closure: not a clone of the
    /// `Lua`, nor a `Table`, `Function`, `AnyUserData` or other value made
    /// or read through it. The VM closes the state when the worker ends, and
    /// mlua does not know. A value made through it fails (mlua reports the
    /// state destroyed) once the state has closed; a clone of the `Lua`
    /// would not, so the process aborts when the state closes while one is
    /// alive. With mlua's `send` feature the handles are `Send`, and none of
    /// them may reach another thread either: the VM runs the worker's state
    /// without mlua's lock.
    #[allow(unsafe_code)] // An unsafe contract for the caller; the body does nothing unsafe.
    pub unsafe fn lua(&self) -> &Lua {
        self.lua
    }
}

/// Sets a new worker's state up for the init closure: nl opened in it, the
/// debug library hidden from its scripts (the VM's `luaL_openlibs` opened
/// it; the runtime took what it needs when it opened), then the closure.
pub(crate) fn init(lua: &Lua, f: &crate::raw::WorkerInitFn) -> Result<()> {
    crate::state::open(lua)?;
    crate::state::hide_debug(lua)?;
    if let Some(st) = lua.app_data_ref::<crate::state::NlState>() {
        st.worker_lua.replace(Some(lua.weak()));
    }
    f(&WorkerInit::new(lua))
}
