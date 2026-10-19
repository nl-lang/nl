//! Prints the tokens and diagnostics of a file: `cargo run --example lex_dump -- file.nl`.
//! Trivia is shown in brackets; `^` marks `nl_before`, `_` marks `sp_before`.

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: lex_dump <file.nl>");
        std::process::exit(2);
    };
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{path}: {e}");
            std::process::exit(1);
        }
    };
    let source = match nl_syntax::source_from_bytes(&bytes) {
        Ok(s) => s,
        Err(d) => {
            eprintln!("{path}: {}", d.message);
            std::process::exit(1);
        }
    };
    let lexed = nl_syntax::lex(source);
    for t in &lexed.tokens {
        let flags = format!("{}{}", if t.nl_before { "^" } else { " " }, if t.sp_before { "_" } else { " " });
        if t.kind.is_trivia() {
            println!("   [{:?} {:?}]", t.kind, t.text(source));
        } else {
            println!("{flags} {:?} {:?}", t.kind, t.text(source));
        }
    }
    for d in &lexed.diagnostics {
        println!("{:?} {:?}: {}", d.severity, d.range, d.message);
    }
}
