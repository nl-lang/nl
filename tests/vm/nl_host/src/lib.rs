//! Helpers shared by the tests of the nl crate (tests/*.rs) and the
//! examples (src/bin).

use std::path::PathBuf;

/// tests/vm/nl_host/ in the repository.
pub fn here() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// A fresh directory under target/nl_host/tmp.
pub fn tmpdir(name: &str) -> PathBuf {
    let d = here().join("../../../target/nl_host/tmp").join(format!("{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).expect("tmp dir");
    d
}

/// A minimal executor for `set_spawner`: each future runs on a thread of
/// its own until it completes. Enough for the examples, which must not
/// depend on tokio outside the tests.
pub fn spawn_on_thread(future: nl::BoxFuture) {
    std::thread::spawn(move || block_on(future));
}

/// Polls `future` on this thread until it completes, parking in between.
pub fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
    use std::sync::Arc;
    use std::task::{Context, Poll, Wake, Waker};
    struct Unpark(std::thread::Thread);
    impl Wake for Unpark {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = Waker::from(Arc::new(Unpark(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        if let Poll::Ready(v) = future.as_mut().poll(&mut cx) {
            return v;
        }
        std::thread::park();
    }
}

/// `lua.set_worker_init(f)`, which is an `unsafe fn` with mlua's `send`
/// feature: the tests and examples keep its contract. Their
/// init closures capture no value of the main state (a `Remote` is not
/// one), and their host functions keep no value of a worker's state past
/// the call, nor send one to another thread, except where a test does it
/// on purpose without `send` (where the values are not `Send`).
pub fn set_worker_init(lua: &nl::mlua::Lua, f: impl Fn(&nl::WorkerInit) -> nl::mlua::Result<()> + Send + Sync + 'static) {
    use nl::NlExt;
    #[cfg(not(feature = "send"))]
    lua.set_worker_init(f);
    // SAFETY: as said above.
    #[cfg(feature = "send")]
    unsafe {
        lua.set_worker_init(f)
    };
}
