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

/// Why a manifest that loaded will very likely fail to start because the folder its relative
/// paths name isn't its project: a copy of a project's own manifest, which runs in the copy's
/// folder (a link is followed back to the project, a copy isn't). Judged only where the answer
/// is certain: a `uv run` whose `--project` is relative (against `cwd`, or uv's `--directory`)
/// names a folder that doesn't exist, or one with no `pyproject.toml` in it or above it, where uv
/// would run a bare interpreter that can't import the engine. Any other command can't be judged
/// from outside (a plain `python -m` may have its package installed), so says nothing.
pub fn folder_warnings(m: &EngineManifest, cwd: &Path) -> Vec<String> {
    let Some((program, args)) = m.command.split_first() else { return Vec::new() };
    let is_uv = Path::new(program).file_stem().is_some_and(|s| s == "uv");
    let runs = args.split(|a| a == "--").next().unwrap_or_default().iter().any(|a| a == "run");
    let Some(project) = option(args, "--project").filter(|_| is_uv && runs).map(expand).filter(|p| p.is_relative()) else { return Vec::new() };
    let base = option(args, "--directory").map_or_else(|| cwd.to_path_buf(), |d| against(cwd, d));
    let dir = base.join(&project);
    let shown = project.display();
    match std::fs::canonicalize(&dir) {
        Err(_) => vec![format!("uv run --project {shown} names {}, which doesn't exist: {COPY_ADVICE}", dir.display())],
        Ok(real) if !has_pyproject(&real) => {
            vec![format!("uv run --project {shown} names {}, with no pyproject.toml in it or above it: {COPY_ADVICE}", real.display())]
        }
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
        assert!(w[0].contains(&copy.display().to_string()), "it names the folder: {}", w[0]);
        assert_eq!(folder_warnings(&parsed(uv), &project), Vec::<String>::new());
        assert_eq!(folder_warnings(&parsed(uv), &project.join("sub")), Vec::<String>::new(), "uv walks up");
        // --project=DIR, uv.exe, and uv's --directory, which relative paths are taken against
        assert_eq!(folder_warnings(&parsed(r#"["/opt/uv.exe", "run", "--project=.", "python"]"#), &copy).len(), 1);
        assert_eq!(folder_warnings(&parsed(r#"["uv", "--directory", "../project", "run", "--project", ".", "python"]"#), &copy), Vec::<String>::new());
        let gone = folder_warnings(&parsed(r#"["uv", "run", "--project", "engine", "python"]"#), &copy);
        assert!(gone.len() == 1 && gone[0].contains("doesn't exist"), "{gone:?}");

        // what the rule can't judge says nothing
        for unjudged in [
            r#"["python", "-m", "w_engine"]"#,                                      // may be installed
            r#"["uv", "run", "python", "-m", "w_engine"]"#,                         // no --project: uv's own search
            r#"["uv", "sync", "--project", "."]"#,                                  // not `uv run`
            r#"["uv", "run", "--project", "/nowhere/at/all", "python"]"#,           // absolute: not the manifest's folder
            r#"["uv", "run", "python", "-m", "w_engine", "--", "--project", "."]"#, // the engine's own argument
            r#"["./not-uv", "run", "--project", ".", "python"]"#,
        ] {
            assert_eq!(folder_warnings(&parsed(unjudged), &copy), Vec::<String>::new(), "{unjudged}");
        }
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
