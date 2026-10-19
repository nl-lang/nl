//! A use-after-free, as the crate now stops it:
//! the init closure keeps a clone of the worker's Lua (only possible through
//! the unsafe `WorkerInit::lua`, against its contract) past the worker. The
//! process aborts when the worker's state closes. tests/workers.rs runs it.

use std::cell::RefCell;

use nl::prelude::*;

thread_local!(static STASH: RefCell<Option<Lua>> = const { RefCell::new(None) });

fn main() -> LuaResult<()> {
    let dir = nl_host::tmpdir("stash");
    std::fs::write(dir.join("jobs.nl"), "pub fn sq(x) x * x end\n").expect("write");
    let lua = nl::new()?;
    lua.add_path(&dir)?;
    lua.set_workers(0); // the worker state lives on this thread
    nl_host::set_worker_init(&lua, |w| {
        // SAFETY: broken on purpose: the clone outlives the closure.
        let l = unsafe { w.lua() };
        STASH.with(|s| *s.borrow_mut() = Some(l.clone()));
        Ok(())
    });
    let jobs = lua.import("jobs")?;
    let t = lua.spawn(&jobs.get::<Function>("sq")?, 7)?;
    lua.drain();
    println!("job done: {}", matches!(t.state::<f64>()?, TaskState::Done(_)));
    drop((t, jobs));
    drop(lua); // closes the worker state: the process stops here
    println!("not reached: the stashed Lua is still alive");
    Ok(())
}
