//! Bakes this engine's code digest into its memo salt (`plumcraft_engine::salt`), and embeds
//! craft-fonts for Draw Text when the build says where they are (`CRAFT_FONTS_DIR`, an absolute
//! path to a craft-fonts checkout; craft-fonts' `docs/integration.md`): never a Cargo dependency,
//! so a build without it is unchanged (Draw Text then has Inter, the UI's own OFL face).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

include!("digest.rs");

fn main() {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    emit_digest("PLUMCRAFT_CODE_DIGEST", &source_digest(&[&here.join("src")], &here.join("../../Cargo.lock")), "");
    println!("cargo::rerun-if-env-changed=CRAFT_FONTS_DIR");
    println!("cargo::rerun-if-env-changed=CRAFT_FONTS_REQUIRED");
    let mut src =
        String::from("/// craft-fonts embedded at build time: (family, style, scripts, bytes).\npub static CRAFT_FONTS: &[(&str, &str, &str, &[u8])] = &[\n");
    if let Some(dir) = std::env::var_os("CRAFT_FONTS_DIR").filter(|d| !d.is_empty()).map(PathBuf::from) {
        match craft_fonts(&dir) {
            Ok(entries) => src.push_str(&entries),
            Err(e) if std::env::var_os("CRAFT_FONTS_REQUIRED").is_some() => println!("cargo::error=CRAFT_FONTS_DIR={}: {e}", dir.display()),
            Err(e) => println!("cargo::warning=building without craft-fonts: CRAFT_FONTS_DIR={}: {e}", dir.display()),
        }
    }
    src.push_str("];\n");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap_or_default()).join("craft_fonts.rs");
    if let Err(e) = std::fs::write(&out, src) {
        println!("cargo::error=writing {}: {e}", out.display());
    }
}

/// One entry per line of craft-fonts' `fonts/manifest.txt`.
fn craft_fonts(dir: &Path) -> Result<String, String> {
    let manifest = dir.join("fonts/manifest.txt");
    println!("cargo::rerun-if-changed={}", manifest.display());
    let text = std::fs::read_to_string(&manifest).map_err(|e| format!("{}: {e}", manifest.display()))?;
    let mut out = String::new();
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let f: Vec<&str> = line.split(" | ").map(str::trim).collect();
        let [family, style, file, scripts, ..] = f.as_slice() else {
            return Err(format!("malformed manifest line: {line}"));
        };
        let path = dir.join(file).canonicalize().map_err(|e| format!("{file}: {e}"))?;
        println!("cargo::rerun-if-changed={}", path.display());
        let _ = writeln!(out, "    ({family:?}, {style:?}, {scripts:?}, include_bytes!({:?})),", path.display().to_string());
    }
    Ok(out)
}
