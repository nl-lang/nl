//! The Rust twin of tests/vm/nl_host/workers/host.c: a host type that crosses between states with its transfer pair,
//! a worker-init closure, `spawn` and `drain`. run.sh runs both with 0, 1
//! and 4 pool workers on workers/scripts and compares what they print.
//!
//!   workers N        (in the directory that holds scripts/)

use nl::prelude::*;

#[derive(Default, Clone, Copy)]
struct Vec2 {
    x: f64,
    y: f64,
}

impl UserData for Vec2 {}

impl Transfer for Vec2 {
    fn encode(&self, lua: &Lua) -> LuaResult<Value> {
        Ok(Value::Table(lua.create_sequence_from([self.x, self.y])?))
    }
    fn decode(_: &Lua, value: Value) -> LuaResult<Self> {
        let t = value.as_table().ok_or_else(|| LuaError::runtime("bad Vec2"))?;
        Ok(Vec2 { x: t.get(1)?, y: t.get(2)? })
    }
}

/// The main state.
fn register_host(lua: &Lua) -> LuaResult<()> {
    lua.register_type::<Vec2>("Vec2")?;
    lua.register_transfer::<Vec2>()?;
    let g = lua.globals();
    g.set("vec2_scale", lua.create_function(|_, (v, k): (AnyUserData, f64)| scale(&v, k))?)?;
    g.set("vec2_str", lua.create_function(|_, v: AnyUserData| vec2_str(&v))?)?;
    g.set("host_isworker", lua.create_function(|lua, ()| Ok(lua.is_worker()))?)?;
    Ok(())
}

/// Each worker, through the WorkerInit the init closure gets.
fn register_worker(w: &WorkerInit) -> LuaResult<()> {
    w.register_type::<Vec2>("Vec2")?;
    w.register_transfer::<Vec2>()?;
    w.set_function("vec2_scale", |(v, k): (AnyUserData, f64)| scale(&v, k))?;
    w.set_function("vec2_str", |v: AnyUserData| vec2_str(&v))?;
    w.set_function("host_isworker", |()| Ok(true))?;
    Ok(())
}

fn scale(v: &AnyUserData, k: f64) -> LuaResult<Vec2> {
    let v = *v.borrow::<Vec2>()?;
    Ok(Vec2 { x: v.x * k, y: v.y * k })
}

fn vec2_str(v: &AnyUserData) -> LuaResult<String> {
    let v = v.borrow::<Vec2>()?;
    Ok(format!("{},{}", v.x, v.y))
}

fn report(err: &LuaError) {
    match err.exception() {
        Some(e) => eprintln!("{e}"),
        None => eprintln!("{err}"),
    }
}

fn main() -> LuaResult<()> {
    let workers: usize = std::env::args().nth(1).and_then(|a| a.parse().ok()).unwrap_or(1);
    let lua = nl::new()?;
    register_host(&lua)?;
    lua.set_unhandled(|_, e| eprintln!("[host] unhandled: {e}"));
    lua.add_path("scripts")?;
    lua.set_workers(workers);
    nl_host::set_worker_init(&lua, register_worker); // in each new worker, on its thread
    println!("[host] the main state is a worker: {}", if lua.is_worker() { "yes" } else { "no" });

    let exports: Table = match lua.load_nl_file("scripts/main.nl").and_then(|f| f.try_call(())) {
        Ok(t) => t,
        Err(err) => {
            report(&err);
            std::process::exit(1);
        }
    };
    let task: Task = exports.get::<Function>("run")?.try_call(Vec2 { x: 1.0, y: 2.0 })?;
    while matches!(task.state::<Value>()?, TaskState::Pending) {
        lua.drain();
    }
    match task.state::<Table>()? {
        TaskState::Done(out) => {
            for line in out.sequence_values::<String>() {
                println!("[script] {}", line?);
            }
        }
        TaskState::Failed(e) => eprintln!("{e}"),
        TaskState::Pending => unreachable!(),
    }

    let triple: Function = exports.get("triple")?; // spawn from the host
    let t = lua.spawn(&triple, 12)?;
    lua.drain();
    if let TaskState::Done(v) = t.state::<f64>()? {
        println!("[host] spawned triple(12): {v}");
    }
    Ok(())
}
