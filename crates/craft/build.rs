//! Builds the table of built-in packs from `packs/`: every `<id>.toml` is a pack, and
//! `<id>-demo.json` beside it, when present, is its demo workflow. Adding a craft is adding its
//! files; no Rust changes.

use std::fmt::Write as _;
use std::path::Path;

include!("digest.rs");

fn main() {
    // The adapter's code digest, for the memo salt (with the craft's own version and executable).
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    println!("cargo:rustc-env=PLUMCRAFT_CODE_DIGEST={}", source_digest(&[&here.join("src")], &here.join("../../Cargo.lock")));
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("packs");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut ids: Vec<String> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.filter_map(Result::ok)
                .filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".toml")).map(str::to_owned))
                .filter(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'))
                .collect()
        })
        .unwrap_or_default();
    ids.sort();
    let mut out = String::from("/// The built-in packs from `packs/`: (id, manifest, demo workflow).\npub const PACKS: &[(&str, &str, Option<&str>)] = &[\n");
    for id in &ids {
        let toml = dir.join(format!("{id}.toml"));
        let demo = dir.join(format!("{id}-demo.json"));
        println!("cargo:rerun-if-changed={}", toml.display());
        println!("cargo:rerun-if-changed={}", demo.display());
        let demo = if demo.is_file() { format!("Some(include_str!({:?}))", demo.display().to_string()) } else { "None".to_owned() };
        let _ = writeln!(out, "    ({id:?}, include_str!({:?}), {demo}),", toml.display().to_string());
    }
    out.push_str("];\n");
    let dest = Path::new(&std::env::var("OUT_DIR").unwrap_or_default()).join("packs.rs");
    if let Err(e) = std::fs::write(&dest, out) {
        println!("cargo:warning=packs.rs: {e}");
    }
}
