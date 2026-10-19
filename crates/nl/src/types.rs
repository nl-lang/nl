//! Types, values, methods and member calls.

use mlua::{
    AnyUserData, Error, FromLua, FromLuaMulti, Function, IntoLua, IntoLuaMulti, Lua, MaybeSend, MaybeSync, Result,
    Table, UserData, Value, WeakLua,
};

use crate::exception::{NlErrorExt, convert, values};
use crate::raw;

/// The `Lua` a value belongs to.
pub(crate) fn lua_of(weak: &WeakLua) -> Result<Lua> {
    weak.try_upgrade().ok_or_else(|| Error::runtime("nl: the Lua state of this value has been closed"))
}

/// A struct, interface, enum or host type: a type value, as
/// NL code holds it. It converts from the value an export table or
/// `nl_type` gives.
#[derive(Clone, Debug)]
pub struct Type(Table);

impl Type {
    /// The type's NL name (`Player`, `Unit`).
    ///
    /// No function of `nl.h` names a type; this reads the name from the
    /// `TypeError` that `nlL_checkisa` raises for `nil` against it.
    pub fn name(&self) -> Result<String> {
        let lua = lua_of(self.0.weak_lua())?;
        let err = match values(&lua, || raw::checknil(&lua, &self.0)) {
            Ok(_) => return Err(Error::runtime("nl: this type accepts nil, so it cannot be named")),
            Err(e) => e,
        };
        let message = err.exception().map(|e| e.message().to_owned()).unwrap_or_default();
        message
            .strip_prefix("argument 2 expects ")
            .and_then(|rest| rest.rsplit_once(", got "))
            .map(|(name, _)| name.to_owned())
            .ok_or(err)
    }

    /// `value is T`. An enum's values are its strings or numbers.
    pub fn has(&self, value: &Value) -> bool {
        lua_of(self.0.weak_lua()).and_then(|lua| raw::isa(&lua, value.clone(), &self.0)).unwrap_or(false)
    }

    /// `Name { ... }`: the instance, with defaults filled in and field types
    /// checked (`nl_new`).
    #[allow(clippy::new_ret_no_self)] // Named after NL's `Name { ... }`.
    pub fn new(&self, fields: impl IntoLua) -> Result<Table> {
        let lua = lua_of(self.0.weak_lua())?;
        let fields = fields.into_lua(&lua)?;
        crate::state::scoped(&lua, || instantiate(&lua, &self.0, fields))
    }

    /// The type value itself.
    pub fn as_table(&self) -> &Table {
        &self.0
    }
}

impl FromLua for Type {
    fn from_lua(value: Value, _lua: &Lua) -> Result<Self> {
        match value {
            Value::Table(t) => Ok(Type(t)),
            other => Err(Error::FromLuaConversionError {
                from: other.type_name(),
                to: "Type".to_owned(),
                message: Some("a type is a struct, interface, enum or host type".to_owned()),
            }),
        }
    }
}

impl IntoLua for Type {
    fn into_lua(self, _lua: &Lua) -> Result<Value> {
        Ok(Value::Table(self.0))
    }
}

/// `nl_new`, without resuming a parked panic (`Type::new` does).
pub(crate) fn instantiate(lua: &Lua, ty: &Table, fields: Value) -> Result<Table> {
    match convert(lua, raw::new_instance(lua, ty, fields)?)?.into_iter().next() {
        Some(Value::Table(t)) => Ok(t),
        other => Err(Error::runtime(format!("nl_new gave {other:?}, not an instance"))),
    }
}

pub(crate) fn nl_type(lua: &Lua, name: &str) -> Result<Option<Type>> {
    Ok(match raw::gettype(lua, name)? {
        Value::Table(t) => Some(Type(t)),
        _ => None,
    })
}

pub(crate) fn type_of(lua: &Lua, value: &Value) -> Result<String> {
    raw::typeof_(lua, value.clone())?.ok_or_else(|| Error::runtime("nl_typeof failed"))
}

/// Creates `value` as userdata, which makes mlua build the metatable for
/// `T`, and makes that metatable the host type `name` with `__methods`.
pub(crate) fn register_type_with<T>(lua: &Lua, name: &str, value: T) -> Result<AnyUserData>
where
    T: UserData + MaybeSend + MaybeSync + 'static,
{
    let ud = lua.create_userdata(value)?;
    values(lua, || raw::settype(lua, name, Value::UserData(ud.clone())))?;
    if let Some(st) = lua.app_data_ref::<crate::state::NlState>() {
        st.types.borrow_mut().insert(std::any::TypeId::of::<T>(), name.to_owned());
    }
    Ok(ud)
}

/// A host type whose values cross between states:
/// `encode` runs in the sending state and returns a value that can itself
/// be sent, `decode` runs in the receiving state and makes the new value.
pub trait Transfer: UserData + MaybeSend + MaybeSync + Sized + 'static {
    fn encode(&self, lua: &Lua) -> Result<Value>;
    fn decode(lua: &Lua, value: Value) -> Result<Self>;
}

/// `nl_settransfer` for the name `register_type` gave `T` in this state.
pub(crate) fn register_transfer<T: Transfer>(lua: &Lua) -> Result<()> {
    let name = crate::state::nl_state(lua)?.types.borrow().get(&std::any::TypeId::of::<T>()).cloned();
    let Some(name) = name else {
        return Err(Error::runtime(format!(
            "register_transfer: {} is not a host type of this state (register_type first)",
            std::any::type_name::<T>()
        )));
    };
    let encode = lua.create_function(|lua, ud: AnyUserData| ud.borrow::<T>()?.encode(lua))?;
    let decode = lua.create_function(|lua, v: Value| lua.create_userdata(T::decode(lua, v)?))?;
    values(lua, || raw::settransfer(lua, &name, encode, decode)).map(drop)
}

pub(crate) fn keyed(lua: &Lua, opts: Table) -> Result<Value> {
    Ok(values(lua, || raw::keyed(lua, opts))?.into_iter().next().unwrap_or(Value::Nil))
}

/// A range, `a..b` or `a...b` (`nl_pushrange`, `nl_torange`). Rust's
/// `a..=b` is NL's `a..b`; Rust's `a..b` is NL's `a...b`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Range {
    pub first: f64,
    pub last: f64,
    pub exclusive: bool,
}

impl From<std::ops::RangeInclusive<f64>> for Range {
    fn from(r: std::ops::RangeInclusive<f64>) -> Range {
        Range { first: *r.start(), last: *r.end(), exclusive: false }
    }
}

impl From<std::ops::Range<f64>> for Range {
    fn from(r: std::ops::Range<f64>) -> Range {
        Range { first: r.start, last: r.end, exclusive: true }
    }
}

impl FromLua for Range {
    fn from_lua(value: Value, lua: &Lua) -> Result<Self> {
        let from = value.type_name();
        match raw::torange(lua, value)? {
            Some((first, last, exclusive)) => Ok(Range { first, last, exclusive }),
            None => Err(Error::FromLuaConversionError { from, to: "Range".to_owned(), message: None }),
        }
    }
}

impl IntoLua for Range {
    fn into_lua(self, lua: &Lua) -> Result<Value> {
        raw::pushrange(lua, self.first, self.last, self.exclusive)
    }
}

/// NL's method mark on functions, and calls that keep NL exceptions.
pub trait NlFunctionExt {
    /// Marks the function as a method (`nl_markmethod`): NL's `x.m(a)` then
    /// passes `x` as `self`.
    fn mark_method(&self) -> Result<()>;
    /// Whether the function carries the mark (`nl_ismethod`).
    fn is_method(&self) -> bool;
    /// Calls the function under `nl_pcall`: a failure is an `mlua::Error`
    /// holding the [`Exception`](crate::Exception).
    fn try_call<R: FromLuaMulti>(&self, args: impl IntoLuaMulti) -> Result<R>;
}

impl NlFunctionExt for Function {
    fn mark_method(&self) -> Result<()> {
        raw::markmethod(&lua_of(self.weak_lua())?, self)
    }

    fn is_method(&self) -> bool {
        lua_of(self.weak_lua()).and_then(|lua| raw::ismethod(&lua, self)).unwrap_or(false)
    }

    fn try_call<R: FromLuaMulti>(&self, args: impl IntoLuaMulti) -> Result<R> {
        let lua = lua_of(self.weak_lua())?;
        let args = args.into_lua_multi(&lua)?;
        let v = values(&lua, || raw::pcall(&lua, self, args))?;
        R::from_lua_multi(v, &lua)
    }
}

/// NL's member call, `x.name(args)` (`nlL_callmember`).
pub trait NlObjectExt {
    /// Reads the member and passes the receiver only when it is a method,
    /// as NL code does. mlua's `call_method` always passes it.
    fn call_member<R: FromLuaMulti>(&self, name: &str, args: impl IntoLuaMulti) -> Result<R>;
}

fn call_member<R: FromLuaMulti>(weak: &WeakLua, recv: Value, name: &str, args: impl IntoLuaMulti) -> Result<R> {
    let lua = lua_of(weak)?;
    let args = args.into_lua_multi(&lua)?;
    let v = values(&lua, || raw::callmember(&lua, name, recv, args))?;
    R::from_lua_multi(v, &lua)
}

impl NlObjectExt for Table {
    fn call_member<R: FromLuaMulti>(&self, name: &str, args: impl IntoLuaMulti) -> Result<R> {
        call_member(self.weak_lua(), Value::Table(self.clone()), name, args)
    }
}

impl NlObjectExt for AnyUserData {
    fn call_member<R: FromLuaMulti>(&self, name: &str, args: impl IntoLuaMulti) -> Result<R> {
        call_member(self.weak_lua(), Value::UserData(self.clone()), name, args)
    }
}
