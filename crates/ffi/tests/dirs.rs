//! B42 on the C ABI: the core writes only where the shell's `dirs` say, never under a folder it
//! derives from `HOME`. On a physical iPad an app may write inside its container's folders
//! (`Documents`, `Library`, `tmp`) but not at the container's root, which is `HOME`; the simulator
//! and the Mac allow it, so `value.export` writing under `HOME/.cache` passed everywhere but the
//! device. Here the test binary runs itself again with `HOME` a read-only folder and every
//! `dirs` root writable, as on the device, and takes a value out every way the apps do: export
//! (Drag Out and Share write the same file), Share's reply, the graph's Share, autosave (into the document's file and into the shell's scratch), and the open-value
//! cache's root.

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::Client;
use serde_json::{Value, json};

/// Set in the child: the folder holding the read-only home and the container's folders.
const CHILD: &str = "PLUMCRAFT_TEST_DIRS_CHILD";

fn inside(p: &Path, dir: &Path) -> bool {
    p.starts_with(dir)
}

/// Every file under `dir`.
fn files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() { todo.push(p) } else { out.push(p) }
        }
    }
    out
}

#[cfg(unix)]
#[test]
fn nothing_is_written_under_home_when_the_shell_gives_its_folders() {
    use std::os::unix::fs::PermissionsExt;
    if std::env::var_os(CHILD).is_some() {
        return; // the child runs the other test
    }
    let base = plumcraft_scratch::dir("ffi-dirs-readonly-home");
    let home = base.join("home");
    let container = base.join("container");
    for d in [&home, &container] {
        let _ = std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o755));
        let _ = std::fs::remove_dir_all(d);
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o555)).unwrap();
    // a read-only folder is writable to root: then this test can't tell, so say so and pass
    let probe = home.join(".probe");
    if std::fs::write(&probe, b"x").is_ok() {
        let _ = std::fs::remove_file(&probe);
        eprintln!("running as a user who can write a read-only folder: the check can't fail here");
        return;
    }
    let exe = std::env::current_exe().unwrap();
    let out = std::process::Command::new(exe)
        .args(["--exact", "child_takes_values_out_with_home_read_only", "--nocapture", "--test-threads", "1"])
        .env(CHILD, &base)
        .env("HOME", &home)
        .env_remove("XDG_CACHE_HOME")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("PLUMCRAFT_EDITOR_PREFS")
        .env_remove("PLUMCRAFT_CRAFT_ROOT")
        .env("COMMONKIT_STORE_ROOT", container.join("Library/Caches/commonkit/store"))
        .env("COMMONKIT_MARK_DIR", container.join("Library/Caches/plumcraft/marks"))
        .output()
        .unwrap();
    std::fs::set_permissions(&home, std::fs::Permissions::from_mode(0o755)).unwrap();
    let text = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "the child failed:\n{text}");
    assert!(text.contains("1 passed"), "the child test didn't run:\n{text}");
    assert!(files(&home).is_empty(), "written under HOME: {:?}", files(&home));
}

#[test]
fn child_takes_values_out_with_home_read_only() {
    let Some(base) = std::env::var_os(CHILD).map(PathBuf::from) else { return };
    let c = base.join("container");
    let dirs = [
        ("documents", c.join("Documents")),
        ("caches", c.join("Library/Caches/plumcraft")),
        ("scratch", c.join("tmp/plumcraft")),
        ("prefs", c.join("Library/Application Support/plumcraft")),
        ("logs", c.join("Library/Logs/plumcraft")),
    ];
    for (_, d) in &dirs {
        std::fs::create_dir_all(d).unwrap();
    }
    let dir = |k: &str| dirs.iter().find(|(n, _)| *n == k).map(|(_, d)| d.clone()).unwrap();
    let config = json!({
        "store_root": c.join("Library/Caches/commonkit/store"),
        "engines": {"default": "text"},
        "platform": {"os": "ios", "subprocess": false, "craft_apps": false, "python": false},
        "dirs": dirs.iter().map(|(k, d)| (k.to_string(), json!(d))).collect::<serde_json::Map<String, Value>>(),
    });
    let mut cl = Client::open(&config);
    // the open-value cache's root is the shell's caches
    let root = PathBuf::from(cl.call("ui.inspect", json!({}))["result"]["docs"]["root"].as_str().unwrap());
    assert!(inside(&root, &dir("caches")), "copies for apps go to {root:?}");
    // a value popped out, run
    let lit = cl.run("node.add", json!({"kind": "text.literal", "pos": [0, 0]}))["id"].as_str().unwrap().to_owned();
    cl.run("node.set", json!({"node": lit, "port": "text", "value": "corgi"}));
    let up = cl.run("node.add", json!({"kind": "text.upper", "pos": [260, 0]}))["id"].as_str().unwrap().to_owned();
    cl.run("wire.connect", json!({"from": {"node": lit, "port": "text"}, "to": {"node": up, "port": "text"}}));
    let p = cl.run("view.popout", json!({"node": up, "port": "text"}))["id"].as_u64().unwrap();
    cl.run("run.start", json!({}));
    let done = cl.call("ui.waitRun", json!({"timeout": 30}));
    assert_eq!(done["result"]["running"], false, "{done}");
    let t0 = Instant::now();
    let exported = loop {
        let r = cl.call("command.run", json!({"command": "value.export", "params": {"popout": p}}));
        if r["ok"] == true {
            break r["result"].clone();
        }
        let e = r["error"].as_str().unwrap_or_default().to_owned();
        assert!(e.contains("no value yet") || e.contains("run again"), "value.export: {r}");
        assert!(t0.elapsed() < Duration::from_secs(60), "the value never came: {r}");
        std::thread::sleep(Duration::from_millis(100));
    };
    // export (Drag Out writes the same file): under the shell's caches, with the value in it
    let file = PathBuf::from(exported["file"].as_str().unwrap());
    assert!(inside(&file, &dir("caches")), "exported to {file:?}");
    let pg = cl.call("popout.get", json!({"id": p}));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "CORGI", "{exported} {pg}");
    // the text engine tracks no lineage: no recipe, so a shell offers no Recipe (2026-11-22)
    assert_eq!(pg["result"]["hasRecipe"], false, "{pg}");
    // Share: the same file
    let shared = cl.run("value.share", json!({"popout": p}));
    assert!(inside(Path::new(shared["file"].as_str().unwrap()), &dir("caches")), "{shared}");
    // Save As writes through the engine to the path the shell's file.save ask gave (Documents on
    // the iPad): the compiled-in text engine saves TEXT through the SDK's defaults (2026-11-22)
    let saved_as = dir("documents").join("x.txt");
    let r = cl.call("command.run", json!({"command": "value.saveAs", "params": {"popout": p, "path": saved_as}}));
    assert!(r["ok"] == true, "{r}");
    let t0 = Instant::now();
    while std::fs::read_to_string(&saved_as).ok().as_deref() != Some("CORGI") {
        assert!(t0.elapsed() < Duration::from_secs(60), "the save never came: {}", cl.call("ui.inspect", json!({}))["result"]["save"]);
        std::thread::sleep(Duration::from_millis(100));
    }
    // the graph saved into Documents and shared
    let graph = dir("documents").join("corgi.json");
    cl.run("graph.saveAs", json!({"path": graph}));
    assert_eq!(cl.run("graph.share", json!({}))["file"], json!(graph));
    // autosave into the shell's scratch, then into the document's own file
    let a = cl.call("document.autosave", json!({"on": true, "scratch": "autosaved.json"}));
    assert_eq!(a["ok"], true, "{a}");
    let at = PathBuf::from(a["result"]["path"].as_str().unwrap());
    assert!(inside(&at, &dir("scratch")), "autosave's scratch file is {at:?}");
    cl.run("node.set", json!({"node": lit, "port": "text", "value": "two corgis"}));
    let t0 = Instant::now();
    while !std::fs::read_to_string(&at).is_ok_and(|t| t.contains("two corgis")) {
        assert!(t0.elapsed() < Duration::from_secs(30), "autosave never wrote {at:?}");
        cl.poll();
        std::thread::sleep(Duration::from_millis(200));
    }
    cl.close();
    println!("child: every way out wrote inside the container's folders");
}
