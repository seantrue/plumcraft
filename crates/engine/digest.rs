// The code digest a build script bakes into an engine's memo salt (spec 14.2): the MD5 of every
// file under the given folders (sorted by path; path and bytes both hashed) and of the
// workspace's Cargo.lock, so a change to the code or to a dependency changes every plan key.
// Shared by `include!` from each engine crate's build.rs; not a module of any crate.

#[allow(dead_code)]
fn source_digest(dirs: &[&std::path::Path], lock: &std::path::Path) -> String {
    use md5::{Digest, Md5};
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
    let mut h = Md5::new();
    for dir in dirs {
        println!("cargo:rerun-if-changed={}", dir.display());
        let mut files = Vec::new();
        walk(dir, &mut files);
        files.sort();
        for f in files {
            let rel = f.strip_prefix(dir).unwrap_or(&f);
            h.update(rel.to_string_lossy().as_bytes());
            h.update([0u8]);
            h.update(std::fs::read(&f).unwrap_or_default());
            h.update([0u8]);
        }
    }
    println!("cargo:rerun-if-changed={}", lock.display());
    h.update(std::fs::read(lock).unwrap_or_default());
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}
