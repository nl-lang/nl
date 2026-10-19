//! The Rust twin of the complete C embedding example,
//! tests/vm/nl_host/embedding/host.c: the program of tests/vm/nl_example
//! plus `add_path`, `on_event`, a game-time timer and the shutdown loop. Run
//! in a directory that holds the example's `scripts/`; tests/vm/nl_host/run.sh
//! compares its output with the C host's. `fetch` and `game_running` are
//! replaced by something deterministic.

#![cfg_attr(feature = "send", allow(unused))]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use nl::prelude::*;

#[derive(Default)]
struct Unit {
    name: String,
    hp: f64,
}

impl UserData for Unit {
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        // NL: u.damage 5. A function, not a method of mlua's, so that the
        // borrow of the unit ends before UnitDied runs: its handlers read
        // the unit again.
        methods.add_function("damage", |lua, (ud, amount): (AnyUserData, f64)| {
            if amount < 0.0 {
                return Err(Exception::new("TypeError", format!("amount must not be negative, got {amount}")).into());
            }
            let died = {
                let mut unit = ud.borrow_mut::<Unit>()?;
                if unit.hp <= 0.0 {
                    return Ok(());
                }
                unit.hp = (unit.hp - amount).max(0.0);
                unit.hp == 0.0
            };
            if died {
                lua.run_event::<()>("UnitDied", ud)?; // on UnitDied ...
            }
            Ok(())
        });
        methods.add_method("hp", |_, unit, ()| Ok(unit.hp)); // NL: u.hp, auto-called
        // "#{u}": Rust prints 5.0 as 5, as Lua's %.14g does in the C example.
        methods.add_meta_method(LuaMetaMethod::ToString, |_, u, ()| Ok(format!("{}, {} hp", u.name, u.hp)));
    }
}

fn report(err: &LuaError) {
    match err.exception() {
        Some(e) => {
            eprintln!("{e}");
            if let Some(trace) = e.trace() {
                eprintln!("{trace}");
            }
        }
        None => eprintln!("{err}"),
    }
}

async fn fetch(url: &str) -> LuaResult<String> {
    Ok(format!("<body of {url}>"))
}

fn run_timers(timers: &RefCell<Vec<(f64, Wake)>>, now: f64) {
    let due: Vec<Wake> = {
        let mut list = timers.borrow_mut();
        let (due, rest): (Vec<_>, Vec<_>) = list.drain(..).partition(|(at, _)| *at <= now);
        *list = rest;
        due.into_iter().map(|(_, wake)| wake).collect()
    }; // the borrow ends here
    for wake in due {
        wake.wake(); // may start a new sleep, which borrows again
    }
}

/// The twin is a program without mlua's `send` feature (its timer state is
/// in an `Rc`); run.sh builds it so.
#[cfg(feature = "send")]
fn main() {
    eprintln!("the embedding twin is built without mlua's send feature");
    std::process::exit(2);
}

#[cfg(not(feature = "send"))]
fn main() -> LuaResult<()> {
    let lua = nl::new()?;
    lua.set_reporter(|d| {
        let sev = match d.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Lint => "lint",
        };
        eprintln!("{}:{}:{}: {}: {} [{}]", d.file, d.line, d.column, sev, d.message, d.code);
    });
    lua.add_path("scripts")?; // imports, and scripts/game.d.nl
    lua.register_type::<Unit>("Unit")?; // `unit.damage 10`, `u is Unit`

    let engine = lua.create_table()?; // plain functions: not marked
    engine.set(
        "new_unit",
        lua.create_function(|_, (name, hp): (String, f64)| Ok(Unit { name, hp }))?,
    )?;
    engine.set(
        "log",
        lua.create_function(|_, text: String| {
            println!("[script] {text}");
            Ok(())
        })?,
    )?;
    lua.globals().set("engine", engine)?;

    lua.on_event("UnitDied", "host:log", |_, unit: AnyUserData| {
        println!("[host] {} died", unit.to_string()?);
        Ok(()) // nothing returned: the script's handler runs too
    })?;
    lua.set_unhandled(|_, e| eprintln!("[host] unhandled: {e}"));

    let timers: Rc<RefCell<Vec<(f64, Wake)>>> = Rc::default();
    let clock = Rc::new(Cell::new(0.0_f64));
    {
        let (timers, clock) = (timers.clone(), clock.clone());
        lua.set_timer(move |delay, wake| {
            timers.borrow_mut().push((clock.get() + delay.as_secs_f64(), wake));
        });
    }

    lua.set_spawner(nl_host::spawn_on_thread);
    lua.globals().set(
        "http_get",
        lua.create_task_function(|url: String| async move {
            fetch(&url).await // NL: http_get(url).await
        })?,
    )?;

    let exports: Table = match lua.load_nl_file("scripts/main.nl").and_then(|f| f.try_call(())) {
        Ok(t) => t,
        Err(err) => {
            report(&err);
            std::process::exit(1);
        }
    };
    let start: Function = exports.get("start")?;
    if let Err(err) = start.try_call::<()>(3) {
        report(&err);
    }

    let mut frame = 0;
    let game_running = |frame: &mut i32| {
        *frame += 1;
        *frame <= 50 // the game: 5 s at 10 frames a second
    };
    while game_running(&mut frame) {
        clock.set(clock.get() + 0.1);
        if let Err(err) = lua.run_event::<()>("Game.Tick", 0.1) {
            report(&err);
        }
        run_timers(&timers, clock.get());
        lua.pump();
    }

    loop {
        // shutdown: finish what is pending
        clock.set(clock.get() + 0.1);
        run_timers(&timers, clock.get());
        if lua.pump() == 0 && timers.borrow().is_empty() {
            break;
        }
    }
    Ok(())
}
