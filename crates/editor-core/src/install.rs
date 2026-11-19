//! Engine › Install Engine…: a Python engine from GitHub, installed with `uv` and registered,
//! off the shell's thread. Moved out of the windowed editor and `plumcraft-ui`'s engine view
//! (stage N1, slice 2); the Install window itself stays a shell's.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use crate::link::Waker;

/// Downloads an engine from GitHub (its https URL) and installs it, off the shell's thread: what
/// to tell the person, or why it failed. The application supplies it ([`install_engine`] is
/// PlumCraft's).
pub type Installer = Arc<dyn Fn(&str) -> Result<String, String> + Send + Sync>;

/// Public engines the Install Engine… window offers, as `(label, owner/repo)`.
pub const KNOWN_ENGINES: &[(&str, &str)] = &[
    ("DuckDB: data files into tables, reshaped with SQL", "seantrue/plumcraft-duckdb-engine"),
    ("OpenCV: filters, edges, thresholds, morphology, geometry", "seantrue/plumcraft-opencv-engine"),
    ("Atlas: a sample engine with every type", "seantrue/plumcraft-atlas-engine"),
];

/// An install running in the background.
pub struct Installing {
    pub url: String,
    rx: Receiver<Result<String, String>>,
}

impl Installing {
    /// Run `installer` on `url` on a thread of its own; `waker` is called when it ends.
    pub fn start(installer: Installer, url: String, waker: Option<Waker>) -> Result<Installing, String> {
        let (tx, rx) = std::sync::mpsc::channel();
        let u = url.clone();
        std::thread::Builder::new()
            .name("plumcraft-install".into())
            .spawn(move || {
                let _ = tx.send(installer(&u));
                if let Some(w) = waker {
                    w.wake();
                }
            })
            .map_err(|e| format!("can't start the install: {e}"))?;
        Ok(Installing { url, rx })
    }

    /// How it ended, once it has: `None` while it runs.
    pub fn poll(&self) -> Option<Result<String, String>> {
        match self.rx.try_recv() {
            Ok(r) => Some(r),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("the install stopped without saying why".into())),
        }
    }
}

/// The commands `uv tool install` says it installed ("Installed 2 executables: a, b").
pub fn installed_executables(said: &str) -> Vec<String> {
    said.lines()
        .filter_map(|l| l.trim().strip_prefix("Installed ")?.split_once(": ").map(|(_, names)| names.to_owned()))
        .flat_map(|names| names.split(", ").map(|n| n.trim().to_owned()).collect::<Vec<_>>())
        .filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)))
        .collect()
}

/// The last lines of a command's output, for an error.
fn tail(out: &std::process::Output) -> String {
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(4)..].join("\n")
}

/// `uv tool install --reinstall git+URL`, then `register` on each command it installed, so the
/// engine's manifest is in `~/.config/plumcraft/engines/`. `env` is set on both (a test's own tool
/// and config folders).
pub fn install_engine(url: &str, env: &[(&str, &std::path::Path)]) -> Result<String, String> {
    use std::process::Command;
    let uv = plumcraft_host::manifest::program("uv").ok_or("needs uv (https://docs.astral.sh/uv), or set PLUMCRAFT_UV")?;
    let run = |cmd: &mut Command| -> Result<std::process::Output, String> {
        for (k, v) in env {
            cmd.env(k, v);
        }
        cmd.output().map_err(|e| e.to_string())
    };
    let out = run(Command::new(&uv).args(["tool", "install", "--reinstall"]).arg(format!("git+{url}")))?;
    if !out.status.success() {
        return Err(format!("uv tool install failed: {}", tail(&out)));
    }
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let exes = installed_executables(&said);
    if exes.is_empty() {
        return Err(format!("uv installed it but named no command: {}", tail(&out)));
    }
    let bin = run(Command::new(&uv).args(["tool", "dir", "--bin"]))?;
    let bin = std::path::PathBuf::from(String::from_utf8_lossy(&bin.stdout).trim());
    let mut registered = Vec::new();
    for exe in &exes {
        let out = run(Command::new(bin.join(exe)).arg("register"))?;
        if !out.status.success() {
            return Err(format!("installed {exe}, but `{exe} register` failed: {}", tail(&out)));
        }
        registered.push(exe.as_str());
    }
    Ok(format!("installed and registered {} from {url}", registered.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    #[test]
    fn an_install_runs_off_the_callers_thread_and_wakes_it_when_done() {
        let woken = Arc::new(AtomicUsize::new(0));
        let w = Arc::clone(&woken);
        let installer: Installer = Arc::new(|url: &str| {
            std::thread::sleep(Duration::from_millis(50));
            if url.contains("bad") { Err("no such repository".into()) } else { Ok(format!("installed {url}")) }
        });
        let i =
            Installing::start(Arc::clone(&installer), "https://github.com/a/b".into(), Some(Waker::new(move || _ = w.fetch_add(1, Ordering::SeqCst)))).unwrap();
        assert!(i.poll().is_none(), "still running");
        let deadline = Instant::now() + Duration::from_secs(10);
        let said = loop {
            if let Some(r) = i.poll() {
                break r;
            }
            assert!(Instant::now() < deadline, "the install never ended");
            std::thread::sleep(Duration::from_millis(5));
        };
        assert_eq!(said.as_deref(), Ok("installed https://github.com/a/b"));
        assert_eq!(woken.load(Ordering::SeqCst), 1);
        let bad = Installing::start(installer, "https://github.com/bad/one".into(), None).unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        while bad.poll().is_none() {
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
