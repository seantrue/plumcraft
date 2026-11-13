//! `plumcraft scan-store`: every digest in a tree that isn't the process's algorithm (#91, #92;
//! the commonkit hash facade, D25 G). A diagnostic and a test aid, private to this binary; it
//! reads every memo record, so it is slow on a large tree.
//!
//! **It never writes.** It runs as a gate on the maintainer's own trees, so it opens files for
//! reading only: no `Store` (whose construction creates the root and probes it, and whose
//! reads touch a record's mtime). A root that doesn't exist is an error, not a clean tree.
//!
//! - `scan-store <root>`: a commonkit tree (or the UI's durable CAS, which is a
//!   `plumcraft_store::Store` too): segment names under the `plum*` forms, `.sources/<digest>/`
//!   names, and the handles and plan keys inside each memo record.
//! - `scan-store --craft <craft-root>`: the names under each `<id>/cas/`, and the handle in each
//!   `<id>/exe-digest.json`.
//!
//! One line per finding, `<relative path>: <what>`.

use std::path::{Path, PathBuf};

use plumcraft_proto::handle::{Algo, Handle};
use plumcraft_store::{FORM_ARRAY, FORM_MEMO, FORM_RAW, FORM_Z};

fn is_hex(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A name that looks like a digest of either length.
fn is_digest(s: &str) -> bool {
    (s.len() == 32 || s.len() == 64) && is_hex(s)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() { walk(&p, out) } else { out.push(p) }
    }
}

fn other(algo: Algo) -> Algo {
    if algo == Algo::Md5 { Algo::B3 } else { Algo::Md5 }
}

/// Handles and plan keys of the other algorithm inside `text`.
fn foreign_in(text: &str, algo: Algo) -> Vec<String> {
    let o = other(algo).as_str();
    let mut found = Vec::new();
    for (needle, what) in [(format!("\"{o}:"), "a handle"), (format!("\"memo:{o}:"), "a plan key")] {
        for (at, _) in text.match_indices(&needle) {
            let rest = text.get(at + 1..).unwrap_or_default();
            found.push(format!("{what} {}", rest.split('"').next().unwrap_or_default()));
        }
    }
    found
}

/// A root to scan: an existing directory, or why not.
fn existing(root: &Path) -> Result<(), String> {
    if root.is_dir() { Ok(()) } else { Err(format!("{}: not a directory", root.display())) }
}

/// A memo record's bytes, read from its segment file without writing anything: the `.npy`
/// payload (a 1-d `|u1` array), inflated (memo records are always compressed).
fn read_record_file(path: &Path) -> Option<Vec<u8>> {
    let b = std::fs::read(path).ok()?;
    if b.get(..6)? != b"\x93NUMPY" {
        return None;
    }
    let start = match b.get(6)? {
        1 => 10 + usize::from(u16::from_le_bytes([*b.get(8)?, *b.get(9)?])),
        2 | 3 => 12 + usize::try_from(u32::from_le_bytes([*b.get(8)?, *b.get(9)?, *b.get(10)?, *b.get(11)?])).ok()?,
        _ => return None,
    };
    plumcraft_store::inflate(b.get(start..)?)
}

/// Every digest under a store `root` that isn't `algo`'s; an error when `root` isn't a directory.
pub fn foreign_digests(root: &Path, algo: Algo) -> Result<Vec<String>, String> {
    existing(root)?;
    let want = algo.hex_len();
    let mut files = Vec::new();
    for form in [FORM_ARRAY, FORM_RAW, FORM_Z, FORM_MEMO, ".sources"] {
        walk(&root.join(form), &mut files);
    }
    files.sort();
    let mut found = Vec::new();
    for f in files {
        let Ok(rel) = f.strip_prefix(root) else { continue };
        let shown = rel.display().to_string();
        let parts: Vec<String> = rel.iter().map(|c| c.to_string_lossy().into_owned()).collect();
        for (i, part) in parts.iter().enumerate() {
            let stem = if i + 1 == parts.len() { part.split('.').next().unwrap_or_default() } else { part.as_str() };
            if is_digest(stem) && stem.len() != want {
                found.push(format!("{shown}: a {}-digit digest in its name", stem.len()));
            }
        }
        if parts.first().is_some_and(|p| p == FORM_MEMO) {
            let stem = parts.last().and_then(|p| p.split('.').next()).unwrap_or_default();
            if stem.len() != want {
                continue; // already reported by its name
            }
            let Some(rec) = read_record_file(&f) else {
                found.push(format!("{shown}: a memo record that doesn't read"));
                continue;
            };
            found.extend(foreign_in(&String::from_utf8_lossy(&rec), algo).into_iter().map(|w| format!("{shown}: {w}")));
        }
    }
    Ok(found)
}

/// Every digest in a craft root (`PLUMCRAFT_CRAFT_ROOT`) that isn't `algo`'s; an error when
/// `root` isn't a directory.
pub fn foreign_craft(root: &Path, algo: Algo) -> Result<Vec<String>, String> {
    existing(root)?;
    let want = algo.hex_len();
    let mut found = Vec::new();
    let rd = std::fs::read_dir(root).map_err(|e| format!("{}: {e}", root.display()))?;
    let mut crafts: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    crafts.sort();
    for craft in crafts {
        let mut files = Vec::new();
        walk(&craft.join("cas"), &mut files);
        files.sort();
        for f in files {
            let Ok(rel) = f.strip_prefix(root) else { continue };
            for c in rel.iter().map(|c| c.to_string_lossy()) {
                let stem = c.split('.').next().unwrap_or_default();
                if is_digest(stem) && stem.len() != want {
                    found.push(format!("{}: a {}-digit digest in its name", rel.display(), stem.len()));
                }
            }
        }
        let exe = craft.join("exe-digest.json");
        if let Ok(text) = std::fs::read_to_string(&exe) {
            let shown = exe.strip_prefix(root).unwrap_or(&exe).display().to_string();
            let digest = serde_json::from_str::<serde_json::Value>(&text).ok().and_then(|v| v["digest"].as_str().map(str::to_owned));
            match digest.as_deref().map(str::parse::<Handle>) {
                Some(Ok(h)) if h.algo() == algo => {}
                Some(Ok(h)) => found.push(format!("{shown}: the handle {h}")),
                _ => found.push(format!("{shown}: no handle")),
            }
        }
    }
    Ok(found)
}

/// `plumcraft scan-store [--craft] ROOT`: exit 1 when anything is found.
pub fn main(args: &[String]) -> std::process::ExitCode {
    let (craft, root) = match args {
        [flag, root] if flag == "--craft" => (true, root),
        [root] if root != "--craft" => (false, root),
        _ => {
            eprintln!("usage: plumcraft scan-store ROOT | plumcraft scan-store --craft CRAFT_ROOT");
            return std::process::ExitCode::from(2);
        }
    };
    let algo = Algo::current();
    let found = match if craft { foreign_craft(Path::new(root), algo) } else { foreign_digests(Path::new(root), algo) } {
        Ok(f) => f,
        Err(e) => {
            eprintln!("scan-store: {e}");
            return std::process::ExitCode::from(2);
        }
    };
    for f in &found {
        println!("{f}");
    }
    eprintln!("scan-store: {} finding(s) not {} in {root}", found.len(), algo.as_str());
    if found.is_empty() { std::process::ExitCode::SUCCESS } else { std::process::ExitCode::from(1) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use plumcraft_store::Store;

    fn fresh(tag: &str) -> PathBuf {
        let d = plumcraft_scratch::root().join(format!("scan-store-{tag}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn plant(root: &Path, rel: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"x").unwrap();
    }

    fn digest(n: usize) -> String {
        "ab".repeat(n / 2)
    }

    /// A tree written through the store in this algorithm is clean; a planted segment, source and
    /// record name of the other length are each found. The same holds for a durable-CAS folder,
    /// which is a `Store` too.
    #[test]
    fn scan_store_finds_the_other_length() {
        let algo = Algo::current();
        let (ours, theirs) = (algo.hex_len(), other(algo).hex_len());
        for tag in ["tree", "durable-cas"] {
            let root = fresh(tag);
            let s = Store::new(&root);
            s.publish_raw(&b"y".repeat(100_000)).unwrap();
            s.publish_array("<f4", &[2], &[0; 8]).unwrap();
            assert!(s.put_record(&format!("memo:{}:{}", algo.as_str(), digest(ours)), format!("{{\"handle\": \"{}\"}}", Handle::raw(b"z")).as_bytes()));
            plant(&root, &format!(".sources/{}/a.png", digest(ours)));
            assert_eq!(foreign_digests(&root, algo).unwrap(), Vec::<String>::new(), "{tag}: clean");
            let t = digest(theirs);
            plant(&root, &format!("plumraw/{}/{t}.c.npy", &t[..2]));
            plant(&root, &format!(".sources/{t}/a.png"));
            plant(&root, &format!("plummemo/{}/{t}.c.npy", &t[..2]));
            let found = foreign_digests(&root, algo).unwrap();
            assert_eq!(found.len(), 3, "{tag}: {found:?}");
            assert!(found.iter().all(|f| f.contains(&format!("a {theirs}-digit digest"))), "{found:?}");
        }
    }

    /// A record of this algorithm that names a handle or plan key of the other is found.
    #[test]
    fn scan_store_reads_inside_records() {
        let algo = Algo::current();
        let o = other(algo);
        let root = fresh("records");
        let s = Store::new(&root);
        let body = format!("{{\"plan_key\": \"memo:{}:{}\", \"handle\": \"{}:raw:{}\"}}", o.as_str(), digest(o.hex_len()), o.as_str(), digest(o.hex_len()));
        assert!(s.put_record(&format!("memo:{}:{}", algo.as_str(), digest(algo.hex_len())), body.as_bytes()));
        let found = foreign_digests(&root, algo).unwrap();
        assert_eq!(found.len(), 2, "{found:?}");
    }

    /// `--craft`: a planted CAS name of the other length, and an `exe-digest.json` of the other
    /// algorithm, are found; this algorithm's are not.
    #[test]
    fn scan_store_craft() {
        let algo = Algo::current();
        let o = other(algo);
        let root = fresh("craft");
        let ours = digest(algo.hex_len());
        plant(&root, &format!("vectorcraft/cas/{}/{ours}.svg", &ours[..2]));
        plant(&root, &format!("vectorcraft/cas/src/{ours}/a.png"));
        std::fs::write(root.join("vectorcraft/exe-digest.json"), format!("{{\"key\": {{}}, \"digest\": \"{}\"}}", Handle::raw(b"exe"))).unwrap();
        assert_eq!(foreign_craft(&root, algo).unwrap(), Vec::<String>::new());
        let t = digest(o.hex_len());
        plant(&root, &format!("vectorcraft/cas/{}/{t}.svg", &t[..2]));
        std::fs::write(root.join("vectorcraft/exe-digest.json"), format!("{{\"key\": {{}}, \"digest\": \"{}:raw:{t}\"}}", o.as_str())).unwrap();
        let found = foreign_craft(&root, algo).unwrap();
        assert_eq!(found.len(), 2, "{found:?}");
    }

    /// Every entry under `root` (the root too) with its mtime.
    fn mtimes(root: &Path) -> Vec<(PathBuf, std::time::SystemTime)> {
        fn go(p: &Path, out: &mut Vec<(PathBuf, std::time::SystemTime)>) {
            if let Ok(m) = std::fs::symlink_metadata(p) {
                out.push((p.to_path_buf(), m.modified().unwrap()));
            }
            if p.is_dir() {
                let mut kids: Vec<PathBuf> = std::fs::read_dir(p).unwrap().flatten().map(|e| e.path()).collect();
                kids.sort();
                kids.iter().for_each(|k| go(k, out));
            }
        }
        let mut out = Vec::new();
        go(root, &mut out);
        out
    }

    /// The scan runs on the maintainer's own trees as a gate, so it must not write: after it,
    /// nothing in the tree (files, folders, the root) is new or has a newer mtime.
    #[test]
    fn scan_store_writes_nothing() {
        let algo = Algo::current();
        let root = fresh("readonly");
        let s = Store::new(&root);
        s.publish_raw(&b"y".repeat(100_000)).unwrap();
        assert!(s.put_record(&format!("memo:{}:{}", algo.as_str(), digest(algo.hex_len())), b"{\"v\": 1}"));
        drop(s);
        let craft = fresh("readonly-craft");
        plant(&craft, &format!("c/cas/src/{}/a.png", digest(algo.hex_len())));
        std::fs::write(craft.join("c/exe-digest.json"), format!("{{\"digest\": \"{}\"}}", Handle::raw(b"e"))).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let (before, before_craft) = (mtimes(&root), mtimes(&craft));
        assert_eq!(foreign_digests(&root, algo).unwrap(), Vec::<String>::new());
        assert_eq!(foreign_craft(&craft, algo).unwrap(), Vec::<String>::new());
        assert_eq!(mtimes(&root), before, "the store tree changed");
        assert_eq!(mtimes(&craft), before_craft, "the craft tree changed");
    }

    /// A root that doesn't exist fails (and isn't created), never a clean "0 findings".
    #[test]
    fn scan_store_fails_on_a_missing_root() {
        let missing = fresh("missing-parent").join("no-such-root");
        assert!(foreign_digests(&missing, Algo::current()).is_err());
        assert!(foreign_craft(&missing, Algo::current()).is_err());
        assert!(!missing.exists(), "the scan created it");
        assert_eq!(main(&[missing.display().to_string()]), std::process::ExitCode::from(2));
        assert_eq!(main(&["--craft".into(), missing.display().to_string()]), std::process::ExitCode::from(2));
    }
}
