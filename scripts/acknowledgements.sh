#!/usr/bin/env bash
# Make PlumCraft's acknowledgements (FR4, research note 29): the licences of the Rust crates the
# editor is built from, by `cargo about` (install it once with `cargo install cargo-about --locked
# --features cli`), into crates/editor-core/acknowledgements/rust.json, and Cargo.lock's digest
# beside it, which a test compares. cargo about reads the local registry and fetches the licence
# files about.toml names from the crates' own repositories (crate names, versions and paths are
# all it sends). Never run by the build, the tests or CI.
set -euo pipefail
cd "$(dirname "$0")/.."
out=crates/editor-core/acknowledgements
cargo about generate --locked -c about.toml "$out/about.hbs" -o "$out/rust.json"
shasum -a 256 Cargo.lock | cut -d' ' -f1 > "$out/cargo-lock.sha256"
echo "wrote $out/rust.json and $out/cargo-lock.sha256"
