//! A posted closure, never pumped, whose capture panics
//! when dropped. lua_close frees it from inside the VM, where a panic may not
//! unwind: the process aborts. tests/threads.rs runs it.

use nl::prelude::*;

struct Bomb;

impl Drop for Bomb {
    fn drop(&mut self) {
        panic!("drop of a posted closure's capture")
    }
}

fn main() -> LuaResult<()> {
    let lua = nl::new()?;
    let b = Bomb;
    println!("posted: {}", lua.remote().post(move |_| {
        let _keep = &b;
        Ok(())
    }));
    drop(lua); // the process stops here
    println!("not reached");
    Ok(())
}
