//! The text engine as a separate process:
//!
//! - mode 2: `plumcraft-text-engine --connect unix:<path>`, or with `PLUMCRAFT_ENGINE_SOCKET` and
//!   `PLUMCRAFT_ENGINE_TOKEN` set by the UI that spawns it;
//! - mode 3: `plumcraft-text-engine --listen 127.0.0.1:0 [--port-file <f>]`, HTTP, with the
//!   bearer token from `PLUMCRAFT_ENGINE_TOKEN` (required); `--lifeline` exits
//!   when stdin closes (a UI that spawns the engine passes it);
//! - `register` says it needs none: it is one of PlumCraft's own engines.

use std::process::ExitCode;

use plumcraft_engine::register::Registration;

fn main() -> ExitCode {
    plumcraft_engine::serve::main("plumcraft-text-engine", Box::new(|| Ok(plumcraft_text::engine())), &Registration::BuiltIn)
}
