//! Prints the syntax tree and diagnostics of a file or a snippet:
//! `cargo run --example parse_dump -- [--no-trivia] (file.nl | -e 'code')`.

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let skip_trivia = args.first().is_some_and(|a| a == "--no-trivia");
    if skip_trivia {
        args.remove(0);
    }
    let source = match args.as_slice() {
        [flag, code] if flag == "-e" => code.clone(),
        [path] => match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{path}: {e}");
                std::process::exit(1);
            }
        },
        _ => {
            eprintln!("usage: parse_dump [--no-trivia] (file.nl | -e 'code')");
            std::process::exit(2);
        }
    };
    let parse = nl_syntax::parse(&source);
    let options = nl_syntax::DumpOptions { skip_trivia };
    print!("{}", nl_syntax::dump_tree(&parse.syntax(), options));
    if !parse.diagnostics().is_empty() {
        println!("---");
        print!("{}", nl_syntax::dump_diagnostics(&source, parse.diagnostics()));
    }
}
