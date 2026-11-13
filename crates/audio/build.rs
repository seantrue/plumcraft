//! Bakes this engine's code digest into its memo salt (`plumcraft_engine::salt`).

include!("digest.rs");

fn main() {
    let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    emit_digest("PLUMCRAFT_CODE_DIGEST", &source_digest(&[&here.join("src")], &here.join("../../Cargo.lock")), "");
}
