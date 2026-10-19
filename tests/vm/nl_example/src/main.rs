//! An example host on the nl crate, as a fresh crate with only the `nl`
//! dependency and the patch of luajit-src.
//! `fetch`, `game_running` and `frame_time` are deterministic stand-ins,
//! and the spawner runs each future on a thread of its own, so that the
//! crate needs no executor crate.

use nl::prelude::*; // mlua::prelude, NlExt and the types above

#[derive(Default)]
struct Unit {
    hp: f64,
}

impl UserData for Unit {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("hp", |_, unit| Ok(unit.hp));
    }
    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method_mut("damage", |_, unit, amount: f64| {
            if amount < 0.0 {
                return Err(Exception::new("TypeError", "amount must not be negative").into());
            }
            unit.hp -= amount;
            Ok(())
        });
    }
}

async fn fetch(url: &str) -> LuaResult<String> {
    Ok(format!("<body of {url}>"))
}

fn game_running(frame: &mut u32) -> bool {
    *frame += 1;
    std::thread::sleep(std::time::Duration::from_millis(5));
    *frame <= 20
}

fn frame_time() -> f64 {
    0.1
}

/// Runs a future to completion on a thread of its own.
fn spawn(future: nl::BoxFuture) {
    use std::sync::Arc;
    use std::task::{Context, Poll, Wake, Waker};
    struct Unpark(std::thread::Thread);
    impl Wake for Unpark {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    std::thread::spawn(move || {
        let waker = Waker::from(Arc::new(Unpark(std::thread::current())));
        let mut cx = Context::from_waker(&waker);
        let mut future = future;
        while future.as_mut().poll(&mut cx) == Poll::Pending {
            std::thread::park();
        }
    });
}

fn main() -> LuaResult<()> {
    let lua = nl::new()?;
    lua.register_type::<Unit>("Unit")?; // `unit.damage 10`, `u is Unit`
    lua.set_spawner(|future| {
        spawn(future);
    });
    lua.globals().set(
        "http_get",
        lua.create_task_function(|url: String| async move {
            Ok(fetch(&url).await?) // NL: http_get(url).await
        })?,
    )?;

    lua.load_nl_file("main.nl")?.try_call::<()>(())?;
    let mut frame = 0;
    while game_running(&mut frame) {
        if let Err(err) = lua.run_event::<()>("Game.Tick", frame_time()) {
            match err.exception() {
                Some(e) => eprintln!("{}: {}\n{}", e.type_name(), e.message(), e.trace().unwrap_or("")),
                None => eprintln!("{err}"),
            }
        }
        lua.pump();
    }
    Ok(())
}
