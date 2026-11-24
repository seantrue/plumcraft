//! The build-time half: run from a build script, never at run time. [`emit`] asks git (when there
//! is one) what the source is, and tells cargo three values for the crate being built
//! (`PLUMCRAFT_STAMP_COMMIT`, `PLUMCRAFT_STAMP_DIRTY`, `PLUMCRAFT_STAMP_DATE`, read back by
//! [`crate::stamp!`]) and when to ask again.
//!
//! Cargo runs the build script again when a path it was told to watch changes: git's `HEAD`, the
//! branch it names, `packed-refs` and the index (a commit, a checkout, a `git add`), and the
//! paths the caller names (for PlumCraft, `crates/`, `Cargo.toml` and `Cargo.lock`, so a source
//! edit marks the build as having changes). Only this crate and those that use it are built
//! again. What it cannot see: an edit outside the watched paths that git counts (a document, the
//! Swift sources) leaves the flag as it was until something watched changes. With no git, or
//! outside a repository (a source tarball, a crates.io build), everything is "unknown" and the
//! build goes on.
//!
//! Nothing personal is kept: a short hash, a yes or no, and a UTC time (`SOURCE_DATE_EPOCH`
//! when it is set, for a reproducible build). The paths printed for cargo stay in cargo.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// What git says of `dir`'s repository: its short commit, whether tracked files have changes
/// (None: unknown), its git files to watch, and the repository's top folder.
pub struct Source {
    pub commit: Option<String>,
    pub dirty: Option<bool>,
    pub watch: Vec<PathBuf>,
    pub top: Option<PathBuf>,
}

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    // --no-optional-locks: `git status` would otherwise refresh the index, which cargo watches,
    // and every build would stamp again
    let out = Command::new("git").arg("--no-optional-locks").arg("-C").arg(dir).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?;
    Some(s.trim_end_matches(['\n', '\r']).to_owned())
}

/// Ask git about `dir` (nothing when there is no git, or `dir` is in no repository).
pub fn source(dir: &Path) -> Source {
    let commit = git(dir, &["rev-parse", "--short=7", "HEAD"]).filter(|c| !c.is_empty() && c.len() <= 40 && c.chars().all(|ch| ch.is_ascii_hexdigit()));
    let dirty = commit.as_ref().and_then(|_| git(dir, &["status", "--porcelain", "--untracked-files=no"])).map(|s| !s.trim().is_empty());
    let top = git(dir, &["rev-parse", "--show-toplevel"]).map(PathBuf::from);
    let mut watch = Vec::new();
    if commit.is_some() {
        let mut names = vec!["HEAD".to_owned(), "index".to_owned(), "packed-refs".to_owned()];
        if let Some(r) = git(dir, &["symbolic-ref", "-q", "HEAD"]) {
            names.push(r);
        }
        for n in names {
            if let Some(p) = git(dir, &["rev-parse", "--git-path", &n]) {
                let p = PathBuf::from(p);
                let p = if p.is_absolute() { p } else { dir.join(p) };
                // cargo runs a script again on every build while a path it watches is missing:
                // a branch that lives in packed-refs is watched by its folder, where a commit
                // writes it
                if p.exists() {
                    watch.push(p);
                } else if let Some(up) = p.parent().filter(|u| u.is_dir()) {
                    watch.push(up.to_path_buf());
                }
            }
        }
    }
    Source { commit, dirty, watch, top }
}

/// Now in UTC as `YYYY-MM-DDTHH:MM:SSZ`, or `SOURCE_DATE_EPOCH`'s time when that is set.
pub fn now() -> String {
    let secs = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or_else(|| SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0));
    iso(secs)
}

/// Seconds since 1970 as a UTC time, `YYYY-MM-DDTHH:MM:SSZ` (the civil-from-days algorithm).
pub fn iso(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// From a build script: stamp the crate being built with `dir`'s source, and watch `paths`
/// (relative to the repository's top, or absolute) beside git's own files.
pub fn emit(dir: &Path, paths: &[&str]) {
    let s = source(dir);
    println!("cargo:rustc-env=PLUMCRAFT_STAMP_COMMIT={}", s.commit.as_deref().unwrap_or("unknown"));
    println!(
        "cargo:rustc-env=PLUMCRAFT_STAMP_DIRTY={}",
        match s.dirty {
            Some(true) => "yes",
            Some(false) => "no",
            None => "unknown",
        }
    );
    println!("cargo:rustc-env=PLUMCRAFT_STAMP_DATE={}", now());
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    println!("cargo:rerun-if-changed=build.rs");
    for w in &s.watch {
        println!("cargo:rerun-if-changed={}", w.display());
    }
    if let Some(top) = &s.top {
        for p in paths {
            let p = Path::new(p);
            let p = if p.is_absolute() { p.to_path_buf() } else { top.join(p) };
            if p.exists() {
                println!("cargo:rerun-if-changed={}", p.display());
            }
        }
    }
}
