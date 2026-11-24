// PlumCraft's own stamp (B151): the commit of the source this library is built from, whether it
// had changes, and when (src/capture.rs says what is watched and what is not).

#[path = "src/capture.rs"]
#[allow(dead_code)]
mod capture;

fn main() {
    let dir = std::env::var_os("CARGO_MANIFEST_DIR").map(std::path::PathBuf::from).unwrap_or_default();
    capture::emit(&dir, &["crates", "Cargo.toml", "Cargo.lock"]);
}
