//! `<engine> register` and `<engine> unregister`: offer an installed engine binary to PlumCraft,
//! as the Python SDK's do (`python/src/plumcraft_engine/register.py`).
//!
//! An engine installed as a binary (`cargo install`, a release archive) has nowhere PlumCraft
//! looks. `register` writes `~/.config/plumcraft/engines/<id>.toml` (the folder every editor
//! reads, `docs/interface.md` 16.6) that starts this very executable, with the arguments the
//! engine names; its demo, when it has one, goes beside it as `<id>.demo.json`. `unregister`
//! removes what `register` wrote, and nothing else: a checkout's link to its own manifest is left
//! alone unless `--force`.
//!
//! PlumCraft's own engines ([`Registration::BuiltIn`]) need none: the editor knows them, and a
//! manifest with their id would be refused.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// What `register` writes for an engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Registration {
    /// One of PlumCraft's own engines: nothing to register.
    BuiltIn,
    /// An engine PlumCraft doesn't know.
    Own(Own),
}

/// An engine's manifest fields.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Own {
    pub id: String,
    pub name: String,
    /// What follows the executable in `command` (`--craft /path/pack.toml`, say).
    pub args: Vec<String>,
    /// The demo workflow's JSON, written beside the manifest.
    pub demo: Option<String>,
}

const MARK: &str = "# Written by `";

fn usage(program: &str) -> String {
    format!(
        "usage: {program} register [--force] [--print] [--folder DIR]\n       {program} unregister [--folder DIR]\n\n  \
         register     offer this engine in every PlumCraft editor: write ~/.config/plumcraft/engines/<id>.toml,\n               \
         which starts this executable\n    --force    replace a manifest this command didn't write (a checkout's link)\n    \
         --print    print the manifest instead of writing it\n    --folder   another engines folder\n  \
         unregister   remove the manifest register wrote\n"
    )
}

fn folder(args: &[String]) -> Option<PathBuf> {
    if let Some(dir) = args.iter().position(|a| a == "--folder").and_then(|i| args.get(i + 1)) {
        return Some(PathBuf::from(dir));
    }
    plumcraft_proto::programs::home().map(|h| h.join(".config").join("plumcraft").join("engines"))
}

/// A TOML basic string (JSON's escapes are TOML's).
fn quoted(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

/// The manifest for `own`, started by `exe`.
pub fn manifest(own: &Own, program: &str, exe: &Path) -> String {
    let mut command = vec![quoted(&exe.to_string_lossy())];
    command.extend(own.args.iter().map(|a| quoted(a)));
    let name = if own.name.is_empty() { &own.id } else { &own.name };
    let mut lines = vec![
        format!("{MARK}{program} register` (plumcraft-engine {}): how PlumCraft starts the installed {name}.", env!("CARGO_PKG_VERSION")),
        format!("# `{program} unregister` removes it; run `{program} register` again after moving it."),
        format!("id = {}", quoted(&own.id)),
        format!("name = {}", quoted(name)),
        "mode = \"process\"".into(),
        format!("command = [{}]", command.join(", ")),
        "restart = \"on-crash\"".into(),
        "max_restarts = 5".into(),
        format!("hint = {}", quoted(&format!("reinstall it, then run `{program} register` again"))),
    ];
    if own.demo.is_some() {
        lines.push(format!("demo = {}", quoted(&format!("{}.demo.json", own.id))));
    }
    lines.join("\n") + "\n"
}

fn ours(path: &Path) -> bool {
    !path.is_symlink() && std::fs::read_to_string(path).is_ok_and(|t| t.lines().next().is_some_and(|l| l.starts_with(MARK) && l.contains(" register`")))
}

/// What a cache folder's `CACHEDIR.TAG` starts with (the Cache Directory Tagging spec, which uv
/// follows: it writes one at the root of its cache).
pub const CACHEDIR_SIGNATURE: &str = "Signature: 8a477f597d28d172789f06886806bc55";

/// A uv cache bucket's name (`archive-v0`, `builds-v0`, `environments-v2`), or one of its
/// temporary folders (`.tmpXXXX`).
fn bucket(name: &str) -> bool {
    name.starts_with(".tmp")
        || name.rsplit_once("-v").is_some_and(|(stem, v)| {
            !stem.is_empty() && stem.bytes().all(|b| b.is_ascii_lowercase() || b == b'-') && !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit())
        })
}

/// Where `exe` is when it is inside uv's cache (a binary a Python package carries, run with
/// `uvx`): the cache's root, and whether it is in a temporary environment there (a `.tmp…`
/// folder or `builds-v0`, which `uv run --with` removes when it ends). `None` otherwise. Found by
/// what the cache is, as the Python SDK's `register.uv_cache` does: a folder holding a
/// `CACHEDIR.TAG` whose child on the way down from it is a uv bucket, the folder holding `exe`
/// taken as a real path.
pub fn uv_cache(exe: &Path) -> Option<(PathBuf, bool)> {
    let here = std::fs::canonicalize(exe.parent()?).ok()?;
    let mut below: Vec<String> = Vec::new();
    for d in here.ancestors() {
        let tagged = std::fs::read_to_string(d.join("CACHEDIR.TAG")).is_ok_and(|t| t.starts_with(CACHEDIR_SIGNATURE));
        if tagged && below.last().is_some_and(|b| bucket(b)) {
            let temporary = below.last().is_some_and(|b| b == "builds-v0") || below.iter().any(|p| p.starts_with(".tmp"));
            return Some((d.to_path_buf(), temporary));
        }
        below.push(d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
    }
    None
}

fn register(own: &Own, args: &[String], program: &str) -> Result<String, String> {
    let exe = std::env::current_exe().and_then(std::fs::canonicalize).map_err(|e| format!("where am I? {e}"))?;
    let (text, warning) = register_as(own, args, program, &exe)?;
    if let Some(w) = warning {
        eprintln!("{program}: warning: {w}");
    }
    Ok(text)
}

/// [`register`] for the executable `exe`: what to print, and a warning for stderr.
fn register_as(own: &Own, args: &[String], program: &str, exe: &Path) -> Result<(String, Option<String>), String> {
    let text = manifest(own, program, exe);
    if args.iter().any(|a| a == "--print") {
        return Ok((text, None));
    }
    let warning = match uv_cache(exe) {
        Some((root, true)) => {
            return Err(format!(
                "not registered: this executable is in a temporary environment in uv's cache ({}), as `uv run --with` makes, which uv removes when that command ends, so the manifest could never start it. Install the engine (`uv tool install`, `cargo install`) and run register from the installed command",
                root.display()
            ));
        }
        Some((root, false)) => Some(format!(
            "this executable is in uv's cache ({}), as `uvx` and `uv tool run` make: a cache clean (`uv cache clean`, `uv cache prune`) removes it, and the engine stops starting. For an install that lasts, `uv tool install` (or `cargo install`) and run register from the installed command",
            root.display()
        )),
        None => None,
    };
    let dir = folder(args).ok_or("no HOME: give --folder")?;
    let target = dir.join(format!("{}.toml", own.id));
    if (target.exists() || target.is_symlink()) && !ours(&target) && !args.iter().any(|a| a == "--force") {
        let what = if target.is_symlink() { "a link (a checkout's own manifest)" } else { "a manifest this command didn't write" };
        return Err(format!("{} is {what}; leaving it. --force replaces it", target.display()));
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    if target.is_symlink() {
        std::fs::remove_file(&target).map_err(|e| format!("{}: {e}", target.display()))?;
    }
    let demo = dir.join(format!("{}.demo.json", own.id));
    match &own.demo {
        Some(d) => std::fs::write(&demo, d).map_err(|e| format!("{}: {e}", demo.display()))?,
        None => {
            let _ = std::fs::remove_file(&demo);
        }
    }
    let tmp = dir.join(format!("{}.toml.tmp", own.id));
    std::fs::write(&tmp, &text).and_then(|()| std::fs::rename(&tmp, &target)).map_err(|e| format!("{}: {e}", target.display()))?;
    Ok((format!("registered {}: {}\nA running editor offers it after Engine › Reload Engine List.\n", own.id, target.display()), warning))
}

fn unregister(own: &Own, args: &[String], program: &str) -> Result<String, String> {
    let dir = folder(args).ok_or("no HOME: give --folder")?;
    let target = dir.join(format!("{}.toml", own.id));
    if !(target.exists() || target.is_symlink()) {
        return Err(format!("{} isn't registered ({})", own.id, target.display()));
    }
    if !ours(&target) {
        return Err(format!("{} wasn't written by `{program} register`; leaving it", target.display()));
    }
    std::fs::remove_file(&target).map_err(|e| format!("{}: {e}", target.display()))?;
    let _ = std::fs::remove_file(dir.join(format!("{}.demo.json", own.id)));
    Ok(format!("unregistered {}\n", own.id))
}

/// Handle `register`, `unregister` and `--help` in `args` (the command line, the program first);
/// `None` for anything else, which the caller serves.
pub fn handle(reg: &Registration, program: &str, args: &[String]) -> Option<ExitCode> {
    // the verb may follow an engine's own options (`--craft PACK register`)
    let at = args.iter().skip(1).position(|a| matches!(a.as_str(), "register" | "unregister" | "-h" | "--help" | "help"))? + 1;
    let rest = args.get(at..).unwrap_or_default();
    let verb = rest.first().map(String::as_str)?;
    let out = match (verb, reg) {
        ("-h" | "--help" | "help", _) => Ok(usage(program)),
        ("register" | "unregister", Registration::BuiltIn) => Ok(format!("{program} is one of PlumCraft's own engines: every editor already offers it\n")),
        ("register", Registration::Own(own)) => register(own, &rest[1..], program),
        ("unregister", Registration::Own(own)) => unregister(own, &rest[1..], program),
        _ => return None,
    };
    Some(match out {
        Ok(text) => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{program}: {e}");
            ExitCode::FAILURE
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn own() -> Own {
        Own { id: "river".into(), name: "River gauges".into(), args: vec!["--pack".into(), "/a b/p.toml".into()], demo: Some("{\"nodes\": {}}".into()) }
    }

    fn args(v: &[&str], dir: &Path) -> Vec<String> {
        let mut a: Vec<String> = std::iter::once("river-engine").chain(v.iter().copied()).map(str::to_owned).collect();
        a.extend(["--folder".to_owned(), dir.to_string_lossy().into_owned()]);
        a
    }

    #[test]
    fn the_manifest_starts_this_executable_with_its_arguments() {
        let m = manifest(&own(), "river-engine", Path::new("/opt/bin/river-engine"));
        assert!(m.starts_with("# Written by `river-engine register`"), "{m}");
        assert!(m.contains(r#"command = ["/opt/bin/river-engine", "--pack", "/a b/p.toml"]"#), "{m}");
        assert!(m.contains(r#"demo = "river.demo.json""#) && m.contains(r#"name = "River gauges""#), "{m}");
    }

    #[test]
    fn register_and_unregister_write_and_remove_only_their_own() {
        let dir = plumcraft_scratch::dir("register").join("engines");
        let reg = Registration::Own(own());
        assert_eq!(handle(&reg, "river-engine", &args(&["register"], &dir)), Some(ExitCode::SUCCESS));
        let text = std::fs::read_to_string(dir.join("river.toml")).unwrap();
        assert!(text.contains("mode = \"process\""));
        assert_eq!(std::fs::read_to_string(dir.join("river.demo.json")).unwrap(), "{\"nodes\": {}}");
        // again: its own is replaced
        assert_eq!(handle(&reg, "river-engine", &args(&["register"], &dir)), Some(ExitCode::SUCCESS));
        assert_eq!(handle(&reg, "river-engine", &args(&["unregister"], &dir)), Some(ExitCode::SUCCESS));
        assert!(!dir.join("river.toml").exists() && !dir.join("river.demo.json").exists());
        assert_eq!(handle(&reg, "river-engine", &args(&["unregister"], &dir)), Some(ExitCode::FAILURE), "nothing there");
        // a checkout's link is left alone unless forced, and never unregistered
        let checkout = dir.parent().unwrap().join("plumcraft-engine.toml");
        std::fs::write(&checkout, "id = \"river\"\n").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&checkout, dir.join("river.toml")).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&checkout, dir.join("river.toml")).unwrap();
        assert_eq!(handle(&reg, "river-engine", &args(&["register"], &dir)), Some(ExitCode::FAILURE));
        assert_eq!(handle(&reg, "river-engine", &args(&["unregister"], &dir)), Some(ExitCode::FAILURE));
        assert!(dir.join("river.toml").is_symlink());
        assert_eq!(handle(&reg, "river-engine", &args(&["register", "--force"], &dir)), Some(ExitCode::SUCCESS));
        assert!(!dir.join("river.toml").is_symlink());
        assert_eq!(std::fs::read_to_string(&checkout).unwrap(), "id = \"river\"\n", "the checkout's own is untouched");
        // a hand-written manifest is not register's either
        std::fs::write(dir.join("river.toml"), "id = \"river\"\n").unwrap();
        assert_eq!(handle(&reg, "river-engine", &args(&["unregister"], &dir)), Some(ExitCode::FAILURE));
    }

    /// A folder laid out as uv's cache, and an executable stand-in at `rel/bin/engine` in it.
    fn in_uv_cache(root: &Path, rel: &str) -> PathBuf {
        let bin = root.join(rel).join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(root.join("CACHEDIR.TAG"), format!("{CACHEDIR_SIGNATURE}\n# created by uv\n")).unwrap();
        std::fs::write(bin.join("engine"), "").unwrap();
        bin.join("engine")
    }

    #[test]
    fn uvs_cache_is_found_by_its_tag_and_bucket() {
        let root = std::fs::canonicalize(plumcraft_scratch::dir("register-uv-cache")).unwrap();
        let cache = root.join("uv");
        assert_eq!(uv_cache(&in_uv_cache(&cache, "archive-v0/AbC")), Some((cache.clone(), false)), "uvx");
        assert_eq!(uv_cache(&in_uv_cache(&cache, "environments-v2/w-1")), Some((cache.clone(), false)));
        assert_eq!(uv_cache(&in_uv_cache(&cache, "builds-v0/.tmpQx")), Some((cache.clone(), true)), "uv run --with");
        assert_eq!(uv_cache(&in_uv_cache(&cache, ".tmpZz")), Some((cache.clone(), true)));
        assert_eq!(uv_cache(&in_uv_cache(&cache, "mine/x")), None, "not a bucket");
        let tools = root.join("tools/w/bin");
        std::fs::create_dir_all(&tools).unwrap();
        assert_eq!(uv_cache(&tools.join("engine")), None, "a tool install");
        std::fs::create_dir_all(root.join("other/archive-v0/x")).unwrap();
        std::fs::write(root.join("other/CACHEDIR.TAG"), "no signature").unwrap();
        assert_eq!(uv_cache(&root.join("other/archive-v0/x/engine")), None);
        assert_eq!(uv_cache(Path::new("")), None);
        assert_eq!(uv_cache(Path::new("/")), None);
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&cache, root.join("linked")).unwrap();
            assert_eq!(uv_cache(&root.join("linked/archive-v0/AbC/bin/engine")), Some((cache.clone(), false)), "through a link");
        }
    }

    #[test]
    fn register_from_uvs_cache_warns_and_from_a_temporary_one_refuses() {
        let root = std::fs::canonicalize(plumcraft_scratch::dir("register-from-cache")).unwrap();
        let dir = root.join("engines");
        let a = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).chain(["--folder".to_owned(), dir.to_string_lossy().into_owned()]).collect::<Vec<_>>();
        let lasting = in_uv_cache(&root.join("uv"), "archive-v0/AbC");
        let (said, warning) = register_as(&own(), &a(&[]), "river-engine", &lasting).unwrap();
        assert!(said.starts_with("registered river"), "{said}");
        assert!(warning.as_deref().is_some_and(|w| w.contains("uv's cache") && w.contains("uv cache clean")), "{warning:?}");
        assert!(std::fs::read_to_string(dir.join("river.toml")).unwrap().contains(&lasting.display().to_string()), "still written");
        std::fs::remove_file(dir.join("river.toml")).unwrap();
        let temporary = in_uv_cache(&root.join("uv"), "builds-v0/.tmpQx");
        let e = register_as(&own(), &a(&[]), "river-engine", &temporary).unwrap_err();
        assert!(e.starts_with("not registered") && e.contains("temporary environment"), "{e}");
        assert!(!dir.join("river.toml").exists(), "nothing written");
        // --print writes nothing, and says nothing about where it is
        assert_eq!(register_as(&own(), &a(&["--print"]), "river-engine", &temporary).unwrap().1, None);
        // elsewhere: no warning
        let plain = root.join("bin/engine");
        assert_eq!(register_as(&own(), &a(&[]), "river-engine", &plain).unwrap().1, None);
    }

    #[test]
    fn built_in_engines_say_so_and_anything_else_is_served() {
        let dir = plumcraft_scratch::dir("register-builtin");
        assert_eq!(handle(&Registration::BuiltIn, "plumcraft-text-engine", &args(&["register"], &dir)), Some(ExitCode::SUCCESS));
        assert!(std::fs::read_dir(&dir).map(|d| d.count()).unwrap_or(0) == 0, "nothing written");
        let serve = |a: &[&str]| a.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert_eq!(handle(&Registration::BuiltIn, "x", &serve(&["x"])), None);
        assert_eq!(handle(&Registration::BuiltIn, "x", &serve(&["x", "--listen", "127.0.0.1:0"])), None);
        assert_eq!(handle(&Registration::Own(own()), "x", &serve(&["x", "--connect", "unix:/s"])), None);
        // after an engine's own options
        let dir = plumcraft_scratch::dir("register-after");
        let a = serve(&["x", "--craft", "/p.toml", "register", "--folder", &dir.to_string_lossy()]);
        assert_eq!(handle(&Registration::Own(own()), "x", &a), Some(ExitCode::SUCCESS));
        assert!(dir.join("river.toml").exists());
    }
}
