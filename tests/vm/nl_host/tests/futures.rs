//! Rust futures as NL tasks and NL tasks as Rust futures, on a
//! current-thread runtime (feature `async`).

use std::time::Duration;

use nl::prelude::*;

const SRC: &str = r##"
pub async fn fetch_twice(url: String)
  let a = :fetch(url)
  let x = a.await
  let b = :fetch("#{url}/again")
  let y = b.await
  "#{x} + #{y}"
end

pub async fn fetch_fails()
  try
    let t = :broken()
    t.await
  catch e: TypeError
    "caught #{e.message}"
  end
end

pub async fn double(t)
  let v = t.await
  v * 2
end
"##;

/// Pumps the state until `fut` completes.
async fn pumping<T>(lua: &Lua, fut: impl std::future::Future<Output = T>) -> T {
    let pump = async {
        loop {
            lua.pump();
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    };
    // The future first: a task that failed before it was awaited counts as
    // handled once the future is polled, and the pump decides that.
    tokio::select! {
        biased;
        v = fut => v,
        _ = pump => unreachable!(),
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread().enable_time().build().expect("runtime")
}

#[test]
fn create_task_function_and_into_future() -> LuaResult<()> {
    let rt = runtime();
    let lua = nl::new()?;
    lua.set_spawner(|future| {
        tokio::spawn(future);
    });
    lua.globals().set(
        "fetch",
        lua.create_task_function(|url: String| async move {
            tokio::time::sleep(Duration::from_millis(2)).await;
            Ok(format!("<{url}>"))
        })?,
    )?;
    lua.globals().set(
        "broken",
        lua.create_task_function(|(): ()| async move {
            Err::<bool, _>(Exception::new("TypeError", "bad future").into())
        })?,
    )?;
    let m: Table = lua.load_nl(SRC, "=futures")?.try_call(())?;
    rt.block_on(async {
        let t: Task = m.get::<Function>("fetch_twice")?.try_call("a")?;
        let s: String = pumping(&lua, t.into_future()).await?;
        assert_eq!(s, "<a> + <a/again>");

        let t: Task = m.get::<Function>("fetch_fails")?.try_call(())?;
        let s: String = pumping(&lua, t.into_future()).await?;
        assert_eq!(s, "caught bad future");

        // An NL task awaited from Rust, finished by a Resolver on another
        // thread.
        let src = lua.create_task()?;
        let t: Task = m.get::<Function>("double")?.try_call(src.clone())?;
        let r = src.resolver();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(5));
            r.resolve(21);
        });
        let v: f64 = pumping(&lua, t.into_future()).await?;
        assert_eq!(v, 42.0);

        // A failed task is an error holding its exception.
        let src = lua.create_task()?;
        let t: Task = m.get::<Function>("double")?.try_call(src.clone())?;
        src.reject(Exception::new("TypeError", "no number"))?;
        let err = pumping(&lua, t.into_future::<f64>()).await.unwrap_err();
        assert_eq!(err.exception().map(|e| e.message().to_owned()).as_deref(), Some("no number"));
        LuaResult::Ok(())
    })
}

#[test]
fn create_task_function_needs_a_spawner() -> LuaResult<()> {
    let lua = nl::new()?;
    let f = lua.create_task_function(|(): ()| async move { Ok(1) })?;
    let err = f.try_call::<Task>(()).unwrap_err();
    assert!(err.to_string().contains("set_spawner"), "{err}");
    Ok(())
}

#[test]
fn a_spawner_that_panics_stays_installed() -> LuaResult<()> {
    // Regression: the spawner was taken out while it ran
    // and not put back when it panicked; every later call then failed.
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::atomic::{AtomicUsize, Ordering};
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    let lua = nl::new()?;
    lua.set_unhandled(|_, _| {});
    lua.set_spawner(|fut| {
        if CALLS.fetch_add(1, Ordering::SeqCst) == 0 {
            panic!("spawner panics once")
        }
        drop(fut);
    });
    let f = lua.create_task_function(|x: f64| async move { Ok(x) })?;
    let call: Function = lua.load_nl("pub fn call(f) f(1) end", "=c")?.try_call::<Table>(())?.get("call")?;
    let first = catch_unwind(AssertUnwindSafe(|| call.try_call::<Value>(f.clone())));
    assert_eq!(first.expect_err("the spawner's panic").downcast_ref::<&str>().copied(), Some("spawner panics once"));
    let second: Task = call.try_call(f)?;
    assert_eq!(CALLS.load(Ordering::SeqCst), 2, "the spawner ran again");
    // That spawner drops the future: its Resolver rejects the task.
    lua.pump();
    assert!(matches!(second.state::<Value>()?, TaskState::Failed(_)));
    Ok(())
}
