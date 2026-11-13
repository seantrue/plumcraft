//! The craft CAS (note 14, 3.2): raw files with real extensions, named by the digest of their
//! bytes in the process's algorithm (commonkit's `HASH`: md5 or BLAKE3), which the craft opens by path. It is the craft's only read root; `work/` is its only
//! write root. A file the craft writes lands in `work/tmp/<unique>/`, is hashed, then renamed
//! into the CAS, so a reader sees a whole file or none.
//!
//! - Documents and exports: `cas/<d[:2]>/<d>.<ext>`.
//! - Sources (a file or image a node opens): `cas/src/<d>/<name>`, under their own file name,
//!   because a craft records the name it opened (VectorCraft's File Info title).
//!
//! **It is a working store, not the store of record.** Documents and exports the UI sees are
//! published to the commonkit store too (`Blob::publish`), under the tree's own budget and TTL.
//! The CAS is reaped as commonkit reaps (contract 9.6): a file unused for the commonkit tree's
//! TTL goes, then the least recently used until the CAS is under 80% of its budget
//! (`PLUMCRAFT_CRAFT_MAX_GB`, default 1 per craft). Use is a file's mtime: adopting or staging
//! bytes already there, opening a document and a memo hit touch it. A memo hit on a reaped file
//! puts it back from commonkit, or runs the node again (`exec::files_present`).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use plumcraft_proto::Handle;
use plumcraft_proto::handle::{Algo, Recipe};

/// The largest file hashed (a 218 MB HFP trace is the largest seen; this is twenty of them).
pub const MAX_FILE: u64 = 4 << 30;

/// The CAS's budget when `PLUMCRAFT_CRAFT_MAX_GB` doesn't say.
pub const DEFAULT_MAX_BYTES: u64 = 1 << 30;

/// A `work/tmp` folder this old belongs to a process that died mid-call.
const STALE_TMP: Duration = Duration::from_secs(3600);

/// How long files are kept and how much the CAS may hold, and how often it is reaped.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    pub ttl: Duration,
    pub max_bytes: u64,
    pub interval: Duration,
}

impl Limits {
    /// The commonkit tree's TTL and reap interval, and `PLUMCRAFT_CRAFT_MAX_GB` (default 1).
    pub fn from_env() -> Limits {
        let policy = plumcraft_store::tree_policy(&plumcraft_store::default_root());
        let secs = |s: f64| if s.is_finite() && s > 0.0 { Duration::from_secs_f64(s.min(1e9)) } else { Duration::ZERO };
        let gb = std::env::var("PLUMCRAFT_CRAFT_MAX_GB").ok().and_then(|v| v.trim().parse::<f64>().ok()).filter(|g| g.is_finite() && *g > 0.0);
        let max_bytes = gb.map_or(DEFAULT_MAX_BYTES, |g| (g.min(1e6) * f64::from(1u32 << 30)) as u64);
        Limits { ttl: secs(policy.ttl_s), max_bytes, interval: secs(policy.reap_interval_s).max(Duration::from_secs(1)) }
    }
}

/// What a reap removed and kept.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Reaped {
    pub files: u64,
    pub bytes: u64,
    pub kept: u64,
    pub kept_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct Cas {
    root: PathBuf,
    cas: PathBuf,
    work: PathBuf,
    limits: Limits,
    /// When this process last reaped, in seconds since the epoch.
    last_reap: Arc<AtomicU64>,
}

impl Cas {
    /// The CAS under `root`: `root/cas` and `root/work`, created.
    pub fn new(root: &Path) -> std::io::Result<Cas> {
        let cas = root.join("cas");
        let work = root.join("work");
        std::fs::create_dir_all(cas.join("src"))?;
        std::fs::create_dir_all(work.join("tmp"))?;
        // Canonical, so the craft's root check and ours agree (macOS /var → /private/var).
        let (cas, work) = (cas.canonicalize()?, work.canonicalize()?);
        let c = Cas { root: root.to_path_buf(), cas, work, limits: Limits::from_env(), last_reap: Arc::new(AtomicU64::new(0)) };
        c.maybe_reap();
        Ok(c)
    }

    /// The same CAS with other limits (tests), reaped by them now.
    pub fn with_limits(mut self, limits: Limits) -> Cas {
        self.limits = limits;
        self.last_reap.store(0, Ordering::SeqCst);
        self.maybe_reap();
        self
    }

    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// Reap when this process hasn't for the reap interval. Only one caller reaps at a time.
    pub fn maybe_reap(&self) -> Option<Reaped> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let last = self.last_reap.load(Ordering::SeqCst);
        if last != 0 && now.saturating_sub(last) < self.limits.interval.as_secs() {
            return None;
        }
        self.last_reap.compare_exchange(last, now.max(1), Ordering::SeqCst, Ordering::SeqCst).ok()?;
        Some(self.reap(SystemTime::now()))
    }

    /// Reap as commonkit reaps (contract 9.6): every file last used before `now - ttl`, then the
    /// least recently used until what is kept is at most 80% of the budget; then stale
    /// `work/tmp` folders and empty source folders. A failure is skipped (another engine
    /// sharing the root may have taken the file first).
    pub fn reap(&self, now: SystemTime) -> Reaped {
        let mut files = Vec::new();
        walk(&self.cas, 0, &mut files);
        files.sort_by_key(|f| f.1);
        let cutoff = now.checked_sub(self.limits.ttl);
        let low_water = self.limits.max_bytes / 10 * 8;
        let mut total: u64 = files.iter().map(|f| f.2).sum();
        let mut out = Reaped::default();
        for (path, used, size) in files {
            let stale = cutoff.is_some_and(|c| used < c);
            if (stale || total > low_water) && std::fs::remove_file(&path).is_ok() {
                out.files += 1;
                out.bytes += size;
                total = total.saturating_sub(size);
            } else {
                out.kept += 1;
                out.kept_bytes += size;
            }
        }
        for dir in read_dirs(&self.cas.join("src")) {
            let _ = std::fs::remove_dir(dir); // only when empty
        }
        for dir in read_dirs(&self.work.join("tmp")) {
            let old = std::fs::metadata(&dir).and_then(|m| m.modified()).is_ok_and(|t| now.duration_since(t).is_ok_and(|age| age > STALE_TMP));
            if old {
                let _ = std::fs::remove_dir_all(dir);
            }
        }
        out
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The craft's read root.
    pub fn read_root(&self) -> &Path {
        &self.cas
    }

    /// The craft's write root.
    pub fn write_root(&self) -> &Path {
        &self.work
    }

    /// A fresh empty folder in `work/tmp`, for one call's output.
    pub fn tmp_dir(&self) -> std::io::Result<PathBuf> {
        static N: AtomicU64 = AtomicU64::new(0);
        let d = self.work.join("tmp").join(format!("{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        std::fs::create_dir_all(&d)?;
        Ok(d)
    }

    /// Where a handle's file with extension `ext` lives.
    pub fn path_for(&self, handle: &Handle, ext: &str) -> PathBuf {
        let d = handle.digest();
        self.cas.join(d.get(..2).unwrap_or("00")).join(format!("{d}.{ext}"))
    }

    /// Move a file the craft wrote into the CAS: hash it, rename it to its digest. Returns the
    /// handle, the CAS path and the size. The temp file is gone either way.
    pub fn adopt(&self, tmp: &Path, ext: &str) -> std::io::Result<(Handle, PathBuf, u64)> {
        let out = hash_file(tmp).and_then(|(h, size)| {
            let dest = self.path_for(&h, ext);
            if let Some(dir) = dest.parent() {
                std::fs::create_dir_all(dir)?;
            }
            if dest.is_file() {
                std::fs::remove_file(tmp)?;
                touch(&dest);
            } else {
                plumcraft_store::rename_addressed(tmp, &dest)?;
            }
            Ok((h, dest, size))
        });
        if out.is_err() {
            let _ = std::fs::remove_file(tmp);
        }
        self.maybe_reap();
        out
    }

    /// Stage a source file: copied (a clone on APFS) to `cas/src/<d>/<name>` once.
    pub fn stage_file(&self, src: &Path) -> std::io::Result<(Handle, PathBuf)> {
        let (h, _) = hash_file(src)?;
        let name = src.file_name().and_then(|n| n.to_str()).map_or_else(|| "source".to_owned(), clean_name);
        let dest = self.cas.join("src").join(h.digest()).join(name);
        if !dest.is_file() {
            let dir = dest.parent().ok_or_else(|| std::io::Error::other("no parent"))?;
            std::fs::create_dir_all(dir)?;
            let tmp = temp_in(dir);
            std::fs::copy(src, &tmp).and_then(|_| plumcraft_store::rename_addressed(&tmp, &dest)).inspect_err(|_| {
                let _ = std::fs::remove_file(&tmp);
            })?;
        } else {
            touch(&dest);
        }
        Ok((h, dest))
    }

    /// Stage bytes as `cas/src/<d>/<name>`.
    pub fn stage_bytes(&self, bytes: &[u8], name: &str) -> std::io::Result<(Handle, PathBuf)> {
        let h = Handle::raw(bytes);
        let dest = self.cas.join("src").join(h.digest()).join(clean_name(name));
        if !dest.is_file() {
            let dir = dest.parent().ok_or_else(|| std::io::Error::other("no parent"))?;
            std::fs::create_dir_all(dir)?;
            let tmp = temp_in(dir);
            std::fs::write(&tmp, bytes).and_then(|()| plumcraft_store::rename_addressed(&tmp, &dest)).inspect_err(|_| {
                let _ = std::fs::remove_file(&tmp);
            })?;
        } else {
            touch(&dest);
        }
        Ok((h, dest))
    }

    /// Whether `p` is inside the CAS (canonically, no symlink escape).
    pub fn contains(&self, p: &Path) -> bool {
        p.canonicalize().is_ok_and(|c| c.starts_with(&self.cas))
    }
}

/// A temp name in `dir`, the reaper leaves it (a leading dot). Unique per call: two threads can
/// stage the same bytes at once, and a name they shared would be renamed away from under one.
fn temp_in(dir: &Path) -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    dir.join(format!(".{}-{}.tmp", std::process::id(), N.fetch_add(1, Ordering::SeqCst)))
}

/// Mark a file used now (its mtime), so the reaper keeps it. Best effort.
pub fn touch(p: &Path) {
    let _ = std::fs::File::options().append(true).open(p).and_then(|f| f.set_modified(SystemTime::now()));
}

/// The folders directly in `dir`.
fn read_dirs(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect()).unwrap_or_default()
}

/// The CAS's files with when they were last used and their size: `<d[:2]>/<file>` and
/// `src/<d>/<name>`, two levels down, never following a link. Temp files being written (a
/// leading dot) are left alone.
fn walk(dir: &Path, depth: u8, out: &mut Vec<(PathBuf, SystemTime, u64)>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let Ok(m) = std::fs::symlink_metadata(e.path()) else { continue };
        if m.is_dir() && depth < 2 {
            walk(&e.path(), depth + 1, out);
        } else if m.is_file() && depth > 0 && !e.file_name().to_string_lossy().starts_with('.') {
            out.push((e.path(), m.modified().unwrap_or(UNIX_EPOCH), m.len()));
        }
    }
}

/// A file name safe to create: its last component, no leading dot, at most 120 characters.
fn clean_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned: String = base.chars().filter(|c| !c.is_control()).take(120).collect();
    let cleaned = cleaned.trim_start_matches('.').to_owned();
    if cleaned.is_empty() { "source".into() } else { cleaned }
}

/// The `<current>:raw:` handle and size of a file, streamed through `commonkit::digest`, at most
/// [`MAX_FILE`] bytes.
pub fn hash_file(p: &Path) -> std::io::Result<(Handle, u64)> {
    let (hex, total) = commonkit::digest::file(p, MAX_FILE)?;
    let handle = Handle::from_digest(Algo::current(), Recipe::Raw, &hex).map_err(|e| std::io::Error::other(format!("{e:?}")))?;
    Ok((handle, total))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_threads_can_stage_the_same_bytes() {
        let dir = plumcraft_scratch::root().join("cas-race");
        let _ = std::fs::remove_dir_all(&dir);
        let cas = Cas::new(&dir).unwrap();
        for round in 0..200 {
            let bytes = format!("the same document {round}").into_bytes();
            let gate = std::sync::Barrier::new(8);
            std::thread::scope(|t| {
                for _ in 0..8 {
                    t.spawn(|| {
                        gate.wait();
                        cas.stage_bytes(&bytes, "doc.svg").unwrap_or_else(|e| panic!("round {round}: {e}"));
                    });
                }
            });
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn adopt_and_stage() {
        let dir = plumcraft_scratch::root().join("cas-test");
        let cas = Cas::new(&dir).unwrap();
        let t = cas.tmp_dir().unwrap().join("x.svg");
        std::fs::write(&t, b"<svg/>").unwrap();
        let (h, p, n) = cas.adopt(&t, "svg").unwrap();
        assert_eq!(h, Handle::raw(b"<svg/>"));
        assert_eq!(n, 6);
        assert_eq!(std::fs::read(&p).unwrap(), b"<svg/>");
        assert!(!t.exists());
        assert!(cas.contains(&p));
        // Adopting the same bytes again keeps one file.
        let t2 = cas.tmp_dir().unwrap().join("y.svg");
        std::fs::write(&t2, b"<svg/>").unwrap();
        assert_eq!(cas.adopt(&t2, "svg").unwrap().1, p);
        let (h2, s) = cas.stage_bytes(b"jpeg", "../../etc/test.jpg").unwrap();
        assert_eq!(h2, Handle::raw(b"jpeg"));
        assert!(s.ends_with("test.jpg") && cas.contains(&s));
        let (h3, s3) = cas.stage_file(&s).unwrap();
        assert_eq!((h3, s3.file_name()), (h2, s.file_name()));
        assert_eq!(clean_name(".hidden"), "hidden");
        assert_eq!(clean_name(""), "source");
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn aged(p: &Path, secs: u64) {
        let t = SystemTime::now() - Duration::from_secs(secs);
        std::fs::File::options().append(true).open(p).unwrap().set_modified(t).unwrap();
    }

    fn adopt(cas: &Cas, bytes: &[u8]) -> PathBuf {
        let t = cas.tmp_dir().unwrap().join("x.svg");
        std::fs::write(&t, bytes).unwrap();
        cas.adopt(&t, "svg").unwrap().1
    }

    #[test]
    fn reaping_takes_what_outlived_the_ttl_then_the_least_recently_used() {
        let dir = plumcraft_scratch::root().join("cas-reap");
        let _ = std::fs::remove_dir_all(&dir);
        let day = Duration::from_secs(86_400);
        let cas = Cas::new(&dir).unwrap().with_limits(Limits { ttl: day, max_bytes: 1000, interval: Duration::from_secs(3600) });
        let old = adopt(&cas, &[1u8; 100]);
        let (_, src) = cas.stage_bytes(&[2u8; 100], "photo.png").unwrap();
        let recent = adopt(&cas, &[3u8; 100]);
        aged(&old, 2 * 86_400);
        aged(&src, 2 * 86_400);
        aged(&recent, 3600);
        let r = cas.reap(SystemTime::now());
        assert_eq!((r.files, r.bytes, r.kept), (2, 200, 1), "{r:?}");
        assert!(!old.exists() && !src.exists() && recent.exists());
        assert!(!src.parent().unwrap().exists(), "the empty source folder goes too");

        // Over budget: the least recently used go until 80% of it is left.
        let a = adopt(&cas, &[4u8; 300]);
        let b = adopt(&cas, &[5u8; 300]);
        let c = adopt(&cas, &[6u8; 300]);
        aged(&a, 300);
        aged(&recent, 200);
        aged(&b, 100);
        let r = cas.reap(SystemTime::now());
        assert!(!a.exists() && recent.exists() && b.exists() && c.exists(), "{r:?}");
        assert_eq!((r.files, r.kept_bytes), (1, 700), "1000 bytes against a budget of 1000: down to 800 or less");

        // Using a file again (adopting or staging the same bytes) makes it recent.
        aged(&b, 2 * 86_400);
        assert_eq!(adopt(&cas, &[5u8; 300]), b);
        assert_eq!(cas.reap(SystemTime::now()).files, 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_dead_process_work_folder_goes_after_an_hour() {
        let dir = plumcraft_scratch::root().join("cas-tmp");
        let _ = std::fs::remove_dir_all(&dir);
        let cas = Cas::new(&dir).unwrap();
        let (stale, live) = (cas.tmp_dir().unwrap(), cas.tmp_dir().unwrap());
        let mut o = std::fs::OpenOptions::new();
        o.read(true);
        // Windows opens a folder only with backup semantics, and setting its time needs
        // FILE_WRITE_ATTRIBUTES
        #[cfg(windows)]
        std::os::windows::fs::OpenOptionsExt::custom_flags(std::os::windows::fs::OpenOptionsExt::access_mode(&mut o, 0x0100), 0x0200_0000);
        o.open(&stale).unwrap().set_modified(SystemTime::now() - Duration::from_secs(7200)).unwrap();
        cas.reap(SystemTime::now());
        assert!(!stale.exists() && live.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn limits_come_from_the_environment() {
        let l = Limits::from_env();
        assert!(l.interval >= Duration::from_secs(1));
        assert!(l.max_bytes > 0);
    }

    /// The craft CAS follows the switch: `hash_file` and `stage_bytes` name files with the
    /// process's algorithm, its length and prefix (the suite runs once per algorithm).
    #[test]
    fn names_follow_the_algorithm() {
        let algo = Algo::current();
        let dir = plumcraft_scratch::root().join("cas-algo");
        let _ = std::fs::remove_dir_all(&dir);
        let cas = Cas::new(&dir).unwrap();
        let (h, p) = cas.stage_bytes(b"a source", "a.png").unwrap();
        assert_eq!((h.algo(), h.digest().len()), (algo, algo.hex_len()));
        assert!(p.display().to_string().contains(&format!("/src/{}/", h.digest())), "{}", p.display());
        let (f, n) = hash_file(&p).unwrap();
        assert_eq!((&f, n), (&h, 8));
        assert!(f.to_string().starts_with(&format!("{}:raw:", algo.as_str())));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// An `exe-digest.json` written under the other algorithm is a miss, and is rewritten.
    #[test]
    fn an_exe_digest_of_the_other_algorithm_is_rewritten() {
        let algo = Algo::current();
        let other = if algo == Algo::Md5 { "b3" } else { "md5" };
        let dir = plumcraft_scratch::root().join("cas-exe-digest");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("app");
        std::fs::write(&exe, b"an executable").unwrap();
        let first = crate::exe_digest(&exe, &dir).unwrap();
        assert_eq!(first.algo(), algo);
        let file = dir.join("exe-digest.json");
        let mut rec: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(rec["key"]["algo"], algo.as_str());
        let n = if other == "md5" { 32 } else { 64 };
        rec["key"]["algo"] = other.into();
        rec["digest"] = format!("{other}:raw:{}", "0".repeat(n)).into();
        std::fs::write(&file, rec.to_string()).unwrap();
        assert_eq!(crate::exe_digest(&exe, &dir).unwrap(), first, "a miss: hashed again");
        let back: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!((back["key"]["algo"].as_str(), back["digest"].as_str()), (Some(algo.as_str()), Some(first.to_string().as_str())));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
