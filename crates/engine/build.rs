//! Bakes the SDK's code digest into `CODE_DIGEST` (the executor, the protocol and the store all
//! shape results), for every engine's memo salt.

include!("digest.rs");

fn main() {
    let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let crates = here.join("..");
    let dirs = [here.join("src"), crates.join("proto/src"), crates.join("store/src")];
    let dirs: Vec<&std::path::Path> = dirs.iter().map(std::path::PathBuf::as_path).collect();
    println!("cargo:rustc-env=PLUMCRAFT_SDK_DIGEST={}", source_digest(&dirs, &crates.join("../Cargo.lock")));
}
