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
    std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config/plumcraft/engines"))
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

fn register(own: &Own, args: &[String], program: &str) -> Result<String, String> {
    let exe = std::env::current_exe().and_then(std::fs::canonicalize).map_err(|e| format!("where am I? {e}"))?;
    let text = manifest(own, program, &exe);
    if args.iter().any(|a| a == "--print") {
        return Ok(text);
    }
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
    Ok(format!("registered {}: {}\nA running editor offers it after Engine › Reload Engine List.\n", own.id, target.display()))
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
