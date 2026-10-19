//! Exceptions through NL, Rust and NL again, and panics.

use std::cell::RefCell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use nl::prelude::*;

const GAME: &str = r##"
struct GameError : Exception
  code: Number
end

let last = nil

fn fail()
  let e = GameError { message: "boom", code: 7 }
  last = e
  raise e
end

pub fn last_error() last end

# NL to Rust to NL: `pass` is Rust; it calls `fail` with try_call and
# returns the error with `?`.
pub fn through_rust()
  try
    :pass(fail!)
  catch err: GameError
    return [err == last, typeof(err), err.code, err.message, err.trace != nil]
  end
  return nil
end

pub fn catch_type(f)
  try
    f()
  catch err: TypeError
    return ["TypeError", err.message]
  catch err: LuaError
    return ["LuaError", err.message]
  catch err
    return [typeof(err), err.message]
  end
  return nil
end

pub fn same_twice(f)
  let a = nil
  let b = nil
  try
    f()
  catch e
    a = e
  end
  try
    f()
  catch e
    b = e
  end
  return a == b
end

pub fn fail_now() fail() end
"##;

fn game(lua: &Lua) -> LuaResult<Table> {
    lua.load_nl(GAME, "=game")?.try_call(())
}

#[test]
fn an_exception_keeps_type_and_identity_through_rust() -> LuaResult<()> {
    let lua = nl::new()?;
    let seen: Rc<RefCell<Option<Exception>>> = Rc::default();
    let s = seen.clone();
    lua.globals().set(
        "pass",
        lua.create_function(move |_, f: Function| {
            let r = f.try_call::<()>(());
            if let Err(err) = &r {
                *s.borrow_mut() = err.exception().cloned();
            }
            r
        })?,
    )?;
    let g = game(&lua)?;
    let r: Table = g.get::<Function>("through_rust")?.try_call(())?;
    assert!(r.get::<bool>(1)?, "the exception NL catches is the instance NL raised");
    assert_eq!(r.get::<String>(2)?, "GameError");
    assert_eq!(r.get::<f64>(3)?, 7.0);
    assert_eq!(r.get::<String>(4)?, "boom");
    assert!(r.get::<bool>(5)?, "the trace survives");

    let e = seen.borrow().clone().expect("Rust saw an Exception");
    assert_eq!(e.type_name(), "GameError");
    assert_eq!(e.message(), "boom");
    assert!(e.trace().unwrap_or("").contains("stack traceback"), "trace: {:?}", e.trace());
    let last: Table = g.get::<Function>("last_error")?.try_call(())?;
    assert_eq!(e.value(&lua)?, last, "value() is the raised table");
    assert_eq!(e.to_string(), "GameError: boom");
    Ok(())
}

#[test]
fn try_call_returns_the_exception() -> LuaResult<()> {
    let lua = nl::new()?;
    let g = game(&lua)?;
    let err = g.get::<Function>("fail_now")?.try_call::<()>(()).unwrap_err();
    let e = err.exception().expect("an Exception");
    assert_eq!(e.type_name(), "GameError");
    assert_eq!(e.value(&lua)?.get::<f64>("code")?, 7.0);
    // mlua's own call still works, and gives the text only.
    match g.get::<Function>("fail_now")?.call::<()>(()) {
        Err(LuaError::RuntimeError(text)) => assert!(text.contains("GameError: boom"), "{text}"),
        other => panic!("expected RuntimeError, got {other:?}"),
    }
    Ok(())
}

#[test]
fn rust_errors_reach_nl_as_exceptions() -> LuaResult<()> {
    let lua = nl::new()?;
    let g = game(&lua)?;
    let catch_type: Function = g.get("catch_type")?;

    let f = lua.create_function(|_, ()| -> LuaResult<()> {
        Err(Exception::new("TypeError", "amount must not be negative").into())
    })?;
    let r: Table = catch_type.try_call(f)?;
    assert_eq!((r.get::<String>(1)?, r.get::<String>(2)?), ("TypeError".into(), "amount must not be negative".into()));

    let f = lua.create_function(|_, ()| -> LuaResult<()> { Err(LuaError::runtime("plain rust error")) })?;
    let r: Table = catch_type.try_call(f)?;
    assert_eq!((r.get::<String>(1)?, r.get::<String>(2)?), ("LuaError".into(), "plain rust error".into()));

    let f = lua.create_function(|_, ()| -> LuaResult<()> { Err(LuaError::external(std::fmt::Error)) })?;
    let r: Table = catch_type.try_call(f)?;
    assert_eq!(r.get::<String>(1)?, "LuaError");
    assert_eq!(r.get::<String>(2)?, std::fmt::Error.to_string());

    // A type name that is not an exception type: a LuaError with the text.
    let f = lua.create_function(|_, ()| -> LuaResult<()> { Err(Exception::new("NoSuchType", "odd").into()) })?;
    let r: Table = catch_type.try_call(f)?;
    assert_eq!((r.get::<String>(1)?, r.get::<String>(2)?), ("LuaError".into(), "NoSuchType: odd".into()));

    // An exception made in Rust and raised twice is one instance.
    let e = Exception::new("TypeError", "same");
    let f = lua.create_function(move |_, ()| -> LuaResult<()> { Err(e.clone().into()) })?;
    assert!(g.get::<Function>("same_twice")?.try_call::<bool>(f)?);
    Ok(())
}

#[test]
fn an_exception_with_context_is_found() -> LuaResult<()> {
    let lua = nl::new()?;
    let g = game(&lua)?;
    let err = g.get::<Function>("fail_now")?.try_call::<()>(()).unwrap_err();
    let err = LuaError::WithContext { context: "while ticking".into(), cause: std::sync::Arc::new(err) };
    let err = LuaError::CallbackError { traceback: String::new(), cause: std::sync::Arc::new(err) };
    assert_eq!(err.exception().map(|e| e.type_name().to_owned()).as_deref(), Some("GameError"));
    Ok(())
}

#[test]
fn a_panic_in_a_callback_is_resumed_in_the_rust_caller() -> LuaResult<()> {
    let lua = nl::new()?;
    let g = game(&lua)?;
    let f = lua.create_function(|_, ()| -> LuaResult<()> { panic!("callback went wrong") })?;
    // Straight through NL.
    let call: Function = lua.load_nl("pub fn call(f) f() end", "=call")?.try_call::<Table>(())?.get("call")?;
    let r = catch_unwind(AssertUnwindSafe(|| call.try_call::<()>(f.clone())));
    let p = r.expect_err("the panic is resumed");
    assert_eq!(p.downcast_ref::<&str>().copied(), Some("callback went wrong"));
    // Through a `try` that catches it: NL sees a LuaError, and the panic is
    // still resumed when the nl call returns.
    let r = catch_unwind(AssertUnwindSafe(|| g.get::<Function>("catch_type")?.try_call::<Table>(f.clone())));
    let p = r.expect_err("the panic is resumed after NL caught it");
    assert_eq!(p.downcast_ref::<&str>().copied(), Some("callback went wrong"));
    // The state is still usable.
    assert_eq!(call.try_call::<Option<f64>>(lua.create_function(|_, ()| Ok(4.0))?)?, Some(4.0));
    Ok(())
}

#[test]
fn a_panic_parked_outside_a_call_of_the_crate_is_not_resumed_later() -> LuaResult<()> {
    // Regression: a panic that NL caught under mlua's own Function::call
    // was resumed by the next, unrelated, try_call.
    let lua = nl::new()?;
    lua.globals().set("host_panic", lua.create_function(|_, ()| -> LuaResult<()> { panic!("old panic") })?)?;
    let m: Table = lua
        .load_nl("pub fn swallow()\n  try\n    :host_panic()\n  catch e\n    return e.message\n  end\nend\npub fn two() 2 end\n", "=m")?
        .try_call(())?;
    let msg: String = m.get::<Function>("swallow")?.call(())?; // mlua's own call
    assert_eq!(msg, "a Rust panic: old panic");
    assert_eq!(m.get::<Function>("two")?.try_call::<f64>(())?, 2.0);
    // Under a call of the crate the same panic is resumed by that call.
    let r = catch_unwind(AssertUnwindSafe(|| m.get::<Function>("swallow").and_then(|f| f.try_call::<String>(()))));
    assert_eq!(r.expect_err("resumed").downcast_ref::<&str>().copied(), Some("old panic"));
    assert_eq!(m.get::<Function>("two")?.try_call::<f64>(())?, 2.0);
    Ok(())
}
