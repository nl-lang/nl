//! `nl-lsp`: the NL language server over stdio.
//! Point an editor's LSP client at this binary for `.nl` files; it logs
//! to stderr and speaks the protocol on stdin and stdout.

use std::process::ExitCode;

use lsp_server::Connection;

fn main() -> ExitCode {
    let (connection, io_threads) = Connection::stdio();
    // The compiler needs a deep stack (lib docs); the protocol threads of
    // `lsp-server` keep their own.
    let server = std::thread::Builder::new().name("nl-lsp".to_string()).stack_size(nl_lsp::STACK_SIZE).spawn(move || nl_lsp::run(connection));
    let result = match server {
        Ok(handle) => handle.join().unwrap_or_else(|_| Err("the server thread panicked".into())),
        Err(e) => Err(format!("cannot start the server thread: {e}").into()),
    };
    let io = io_threads.join();
    let mut code = ExitCode::SUCCESS;
    if let Err(e) = result {
        eprintln!("nl-lsp: {e}");
        code = ExitCode::FAILURE;
    }
    if let Err(e) = io {
        eprintln!("nl-lsp: {e}");
        code = ExitCode::FAILURE;
    }
    code
}
