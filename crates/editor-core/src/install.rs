//! Engine › Install Engine…: a Python engine from GitHub, installed with `uv` and registered,
//! off the shell's thread, saying what it is doing and stopped when asked. Moved out of the
//! windowed editor and `plumcraft-ui`'s engine view (stage N1, slice 2); since B71 (2026-11-21)
//! the whole of it is here, reached by every shell through the same commands: what it runs
//! ([`install_engine`]), where it installs and registers ([`Setup`]), whether this platform may
//! install at all ([`refusal`]), its progress and cancellation ([`Progress`]). The Install window
//! itself is a shell's, or the core's form ask (`invoke::engine_form`).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::engines::Capabilities;
use crate::link::Waker;

/// Downloads an engine from GitHub (its https URL) and installs it, off the shell's thread,
/// saying what it is doing and stopping when asked through `progress`: what to tell the person,
/// or why it failed. [`Setup::installer`] is PlumCraft's; tests give stand-ins.
pub type Installer = Arc<dyn Fn(&str, &Progress) -> Result<String, String> + Send + Sync>;

/// Public engines the Install Engine… window offers, as `(label, owner/repo)`.
pub const KNOWN_ENGINES: &[(&str, &str)] = &[
    ("DuckDB: data files into tables, reshaped with SQL", "seantrue/plumcraft-duckdb-engine"),
    ("OpenCV: filters, edges, thresholds, morphology, geometry", "seantrue/plumcraft-opencv-engine"),
    ("Atlas: a sample engine with every type", "seantrue/plumcraft-atlas-engine"),
];

/// The longest one step of an install (`uv tool install`, a `register`) may take before it is
/// stopped: uv may build a Python and every wheel the engine needs on a slow network.
pub const STEP_LIMIT: Duration = Duration::from_secs(30 * 60);

/// What the person is told when an install was cancelled.
pub const CANCELLED: &str = "cancelled";

/// Why a platform with `caps` can't install engines, in words a person reads; `None` when it
/// can. Installing starts `uv` and the engine's own `register`, and the engine runs as a Python
/// process: it needs `subprocess` and `python` (`docs/shell-interface.md` 4.2).
pub fn refusal(caps: &Capabilities) -> Option<String> {
    if !caps.subprocess {
        Some("This device can't install engines: it can't start programs".into())
    } else if !caps.python {
        Some("This device can't install engines: it has no Python".into())
    } else {
        None
    }
}

/// What an install says it is doing, and whether it was asked to stop: shared by the install's
/// thread and the document that started it.
#[derive(Debug, Clone, Default)]
pub struct Progress {
    status: Arc<Mutex<Option<String>>>,
    cancel: Arc<AtomicBool>,
}

impl Progress {
    /// Say what the install is doing now.
    pub fn say(&self, text: impl Into<String>) {
        *self.status.lock().unwrap_or_else(PoisonError::into_inner) = Some(text.into());
    }

    /// What it last said.
    pub fn status(&self) -> Option<String> {
        self.status.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Ask it to stop.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    /// Was it asked to stop?
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

/// An install running in the background.
pub struct Installing {
    pub url: String,
    rx: Receiver<Result<String, String>>,
    pub progress: Progress,
}

impl Installing {
    /// Run `installer` on `url` on a thread of its own; `waker` is called when it ends.
    pub fn start(installer: Installer, url: String, waker: Option<Waker>) -> Result<Installing, String> {
        let (tx, rx) = std::sync::mpsc::channel();
        let u = url.clone();
        let progress = Progress::default();
        let p = progress.clone();
        std::thread::Builder::new()
            .name("plumcraft-install".into())
            .spawn(move || {
                let _ = tx.send(installer(&u, &p));
                if let Some(w) = waker {
                    w.wake();
                }
            })
            .map_err(|e| format!("can't start the install: {e}"))?;
        Ok(Installing { url, rx, progress })
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

/// How engines are installed here: the `uv` to use, the engines folder `register` writes to,
/// and what the programs it starts are given. The editor core's [`crate::engines::Launcher`]
/// makes one ([`crate::engines::Launcher::install_setup`]).
#[derive(Debug, Clone)]
pub struct Setup {
    /// The `uv` program; `None`: found as a manifest's `uv` is ([`find_uv`]).
    pub uv: Option<PathBuf>,
    /// The engines folder each engine's `register --folder` writes its manifest to: the one the
    /// editor reads ([`crate::engines::manifest_folder`] unless told otherwise).
    pub folder: Option<PathBuf>,
    /// Set on every program the install starts (a test's own folders).
    pub env: Vec<(String, PathBuf)>,
    /// The longest one step may take.
    pub limit: Duration,
}

impl Default for Setup {
    fn default() -> Self {
        Setup { uv: None, folder: None, env: Vec::new(), limit: STEP_LIMIT }
    }
}

impl Setup {
    /// This setup as an [`Installer`].
    pub fn installer(self) -> Installer {
        Arc::new(move |url: &str, progress: &Progress| install_engine(url, &self, progress))
    }
}

/// Where `uv` is: `given` when it names one, else `$PLUMCRAFT_UV`, PATH, the login shell's PATH,
/// then `~/bin`, `~/.local/bin` (uv's own installer puts it there), `~/.cargo/bin`,
/// `/opt/homebrew/bin` and `/usr/local/bin` ([`plumcraft_host::manifest::program`]): an app
/// started from the Finder doesn't have the shell's PATH. The error says where it looked.
pub fn find_uv(given: Option<&Path>) -> Result<PathBuf, String> {
    if let Some(p) = given {
        return if p.is_file() { Ok(p.to_path_buf()) } else { Err(format!("Installing an engine needs uv: there is none at {}", p.display())) };
    }
    plumcraft_host::manifest::program("uv").ok_or_else(|| {
        "Installing an engine needs uv (https://docs.astral.sh/uv), which wasn't found: not on PATH, the login shell's PATH, ~/bin, ~/.local/bin, \
         ~/.cargo/bin, /opt/homebrew/bin or /usr/local/bin. Install it, or set PLUMCRAFT_UV to it"
            .to_owned()
    })
}

/// The commands `uv tool install` says it installed ("Installed 2 executables: a, b").
pub fn installed_executables(said: &str) -> Vec<String> {
    said.lines()
        .filter_map(|l| l.trim().strip_prefix("Installed ")?.split_once(": ").map(|(_, names)| names.to_owned()))
        .flat_map(|names| names.split(", ").map(|n| n.trim().to_owned()).collect::<Vec<_>>())
        .filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c)))
        .collect()
}

/// A line a program printed, fit to show: no control characters, at most 160 characters.
fn shown_line(line: &str) -> String {
    let clean: String = line.chars().filter(|c| !c.is_control()).collect();
    let clean = clean.trim();
    if clean.chars().count() > 160 { format!("{}…", clean.chars().take(159).collect::<String>()) } else { clean.to_owned() }
}

/// How a step ended: its exit status and everything it printed (stdout, then stderr).
struct Ran {
    ok: bool,
    text: String,
}

impl Ran {
    /// Its last lines, for an error.
    fn tail(&self) -> String {
        let lines: Vec<&str> = self.text.lines().filter(|l| !l.trim().is_empty()).collect();
        lines[lines.len().saturating_sub(4)..].join("\n")
    }
}

/// Read `from` to its end on a thread of its own, saying each line as `stage (line)`.
fn reader(mut from: impl Read + Send + 'static, stage: String, progress: Progress) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut all = Vec::new();
        let mut buf = [0u8; 4096];
        let mut line = Vec::new();
        while let Ok(n) = from.read(&mut buf) {
            if n == 0 {
                break;
            }
            all.extend_from_slice(&buf[..n]);
            for b in &buf[..n] {
                if *b == b'\n' || *b == b'\r' {
                    let l = shown_line(&String::from_utf8_lossy(&line));
                    if !l.is_empty() {
                        progress.say(format!("{stage} ({l})"));
                    }
                    line.clear();
                } else {
                    line.push(*b);
                }
            }
        }
        String::from_utf8_lossy(&all).into_owned()
    })
}

/// Run `cmd` (stdin closed, in a process group of its own, with the search PATH and `setup`'s
/// environment), saying `stage` and what it prints, until it ends, it is cancelled, or
/// `setup.limit` passes; the last two stop it and all it started.
fn run(cmd: &mut Command, stage: &str, setup: &Setup, progress: &Progress) -> Result<Ran, String> {
    if progress.cancelled() {
        return Err(CANCELLED.into());
    }
    progress.say(stage);
    plumcraft_proto::programs::apply(cmd);
    plumcraft_proto::programs::own_group(cmd);
    for (k, v) in &setup.env {
        cmd.env(k, v);
    }
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("{stage}: can't start it: {e}"))?;
    let out = child.stdout.take().map(|o| reader(o, stage.to_owned(), progress.clone()));
    let err = child.stderr.take().map(|e| reader(e, stage.to_owned(), progress.clone()));
    let deadline = Instant::now() + setup.limit;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if progress.cancelled() => {
                plumcraft_proto::programs::kill_tree(&mut child);
                let _ = child.wait();
                return Err(CANCELLED.into());
            }
            Ok(None) if Instant::now() >= deadline => {
                plumcraft_proto::programs::kill_tree(&mut child);
                let _ = child.wait();
                let took = setup.limit.as_secs();
                let took = if took >= 120 { format!("{} minutes", took / 60) } else { format!("{:.1} s", setup.limit.as_secs_f64()) };
                return Err(format!("{stage}: stopped after {took}"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(format!("{stage}: {e}")),
        }
    };
    let text = [out, err].into_iter().flatten().map(|h| h.join().unwrap_or_default()).collect::<Vec<_>>().join("");
    Ok(Ran { ok: status.success(), text })
}

/// `uv tool install --reinstall git+URL`, then `register --folder <engines folder>` on each
/// command it installed, so the engine's manifest is in the folder the editor reads. Says each
/// step through `progress`, and stops (with [`CANCELLED`]) when it is cancelled.
pub fn install_engine(url: &str, setup: &Setup, progress: &Progress) -> Result<String, String> {
    let uv = find_uv(setup.uv.as_deref())?;
    let folder = setup.folder.clone().or_else(crate::engines::manifest_folder).ok_or("Installing an engine needs a home folder for the engines folder")?;
    let short = url.trim_start_matches("https://github.com/");
    let ran = run(
        Command::new(&uv).args(["tool", "install", "--reinstall"]).arg(format!("git+{url}")),
        &format!("Installing {short}: uv is downloading and building it"),
        setup,
        progress,
    )?;
    if !ran.ok {
        return Err(format!("uv tool install failed: {}", ran.tail()));
    }
    let exes = installed_executables(&ran.text);
    if exes.is_empty() {
        return Err(format!("uv installed it but named no command: {}", ran.tail()));
    }
    let bin = run(Command::new(&uv).args(["tool", "dir", "--bin"]), &format!("Installing {short}: finding its commands"), setup, progress)?;
    let bin = PathBuf::from(bin.text.lines().map(str::trim).rfind(|l| !l.is_empty()).unwrap_or_default());
    std::fs::create_dir_all(&folder).map_err(|e| format!("the engines folder {}: {e}", folder.display()))?;
    let mut registered = Vec::new();
    for exe in &exes {
        // `duckdb-engine.exe` on Windows
        let program = plumcraft_proto::programs::with_extensions(&bin.join(exe)).into_iter().find(|p| p.is_file()).unwrap_or_else(|| bin.join(exe));
        let ran = run(Command::new(program).arg("register").arg("--folder").arg(&folder), &format!("Installing {short}: registering {exe}"), setup, progress)?;
        if !ran.ok {
            return Err(format!("installed {exe}, but `{exe} register` failed: {}", ran.tail()));
        }
        registered.push(exe.as_str());
    }
    Ok(format!("Installed and registered {} from {url}", registered.join(", ")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    fn wait(i: &Installing) -> Result<String, String> {
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if let Some(r) = i.poll() {
                return r;
            }
            assert!(Instant::now() < deadline, "the install never ended");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn an_install_runs_off_the_callers_thread_and_wakes_it_when_done() {
        let woken = Arc::new(AtomicUsize::new(0));
        let w = Arc::clone(&woken);
        let installer: Installer = Arc::new(|url: &str, p: &Progress| {
            p.say("working");
            std::thread::sleep(Duration::from_millis(50));
            if url.contains("bad") { Err("no such repository".into()) } else { Ok(format!("installed {url}")) }
        });
        let i =
            Installing::start(Arc::clone(&installer), "https://github.com/a/b".into(), Some(Waker::new(move || _ = w.fetch_add(1, Ordering::SeqCst)))).unwrap();
        assert!(i.poll().is_none(), "still running");
        assert_eq!(wait(&i).as_deref(), Ok("installed https://github.com/a/b"));
        assert_eq!(i.progress.status().as_deref(), Some("working"));
        assert_eq!(woken.load(Ordering::SeqCst), 1);
        let bad = Installing::start(installer, "https://github.com/bad/one".into(), None).unwrap();
        assert!(wait(&bad).is_err());
    }

    #[test]
    fn only_a_platform_that_starts_programs_and_has_python_installs() {
        assert_eq!(refusal(&Capabilities::DESKTOP), None);
        assert_eq!(refusal(&Capabilities::NONE).as_deref(), Some("This device can't install engines: it can't start programs"));
        let no_python = Capabilities { python: false, ..Capabilities::DESKTOP };
        assert_eq!(refusal(&no_python).as_deref(), Some("This device can't install engines: it has no Python"));
    }

    #[test]
    fn a_uv_named_must_be_there_and_the_error_says_where_it_looked() {
        let gone = plumcraft_scratch::dir("install-no-uv").join("uv");
        assert!(find_uv(Some(&gone)).unwrap_err().contains("there is none at"));
        let e = install_engine("https://github.com/a/b", &Setup { uv: Some(gone), ..Setup::default() }, &Progress::default()).unwrap_err();
        assert!(e.starts_with("Installing an engine needs uv"), "{e}");
    }

    #[test]
    fn what_uv_says_it_installed_is_read_and_junk_is_not() {
        let said = "Resolved 3 packages\nInstalled 2 executables: duckdb-engine, other_one\nInstalled 1 executable: x; rm -rf ~\n";
        assert_eq!(installed_executables(said), ["duckdb-engine", "other_one"]);
        assert_eq!(shown_line("\u{1b}[1mBuilt\u{7} wheel  "), "[1mBuilt wheel");
        assert!(shown_line(&"x".repeat(500)).chars().count() <= 160);
    }

    /// A stand-in for uv (no network in tests): `tool install --reinstall git+URL` makes an
    /// engine command that `register --folder F` answers by writing `F/<name>.toml`; a URL with
    /// `slow` in it sleeps, one with `broken` fails as uv does; `tool dir --bin` names its bin.
    #[cfg(unix)]
    pub(crate) fn stub_uv(dir: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        std::fs::create_dir_all(dir).unwrap();
        let uv = dir.join("uv");
        let script = r#"#!/bin/sh
here="$(cd "$(dirname "$0")" && pwd)"
echo "$@" >> "$here/uv-args.txt"
case "$1 $2" in
  "tool dir") echo "$here/bin"; exit 0 ;;
  "tool install") ;;
  *) echo "stub uv: $*" >&2; exit 2 ;;
esac
url="$4"
case "$url" in
  *slow*) echo "Resolved 9 packages in 3ms" >&2; sleep 30; echo done > "$here/slow-finished"; exit 0 ;;
  *broken*) echo "error: Failed to fetch: $url" >&2; echo "  Caused by: repository not found" >&2; exit 2 ;;
esac
name="$(basename "$url")"
id="$(echo "$name" | sed 's/^plumcraft-//; s/-engine$//')"
mkdir -p "$here/bin"
cat > "$here/bin/$name" <<EOF
#!/bin/sh
[ "\$1" = register ] && [ "\$2" = --folder ] || { echo "usage: $name register --folder DIR" >&2; exit 2; }
echo "\$@" >> "$here/register-args.txt"
mkdir -p "\$3"
printf 'id = "%s"\nname = "Stub %s"\nmode = "http"\nurl = "http://127.0.0.1:9"\n' "$id" "$id" > "\$3/$id.toml"
echo "registered $id"
EOF
chmod +x "$here/bin/$name"
echo "Resolved 4 packages in 2ms" >&2
echo "Installed 1 executable: $name" >&2
"#;
        std::fs::write(&uv, script).unwrap();
        std::fs::set_permissions(&uv, std::fs::Permissions::from_mode(0o755)).unwrap();
        uv
    }

    #[cfg(unix)]
    #[test]
    fn an_install_runs_uv_then_register_into_the_engines_folder_saying_each_step() {
        let dir = plumcraft_scratch::dir("install-stub-uv");
        let _ = std::fs::remove_dir_all(&dir);
        let uv = stub_uv(&dir.join("uv"));
        let folder = dir.join("engines");
        let setup = Setup { uv: Some(uv), folder: Some(folder.clone()), ..Setup::default() };
        let p = Progress::default();
        let said = install_engine("https://github.com/someone/plumcraft-stub-engine", &setup, &p).unwrap();
        assert_eq!(said, "Installed and registered plumcraft-stub-engine from https://github.com/someone/plumcraft-stub-engine");
        let manifest = std::fs::read_to_string(folder.join("stub.toml")).unwrap();
        assert!(manifest.contains("id = \"stub\""), "{manifest}");
        let args = std::fs::read_to_string(dir.join("uv/uv-args.txt")).unwrap();
        assert!(args.contains("tool install --reinstall git+https://github.com/someone/plumcraft-stub-engine"), "{args}");
        let reg = std::fs::read_to_string(dir.join("uv/register-args.txt")).unwrap();
        assert_eq!(reg.trim(), format!("register --folder {}", folder.display()));
        // the last thing said was its register step
        assert!(p.status().is_some_and(|s| s.starts_with("Installing someone/plumcraft-stub-engine: registering")), "{:?}", p.status());

        // uv's failure: its last lines
        let e = install_engine("https://github.com/someone/broken", &setup, &Progress::default()).unwrap_err();
        assert!(e.starts_with("uv tool install failed") && e.contains("repository not found"), "{e}");
    }

    #[cfg(unix)]
    #[test]
    fn a_cancelled_install_stops_its_uv_at_once() {
        let dir = plumcraft_scratch::dir("install-stub-cancel");
        let _ = std::fs::remove_dir_all(&dir);
        let uv = stub_uv(&dir.join("uv"));
        let setup = Setup { uv: Some(uv), folder: Some(dir.join("engines")), ..Setup::default() };
        let i = Installing::start(setup.clone().installer(), "https://github.com/someone/slow-engine".into(), None).unwrap();
        // uv has started (it said what it resolved)
        let deadline = Instant::now() + Duration::from_secs(20);
        while !i.progress.status().is_some_and(|s| s.contains("Resolved")) {
            assert!(Instant::now() < deadline, "uv never started: {:?}", i.progress.status());
            std::thread::sleep(Duration::from_millis(10));
        }
        let t0 = Instant::now();
        i.progress.cancel();
        assert_eq!(wait(&i).unwrap_err(), CANCELLED);
        assert!(t0.elapsed() < Duration::from_secs(5), "{:?}", t0.elapsed());
        assert!(!dir.join("uv/slow-finished").exists());
        assert!(!dir.join("engines").exists(), "nothing registered");

        // a step that runs past its limit is stopped, saying so
        let short = Setup { limit: Duration::from_millis(300), ..setup };
        let e = install_engine("https://github.com/someone/slow-engine", &short, &Progress::default()).unwrap_err();
        assert!(e.contains("stopped after"), "{e}");
    }
}
