//! Workers from Rust: pool sizes 0, 1 and N, the worker-init
//! closure (host functions, a host type with its transfer, `is_worker`, a
//! failure), `spawn`, `drain`, `Task::into_future`, `Remote::post` from a
//! worker and the wakeup, and the worker options.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use nl::prelude::*;

#[derive(Default, Clone, Copy, Debug, PartialEq)]
struct Vec2 {
    x: f64,
    y: f64,
}

impl UserData for Vec2 {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("x", |_, v| Ok(v.x));
        fields.add_field_method_get("y", |_, v| Ok(v.y));
    }
}

impl Transfer for Vec2 {
    fn encode(&self, lua: &Lua) -> LuaResult<Value> {
        Ok(Value::Table(lua.create_sequence_from([self.x, self.y])?))
    }
    fn decode(_: &Lua, value: Value) -> LuaResult<Self> {
        let t = value.as_table().ok_or_else(|| LuaError::runtime("Vec2: a table expected"))?;
        Ok(Vec2 { x: t.get(1)?, y: t.get(2)? })
    }
}

const JOBS: &str = r##"
pub fn square(x) x * x end
pub fn double_on_host(x) :host_double(x) end
pub fn init_ran() "#{:init_isworker}" end
pub fn scaled(v, k) :vec2_scale(v, k) end
pub fn describe(v) "#{typeof(v)} #{v.x},#{v.y}" end
pub fn tell_main(v)
  :notify_main(v)
  v
end
pub fn fail(msg) raise msg end
pub fn host_panics() :host_panic() end
pub fn calls_back() :host_calls_nl() end
pub fn debug_hidden() :debug == nil end
pub fn allocate(n)
  let t = []
  for i in 1..n
    t[i] = "a string of some length, number #{i}"
  end
  t.len
end
"##;

const SERVICE: &str = "pub fn add(a, b) a + b end\n";

/// Host functions and the host type, in the main state.
fn host_main(lua: &Lua) -> LuaResult<()> {
    lua.globals().set("host_double", lua.create_function(|_, x: f64| Ok(2.0 * x))?)?;
    lua.globals().set("init_isworker", lua.is_worker())?;
    lua.register_type::<Vec2>("Vec2")?;
    lua.register_transfer::<Vec2>()?;
    lua.globals().set("vec2_scale", lua.create_function(|_, (v, k): (AnyUserData, f64)| scale(&v, k))?)?;
    Ok(())
}

/// The same in each worker, through the init closure's WorkerInit.
fn host_worker(w: &WorkerInit) -> LuaResult<()> {
    w.set_function("host_double", |x: f64| Ok(2.0 * x))?;
    w.set_global("init_isworker", w.is_worker())?;
    w.register_type::<Vec2>("Vec2")?;
    w.register_transfer::<Vec2>()?;
    w.set_function("vec2_scale", |(v, k): (AnyUserData, f64)| scale(&v, k))?;
    w.set_function("host_panic", |()| -> LuaResult<()> { panic!("host function went wrong") })?;
    Ok(())
}

fn scale(v: &AnyUserData, k: f64) -> LuaResult<Vec2> {
    let v = *v.borrow::<Vec2>()?;
    Ok(Vec2 { x: v.x * k, y: v.y * k })
}

struct Setup {
    lua: Lua,
    jobs: Table,
    inits: Arc<AtomicUsize>,
    notified: Arc<std::sync::Mutex<Vec<f64>>>,
}

fn setup(workers: Option<usize>, name: &str) -> LuaResult<Setup> {
    let dir = nl_host::tmpdir(name);
    std::fs::write(dir.join("jobs.nl"), JOBS).expect("write");
    std::fs::write(dir.join("svc.nl"), SERVICE).expect("write");
    let lua = nl::new()?;
    // Tasks read with `state` are not awaited by NL code: their failures
    // reach the hook (as in tests/vm/real/test_workers.c).
    lua.set_unhandled(|_, _| {});
    lua.add_path(&dir)?;
    host_main(&lua)?;
    if let Some(n) = workers {
        lua.set_workers(n);
    }
    let inits = Arc::new(AtomicUsize::new(0));
    let notified = Arc::new(std::sync::Mutex::new(Vec::new()));
    // What the init closure may capture: Send + Sync things, such as the
    // main state's Remote, never the main state's Lua.
    let (i, remote, n) = (inits.clone(), lua.remote(), notified.clone());
    nl_host::set_worker_init(&lua, move |w| {
        i.fetch_add(1, Ordering::SeqCst);
        host_worker(w)?;
        let (remote, n) = (remote.clone(), n.clone());
        w.set_function("notify_main", move |v: f64| {
            let n = n.clone();
            Ok(remote.post(move |_| {
                n.lock().expect("lock").push(v);
                Ok(())
            }))
        })
    });
    let jobs = lua.import("jobs")?;
    Ok(Setup { lua, jobs, inits, notified })
}

fn done<R: FromLua>(lua: &Lua, t: &Task) -> R {
    lua.drain();
    match t.state::<R>().expect("state") {
        TaskState::Done(v) => v,
        TaskState::Failed(e) => panic!("failed: {e}"),
        TaskState::Pending => panic!("still pending after drain"),
    }
}

fn failed(lua: &Lua, t: &Task) -> Exception {
    lua.drain();
    match t.state::<Value>().expect("state") {
        TaskState::Failed(e) => e,
        other => panic!("expected a failure, got {other:?}"),
    }
}

fn basics(workers: usize) -> LuaResult<()> {
    let s = setup(Some(workers), &format!("basics{workers}"))?;
    let (lua, jobs) = (&s.lua, &s.jobs);
    let f = |name: &str| jobs.get::<Function>(name).expect(name);
    assert!(!lua.is_worker());
    let t = lua.spawn(&f("square"), 7)?;
    assert_eq!(done::<f64>(lua, &t), 49.0);
    let tasks: Vec<Task> = (1..=20).map(|i| lua.spawn(&f("square"), i)).collect::<LuaResult<_>>()?;
    lua.drain();
    let sum: f64 = tasks.iter().map(|t| done::<f64>(lua, t)).sum();
    assert_eq!(sum, 2870.0);
    assert_eq!(done::<f64>(lua, &lua.spawn(&f("double_on_host"), 21)?), 42.0);
    assert_eq!(done::<String>(lua, &lua.spawn(&f("init_ran"), ())?), "true", "is_worker in the init closure");
    // A host type crosses both ways through its transfer pair.
    let t = lua.spawn(&f("scaled"), (Vec2 { x: 1.0, y: 2.0 }, 3.0))?;
    let v: AnyUserData = done(lua, &t);
    assert_eq!(*v.borrow::<Vec2>()?, Vec2 { x: 3.0, y: 6.0 });
    assert_eq!(done::<String>(lua, &lua.spawn(&f("describe"), Vec2 { x: 5.0, y: 8.0 })?), "Vec2 5,8");
    // A job's exception comes back as an exception.
    let e = failed(lua, &lua.spawn(&f("fail"), "bad job")?);
    assert_eq!((e.type_name(), e.message()), ("Exception", "bad job"));
    // A worker posts to the main state through a Remote it was given.
    let t = lua.spawn(&f("tell_main"), 9)?;
    assert_eq!(done::<f64>(lua, &t), 9.0);
    lua.pump();
    assert_eq!(*s.notified.lock().expect("lock"), [9.0]);
    assert!(s.inits.load(Ordering::SeqCst) >= 1, "the init closure ran");
    // Nothing outstanding: drain returns at once.
    let t0 = Instant::now();
    lua.drain();
    assert_eq!(lua.drain(), 0);
    assert!(t0.elapsed() < Duration::from_secs(5));
    Ok(())
}

#[test]
fn workers_zero() -> LuaResult<()> {
    basics(0)
}

#[test]
fn workers_one() -> LuaResult<()> {
    basics(1)
}

#[test]
fn workers_four() -> LuaResult<()> {
    basics(4)
}

#[test]
fn spawn_from_nl_and_send_errors() -> LuaResult<()> {
    let s = setup(Some(2), "spawn_nl")?;
    let lua = &s.lua;
    let m: Table = lua
        .load_nl(
            "import { square } from 'jobs'\npub async fn run(n)\n  let t = spawn square(n)\n  t.await + 1\nend\n",
            "=main",
        )?
        .try_call(())?;
    let t: Task = m.get::<Function>("run")?.try_call(4)?;
    assert_eq!(done::<f64>(lua, &t), 17.0);
    // A function without a name cannot be sent.
    let err = lua.spawn(&lua.create_function(|_, ()| Ok(()))?, 1).unwrap_err();
    let e = err.exception().expect("SendError");
    assert_eq!(e.type_name(), "SendError");
    assert_eq!(e.message(), "cannot send a function without a name (argument 1)");
    // A userdata without a transfer pair cannot either.
    struct Opaque;
    impl UserData for Opaque {}
    let err = lua.spawn(&s.jobs.get::<Function>("square")?, lua.create_userdata(Opaque)?).unwrap_err();
    assert_eq!(err.exception().map(|e| e.type_name().to_owned()).as_deref(), Some("SendError"));
    Ok(())
}

#[test]
fn an_init_closure_that_fails() -> LuaResult<()> {
    for workers in [0, 1] {
        let s = setup(Some(workers), &format!("initfail{workers}"))?;
        nl_host::set_worker_init(&s.lua, |_| Err(Exception::new("TypeError", "init broke").into()));
        let e = failed(&s.lua, &s.lua.spawn(&s.jobs.get::<Function>("square")?, 2)?);
        assert_eq!(e.type_name(), "WorkerError");
        assert_eq!(e.message(), "the worker-init function failed: TypeError: init broke");
    }
    let s = setup(Some(1), "initfail_text")?;
    nl_host::set_worker_init(&s.lua, |_| Err(LuaError::runtime("plain text")));
    let e = failed(&s.lua, &s.lua.spawn(&s.jobs.get::<Function>("square")?, 2)?);
    assert_eq!(e.message(), "the worker-init function failed: plain text");
    Ok(())
}

#[test]
fn a_panic_in_a_worker_fails_the_job() -> LuaResult<()> {
    // A worker's loop is C: no Rust caller can receive the panic. The job
    // fails with the LuaError NL saw, and the main state goes on.
    for workers in [0, 2] {
        let s = setup(Some(workers), &format!("panic{workers}"))?;
        let e = failed(&s.lua, &s.lua.spawn(&s.jobs.get::<Function>("host_panics")?, ())?);
        assert_eq!((e.type_name(), e.message()), ("LuaError", "a Rust panic: host function went wrong"));
        assert_eq!(done::<f64>(&s.lua, &s.lua.spawn(&s.jobs.get::<Function>("square")?, 3)?), 9.0);
    }
    Ok(())
}

#[test]
fn a_panic_in_a_worker_is_dropped_not_kept() -> LuaResult<()> {
    // Regression: a panic parked in a worker's state was resumed by the
    // next call of the crate there, failing later jobs with it.
    for workers in [0, 1] {
        let s = setup(Some(workers), &format!("stale{workers}"))?;
        nl_host::set_worker_init(&s.lua, |w| {
            host_worker(w)?;
            // SAFETY: the function holds no handle into the state; the &Lua
            // it is given lives for the call.
            let lua = unsafe { w.lua() };
            lua.globals().set(
                "host_calls_nl",
                lua.create_function(|lua, ()| {
                    let f: Function = lua.load("return function() return 5 end").eval()?;
                    f.try_call::<f64>(())
                })?,
            )
        });
        let calls_back = s.jobs.get::<Function>("calls_back")?;
        assert_eq!(done::<f64>(&s.lua, &s.lua.spawn(&calls_back, ())?), 5.0);
        let e = failed(&s.lua, &s.lua.spawn(&s.jobs.get::<Function>("host_panics")?, ())?);
        assert_eq!(e.message(), "a Rust panic: host function went wrong");
        for _ in 0..2 {
            assert_eq!(done::<f64>(&s.lua, &s.lua.spawn(&calls_back, ())?), 5.0, "a later job ({workers} workers)");
        }
    }
    Ok(())
}

#[test]
fn a_lua_kept_past_the_worker_stops_the_process() {
    // Regression: a clone of the worker's Lua outlived the
    // worker state. Only the unsafe WorkerInit::lua can make one now; if the
    // contract is broken the process aborts instead of using freed memory.
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_stash_lua")).output().expect("run stash_lua");
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success(), "it must not exit normally: {stdout}");
    assert!(stdout.contains("job done: true") && !stdout.contains("not reached"), "{stdout}");
    assert!(stderr.contains("a Lua of a worker's state outlived the state"), "{stderr}");
}

#[test]
fn a_value_of_a_worker_kept_past_the_worker_fails_safely() -> LuaResult<()> {
    // A value (not the Lua) that a host function lets escape: once the worker
    // state has closed, mlua reports the state destroyed instead of reading
    // freed memory.
    thread_local!(static KEPT: std::cell::RefCell<Option<Table>> = const { std::cell::RefCell::new(None) });
    {
        let s = setup(Some(0), "keepvalue")?;
        nl_host::set_worker_init(&s.lua, |w| {
            w.set_function("keep", |t: Table| {
                KEPT.with(|k| *k.borrow_mut() = Some(t));
                Ok(())
            })
        });
        let dir = nl_host::tmpdir("keepvalue_jobs");
        std::fs::write(dir.join("keeper.nl"), "pub fn run() :keep({a: 1}) end\n").expect("write");
        s.lua.add_path(&dir)?;
        let k = s.lua.import("keeper")?;
        let t = s.lua.spawn(&k.get::<Function>("run")?, ())?;
        s.lua.drain();
        assert!(matches!(t.state::<Value>()?, TaskState::Done(_)));
        KEPT.with(|k| assert!(k.borrow().is_some()));
    }
    let r = std::panic::catch_unwind(|| KEPT.with(|k| k.borrow().as_ref().map(|t| t.get::<i64>("a"))));
    match r {
        Ok(Some(Ok(v))) => panic!("read {v} from a closed state"),
        Ok(other) => eprintln!("the kept value failed: {other:?}"),
        Err(_) => eprintln!("the kept value panicked (mlua: the state is destroyed)"),
    }
    // Dropping it is safe too.
    KEPT.with(|k| drop(k.borrow_mut().take()));
    Ok(())
}

#[test]
fn a_worker_close_keeps_mlua_s_drop_order() -> LuaResult<()> {
    // The abort of NlState::drop (crates/nl/src/state.rs) relies on mlua
    // 0.12.2 dropping its own Lua of a worker's state before the app data
    // (crates/nl/Cargo.toml pins mlua for that). If the order changed, the
    // close below would abort the test process, as would every other test
    // here that ends a worker.
    let s = setup(Some(1), "droporder")?;
    assert_eq!(done::<f64>(&s.lua, &s.lua.spawn(&s.jobs.get::<Function>("square")?, 4)?), 16.0);
    drop(s);
    Ok(())
}

#[test]
fn the_main_state_s_paths_and_loader_are_fixed_once_workers_exist() -> LuaResult<()> {
    // nl_addpath, nl_adddecl and nl_setloader on the
    // main state do nothing once workers exist; the crate says so.
    struct NoFiles;
    impl Loader for NoFiles {
        fn resolve(&self, _: Option<&str>, _: &str) -> Option<String> {
            None
        }
        fn read(&self, _: &str) -> Option<std::borrow::Cow<'_, [u8]>> {
            None
        }
    }
    let s = setup(Some(1), "frozen")?;
    let dir = nl_host::tmpdir("frozen_extra");
    s.lua.add_path(&dir)?;
    // Off the search path, so that only add_decl names it.
    let decls = nl_host::tmpdir("frozen_decls");
    std::fs::write(decls.join("before.d.nl"), "declare fn host_before(): Number\n").expect("write");
    s.lua.add_decl(&decls.join("before.d.nl").display().to_string())?;
    assert_eq!(done::<f64>(&s.lua, &s.lua.spawn(&s.jobs.get::<Function>("square")?, 2)?), 4.0);
    for (what, r) in [
        ("add_path", s.lua.add_path(&dir)),
        ("add_decl", s.lua.add_decl("after.d.nl")),
        ("set_loader", s.lua.set_loader(NoFiles)),
    ] {
        let e = r.expect_err(what);
        assert!(e.to_string().contains("workers exist"), "{what}: {e}");
    }
    // Options were already fixed by the C side; that is unchanged.
    assert!(s.lua.set_option(NlOption::Checks(false)).is_err());
    Ok(())
}

#[test]
fn a_worker_has_no_debug_library() -> LuaResult<()> {
    let s = setup(Some(1), "debug")?;
    assert!(done::<bool>(&s.lua, &s.lua.spawn(&s.jobs.get::<Function>("debug_hidden")?, ())?));
    Ok(())
}

#[test]
fn a_memory_limit_near_what_the_libraries_need() -> LuaResult<()> {
    // Regression: a limit just above what a worker's libraries need
    // aborted the process inside mlua's attachment. Every limit in the band
    // either works or fails the job with WorkerError.
    //
    // It starts at 360 KiB: below that the VM's own accounting of a worker
    // whose libraries do not fit trips LuaJIT's assertion "close_state:
    // memory leak" now and then in builds with assertions (debug builds of
    // luajit-src), with no Rust code involved. A release
    // build can start lower: NL_HOST_MEM_FROM=300.
    let from: usize = std::env::var("NL_HOST_MEM_FROM").ok().and_then(|v| v.parse().ok()).unwrap_or(360);
    let mut kinds = std::collections::BTreeSet::new();
    for kib in (from..=600).step_by(4) {
        let s = setup(Some(1), &format!("mem{kib}"))?;
        s.lua.set_option(NlOption::WorkerMemory(kib * 1024))?;
        let t = s.lua.spawn(&s.jobs.get::<Function>("square")?, 3)?;
        s.lua.drain();
        match t.state::<f64>()? {
            TaskState::Done(v) => {
                assert_eq!(v, 9.0);
                kinds.insert("done");
            }
            TaskState::Failed(e) => {
                assert_eq!(e.type_name(), "WorkerError", "{kib} KiB: {e}");
                kinds.insert("WorkerError");
            }
            TaskState::Pending => panic!("{kib} KiB: pending after drain"),
        }
    }
    assert_eq!(kinds.len(), 2, "the band covers both outcomes: {kinds:?}");
    Ok(())
}

#[test]
fn the_init_closure_runs_without_the_memory_limit() -> LuaResult<()> {
    // The closure allocates past the worker's limit; what it leaves counts:
    // garbage is collected, data kept alive fails the worker with
    // WorkerError.
    for keep in [false, true] {
        let dir = nl_host::tmpdir(if keep { "initkeep" } else { "initgarbage" });
        std::fs::write(dir.join("jobs.nl"), JOBS).expect("write");
        let lua = nl::new()?;
        lua.set_unhandled(|_, _| {});
        lua.add_path(&dir)?;
        lua.set_workers(1);
        lua.set_option(NlOption::WorkerMemory(2 << 20))?;
        nl_host::set_worker_init(&lua, move |w| {
            let big: Vec<String> = (0..100_000).map(|i| format!("a string of some length, number {i}")).collect();
            w.set_global("big", big)?;
            if !keep {
                w.set_global("big", Value::Nil)?;
            }
            Ok(())
        });
        let jobs = lua.import("jobs")?;
        let t = lua.spawn(&jobs.get::<Function>("square")?, 3)?;
        if keep {
            let e = failed(&lua, &t);
            assert_eq!(e.type_name(), "WorkerError");
            assert!(e.message().contains("memory limit"), "{e}");
        } else {
            assert_eq!(done::<f64>(&lua, &t), 9.0);
        }
    }
    Ok(())
}

#[test]
fn register_transfer_needs_register_type() -> LuaResult<()> {
    #[derive(Default)]
    struct Other;
    impl UserData for Other {}
    impl Transfer for Other {
        fn encode(&self, _: &Lua) -> LuaResult<Value> {
            Ok(Value::Nil)
        }
        fn decode(_: &Lua, _: Value) -> LuaResult<Self> {
            Ok(Other)
        }
    }
    let lua = nl::new()?;
    let err = lua.register_transfer::<Other>().unwrap_err();
    assert!(err.to_string().contains("register_type first"), "{err}");
    lua.register_type::<Other>("Other")?;
    lua.register_transfer::<Other>()?;
    Ok(())
}

#[test]
fn into_future_of_a_spawned_job() -> LuaResult<()> {
    let s = setup(Some(2), "future")?;
    let rt = tokio::runtime::Builder::new_current_thread().enable_time().build().expect("runtime");
    let t = s.lua.spawn(&s.jobs.get::<Function>("square")?, 12)?;
    let v: f64 = rt.block_on(async {
        let fut = t.into_future::<f64>();
        let pump = async {
            loop {
                s.lua.pump();
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        };
        tokio::select! {
            biased;
            v = fut => v,
            _ = pump => unreachable!(),
        }
    })?;
    assert_eq!(v, 144.0);
    Ok(())
}

#[test]
fn the_wakeup_fires_for_a_worker_result() -> LuaResult<()> {
    let s = setup(Some(1), "wakeup")?;
    let woke = Arc::new(AtomicUsize::new(0));
    let w = woke.clone();
    s.lua.set_wakeup(move || {
        w.fetch_add(1, Ordering::SeqCst);
    });
    let t = s.lua.spawn(&s.jobs.get::<Function>("square")?, 3)?;
    let t0 = Instant::now();
    while woke.load(Ordering::SeqCst) == 0 {
        assert!(t0.elapsed() < Duration::from_secs(10), "no wakeup");
        std::thread::sleep(Duration::from_millis(1));
    }
    let mut n = 0;
    while matches!(t.state::<f64>()?, TaskState::Pending) {
        assert!(t0.elapsed() < Duration::from_secs(10), "no result");
        n += s.lua.pump();
    }
    assert!(n >= 1);
    assert!(matches!(t.state::<f64>()?, TaskState::Done(v) if v == 9.0));
    Ok(())
}

#[test]
fn the_worker_options_are_enforced() -> LuaResult<()> {
    let s = setup(Some(1), "options")?;
    s.lua.set_option(NlOption::MaxWorkers(1))?;
    let m: Table = s
        .lua
        .load_nl(
            "pub async fn two()\n  let a = Worker.new 'svc'\n  let b = Worker.new 'svc'\n  a.add(1, 2).await\nend\npub async fn one()\n  let a = Worker.new 'svc'\n  a.add(1, 2).await\nend\n",
            "=dedicated",
        )?
        .try_call(())?;
    let t: Task = m.get::<Function>("one")?.try_call(())?;
    assert_eq!(done::<f64>(&s.lua, &t), 3.0);
    let t: Task = m.get::<Function>("two")?.try_call(())?;
    let e = failed(&s.lua, &t);
    assert_eq!(e.type_name(), "WorkerError");

    let s = setup(Some(1), "memory")?;
    s.lua.set_option(NlOption::WorkerMemory(8 << 20))?;
    let jobs = s.jobs.get::<Function>("allocate")?;
    assert_eq!(done::<f64>(&s.lua, &s.lua.spawn(&jobs, 10)?), 10.0);
    let e = failed(&s.lua, &s.lua.spawn(&jobs, 400_000)?);
    assert_eq!(e.type_name(), "WorkerError", "{e}");
    // The pool worker was replaced.
    assert_eq!(done::<f64>(&s.lua, &s.lua.spawn(&jobs, 10)?), 10.0);
    Ok(())
}

#[test]
fn closing_with_workers_busy() -> LuaResult<()> {
    let s = setup(Some(2), "close")?;
    let f = s.jobs.get::<Function>("allocate")?;
    let remote = s.lua.remote();
    for _ in 0..8 {
        s.lua.spawn(&f, 20_000)?;
    }
    drop(f);
    drop(s);
    assert!(!remote.post(|_| Ok(())));
    Ok(())
}
