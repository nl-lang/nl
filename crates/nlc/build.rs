//! Embeds the runtime library: the files `runtime/*.lua`, concatenated in
//! the order of their names (runtime/nl.lua, header).

use std::path::Path;

fn main() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../runtime");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut files: Vec<_> = match std::fs::read_dir(&dir) {
        Ok(rd) => rd.filter_map(Result::ok).map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "lua")).collect(),
        Err(e) => {
            println!("cargo:warning=cannot read {}: {e}", dir.display());
            Vec::new()
        }
    };
    files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
    let mut out = String::new();
    for f in &files {
        println!("cargo:rerun-if-changed={}", f.display());
        match std::fs::read_to_string(f) {
            Ok(text) => {
                out.push_str(&text);
                if !text.ends_with('\n') {
                    out.push('\n');
                }
            }
            Err(e) => println!("cargo:warning=cannot read {}: {e}", f.display()),
        }
    }
    let target = Path::new(&std::env::var("OUT_DIR").unwrap_or_else(|_| ".".to_string())).join("runtime.lua");
    if let Err(e) = std::fs::write(&target, out) {
        println!("cargo:warning=cannot write {}: {e}", target.display());
    }
}
