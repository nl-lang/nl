//! Remote, the wakeup and the pump.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use nl::prelude::*;

#[test]
fn remote_post_from_several_threads_with_the_wakeup() -> LuaResult<()> {
    let lua = nl::new()?;
    let wakeups = Arc::new(AtomicUsize::new(0));
    let w = wakeups.clone();
    lua.set_wakeup(move || {
        w.fetch_add(1, Ordering::SeqCst);
    });
    let total = lua.create_table()?;
    lua.globals().set("total", &total)?;
    let remote = lua.remote();
    let threads: Vec<_> = (0..8)
        .map(|i| {
            let remote = remote.clone();
            std::thread::spawn(move || {
                for j in 0..250 {
                    assert!(remote.post(move |lua| {
                        let t: Table = lua.globals().get("total")?;
                        t.raw_push(i * 1000 + j)
                    }));
                }
            })
        })
        .collect();
    for t in threads {
        t.join().expect("poster");
    }
    // The inbox went from empty to not empty once: one wakeup.
    assert_eq!(wakeups.load(Ordering::SeqCst), 1);
    let n = lua.pump();
    assert!(n >= 2000, "pump delivered {n}");
    assert_eq!(total.raw_len(), 2000);
    // Each thread's posts arrive in its order.
    let mut last = [-1i64; 8];
    for v in total.sequence_values::<i64>() {
        let v = v?;
        let (i, j) = ((v / 1000) as usize, v % 1000);
        assert!(j > last[i], "thread {i}: {j} after {}", last[i]);
        last[i] = j;
    }
    // Posting again wakes again.
    remote.post(|_| Ok(()));
    assert_eq!(wakeups.load(Ordering::SeqCst), 2);
    assert_eq!(lua.pump(), 1);
    assert_eq!(lua.pump(), 0);
    Ok(())
}

#[test]
fn a_wakeup_may_post() -> LuaResult<()> {
    let lua = nl::new()?;
    let remote = lua.remote();
    let inner = remote.clone();
    let count = Arc::new(AtomicUsize::new(0));
    let c = count.clone();
    lua.set_wakeup(move || {
        let c = c.clone();
        inner.post(move |_| {
            c.fetch_add(1, Ordering::SeqCst);
            Ok(())
        });
    });
    std::thread::spawn(move || remote.post(|_| Ok(()))).join().expect("poster");
    assert_eq!(lua.pump(), 2);
    assert_eq!(count.load(Ordering::SeqCst), 1);
    Ok(())
}

#[test]
fn a_panic_in_a_posted_closure_is_resumed_when_pump_returns() -> LuaResult<()> {
    let lua = nl::new()?;
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let s = seen.clone();
    lua.set_unhandled(move |_, e| s.lock().expect("lock").push(e.to_string()));
    let ran = Arc::new(AtomicUsize::new(0));
    let r = ran.clone();
    lua.remote().post(|_| panic!("posted closure went wrong"));
    lua.remote().post(move |_| {
        r.fetch_add(1, Ordering::SeqCst);
        Ok(())
    });
    let p = catch_unwind(AssertUnwindSafe(|| lua.pump())).expect_err("resumed");
    assert_eq!(p.downcast_ref::<&str>().copied(), Some("posted closure went wrong"));
    assert_eq!(ran.load(Ordering::SeqCst), 1, "the other closures still ran");
    // The unhandled hook saw it as a LuaError.
    assert_eq!(*seen.lock().expect("lock"), ["LuaError: a Rust panic: posted closure went wrong"]);
    assert_eq!(lua.pump(), 0);
    Ok(())
}

#[test]
fn a_state_without_nl_has_a_closed_remote() {
    let lua = Lua::new();
    assert!(!lua.remote().post(|_| Ok(())));
    assert_eq!(lua.pump(), 0);
    assert!(lua.load_nl("let x = 1", "=x").is_err());
    nl::open(&lua).expect("open");
    nl::open(&lua).expect("open twice does nothing");
    assert!(lua.remote().post(|_| Ok(())));
    assert_eq!(lua.pump(), 1);
}

#[test]
fn two_panics_in_one_pump_come_out_of_that_pump() -> LuaResult<()> {
    // Regression: they came out one per pump.
    let lua = nl::new()?;
    lua.set_unhandled(|_, _| {});
    let r = lua.remote();
    r.post(|_| panic!("first"));
    r.post(|_| panic!("second"));
    let p = catch_unwind(AssertUnwindSafe(|| lua.pump())).expect_err("resumed");
    let all = p.downcast_ref::<nl::Panics>().expect("both, as nl::Panics");
    let texts: Vec<_> = all.0.iter().map(|p| p.downcast_ref::<&str>().copied()).collect();
    assert_eq!(texts, [Some("first"), Some("second")]);
    assert_eq!(lua.pump(), 0, "nothing is left for the next pump");
    Ok(())
}

#[test]
fn a_posted_closure_left_at_the_close_is_dropped_there() -> LuaResult<()> {
    struct Counted(Arc<AtomicUsize>);
    impl Drop for Counted {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let dropped = Arc::new(AtomicUsize::new(0));
    let lua = nl::new()?;
    let c = Counted(dropped.clone());
    assert!(lua.remote().post(move |_| {
        let _keep = &c;
        Ok(())
    }));
    drop(lua);
    assert_eq!(dropped.load(Ordering::SeqCst), 1, "freed, not run");
    Ok(())
}

#[test]
fn a_panicking_drop_at_the_close_aborts() {
    // Regression: the panic unwound out of the VM's
    // callback inside lua_close. It aborts now, as mlua's finalizers do.
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_drop_panic")).output().expect("run drop_panic");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!out.status.success() && stdout.contains("posted: true") && !stdout.contains("not reached"), "{stdout}");
    assert!(stderr.contains("drop of a posted closure's capture"), "{stderr}");
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(out.status.signal(), Some(6), "SIGABRT, not an unwind: {:?}", out.status);
    }
}
