//! Scratch directories that clean up after themselves. Each process gets one root,
//! `$TMPDIR/plumcraft-test-<pid>`, for everything its tests write; the first call to [`root`]
//! sweeps the roots of processes that have exited, and [`sweep`] also takes the socket and HTTP
//! directories of hosts (`plumcraft-<pid>-<n>`, `plumcraft-http-<pid>-<n>`) whose process died
//! before dropping them. A process's own files stay until a later process sweeps them, so a
//! failed run's files can still be looked at. A directory whose owner can't be shown to be gone
//! is kept.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// The name of a process's scratch root.
const ROOT_PREFIX: &str = "plumcraft-test-";

/// This process's scratch root, created, after sweeping those of exited processes.
pub fn root() -> PathBuf {
    static ROOT: OnceLock<PathBuf> = OnceLock::new();
    ROOT.get_or_init(|| {
        sweep_once();
        let root = std::env::temp_dir().join(format!("{ROOT_PREFIX}{}", std::process::id()));
        let _ = std::fs::create_dir_all(&root);
        root
    })
    .clone()
}

/// A directory `name` under this process's root, created (what was there is kept).
pub fn dir(name: &str) -> PathBuf {
    let d = root().join(name);
    let _ = std::fs::create_dir_all(&d);
    d
}

/// [`sweep`] of the temp folder, the first time it is called in this process (hosts call it
/// before making their socket directory).
pub fn sweep_once() {
    static SWEPT: OnceLock<()> = OnceLock::new();
    SWEPT.get_or_init(|| {
        sweep(&std::env::temp_dir());
    });
}

/// Removes, from `tmp`, every scratch root and host directory whose owning process has exited.
/// Returns how many were removed.
pub fn sweep(tmp: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(tmp) else { return 0 };
    let me = std::process::id();
    let mut removed = 0;
    for e in entries.flatten() {
        let name = e.file_name();
        let Some(pid) = name.to_str().and_then(owner) else { continue };
        if pid == me || alive(pid) {
            continue;
        }
        // only directories, never a link out of the temp folder
        if e.file_type().map(|t| t.is_dir()).unwrap_or(false) && std::fs::remove_dir_all(e.path()).is_ok() {
            removed += 1;
        }
    }
    removed
}

/// The process a scratch name belongs to: `plumcraft-test-<pid>`, `plumcraft-<pid>-<n>` (a
/// process host's socket directory) or `plumcraft-http-<pid>-<n>` (an HTTP host's).
pub fn owner(name: &str) -> Option<u32> {
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if let Some(pid) = name.strip_prefix(ROOT_PREFIX) {
        return if digits(pid) { pid.parse().ok() } else { None };
    }
    let rest = name.strip_prefix("plumcraft-")?;
    let rest = rest.strip_prefix("http-").unwrap_or(rest);
    let (pid, n) = rest.split_once('-')?;
    if digits(pid) && digits(n) { pid.parse().ok() } else { None }
}

/// Whether process `pid` may still be running: `kill -0` says it exists (or the check itself
/// fails, which keeps the directory).
#[cfg(unix)]
fn alive(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(true)
}

/// Elsewhere nothing is swept.
#[cfg(not(unix))]
fn alive(_: u32) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owners() {
        assert_eq!(owner("plumcraft-test-123"), Some(123));
        assert_eq!(owner("plumcraft-123-4"), Some(123));
        assert_eq!(owner("plumcraft-http-123-4"), Some(123));
        for not in [
            "plumcraft-test-",
            "plumcraft-test-12x",
            "plumcraft-123",
            "plumcraft-e2-123-hunt",
            "plumcraft-control.token",
            "plumcraft-screenshots",
            "plumcraft-http-123",
            "plumcraft--4",
            "other-123-4",
            "plumcraft-test-99999999999",
        ] {
            assert_eq!(owner(not), None, "{not}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn sweeps_the_dead_and_keeps_the_living() {
        let tmp = root().join("sweep");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        // a process that has exited
        let mut child = std::process::Command::new("true").spawn().unwrap();
        let dead = child.id();
        child.wait().unwrap();
        let me = std::process::id();
        let parent = std::os::unix::process::parent_id();
        let names = [format!("plumcraft-test-{dead}"), format!("plumcraft-{dead}-0"), format!("plumcraft-http-{dead}-3")];
        let kept = [format!("plumcraft-test-{me}"), format!("plumcraft-test-{parent}"), format!("plumcraft-{parent}-1"), "plumcraft-screenshots".to_owned()];
        for n in names.iter().chain(&kept) {
            std::fs::create_dir_all(tmp.join(n).join("inner")).unwrap();
        }
        // a file with a scratch name is not a directory: kept
        std::fs::write(tmp.join(format!("plumcraft-{dead}-9")), b"x").unwrap();
        assert_eq!(sweep(&tmp), names.len());
        for n in &names {
            assert!(!tmp.join(n).exists(), "{n} swept");
        }
        for n in &kept {
            assert!(tmp.join(n).exists(), "{n} kept");
        }
        assert!(tmp.join(format!("plumcraft-{dead}-9")).exists());
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn root_is_per_process_and_dir_nests() {
        let r = root();
        assert!(r.is_dir());
        assert_eq!(r.file_name().and_then(|n| n.to_str()), Some(format!("plumcraft-test-{}", std::process::id()).as_str()));
        let d = dir("nest");
        assert!(d.is_dir() && d.starts_with(&r));
    }
}
