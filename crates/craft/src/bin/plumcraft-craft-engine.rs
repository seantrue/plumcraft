//! The craft adapter as a separate process (plan D49: mode 2 by default):
//!
//! - mode 2: `plumcraft-craft-engine --craft vectorcraft --connect unix:<path>`, or with
//!   `PLUMCRAFT_ENGINE_SOCKET` and `PLUMCRAFT_ENGINE_TOKEN` set by the UI that spawns it;
//! - mode 3: `plumcraft-craft-engine --craft vectorcraft --listen 127.0.0.1:0 [--port-file <f>]`;
//! - the smoke test: `plumcraft-craft-engine --craft vectorcraft --smoke <image> [--only <id>]`
//!   runs every node with its defaults on the image (traced first for document nodes) and prints
//!   one line per node.
//!
//! `--craft` takes a built-in pack (`vectorcraft`, `soundcraft`) or a manifest path.

use std::process::ExitCode;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1)).cloned()
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let craft = arg(&args, "--craft").unwrap_or_else(|| "vectorcraft".into());
    let manifest = match plumcraft_craft::manifest::Manifest::named(&craft) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("plumcraft-craft-engine: {e}");
            return ExitCode::from(2);
        }
    };
    if let Some(image) = arg(&args, "--smoke") {
        let only = arg(&args, "--only");
        return match plumcraft_craft::smoke(&manifest, &plumcraft_craft::Options::default(), std::path::Path::new(&image), only.as_deref()) {
            Ok(rows) => {
                let failed = rows.iter().filter(|r| r.1.is_err()).count();
                for (id, r) in rows {
                    match r {
                        Ok(ms) => println!("ok    {id} {ms:.0} ms"),
                        Err(e) => println!("FAIL  {id} {e}"),
                    }
                }
                if failed == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE }
            }
            Err(e) => {
                eprintln!("plumcraft-craft-engine: {e}");
                ExitCode::FAILURE
            }
        };
    }
    let factory = Box::new(move || plumcraft_craft::engine(&manifest));
    let served = if let Some((listen, port_file)) = plumcraft_engine::http::listen_args(&args) {
        let token = std::env::var(plumcraft_engine::serve::ENV_TOKEN).unwrap_or_default();
        plumcraft_engine::http::lifeline(&args);
        plumcraft_engine::http::serve_http(factory, &listen, &token, port_file.as_deref())
    } else {
        match plumcraft_engine::serve::endpoint_from_env(&args) {
            Ok((socket, token)) => plumcraft_engine::serve::serve_unix(factory, &socket, &token),
            Err(e) => {
                eprintln!("plumcraft-craft-engine: {e}");
                return ExitCode::from(2);
            }
        }
    };
    match served {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("plumcraft-craft-engine: {e}");
            ExitCode::FAILURE
        }
    }
}
