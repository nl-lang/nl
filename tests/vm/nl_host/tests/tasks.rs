//! Events, tasks, resolvers, the timer and the unhandled hook.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use nl::prelude::*;

const WAITER: &str = r##"
let log = []
pub fn log() log end

pub async fn wait(t)
  try
    let v = t.await
    Table.insert(log, "done #{v}")
    v
  catch e
    Table.insert(log, "#{typeof(e)}: #{e.message}")
    nil
  end
end

pub async fn nap(s)
  Task.sleep(s).await
  Table.insert(log, "woke after #{s}")
end

pub async fn fails()
  raise "nobody waits for me"
end
"##;

fn waiter(lua: &Lua) -> LuaResult<Table> {
    lua.load_nl(WAITER, "=waiter")?.try_call(())
}

fn log(m: &Table) -> LuaResult<Vec<String>> {
    m.get::<Function>("log")?.try_call::<Table>(())?.sequence_values().collect()
}

#[test]
fn a_task_resolved_and_rejected_from_rust() -> LuaResult<()> {
    let lua = nl::new()?;
    let m = waiter(&lua)?;
    let wait: Function = m.get("wait")?;

    let t = lua.create_task()?;
    assert!(matches!(t.state::<f64>()?, TaskState::Pending));
    let w: Task = wait.try_call(t.clone())?;
    t.resolve(21)?;
    assert!(matches!(t.state::<f64>()?, TaskState::Done(v) if v == 21.0));
    assert!(matches!(w.state::<f64>()?, TaskState::Done(v) if v == 21.0));

    let t = lua.create_task()?;
    wait.try_call::<Task>(t.clone())?;
    t.reject(Exception::new("TypeError", "no good"))?;
    match t.state::<Value>()? {
        TaskState::Failed(e) => assert_eq!((e.type_name(), e.message()), ("TypeError", "no good")),
        other => panic!("{other:?}"),
    }
    let t = lua.create_task()?;
    wait.try_call::<Task>(t.clone())?;
    t.reject(LuaError::runtime("text only"))?;
    assert_eq!(log(&m)?, ["done 21", "TypeError: no good", "LuaError: text only"]);
    Ok(())
}

#[test]
fn a_resolver_from_another_thread_finishes_the_task_at_the_pump() -> LuaResult<()> {
    let lua = nl::new()?;
    let m = waiter(&lua)?;
    let t = lua.create_task()?;
    m.get::<Function>("wait")?.try_call::<Task>(t.clone())?;
    let r = t.resolver();
    std::thread::spawn(move || r.resolve("from a thread")).join().expect("thread");
    assert!(matches!(t.state::<Value>()?, TaskState::Pending), "not before the pump");
    assert!(lua.pump() >= 1);
    assert_eq!(log(&m)?, ["done from a thread"]);

    let t = lua.create_task()?;
    m.get::<Function>("wait")?.try_call::<Task>(t.clone())?;
    let r = t.resolver();
    std::thread::spawn(move || r.reject(Exception::new("TypeError", "failed far away"))).join().expect("thread");
    lua.pump();
    assert_eq!(log(&m)?[1], "TypeError: failed far away");
    Ok(())
}

#[test]
fn a_dropped_resolver_rejects_its_task() -> LuaResult<()> {
    let lua = nl::new()?;
    let m = waiter(&lua)?;
    let t = lua.create_task()?;
    m.get::<Function>("wait")?.try_call::<Task>(t.clone())?;
    let r = t.resolver();
    std::thread::spawn(move || drop(r)).join().expect("thread");
    lua.pump();
    match t.state::<Value>()? {
        TaskState::Failed(e) => assert!(e.message().contains("dropped"), "{}", e.message()),
        other => panic!("{other:?}"),
    }
    assert!(log(&m)?[0].starts_with("Exception: the task's Resolver was dropped"), "{:?}", log(&m)?);
    Ok(())
}

#[test]
fn a_resolver_and_a_remote_after_the_state_closed() -> LuaResult<()> {
    let lua = nl::new()?;
    let t = lua.create_task()?;
    let r1 = t.resolver();
    let r2 = t.resolver();
    let r3 = t.resolver();
    let remote = lua.remote();
    // Posted before the close and never pumped: freed when the state closes.
    assert!(remote.post(|_| Ok(())));
    drop(t);
    drop(lua);
    assert!(!remote.post(|_| panic!("must not run")), "post returns false after the close");
    // None of these may crash.
    r1.resolve(1);
    // Without mlua's `send` feature mlua::Error is not Send: an Exception is.
    std::thread::spawn(move || r2.reject(Exception::new("LuaError", "late"))).join().expect("thread");
    drop(r3);
    Ok(())
}

#[test]
fn task_sleep_with_a_host_timer() -> LuaResult<()> {
    let lua = nl::new()?;
    let m = waiter(&lua)?;
    let wakes: Rc<RefCell<Vec<(Duration, Wake)>>> = Rc::default();
    let w = wakes.clone();
    lua.set_timer(move |d, wake| w.borrow_mut().push((d, wake)));
    let t: Task = m.get::<Function>("nap")?.try_call(0.25)?;
    let t2: Task = m.get::<Function>("nap")?.try_call(2)?;
    assert_eq!(wakes.borrow().len(), 2);
    assert_eq!(wakes.borrow()[0].0, Duration::from_millis(250));
    let (_, first) = wakes.borrow_mut().remove(0);
    first.wake();
    assert!(matches!(t.state::<Value>()?, TaskState::Pending), "a wake is posted");
    lua.pump();
    assert!(matches!(t.state::<Value>()?, TaskState::Done(_)));
    // A wake dropped unused ends its sleep too.
    wakes.borrow_mut().clear();
    lua.pump();
    assert!(matches!(t2.state::<Value>()?, TaskState::Done(_)));
    assert_eq!(log(&m)?, ["woke after 0.25", "woke after 2"]);
    Ok(())
}

#[test]
fn a_wake_after_the_state_closed() -> LuaResult<()> {
    let wakes: Rc<RefCell<Vec<Wake>>> = Rc::default();
    {
        let lua = nl::new()?;
        let m = waiter(&lua)?;
        let w = wakes.clone();
        lua.set_timer(move |_, wake| w.borrow_mut().push(wake));
        m.get::<Function>("nap")?.try_call::<Task>(1)?;
        m.get::<Function>("nap")?.try_call::<Task>(2)?;
        // One posted and never pumped: dropped when the state closes.
        wakes.borrow_mut().remove(0).wake();
    }
    // The state is gone: waking only frees what the VM allocated.
    let last = wakes.borrow_mut().pop().expect("a wake");
    std::thread::spawn(move || last.wake()).join().expect("thread");
    Ok(())
}

#[test]
fn the_unhandled_hook_gets_failures_nobody_awaits() -> LuaResult<()> {
    let lua = nl::new()?;
    let m = waiter(&lua)?;
    let seen: Rc<RefCell<Vec<String>>> = Rc::default();
    let s = seen.clone();
    lua.set_unhandled(move |_, e| s.borrow_mut().push(e.to_string()));
    m.get::<Function>("fails")?.try_call::<Task>(())?;
    assert!(seen.borrow().is_empty(), "decided at the next pump");
    lua.pump();
    assert_eq!(*seen.borrow(), ["Exception: nobody waits for me"]);
    // A posted closure that fails goes there too.
    lua.remote().post(|_| Err(LuaError::runtime("posted and failed")));
    lua.pump();
    assert_eq!(seen.borrow()[1], "LuaError: posted and failed");
    Ok(())
}

#[test]
fn events_from_rust_and_nl() -> LuaResult<()> {
    let lua = nl::new()?;
    let src = r##"
let hits = []
pub fn hits() hits end
on PlayerHit as 'armor' do |who: String, amount: Number|
  Table.insert(hits, "#{who} #{amount}")
end
on Fails as 'bad' do
  raise "handler failed"
end
"##;
    let m: Table = lua.load_nl(src, "=events")?.try_call(())?;
    // A Rust handler that returns nothing lets the NL one run after it.
    lua.on_event("PlayerHit", "host:first", |_, (who, amount): (String, f64)| {
        assert_eq!(who, "ann");
        let _ = amount;
        Ok(())
    })?;
    let r: Option<f64> = lua.run_event("PlayerHit", ("ann", 10.0))?;
    assert_eq!(r, None);
    // One that returns a value ends the run.
    lua.on_event("Damage", "host:half", |_, amount: f64| Ok(amount / 2.0))?;
    lua.on_event("Damage", "host:never", |_, _: f64| -> LuaResult<f64> { panic!("not reached") })?;
    assert_eq!(lua.run_event::<f64>("Damage", 10.0)?, 5.0);
    lua.off_event("Damage", "host:half")?;
    lua.off_event("Damage", "host:never")?;
    assert_eq!(lua.run_event::<Option<f64>>("Damage", 10.0)?, None);
    // An NL handler's exception comes back whole.
    let err = lua.run_event::<()>("Fails", ()).unwrap_err();
    assert_eq!(err.exception().map(|e| e.message().to_owned()).as_deref(), Some("handler failed"));
    let hits: Vec<String> = m.get::<Function>("hits")?.try_call::<Table>(())?.sequence_values().collect::<LuaResult<_>>()?;
    assert_eq!(hits, ["ann 10"]);
    Ok(())
}
