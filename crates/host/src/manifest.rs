//! An engine manifest (spec 16.6, `plumcraft_proto::manifest`) resolved against the machine: a
//! file read (bounded), `~` expanded, relative paths taken against the manifest's real directory
//! (so a link in `~/.config/plumcraft/engines/` points back at the project), and the program
//! found. A GUI app doesn't inherit the shell's PATH, so a bare program is looked up in
//! `$PLUMCRAFT_UV` (for `uv`), PATH, then `~/.local/bin`, `~/.cargo/bin` and `/opt/homebrew/bin`.

use std::path::{Path, PathBuf};

use plumcraft_proto::manifest::{EngineManifest, MAX_BYTES, Mode};
use plumcraft_proto::programs;

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
    /// What loaded but will very likely stop it starting ([`folder_warnings`]): shown with the
    /// manifests that couldn't be read, and its advice added to a failed start's reason.
    pub warnings: Vec<String>,
}

/// What to do about a manifest whose relative folder holds no project: the line a failed start
/// ends with ([`Resolved::failed`]).
pub const COPY_ADVICE: &str =
    "this manifest's folder has no pyproject.toml: it may be a copy; register the engine from its project (register --folder) or link the manifest instead";

impl Resolved {
    /// `reason`, why its engine didn't start, with [`COPY_ADVICE`] on a line of its own when
    /// loading it warned that its folder holds no project.
    pub fn failed(&self, reason: impl Into<String>) -> String {
        let reason = reason.into();
        if self.warnings.is_empty() { reason } else { format!("{reason}\n{COPY_ADVICE}") }
    }
}

/// `~` and `~/…` expanded (`~\…` too on Windows, where home is `USERPROFILE`).
fn expand(p: &str) -> PathBuf {
    let sep = |c: char| c == '/' || (cfg!(windows) && c == '\\');
    match (p.strip_prefix('~'), plumcraft_proto::programs::home()) {
        (Some(""), Some(home)) => home,
        (Some(rest), Some(home)) if rest.starts_with(sep) => home.join(rest.trim_start_matches(sep)),
        _ => PathBuf::from(p),
    }
}

/// `p` against the manifest's directory, unless it is absolute or starts at home.
fn against(dir: &Path, p: &str) -> PathBuf {
    let e = expand(p);
    if e.is_absolute() { e } else { dir.join(e) }
}

/// Where a bare program may be, in order, for an app that doesn't have the shell's PATH
/// ([`plumcraft_proto::programs::search_path`]: PATH, the login shell's, the usual folders).
fn search_path() -> Vec<PathBuf> {
    std::env::split_paths(plumcraft_proto::programs::search_path()).collect()
}

/// A file this user may run: on Unix an executable bit, on Windows a file at all (what runs is
/// decided by its extension, [`plumcraft_proto::programs::with_extensions`]).
fn executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        p.metadata().is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

/// The program `name` names: itself when it has a path separator (against `dir` when relative),
/// else found in `dirs` (on Windows with `PATHEXT`'s extensions). `None` when it isn't there.
fn find_program(name: &str, dir: &Path, dirs: &[PathBuf]) -> Option<PathBuf> {
    if name.contains('/') || (cfg!(windows) && name.contains('\\')) || name.starts_with('~') {
        let p = against(dir, name);
        return plumcraft_proto::programs::with_extensions(&p).into_iter().find(|p| executable(p));
    }
    if name == "uv"
        && let Some(uv) = std::env::var_os("PLUMCRAFT_UV").map(PathBuf::from).filter(|p| executable(p))
    {
        return Some(uv);
    }
    let names = plumcraft_proto::programs::with_extensions(Path::new(name));
    dirs.iter().find_map(|d| names.iter().map(|n| d.join(n)).find(|p| executable(p)))
}

/// Where the bare program `name` is, looked for as a manifest's `command` is: `$PLUMCRAFT_UV`
/// for `uv`, PATH, then the usual install folders (an app started from the desktop doesn't
/// have the shell's PATH).
pub fn program(name: &str) -> Option<PathBuf> {
    find_program(name, Path::new("/"), &search_path())
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
    let warnings = match &start {
        Start::Command(m) => folder_warnings(&manifest, m.cwd.as_deref().unwrap_or(&dir)),
        Start::Attach { .. } => Vec::new(),
    };
    Ok(Resolved { manifest, path: real, start, demo, warnings })
}

/// The value of option `name` in `args` (`--name VALUE` or `--name=VALUE`), up to a `--`.
fn option<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    let args = args.split(|a| a == "--").next().unwrap_or_default();
    args.iter().enumerate().find_map(|(i, a)| match a.strip_prefix(name) {
        Some("") => args.get(i + 1).map(String::as_str),
        Some(rest) => rest.strip_prefix('='),
        None => None,
    })
}

/// Does a project file `uv run --project` would find sit in `dir` or a folder above it? uv walks
/// up from the folder it is given, as it does from its working directory.
fn has_pyproject(dir: &Path) -> bool {
    dir.ancestors().any(|d| d.join("pyproject.toml").is_file())
}

/// Options of `uv run` that bring an environment of their own, or choose the interpreter, so a
/// folder with no project says nothing about whether the module is there.
const OWN_ENVIRONMENT: [&str; 8] = ["--with", "--script", "--gui-script", "--no-project", "--active", "--python", "--env-file", "--isolated"];

/// Environment variables that point uv or Python somewhere else than the folder's project.
fn points_elsewhere(name: &str) -> bool {
    ["VIRTUAL_ENV", "PYTHONPATH", "PYTHONHOME", "PYTHONUSERBASE"].contains(&name) || name.starts_with("UV_PROJECT") || name.starts_with("UV_PYTHON")
}

/// The module a `uv run … python -m <module>` runs: a `python`, `python3` or `python3.N` (`.exe`
/// too), then `-m` and the module, before any `--`.
fn python_module(args: &[String]) -> Option<&str> {
    let args = args.split(|a| a == "--").next().unwrap_or_default();
    let python = |a: &str| {
        let name = Path::new(a).file_stem().and_then(|s| s.to_str()).unwrap_or_default();
        name == "python"
            || name
                .strip_prefix("python3")
                .is_some_and(|v| v.is_empty() || v.strip_prefix('.').is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit())))
    };
    args.windows(3).find(|w| python(&w[0]) && w[1] == "-m").map(|w| w[2].as_str())
}

/// Why a manifest that loaded will very likely fail to start because the folder its relative
/// paths name isn't its project: a copy of a project's own manifest, which runs in the copy's
/// folder (a link is followed back to the project, a copy isn't). Judged only where the answer
/// is certain, for a `uv run` (uv walks up from the folder it starts from to find a project, as
/// it does from `--project`):
/// - with a relative `--project` (against `cwd`, or uv's `--directory`) that names a folder that
///   doesn't exist, or one with no `pyproject.toml` in it or above it, where uv runs a bare
///   interpreter that can't import the engine;
/// - with no `--project`, run from a relative `cwd` (and a relative `--directory`, if any) with
///   no `pyproject.toml` and no `.venv` in it or above it, when the command is `python -m
///   <module>`, nothing brings an environment of its own ([`OWN_ENVIRONMENT`]), and neither the
///   manifest's `env` nor this process's points uv or Python elsewhere ([`points_elsewhere`]).
///
/// Any other command can't be judged from outside (a plain `python -m` may have its package
/// installed), so says nothing.
pub fn folder_warnings(m: &EngineManifest, cwd: &Path) -> Vec<String> {
    let Some((program, args)) = m.command.split_first() else { return Vec::new() };
    let is_uv = Path::new(program).file_stem().is_some_and(|s| s == "uv");
    let runs = args.split(|a| a == "--").next().unwrap_or_default().iter().any(|a| a == "run");
    if !(is_uv && runs) {
        return Vec::new();
    }
    let Some(project) = option(args, "--project") else { return no_project_warnings(m, args, cwd) };
    let project = expand(project);
    if !from_here(&project) {
        return Vec::new();
    }
    let base = option(args, "--directory").map_or_else(|| cwd.to_path_buf(), |d| against(cwd, d));
    let dir = base.join(&project);
    let shown = project.display();
    match std::fs::canonicalize(&dir) {
        Err(_) => vec![format!("uv run --project {shown} names {}, which doesn't exist: {COPY_ADVICE}", programs::shown(&dir))],
        Ok(real) if !has_pyproject(&real) => {
            vec![format!("uv run --project {shown} names {}, with no pyproject.toml in it or above it: {COPY_ADVICE}", programs::shown(&real))]
        }
        Ok(_) => Vec::new(),
    }
}

/// Whether `p` is taken against the folder it is named from: relative, and on Windows without a root
/// either (`\\elsewhere` names the current drive's root, never the manifest's folder; B97).
fn from_here(p: &Path) -> bool {
    p.is_relative() && !p.has_root()
}

/// [`folder_warnings`] for a `uv run` with no `--project`: uv's own search from where it runs.
fn no_project_warnings(m: &EngineManifest, args: &[String], cwd: &Path) -> Vec<String> {
    let before: &[String] = args.split(|a| a == "--").next().unwrap_or_default();
    let own = before.iter().any(|a| OWN_ENVIRONMENT.iter().any(|o| a.starts_with(o)) || (a.starts_with("-p") && !a.starts_with("--")));
    let elsewhere = m.env.keys().any(|k| points_elsewhere(k)) || std::env::vars_os().any(|(k, _)| k.to_str().is_some_and(points_elsewhere));
    // relative paths only: an absolute cwd or --directory isn't the manifest's folder
    let relative = from_here(&expand(m.cwd.as_deref().unwrap_or("."))) && option(args, "--directory").is_none_or(|d| from_here(&expand(d)));
    let Some(module) = python_module(args).filter(|_| !own && !elsewhere && relative) else { return Vec::new() };
    let base = option(args, "--directory").map_or_else(|| cwd.to_path_buf(), |d| against(cwd, d));
    match std::fs::canonicalize(&base) {
        Err(_) => vec![format!("uv run (no --project) would run in {}, which doesn't exist: {COPY_ADVICE}", programs::shown(&base))],
        Ok(real) if !real.ancestors().any(|d| d.join("pyproject.toml").is_file() || d.join(".venv").is_dir()) => vec![format!(
            "uv run (no --project) runs in {}, with no pyproject.toml or .venv in it or above it, so uv runs a bare Python, very likely without {module}: {COPY_ADVICE}",
            programs::shown(&real)
        )],
        Ok(_) => Vec::new(),
    }
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
    let dirs = search_path();
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
    #[cfg(unix)]
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

    #[cfg(unix)]
    fn program(p: &Path) {
        write(p, "#!/bin/sh\n");
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[cfg(unix)]
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

    #[cfg(windows)]
    #[test]
    fn a_bare_program_is_found_with_its_extension() {
        let d = dir("found-win");
        write(&d.join("tool.exe"), "");
        assert_eq!(find_program("tool", &d, &[PathBuf::from(r"C:\nowhere"), d.clone()]), Some(d.join("tool.exe")));
        assert_eq!(find_program(r".\tool", &d, &[]), Some(d.join(r".\tool.exe")));
        assert_eq!(find_program("nothing", &d, std::slice::from_ref(&d)), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_bare_program_is_found_without_the_shells_path() {
        let d = dir("found");
        program(&d.join("tool"));
        assert_eq!(find_program("tool", &d, &[PathBuf::from("/nowhere"), d.clone()]), Some(d.join("tool")));
        assert_eq!(find_program("nothing", &d, std::slice::from_ref(&d)), None);
        // the shared search path (plumcraft_proto::programs): PATH first, then the usual folders
        let dirs = search_path();
        assert!(dirs.contains(&PathBuf::from("/opt/homebrew/bin")) && dirs.contains(&PathBuf::from("/usr/local/bin")), "{dirs:?}");
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

    fn parsed(command: &str) -> EngineManifest {
        EngineManifest::parse(&format!("id = \"w\"\ncommand = {command}\n")).unwrap()
    }

    /// A `uv run --project` whose relative folder holds no project warns, with the advice; one
    /// that finds a `pyproject.toml` (in the folder or above it, as uv walks up) doesn't, and
    /// neither does any command the rule can't judge.
    #[test]
    fn a_uv_project_folder_with_no_pyproject_warns() {
        let root = dir("folder-rule");
        let (copy, project) = (root.join("copy"), root.join("project"));
        std::fs::create_dir_all(project.join("sub")).unwrap();
        std::fs::create_dir_all(&copy).unwrap();
        assert!(!has_pyproject(&copy), "the scratch folder already has a project above it: the test can't tell");
        write(&project.join("pyproject.toml"), "[project]\nname = \"w\"\n");
        let uv = r#"["uv", "run", "--quiet", "--project", ".", "python", "-m", "w_engine"]"#;

        let w = folder_warnings(&parsed(uv), &copy);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(w[0].contains("no pyproject.toml in it or above it") && w[0].contains(COPY_ADVICE), "{}", w[0]);
        assert!(w[0].contains(&programs::shown(&copy)), "it names the folder: {}", w[0]);
        // as a person reads it (B95): the folder is canonical, and on Windows that spells it \\?\C:\…
        assert!(!w[0].contains(r"\\?\"), "{}", w[0]);
        assert_eq!(folder_warnings(&parsed(uv), &project), Vec::<String>::new());
        assert_eq!(folder_warnings(&parsed(uv), &project.join("sub")), Vec::<String>::new(), "uv walks up");
        // --project=DIR, uv.exe, and uv's --directory, which relative paths are taken against
        assert_eq!(folder_warnings(&parsed(r#"["/opt/uv.exe", "run", "--project=.", "python"]"#), &copy).len(), 1);
        assert_eq!(folder_warnings(&parsed(r#"["uv", "--directory", "../project", "run", "--project", ".", "python"]"#), &copy), Vec::<String>::new());
        let gone = folder_warnings(&parsed(r#"["uv", "run", "--project", "engine", "python"]"#), &copy);
        assert!(gone.len() == 1 && gone[0].contains("doesn't exist"), "{gone:?}");

        // what the rule can't judge says nothing
        for unjudged in [
            r#"["python", "-m", "w_engine"]"#,                                                     // may be installed
            r#"["uv", "sync", "--project", "."]"#,                                                 // not `uv run`
            r#"["uv", "run", "--project", "/nowhere/at/all", "python"]"#,                          // absolute: not the manifest's folder
            r#"["uv", "run", "--with", "w", "python", "-m", "w_engine", "--", "--project", "."]"#, // the engine's own argument
            r#"["./not-uv", "run", "--project", ".", "python"]"#,
        ] {
            assert_eq!(folder_warnings(&parsed(unjudged), &copy), Vec::<String>::new(), "{unjudged}");
        }
    }

    /// A `uv run` with no `--project` from a relative `cwd` is judged as uv searches from there:
    /// it warns where neither a `pyproject.toml` nor a `.venv` is in the folder or above it and the
    /// command is `python -m <module>`; anything that brings its own environment, an absolute
    /// folder, or an environment variable pointing elsewhere says nothing.
    #[test]
    fn a_uv_run_with_no_project_from_a_folder_with_none_warns() {
        let root = dir("no-project-rule");
        let (copy, project, venv) = (root.join("copy"), root.join("project"), root.join("venv"));
        for d in [&copy, &project.join("sub"), &venv.join(".venv")] {
            std::fs::create_dir_all(d).unwrap();
        }
        assert!(
            !copy.ancestors().any(|d| d.join("pyproject.toml").is_file() || d.join(".venv").is_dir()),
            "the scratch folder has a project or .venv above it: the test can't tell"
        );
        if std::env::vars_os().any(|(k, _)| k.to_str().is_some_and(points_elsewhere)) {
            eprintln!("skipped: this process's environment points uv or Python elsewhere, so the rule says nothing");
            return;
        }
        write(&project.join("pyproject.toml"), "[project]\nname = \"w\"\n");
        let uv = r#"["uv", "run", "--quiet", "python", "-m", "w_engine"]"#;
        let w = folder_warnings(&parsed(uv), &copy);
        assert_eq!(w.len(), 1, "{w:?}");
        assert!(
            w[0].contains("no --project") && w[0].contains("no pyproject.toml or .venv") && w[0].contains("w_engine") && w[0].contains(COPY_ADVICE),
            "{}",
            w[0]
        );
        assert_eq!(folder_warnings(&parsed(r#"["uv", "run", "python3.12", "-m", "w_engine"]"#), &copy).len(), 1, "python3.N");
        assert_eq!(folder_warnings(&parsed(uv), &project.join("sub")), Vec::<String>::new(), "a project above");
        assert_eq!(folder_warnings(&parsed(uv), &venv), Vec::<String>::new(), "a .venv there");
        assert!(folder_warnings(&parsed(uv), &root.join("gone"))[0].contains("doesn't exist"));
        let with = |extra: &str| EngineManifest::parse(&format!("id = \"w\"\ncommand = {uv}\n{extra}\n")).unwrap();
        for (unjudged, why) in [
            (parsed(r#"["uv", "run", "--with", "w-engine", "python", "-m", "w_engine"]"#), "its own environment"),
            (parsed(r#"["uv", "run", "--with=w-engine", "python", "-m", "w_engine"]"#), "its own environment"),
            (parsed(r#"["uv", "run", "-p", "3.12", "python", "-m", "w_engine"]"#), "an interpreter chosen"),
            (parsed(r#"["uv", "run", "--python=3.12", "python", "-m", "w_engine"]"#), "an interpreter chosen"),
            (parsed(r#"["uv", "run", "--no-project", "python", "-m", "w_engine"]"#), "asked for no project"),
            (parsed(r#"["uv", "run", "--script", "engine.py"]"#), "a script with its own dependencies"),
            (parsed(r#"["uv", "run", "w-engine"]"#), "not python -m"),
            (parsed(r#"["uv", "run", "pythonista", "-m", "w_engine"]"#), "not python"),
            (parsed(r#"["uv", "--directory", "/elsewhere", "run", "python", "-m", "w_engine"]"#), "an absolute folder"),
            (with("cwd = \"/elsewhere\""), "an absolute cwd"),
            (with("env = { VIRTUAL_ENV = \"/somewhere/.venv\" }"), "VIRTUAL_ENV"),
            (with("env = { PYTHONPATH = \"src\" }"), "PYTHONPATH"),
            (with("env = { UV_PROJECT_ENVIRONMENT = \"/x\" }"), "UV_PROJECT_ENVIRONMENT"),
        ] {
            assert_eq!(folder_warnings(&unjudged, &copy), Vec::<String>::new(), "{why}");
        }
    }

    /// The rule reads a manifest's command, which is hostile input: any arguments, however odd,
    /// give an answer and never panic.
    #[test]
    fn the_folder_rule_never_panics() {
        let cwd = dir("folder-rule-hostile");
        let words = [
            "uv",
            "uv.exe",
            "run",
            "--project",
            "--project=",
            "--directory",
            "--directory=",
            "-p",
            "-m",
            "--",
            "python",
            "python3.",
            "python3.x",
            "",
            "=",
            "~",
            "~/",
            "..",
            "/",
            "\u{0}",
            "é",
            "--with",
        ];
        let mut n = 0u64;
        for a in words {
            for b in words {
                for c in words {
                    for d in ["python", "-m", "--", ""] {
                        let mut m = parsed(r#"["uv"]"#);
                        m.command = vec![a.into(), b.into(), c.into(), d.into()];
                        let _ = folder_warnings(&m, &cwd);
                        m.command.truncate(2);
                        let _ = folder_warnings(&m, &cwd);
                        n += 1;
                    }
                }
            }
        }
        assert!(n > 40_000, "{n}");
    }

    /// The report's case: a project's manifest loads with no warning from the project and through
    /// a link, and a copy of it elsewhere warns; a failed start then ends with the advice.
    #[cfg(unix)]
    #[test]
    fn a_copy_of_a_projects_manifest_warns_and_a_link_does_not() {
        let root = dir("copy-link");
        let (project, config, copies) = (root.join("project"), root.join("config"), root.join("copies"));
        for d in [&project, &config, &copies] {
            std::fs::create_dir_all(d).unwrap();
        }
        let text = "id = \"w\"\ncommand = [\"./uv\", \"run\", \"--project\", \".\", \"python\", \"-m\", \"w_engine\"]\ncwd = \".\"\n";
        write(&project.join("pyproject.toml"), "[project]\nname = \"w\"\n");
        for d in [&project, &copies] {
            program(&d.join("uv"));
            write(&d.join("plumcraft-engine.toml"), text);
        }
        symlink(project.join("plumcraft-engine.toml"), config.join("w.toml")).unwrap();
        let own = load(&project.join("plumcraft-engine.toml")).unwrap();
        let linked = load(&config.join("w.toml")).unwrap();
        let copied = load(&copies.join("plumcraft-engine.toml")).unwrap();
        assert!(own.warnings.is_empty() && linked.warnings.is_empty(), "{:?} {:?}", own.warnings, linked.warnings);
        assert_eq!(copied.warnings.len(), 1, "{:?}", copied.warnings);
        assert_eq!(own.failed("exit 1"), "exit 1");
        assert_eq!(copied.failed("exit 1"), format!("exit 1\n{COPY_ADVICE}"));
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
