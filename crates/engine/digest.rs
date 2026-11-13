// The code digest a build script bakes into an engine's memo salt (spec 14.2): a digest of every
// file under the given folders (sorted by path; path and bytes both hashed) and of the
// workspace's Cargo.lock, so a change to the code or to a dependency changes every plan key.
// A build script runs before the runtime's algorithm is known, so it bakes both
// (`commonkit::digest::build::both`, D4) and the runtime picks (`plumcraft_engine::code_digest`).
// Shared by `include!` from each engine crate's build.rs; not a module of any crate.

#[allow(dead_code)]
fn source_digest(dirs: &[&std::path::Path], lock: &std::path::Path) -> commonkit::digest::build::Salts {
    fn walk(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.is_file() {
                out.push(p);
            }
        }
    }
    let mut all = Vec::new();
    for dir in dirs {
        println!("cargo:rerun-if-changed={}", dir.display());
        let mut files = Vec::new();
        walk(dir, &mut files);
        files.sort();
        for f in files {
            let rel = f.strip_prefix(dir).unwrap_or(&f);
            all.extend_from_slice(rel.to_string_lossy().as_bytes());
            all.push(0u8);
            all.extend(std::fs::read(&f).unwrap_or_default());
            all.push(0u8);
        }
    }
    println!("cargo:rerun-if-changed={}", lock.display());
    all.extend(std::fs::read(lock).unwrap_or_default());
    #[allow(clippy::disallowed_methods)] // a build script: the one place build::both belongs
    let salts = commonkit::digest::build::both(&all);
    salts
}

/// `cargo:rustc-env` lines for `<name>_MD5` and `<name>_B3`.
#[allow(dead_code)]
fn emit_digest(name: &str, s: &commonkit::digest::build::Salts, suffix: &str) {
    println!("cargo:rustc-env={name}_MD5={}{suffix}", s.md5);
    println!("cargo:rustc-env={name}_B3={}{suffix}", s.b3);
}
