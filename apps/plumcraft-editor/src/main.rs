//! `plumcraft-editor`: PlumCraft's own application of the editor library.

// A release build on Windows is a windowed app, so opening the editor doesn't also open a console.
// Its output then goes nowhere: `plumcraft-editor run FILE` prints nothing there, and the
// `plumcraft` command line is the way to run a workflow headless. Debug builds keep the console.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

fn main() -> std::process::ExitCode {
    plumcraft_editor::run(plumcraft_editor::AppConfig::default())
}
