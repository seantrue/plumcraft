//! Rewrite `crates/editor-core/schema/shell1.json`:
//! `cargo run -p plumcraft-editor-core --example export_shell_schema --features schema`.

fn main() -> std::io::Result<()> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/schema/shell1.json");
    let text = serde_json::to_string_pretty(&plumcraft_editor_core::shell_schema::shell1()).map_err(std::io::Error::other)? + "\n";
    std::fs::write(path, text)?;
    println!("wrote {path}");
    Ok(())
}
