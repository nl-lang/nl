//! Embeds the runtime library: the files `runtime/*.lua`, concatenated in
//! the order of their names (runtime/nl.lua, header), and the list of
//! those files with the number of lines each takes in the concatenation
//! (`runtime_files.rs`), which maps a line of the runtime chunk back to
//! its file (the sourcemap of `nlc bundle`).

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
    let mut manifest = String::from("/// The files of the runtime chunk, in order, with the lines each takes.\npub const RUNTIME_FILES: &[(&str, u32)] = &[\n");
    for f in &files {
        println!("cargo:rerun-if-changed={}", f.display());
        match std::fs::read_to_string(f) {
            Ok(text) => {
                let before = out.matches('\n').count();
                out.push_str(&text);
                if !text.ends_with('\n') {
                    out.push('\n');
                }
                let name = f.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                manifest.push_str(&format!("    (\"runtime/{name}\", {}),\n", out.matches('\n').count() - before));
            }
            Err(e) => println!("cargo:warning=cannot read {}: {e}", f.display()),
        }
    }
    manifest.push_str("];\n");
    let dir = std::env::var("OUT_DIR").unwrap_or_else(|_| ".".to_string());
    for (name, text) in [("runtime.lua", &out), ("runtime_files.rs", &manifest)] {
        let target = Path::new(&dir).join(name);
        if let Err(e) = std::fs::write(&target, text) {
            println!("cargo:warning=cannot write {}: {e}", target.display());
        }
    }
}
