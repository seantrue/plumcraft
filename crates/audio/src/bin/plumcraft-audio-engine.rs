//! The audio engine as a separate process:
//!
//! - mode 2: `plumcraft-audio-engine --connect unix:<path>`, or with `PLUMCRAFT_ENGINE_SOCKET` and
//!   `PLUMCRAFT_ENGINE_TOKEN` set by the UI that spawns it;
//! - mode 3: `plumcraft-audio-engine --listen 127.0.0.1:0 [--port-file <f>]`, HTTP, with the
//!   bearer token from `PLUMCRAFT_ENGINE_TOKEN` (required); `--lifeline` exits
//!   when stdin closes (a UI that spawns the engine passes it).

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let factory = Box::new(|| Ok(plumcraft_audio::engine()));
    let served = if let Some((listen, port_file)) = plumcraft_engine::http::listen_args(&args) {
        let token = std::env::var(plumcraft_engine::serve::ENV_TOKEN).unwrap_or_default();
        plumcraft_engine::http::lifeline(&args);
        plumcraft_engine::http::serve_http(factory, &listen, &token, port_file.as_deref())
    } else {
        match plumcraft_engine::serve::endpoint_from_env(&args) {
            Ok((socket, token)) => plumcraft_engine::serve::serve_unix(factory, &socket, &token),
            Err(e) => {
                eprintln!("plumcraft-audio-engine: {e}");
                return ExitCode::from(2);
            }
        }
    };
    match served {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("plumcraft-audio-engine: {e}");
            ExitCode::FAILURE
        }
    }
}
