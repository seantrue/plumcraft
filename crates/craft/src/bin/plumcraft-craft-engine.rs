//! The craft adapter as a separate process (plan D49: mode 2 by default):
//!
//! - mode 2: `plumcraft-craft-engine --craft vectorcraft --connect unix:<path>`, or with
//!   `PLUMCRAFT_ENGINE_SOCKET` and `PLUMCRAFT_ENGINE_TOKEN` set by the UI that spawns it;
//! - mode 3: `plumcraft-craft-engine --craft vectorcraft --listen 127.0.0.1:0 [--port-file <f>]`;
//! - the smoke test: `plumcraft-craft-engine --craft vectorcraft --smoke <image> [--only <id>]`
//!   runs every node with its defaults on the image (traced first for document nodes) and prints
//!   one line per node.
//!
//! `--craft` takes a built-in pack (`vectorcraft`, `soundcraft`) or a manifest path. For a path,
//! `plumcraft-craft-engine --craft PACK.toml register` offers that pack in every editor (a
//! manifest starting this executable with `--craft` and the pack's absolute path); `unregister`
//! withdraws it. A built-in pack needs neither.

use std::process::ExitCode;

use plumcraft_engine::register::{Own, Registration};

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
    // a pack from a file is an engine PlumCraft doesn't know: it can be registered
    let registration = if plumcraft_craft::manifest::BUILTIN.contains(&craft.as_str()) {
        Registration::BuiltIn
    } else {
        let path = std::fs::canonicalize(&craft).map_or(craft.clone(), |p| p.to_string_lossy().into_owned());
        Registration::Own(Own { id: manifest.id.clone(), name: manifest.name.clone(), args: vec!["--craft".into(), path], demo: None })
    };
    let factory = Box::new(move || plumcraft_craft::engine(&manifest));
    plumcraft_engine::serve::main("plumcraft-craft-engine", factory, &registration)
}
