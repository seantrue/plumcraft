//! Bakes the SDK's code digest into `CODE_DIGEST` (the executor, the protocol and the store all
//! shape results), for every engine's memo salt.

include!("digest.rs");

fn main() {
    let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let crates = here.join("..");
    let dirs = [here.join("src"), crates.join("proto/src"), crates.join("store/src")];
    let dirs: Vec<&std::path::Path> = dirs.iter().map(std::path::PathBuf::as_path).collect();
    // A published crate has no siblings or lockfile here. Its internal dependencies are pinned
    // exactly, so its version stands for them.
    let digest = source_digest(&dirs, &crates.join("../Cargo.lock"));
    println!("cargo:rustc-env=PLUMCRAFT_SDK_DIGEST={digest}-{}", env!("CARGO_PKG_VERSION"));
}
