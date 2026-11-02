//! `plumcraft-editor`: PlumCraft's own application of the editor library.

fn main() -> std::process::ExitCode {
    plumcraft_editor::run(plumcraft_editor::AppConfig::default())
}
