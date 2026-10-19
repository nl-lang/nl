//! The portable profile on the stock LuaJIT builds of Garry's Mod
//! (tests/gmod/run.sh, which says what it runs), when they are built for
//! this machine: `target/stock-luajit/build-<version>-native`, which
//! `tests/gmod/run.sh` builds on x86 Linux. Otherwise the test says so
//! and passes; the script itself builds the VMs, in Docker where the
//! machine cannot run them.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::path::Path;
use std::process::Command;

#[test]
fn portable_bundles_on_stock_luajit() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let built: Vec<&str> = ["2.0.4", "2.1.0-beta3"]
        .into_iter()
        .filter(|v| root.join(format!("target/stock-luajit/build-{v}-native/src/luajit")).is_file())
        .collect();
    if built.is_empty() {
        eprintln!("skipped: no stock LuaJIT built for this machine (run tests/gmod/run.sh)");
        return;
    }
    let out = Command::new("sh")
        .arg(root.join("tests/gmod/run.sh"))
        .args(["--no-glua", "--no-build"])
        .env("NL_GMOD_DOCKER", "never")
        .output()
        .expect("tests/gmod/run.sh runs");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(out.status.success(), "tests/gmod/run.sh failed ({built:?}):\n{text}\n{}", String::from_utf8_lossy(&out.stderr));
}
