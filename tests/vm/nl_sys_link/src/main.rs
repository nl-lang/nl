//! nl_sys and mlua linked against the VM fork. Exits non-zero
//! on the first failed check. The runtime is the fake one of tests/vm
//! (.cargo/config.toml), so that the result does not depend on the real
//! runtime; the compiler is the real nl_capi.

use std::ffi::{CStr, CString, c_int, c_void};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use mlua_sys as ffi;
use nl_sys::*;

fn check(ok: bool, what: &str) {
    println!("{} - {what}", if ok { "ok" } else { "not ok" });
    if !ok {
        std::process::exit(1);
    }
}

unsafe extern "C-unwind" fn handler(l: *mut lua_State) -> c_int {
    // SAFETY: called by the VM as a lua_CFunction with a valid state.
    unsafe {
        let x = ffi::lua_tonumber(l, 1);
        ffi::lua_pushnumber(l, x * 2.0);
    }
    1
}

static POSTED: AtomicUsize = AtomicUsize::new(0);

unsafe extern "C-unwind" fn posted(l: *mut lua_State, data: *mut c_void) {
    if !l.is_null() {
        POSTED.fetch_add(data as usize, Ordering::SeqCst);
    }
}

#[derive(Clone, Copy)]
struct State(*mut lua_State);
// SAFETY: only nl_post, which may be called from any thread, uses the state
// on the other threads.
unsafe impl Send for State {}

fn raw_state() {
    // SAFETY: the C API used as documented; the state stays on this thread
    // except for nl_post.
    unsafe {
        nl_version_0_1();
        let l = ffi::luaL_newstate();
        ffi::luaL_openlibs(l);
        let name = CString::new("event").unwrap_or_default();
        ffi::lua_getfield(l, ffi::LUA_GLOBALSINDEX, name.as_ptr());
        check(ffi::lua_istable(l, -1) != 0, "luaL_openlibs of the fork opens the event library");
        ffi::lua_settop(l, 0);

        // Events with a Rust handler.
        let ev = CString::new("Tick").unwrap_or_default();
        let h = CString::new("rust").unwrap_or_default();
        ffi::lua_pushcfunction(l, handler);
        nl_onevent(l, ev.as_ptr(), h.as_ptr());
        ffi::lua_pushnumber(l, 21.0);
        check(nl_prunevent(l, ev.as_ptr(), 1, 1) == 0, "nl_prunevent runs");
        check(ffi::lua_tonumber(l, -1) == 42.0, "the handler's value comes back");
        ffi::lua_settop(l, 0);

        // The real compiler (nl_capi) answers through the VM: a chunk, or an
        // error message naming the chunk.
        let src = "let x = 1\n";
        let chunk = CString::new("=link_test").unwrap_or_default();
        let st = nl_loadbuffer(l, src.as_ptr().cast(), src.len(), chunk.as_ptr());
        let msg = if st != 0 {
            CStr::from_ptr(ffi::lua_tostring(l, -1)).to_string_lossy().into_owned()
        } else {
            String::new()
        };
        println!("# nl_loadbuffer: status {st} {msg}");
        check(st == 0 || msg.contains("link_test"), "nl_compile from nl_capi is linked and called");
        ffi::lua_settop(l, 0);

        // nl_post from other threads.
        let s = State(l);
        let threads: Vec<_> = (0..4)
            .map(|_| {
                thread::spawn(move || {
                    let s = s;
                    for _ in 0..1000 {
                        nl_post(s.0, posted, 1 as *mut c_void);
                    }
                })
            })
            .collect();
        for t in threads {
            let _ = t.join();
        }
        let n = nl_pump(l);
        check(n == 4000 && POSTED.load(Ordering::SeqCst) == 4000, "nl_post from Rust threads, nl_pump");
        ffi::lua_close(l);
    }
}

fn with_mlua() {
    // mlua opens the libraries one by one and never calls luaL_openlibs, so
    // NL is opened explicitly.
    let lua = mlua::Lua::new();
    let before: mlua::Value = lua.globals().get("event").unwrap_or(mlua::Value::Nil);
    check(before.is_nil(), "mlua::Lua::new does not open NL (no luaL_openlibs)");
    // SAFETY: luaopen_nl may raise; exec_raw runs it in a protected call.
    let opened: mlua::Result<()> = unsafe {
        lua.exec_raw((), |l| {
            luaopen_nl(l);
        })
    };
    check(opened.is_ok(), "luaopen_nl through Lua::exec_raw");
    let ran: mlua::Result<f64> = lua
        .load("event.on_run('E', 'h', function(x) return x + 1 end); return event.run('E', 41)")
        .eval();
    check(matches!(ran, Ok(v) if v == 42.0), "the event library from mlua");
    let typed: mlua::Result<String> = lua.load("return typeof(1.5)").eval();
    check(matches!(typed.as_deref(), Ok("Number")), "the runtime's prelude from mlua");
}

fn main() {
    nl_sys::link();
    raw_state();
    with_mlua();
    println!("# nl_sys_link: passed");
}
