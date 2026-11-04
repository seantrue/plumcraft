//! Finding the programs PlumCraft starts (craft CLIs, engine processes, ffmpeg, uv), wherever
//! the editor was started from.
//!
//! An app opened from the Finder or the Dock doesn't get the shell's `PATH`: macOS gives it
//! `/usr/bin:/bin:/usr/sbin:/sbin`. Then `soundcraft-cli` in `~/bin` and ffmpeg in
//! `/opt/homebrew/bin` aren't found, though `which` finds them in a terminal (the installed
//! editor said "SoundCraft isn't installed"). [`search_path`] is the `PATH` the editor runs with,
//! then the login shell's (asked once, only when not started from a terminal), then the folders
//! programs are usually installed in. [`find_program`] looks there, and every program PlumCraft
//! starts is given it as its `PATH` ([`apply`]), so what it starts finds its own tools too.
//!
//! The process's own environment is never changed: setting it is `unsafe` in Rust 2024.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// How long the login shell may take to say its `PATH`.
const SHELL_WAIT: Duration = Duration::from_secs(3);
/// Brackets around the shell's answer, so what a profile prints is ignored.
const MARK: &str = "__plumcraft_path__";

/// Where programs are looked for: the `PATH` the editor runs with, then the login shell's when
/// it wasn't started from a terminal, then `~/bin`, `~/.local/bin`, `~/.cargo/bin`,
/// `/opt/homebrew/bin` and `/usr/local/bin`. Each folder once, in that order. Worked out once.
pub fn search_path() -> &'static OsString {
    static PATH: OnceLock<OsString> = OnceLock::new();
    PATH.get_or_init(|| {
        let own = std::env::var_os("PATH").unwrap_or_default();
        // A terminal's PATH is already the shell's; an app's is the system's (no TERM).
        let shell = if std::env::var_os("TERM").is_none() { login_shell_path() } else { None };
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let usual: Vec<PathBuf> = home
            .iter()
            .flat_map(|h| [h.join("bin"), h.join(".local/bin"), h.join(".cargo/bin")])
            .chain([PathBuf::from("/opt/homebrew/bin"), PathBuf::from("/usr/local/bin")])
            .collect();
        joined(&own, shell.as_deref(), &usual)
    })
}

/// `PATH` from `own`, then `shell`'s, then the `usual` folders, each once.
fn joined(own: &OsStr, shell: Option<&OsStr>, usual: &[PathBuf]) -> OsString {
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut add = |d: PathBuf| {
        if !d.as_os_str().is_empty() && !dirs.contains(&d) {
            dirs.push(d);
        }
    };
    std::env::split_paths(own).for_each(&mut add);
    shell.into_iter().flat_map(std::env::split_paths).for_each(&mut add);
    usual.iter().cloned().for_each(&mut add);
    std::env::join_paths(dirs).unwrap_or_else(|_| own.to_os_string())
}

/// The login shell's `PATH` (`$SHELL -l -c`), or none when there is no shell, it fails or it
/// takes longer than [`SHELL_WAIT`].
fn login_shell_path() -> Option<OsString> {
    let shell = std::env::var_os("SHELL").filter(|s| !s.is_empty())?;
    let script = format!("printf '%s%s%s' '{MARK}' \"$PATH\" '{MARK}'");
    let mut child = Command::new(shell).args(["-l", "-c", &script]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let deadline = Instant::now() + SHELL_WAIT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut out = String::new();
    std::io::Read::read_to_string(&mut child.stdout.take()?, &mut out).ok()?;
    between_marks(&out).map(OsString::from)
}

/// The text between the first two [`MARK`]s.
fn between_marks(out: &str) -> Option<&str> {
    let start = out.find(MARK)? + MARK.len();
    let len = out.get(start..)?.find(MARK)?;
    out.get(start..start + len).filter(|p| !p.is_empty())
}

/// The program `name` names: as given when it is a path, else the first file of that name in
/// [`search_path`].
pub fn find_program(name: impl AsRef<Path>) -> Option<PathBuf> {
    let name = name.as_ref();
    if name.components().count() > 1 {
        return name.is_file().then(|| name.to_path_buf());
    }
    std::env::split_paths(search_path()).map(|d| d.join(name)).find(|c| c.is_file())
}

/// Give `cmd` [`search_path`] as its `PATH`, so it is found and finds its own tools (`Command`
/// looks a bare program name up in the child's `PATH` when one is set).
pub fn apply(cmd: &mut Command) -> &mut Command {
    cmd.env("PATH", search_path())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_path_keeps_its_own_order_then_adds_the_shells_and_the_usual_folders_once() {
        let own = std::env::join_paths(["/usr/bin", "/bin"]).unwrap();
        let shell = std::env::join_paths(["/opt/homebrew/bin", "/usr/bin"]).unwrap();
        let usual = [PathBuf::from("/home/me/bin"), PathBuf::from("/opt/homebrew/bin")];
        let got: Vec<PathBuf> = std::env::split_paths(&joined(&own, Some(&shell), &usual)).collect();
        let want: Vec<PathBuf> = ["/usr/bin", "/bin", "/opt/homebrew/bin", "/home/me/bin"].iter().map(PathBuf::from).collect();
        assert_eq!(got, want);
    }

    #[test]
    fn only_the_marked_answer_counts() {
        assert_eq!(between_marks(&format!("Welcome!\n{MARK}/a:/b{MARK}")), Some("/a:/b"));
        assert_eq!(between_marks("no marks"), None);
        assert_eq!(between_marks(&format!("{MARK}{MARK}")), None, "an empty PATH is no answer");
        assert_eq!(between_marks(&format!("{MARK}/a")), None);
    }

    #[test]
    fn a_path_is_taken_as_it_is_and_a_name_is_looked_up() {
        assert_eq!(find_program("/bin/sh"), Some(PathBuf::from("/bin/sh")));
        assert_eq!(find_program("/no/such/program"), None);
        assert!(find_program("sh").is_some(), "sh is on every PATH");
        assert!(std::env::split_paths(search_path()).any(|d| d == Path::new("/usr/local/bin")));
    }
}
