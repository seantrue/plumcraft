//! Rewrite `crates/proto/schema/protocol2.json`:
//! `cargo run -p plumcraft-proto --example export_schema --features schema`.

fn main() -> std::io::Result<()> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/schema/protocol2.json");
    let text = serde_json::to_string_pretty(&plumcraft_proto::schema::protocol2()).map_err(std::io::Error::other)? + "\n";
    std::fs::write(path, text)?;
    println!("wrote {path}");
    Ok(())
}
