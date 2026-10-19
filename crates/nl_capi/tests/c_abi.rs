//! include/nlbundle.h from C and C++: tests/c/nlbundle_test.c is compiled
//! both ways against the static library this crate builds
//! (`libnl_capi.a`, next to the test binary's `deps/`), linked and run.
//! It sets every option to a value with an effect it checks, so a header
//! that does not match the `#[repr(C)]` types of src/capi_bundle.rs fails.
//! Skipped (with a note) when no C compiler or no static library is found.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn staticlib() -> Option<PathBuf> {
    // target/<profile>/deps/c_abi-… → target/<profile>/libnl_capi.a
    let exe = std::env::current_exe().ok()?;
    let lib = exe.parent()?.parent()?.join("libnl_capi.a");
    lib.is_file().then_some(lib)
}

fn compile_and_run(compiler: &str, args: &[&str], lib: &Path) {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("nlbundle_c_abi");
    std::fs::create_dir_all(&dir).unwrap();
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let exe = dir.join(format!("nlbundle_test_{compiler}"));
    let mut cmd = Command::new(compiler);
    // `-x` (in `args`) names the language of the source only: `-x none`
    // before the library, or the compiler reads the archive as source.
    cmd.args(args).arg("-Wall").arg("-Wextra").arg("-Werror").arg("-I").arg(manifest.join("include")).arg(manifest.join("tests/c/nlbundle_test.c")).arg("-x").arg("none").arg(lib).arg("-o").arg(&exe);
    for l in ["-lpthread", "-ldl", "-lm"] {
        cmd.arg(l);
    }
    let out = match cmd.output() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("skipped {compiler}: {e}");
            return;
        }
    };
    assert!(out.status.success(), "{compiler} failed:\n{}", String::from_utf8_lossy(&out.stderr));
    let run = Command::new(&exe).output().unwrap();
    let stdout = String::from_utf8_lossy(&run.stdout);
    assert!(run.status.success() && stdout.contains("nlbundle_test: ok"), "{compiler}: {stdout}\n{}", String::from_utf8_lossy(&run.stderr));
}

#[test]
fn nlbundle_h_from_c_and_cpp() {
    let Some(lib) = staticlib() else {
        eprintln!("skipped: no libnl_capi.a next to the test binary");
        return;
    };
    if cfg!(not(unix)) {
        eprintln!("skipped: the test links with Unix system libraries");
        return;
    }
    compile_and_run("cc", &["-std=c99", "-x", "c"], &lib);
    compile_and_run("c++", &["-std=c++11", "-x", "c++"], &lib);
}
