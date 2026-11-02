//! Bakes this engine's code digest into its memo salt (`plumcraft_engine::salt`).

include!("../engine/digest.rs");

fn main() {
    let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    println!("cargo:rustc-env=PLUMCRAFT_CODE_DIGEST={}", source_digest(&[&here.join("src")], &here.join("../../Cargo.lock")));
}
