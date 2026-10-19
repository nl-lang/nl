//! With mlua's `send` feature (run.sh's send pass): several threads make
//! calls of the crate on one main state, which mlua's lock lets them do in
//! turns.
#![cfg(feature = "send")]

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Barrier, Mutex};

use nl::prelude::*;

fn what(r: std::thread::Result<LuaResult<Value>>) -> String {
    match r {
        Ok(Ok(_)) => "Ok".into(),
        Ok(Err(e)) => format!("Err {}", e.to_string().lines().next().unwrap_or("")),
        Err(p) => match p.downcast_ref::<&str>() {
            Some(s) => format!("panic {s}"),
            None if p.downcast_ref::<nl::Panics>().is_some() => "Panics".into(),
            None => "panic ?".into(),
        },
    }
}

#[test]
fn each_thread_gets_back_its_own_panic() -> LuaResult<()> {
    // Regression: the scopes of parked panics were shared
    // between threads, so one thread's call could take, or drop, the other's
    // panic, and a call that should have panicked returned Ok.
    let lua = nl::new()?;
    lua.globals().set("boom1", lua.create_function(|_, ()| -> LuaResult<()> { panic!("p1") })?)?;
    lua.globals().set("boom2", lua.create_function(|_, ()| -> LuaResult<()> { panic!("p2") })?)?;
    let m: Table = lua
        .load_nl("pub fn one()\n  try\n    :boom1()\n  catch e\n  end\n  1\nend\npub fn two() :boom2() end\n", "=m")?
        .try_call(())?;
    let one: Function = m.get("one")?;
    let two: Function = m.get("two")?;
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let n = 5000;
    let bar = Arc::new(Barrier::new(2));
    let wrong = Arc::new(Mutex::new(Vec::new()));
    let (b2, w2) = (bar.clone(), wrong.clone());
    let h = std::thread::spawn(move || {
        for _ in 0..n {
            b2.wait();
            let got = what(catch_unwind(AssertUnwindSafe(|| two.try_call::<Value>(()))));
            if got != "panic p2" {
                w2.lock().expect("lock").push(format!("thread 2: {got}"));
            }
        }
    });
    for _ in 0..n {
        bar.wait();
        let got = what(catch_unwind(AssertUnwindSafe(|| one.try_call::<Value>(()))));
        if got != "panic p1" {
            wrong.lock().expect("lock").push(format!("thread 1: {got}"));
        }
    }
    h.join().expect("thread 2");
    std::panic::set_hook(hook);
    let wrong = wrong.lock().expect("lock");
    assert!(wrong.is_empty(), "{} of {} calls got another result, first: {:?}", wrong.len(), 2 * n, wrong.first());
    Ok(())
}
