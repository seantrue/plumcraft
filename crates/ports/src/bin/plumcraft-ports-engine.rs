//! The ports test engine as a separate process (mode 2, `--connect`; mode 3, `--listen`), as the
//! text engine's program is. A launched app's checks install it from a scratch engines folder.

use std::process::ExitCode;

use plumcraft_engine::register::Registration;

fn main() -> ExitCode {
    plumcraft_engine::serve::main("plumcraft-ports-engine", Box::new(|| Ok(plumcraft_ports::engine())), &Registration::BuiltIn)
}
