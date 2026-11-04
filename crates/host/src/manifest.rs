//! An engine manifest (spec 16.6, `plumcraft_proto::manifest`) resolved against the machine: a
//! file read (bounded), `~` expanded, relative paths taken against the manifest's real directory
//! (so a link in `~/.config/plumcraft/engines/` points back at the project), and the program
//! found. A GUI app doesn't inherit the shell's PATH, so a bare program is looked up in
//! `$PLUMCRAFT_UV` (for `uv`), PATH, then `~/.local/bin`, `~/.cargo/bin` and `/opt/homebrew/bin`.

use std::path::{Path, PathBuf};

use plumcraft_proto::manifest::{EngineManifest, MAX_BYTES, Mode};

use crate::process::Manifest;

/// The largest token file read.
const MAX_TOKEN: u64 = 4096;

/// How to reach a manifest's engine.
#[derive(Debug, Clone)]
pub enum Start {
    /// Start it: as a process on a socket (mode 2) or serving HTTP (mode 3, `HttpHost::spawn`).
    Command(Manifest),
    /// It is already running at `url`.
    Attach { url: String, token: String },
}

/// A manifest, resolved.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub manifest: EngineManifest,
    /// The file it was read from.
    pub path: PathBuf,
    pub start: Start,
    /// Its demo workflow, absolute, when it names one.
    pub demo: Option<PathBuf>,
}

/// `~` and `~/…` expanded.
fn expand(p: &str) -> PathBuf {
    match (p.strip_prefix('~'), std::env::var_os("HOME")) {
        (Some(""), Some(home)) => PathBuf::from(home),
        (Some(rest), Some(home)) if rest.starts_with('/') => PathBuf::from(home).join(rest.trim_start_matches('/')),
        _ => PathBuf::from(p),
    }
}

/// `p` against the manifest's directory, unless it is absolute or starts at home.
fn against(dir: &Path, p: &str) -> PathBuf {
    let e = expand(p);
    if e.is_absolute() { e } else { dir.join(e) }
}

/// Where a bare program may be, in order, for an app that doesn't have the shell's PATH.
fn search_path(path: Option<std::ffi::OsString>, home: Option<std::ffi::OsString>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = path.as_deref().map(|p| std::env::split_paths(p).collect()).unwrap_or_default();
    if let Some(h) = home {
        let h = PathBuf::from(h);
        dirs.extend([h.join(".local/bin"), h.join(".cargo/bin")]);
    }
    dirs.push(PathBuf::from("/opt/homebrew/bin"));
    dirs
}

fn executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// The program `name` names: itself when it has a `/` (against `dir` when relative), else found
/// in `dirs`. `None` when it isn't there.
fn find_program(name: &str, dir: &Path, dirs: &[PathBuf]) -> Option<PathBuf> {
    if name.contains('/') || name.starts_with('~') {
        return Some(against(dir, name)).filter(|p| executable(p));
    }
    if name == "uv"
        && let Some(uv) = std::env::var_os("PLUMCRAFT_UV").map(PathBuf::from).filter(|p| executable(p))
    {
        return Some(uv);
    }
    dirs.iter().map(|d| d.join(name)).find(|p| executable(p))
}

/// Where the bare program `name` is, looked for as a manifest's `command` is: `$PLUMCRAFT_UV`
/// for `uv`, PATH, then the usual install folders (an app started from the desktop doesn't
/// have the shell's PATH).
pub fn program(name: &str) -> Option<PathBuf> {
    find_program(name, Path::new("/"), &search_path(std::env::var_os("PATH"), std::env::var_os("HOME")))
}

/// Read and resolve the manifest at `path`.
pub fn load(path: &Path) -> Result<Resolved, String> {
    let shown = path.display();
    let len = std::fs::metadata(path).map_err(|e| format!("{shown}: {e}"))?.len();
    if len > MAX_BYTES as u64 {
        return Err(format!("{shown}: a manifest is at most {} KiB", MAX_BYTES >> 10));
    }
    let text = std::fs::read_to_string(path).map_err(|e| format!("{shown}: {e}"))?;
    let manifest = EngineManifest::parse(&text).map_err(|e| format!("{shown}: {e}"))?;
    // The file's real directory: the link in the config folder points back at the project.
    let real = std::fs::canonicalize(path).map_err(|e| format!("{shown}: {e}"))?;
    let dir = real.parent().unwrap_or(Path::new("/")).to_path_buf();
    let demo = manifest.demo.as_deref().map(|d| against(&dir, d));
    let start = start_of(&manifest, &dir).map_err(|e| format!("{shown}: {e}"))?;
    Ok(Resolved { manifest, path: real, start, demo })
}

fn start_of(m: &EngineManifest, dir: &Path) -> Result<Start, String> {
    let hint = m.hint.clone().unwrap_or_default();
    match m.mode {
        Mode::Inproc => return Err("mode inproc: an in-process engine is compiled in, not named by a file".into()),
        Mode::Pipekit => return Err("mode pipekit isn't supported yet".into()),
        Mode::Process | Mode::Http => {}
    }
    if m.command.is_empty() {
        let url = m.url.clone().unwrap_or_default();
        let token = match &m.token_file {
            Some(f) => read_token(&against(dir, f))?,
            None => String::new(),
        };
        return Ok(Start::Attach { url, token });
    }
    let dirs = search_path(std::env::var_os("PATH"), std::env::var_os("HOME"));
    let Some((name, args)) = m.command.split_first() else { return Err("no command".into()) };
    let program =
        find_program(name, dir, &dirs).ok_or_else(|| format!("{name} wasn't found{}", if hint.is_empty() { String::new() } else { format!(" ({hint})") }))?;
    let mut command = vec![program.display().to_string()];
    command.extend(args.iter().cloned());
    let env = m.env.iter().map(|(k, v)| (k.clone(), expand(v).display().to_string())).collect();
    let cwd = Some(against(dir, m.cwd.as_deref().unwrap_or(".")));
    Ok(Start::Command(Manifest { name: m.title().to_owned(), command, env, hint, store: None, cwd }))
}

fn read_token(f: &Path) -> Result<String, String> {
    let len = std::fs::metadata(f).map_err(|e| format!("token_file {}: {e}", f.display()))?.len();
    if len > MAX_TOKEN {
        return Err(format!("token_file {}: too long", f.display()));
    }
    Ok(std::fs::read_to_string(f).map_err(|e| format!("token_file {}: {e}", f.display()))?.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::{PermissionsExt, symlink};

    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = plumcraft_scratch::dir(&format!("manifest-{name}"));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::canonicalize(d).unwrap()
    }

    fn write(p: &Path, text: &str) {
        std::fs::write(p, text).unwrap();
    }

    fn program(p: &Path) {
        write(p, "#!/bin/sh\n");
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn a_link_resolves_against_the_real_project() {
        let (project, config) = (dir("project"), dir("config"));
        program(&project.join("engine"));
        write(&project.join("plumcraft-engine.toml"), "id = \"w\"\nname = \"W\"\ncommand = [\"./engine\", \"--go\"]\ndemo = \"examples/demo.json\"\n");
        symlink(project.join("plumcraft-engine.toml"), config.join("w.toml")).unwrap();
        let r = load(&config.join("w.toml")).unwrap();
        assert_eq!(r.demo, Some(project.join("examples/demo.json")));
        let Start::Command(m) = r.start else { panic!("a command") };
        assert_eq!(m.cwd, Some(project.join(".")));
        assert_eq!(m.command, vec![project.join("./engine").display().to_string(), "--go".to_owned()]);
        assert_eq!(m.name, "W");
    }

    #[test]
    fn a_bare_program_is_found_without_the_shells_path() {
        let d = dir("found");
        program(&d.join("tool"));
        assert_eq!(find_program("tool", &d, &[PathBuf::from("/nowhere"), d.clone()]), Some(d.join("tool")));
        assert_eq!(find_program("nothing", &d, std::slice::from_ref(&d)), None);
        let dirs = search_path(Some("/a:/b".into()), Some("/home/x".into()));
        assert_eq!(dirs, ["/a", "/b", "/home/x/.local/bin", "/home/x/.cargo/bin", "/opt/homebrew/bin"].map(PathBuf::from));
        write(&d.join("plain"), "not executable");
        assert_eq!(find_program("plain", &d, std::slice::from_ref(&d)), None);
    }

    #[test]
    fn a_missing_program_says_the_hint() {
        let d = dir("missing");
        write(&d.join("m.toml"), "id = \"w\"\ncommand = [\"no-such-program-here\"]\nhint = \"uv sync first\"\n");
        let e = load(&d.join("m.toml")).unwrap_err();
        assert!(e.contains("no-such-program-here wasn't found") && e.contains("uv sync first"), "{e}");
    }

    #[test]
    fn a_url_attaches_with_its_token_file() {
        let d = dir("attach");
        write(&d.join("token"), "secret\n");
        write(&d.join("m.toml"), "id = \"w\"\nmode = \"http\"\nurl = \"http://127.0.0.1:1\"\ntoken_file = \"token\"\n");
        let Start::Attach { url, token } = load(&d.join("m.toml")).unwrap().start else { panic!("attach") };
        assert_eq!((url.as_str(), token.as_str()), ("http://127.0.0.1:1", "secret"));
        write(&d.join("n.toml"), "id = \"w\"\nmode = \"http\"\nurl = \"http://127.0.0.1:1\"\ntoken_file = \"gone\"\n");
        assert!(load(&d.join("n.toml")).unwrap_err().contains("token_file"));
    }

    #[test]
    fn what_cant_be_loaded_says_why_and_which_file() {
        let d = dir("bad");
        assert!(load(&d.join("gone.toml")).unwrap_err().contains("gone.toml"));
        write(&d.join("big.toml"), &"#".repeat(MAX_BYTES + 1));
        assert!(load(&d.join("big.toml")).unwrap_err().contains("at most"));
        write(&d.join("id.toml"), "id = \"Bad\"\ncommand = [\"x\"]\n");
        let e = load(&d.join("id.toml")).unwrap_err();
        assert!(e.contains("id.toml") && e.contains("lower-case"), "{e}");
        write(&d.join("p.toml"), "id = \"w\"\nmode = \"pipekit\"\n");
        assert!(load(&d.join("p.toml")).unwrap_err().contains("pipekit"));
    }
}
