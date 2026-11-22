//! B71 on the C ABI: the native apps offer the engines the engines folder's manifests name (read
//! at start, and again by Reload Engine List or an install), and install engines where the
//! platform declares `subprocess` and `python`, through the same commands as the egui editor. The
//! install is the editor core's own (`plumcraft_editor_core::install`), run here with a stand-in
//! `uv` (`engines.uv`) in a scratch engines folder (`dirs.engines`): no network, and nothing of
//! the person's `~/.config/plumcraft` is read or written.

#![cfg(unix)]

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::{Client, store_root};
use serde_json::{Value, json};

/// A stand-in for uv: `tool install --reinstall git+URL` makes the engine's command, which
/// `register --folder F` answers by writing `F/<id>.toml` (an engine running elsewhere, so
/// nothing starts); a URL with `slow` sleeps; `tool dir --bin` names its bin folder.
fn stub_uv(dir: &Path) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir).unwrap();
    let uv = dir.join("uv");
    let script = r#"#!/bin/sh
here="$(cd "$(dirname "$0")" && pwd)"
case "$1 $2" in
  "tool dir") echo "$here/bin"; exit 0 ;;
  "tool install") ;;
  *) exit 2 ;;
esac
url="$4"
case "$url" in *slow*) echo "Resolved 9 packages" >&2; sleep 30; exit 0 ;; esac
name="$(basename "$url")"
id="$(echo "$name" | sed 's/^plumcraft-//; s/-engine$//')"
mkdir -p "$here/bin"
cat > "$here/bin/$name" <<EOF
#!/bin/sh
mkdir -p "\$3"
printf 'id = "%s"\nname = "Stub %s"\nmode = "http"\nurl = "http://127.0.0.1:9"\n' "$id" "$id" > "\$3/$id.toml"
EOF
chmod +x "$here/bin/$name"
echo "Installed 1 executable: $name" >&2
"#;
    std::fs::write(&uv, script).unwrap();
    std::fs::set_permissions(&uv, std::fs::Permissions::from_mode(0o755)).unwrap();
    uv
}

/// Poll until a `changed` entry of domain `document` matches `want` (one polled already while
/// waiting for a reply counts), taking it and those before it: the entry.
/// B87 on the C ABI: a manifest the core can't read is said at start: the first document's status
/// line (`lastError`, in the feed's document state and `ui.inspect`) and the engine's
/// `manifestProblems`, as a shell shows them, never skipped without a word.
#[test]
fn an_unreadable_manifest_is_said_to_the_first_document() {
    let dir = PathBuf::from(store_root("engines-unreadable"));
    let folder = dir.join("engines");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("broken.toml"), "id = \"person-broken\"\nmode = \"process\"\ncommand = [\"x\"]\n").unwrap();
    let mut c = Client::open(&json!({"store_root": dir.join("store"), "platform": {"os": "macos", "subprocess": true}, "dirs": {"engines": folder}}));
    let i = c.call("ui.inspect", json!({}));
    assert!(i["result"]["engine"]["manifestProblems"][0].as_str().is_some_and(|p| p.contains("broken.toml") && p.contains("lower-case")), "{i}");
    assert!(
        i["result"]["lastError"].as_str().is_some_and(|e| e.starts_with("engine manifests: ") && e.contains("broken.toml")),
        "{}",
        i["result"]["lastError"]
    );
    let st = document_changed(&mut c, |x| x["lastError"].as_str().is_some_and(|e| e.contains("broken.toml")));
    assert!(st["lastError"].as_str().unwrap().starts_with("engine manifests: "), "{st}");
    c.close();
}

fn document_changed(c: &mut Client, want: impl Fn(&Value) -> bool) -> Value {
    let t0 = Instant::now();
    loop {
        if let Some(i) = c.changed.iter().position(|x| x["domain"] == "document" && want(x)) {
            let hit = c.changed[i].clone();
            c.changed.drain(..=i);
            return hit;
        }
        assert!(t0.elapsed() < Duration::from_secs(60), "no such change: {:?}", c.changed);
        if !c.poll() {
            c.wait_wake(Duration::from_millis(100));
        }
    }
}

fn engines_of(c: &mut Client, doc: Option<u64>) -> Vec<String> {
    let mut p = json!({"method": "command.run", "params": {"command": "engine.list", "params": {}}});
    if let Some(d) = doc {
        p["doc"] = json!(d);
    }
    let id = c.send(&p);
    let r = c.wait_reply(id, Duration::from_secs(30));
    r["result"]["choices"].as_array().into_iter().flatten().filter_map(|x| x["engine"].as_str().map(str::to_owned)).collect()
}

#[test]
fn a_platform_that_can_offers_its_manifests_engines_and_installs_through_the_cores_form() {
    let dir = plumcraft_scratch::dir("ffi-engines-install");
    let _ = std::fs::remove_dir_all(&dir);
    let folder = dir.join("engines");
    std::fs::create_dir_all(&folder).unwrap();
    // an engine installed before (by the egui editor, say): offered from the start
    std::fs::write(folder.join("before.toml"), "id = \"before\"\nname = \"Installed Before\"\nmode = \"http\"\nurl = \"http://127.0.0.1:9\"\n").unwrap();
    let uv = stub_uv(&dir.join("uv"));
    let config = json!({"store_root": store_root("engines-install"), "engines": {"default": "text", "uv": uv},
                        "platform": {"os": "macos", "subprocess": true, "python": true}, "dirs": {"engines": folder}});
    let mut c = Client::open(&config);
    let l = c.run("engine.list", json!({}));
    assert_eq!((l["canInstall"].as_bool(), &l["installRefused"]), (Some(true), &Value::Null), "{l}");
    assert!(engines_of(&mut c, None).contains(&"before".to_owned()));
    let m = c.call("menu.get", json!({}));
    assert!(m["result"]["menus"].to_string().contains("Installed Before"), "{m}");
    // a second document, to see the install too
    let other = c.call("document.open", json!({}))["result"]["doc"].as_u64().unwrap();

    // Engine › Install Engine…: the core's form, then the install
    let r = c.call("ui.invoke", json!({"command": "engine.install"}));
    let ask = r["result"]["asks"].as_array().and_then(|a| a.iter().find(|x| x["kind"] == "form").cloned()).unwrap_or_else(|| panic!("no form: {r}"));
    assert_eq!(ask["params"]["command"], "engine.install");
    // decision 5 (N4 person gate): the repository is a pick list of the public engines, with
    // any other typed; the list is the core's
    let repo = &ask["params"]["fields"][0];
    assert_eq!((repo["key"].as_str(), repo["kind"].as_str(), repo["other"].as_bool()), (Some("repo"), Some("pick"), Some(true)), "{repo}");
    let values: Vec<&str> = repo["choices"].as_array().into_iter().flatten().filter_map(|c| c["value"].as_str()).collect();
    assert_eq!(values, plumcraft_editor_core::install::KNOWN_ENGINES.iter().map(|c| c.value).collect::<Vec<_>>());
    let a = c.call("ask.answer", json!({"ask": ask["ask"], "answer": {"values": {"repo": "someone/plumcraft-stub-engine"}}}));
    assert_eq!(a["ok"], true, "{a}");
    let done = document_changed(&mut c, |x| x["lastError"].as_str().is_some_and(|e| e.contains("Installed and registered")));
    assert!(done["lastError"].as_str().unwrap().contains("The Engine menu offers it now"), "{done}");
    assert_eq!(done["installing"], Value::Null, "{done}");
    assert!(folder.join("stub.toml").is_file(), "registered into the engines folder given");
    assert!(engines_of(&mut c, None).contains(&"stub".to_owned()));
    let m = c.call("menu.get", json!({}));
    assert!(m["result"]["menus"].to_string().contains("Stub stub"), "{m}");
    // the other document offers it too
    let t0 = Instant::now();
    while !engines_of(&mut c, Some(other)).contains(&"stub".to_owned()) {
        assert!(t0.elapsed() < Duration::from_secs(10), "the other document never offered it");
        std::thread::sleep(Duration::from_millis(50));
    }

    // a slow one, cancelled: said while it runs (the document's `installing`), then stopped
    c.run("engine.install", json!({"repo": "someone/slow-engine"}));
    let running = document_changed(&mut c, |x| x["installing"]["repo"].as_str().is_some_and(|r| r.contains("slow-engine")));
    assert!(running["installing"]["status"].as_str().is_some(), "{running}");
    assert_eq!(c.run("engine.list", json!({}))["installing"], "https://github.com/someone/slow-engine");
    let t0 = Instant::now();
    c.run("engine.install.cancel", json!({}));
    let cancelled = document_changed(&mut c, |x| x["lastError"].as_str().is_some_and(|e| e.contains("was cancelled")));
    assert!(t0.elapsed() < Duration::from_secs(10), "{:?}", t0.elapsed());
    assert_eq!(cancelled["installing"], Value::Null);
    let l = c.run("engine.list", json!({}));
    assert_eq!((l["installing"].clone(), l["canInstall"].as_bool()), (Value::Null, Some(true)), "{l}");
    c.close();
}

/// The iPad's platform: no programs, no Python. Install Engine… is listed, disabled by the core
/// with a reason a person reads, and a manifest's engine already running elsewhere is still
/// offered (attaching needs no program).
#[test]
fn a_platform_without_programs_offers_attached_manifest_engines_and_refuses_installs_saying_why() {
    let dir = plumcraft_scratch::dir("ffi-engines-ipad");
    let folder = dir.join("engines");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("far.toml"), "id = \"far\"\nname = \"Far Away\"\nmode = \"http\"\nurl = \"http://127.0.0.1:9\"\n").unwrap();
    let config = json!({"store_root": store_root("engines-ipad"), "engines": {"default": "text"},
                        "platform": {"os": "ios", "subprocess": false, "python": false, "craft_apps": false}, "dirs": {"engines": folder}});
    let mut c = Client::open(&config);
    let why = "This device can't install engines: it can't start programs";
    let l = c.run("engine.list", json!({}));
    assert_eq!((l["canInstall"].as_bool(), l["installRefused"].as_str()), (Some(false), Some(why)), "{l}");
    assert!(engines_of(&mut c, None).contains(&"far".to_owned()));
    let m = c.call("menu.get", json!({}));
    let menus = m["result"]["menus"].to_string();
    assert!(menus.contains("Far Away") && menus.contains(why), "{m}");
    let i = c.call("ui.invoke", json!({"command": "engine.install"}));
    assert!(i["result"]["asks"].as_array().is_none_or(|a| !a.iter().any(|x| x["kind"] == "form")), "no form: {i}");
    c.close();
}

/// Decision 17 on the iPad's platform: an engine whose manifest warned (a copy of a project's,
/// which starts a program) is listed in the Engine menu disabled with the platform's reason and
/// still carries its `warning`, which the iPad draws as the symbol and the subtitle.
#[test]
fn a_platform_without_programs_lists_a_warned_engine_disabled_and_marked() {
    let dir = plumcraft_scratch::dir("ffi-engines-ipad-warned");
    let folder = dir.join("engines");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(
        folder.join("copied.toml"),
        "id = \"copied\"\nname = \"Copied Engine\"\nmode = \"process\"\ncommand = [\"./uv\", \"run\", \"--project\", \".\", \"python\", \"-m\", \"copied_engine\"]\ncwd = \".\"\n",
    )
    .unwrap();
    // the program it names must be there for the manifest to load (it is never started here)
    std::fs::write(folder.join("uv"), "#!/bin/sh\nexit 1\n").unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(folder.join("uv"), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let config = json!({"store_root": store_root("engines-ipad-warned"), "engines": {"default": "text"},
                        "platform": {"os": "ios", "subprocess": false, "python": false, "craft_apps": false}, "dirs": {"engines": folder}});
    let mut c = Client::open(&config);
    let m = c.call("menu.get", json!({}));
    fn find(v: &Value) -> Option<Value> {
        match v {
            Value::Object(o) if o.get("label").and_then(Value::as_str).is_some_and(|l| l.starts_with("Copied Engine")) => Some(v.clone()),
            Value::Object(o) => o.values().find_map(find),
            Value::Array(a) => a.iter().find_map(find),
            _ => None,
        }
    }
    let eng = c.call("ui.inspect", json!({}))["result"]["engine"].clone();
    let item = find(&m["result"]).unwrap_or_else(|| panic!("the warned engine is listed: engine {eng}"));
    assert_eq!(item["enabled"], false, "{item}");
    assert!(item["reason"].as_str().is_some_and(|r| r.contains("can't start programs")), "{item}");
    assert!(item["warning"].as_str().is_some_and(|w| w.contains("may be a copy")), "{item}");
    c.close();
}

#[test]
fn a_config_names_its_engines_folder_manifests_and_uv_or_is_refused() {
    let bad = [
        json!({"dirs": {"engines": 3}}),
        json!({"engines": {"manifests": "one.toml"}}),
        json!({"engines": {"manifests": [1]}}),
        json!({"engines": {"uv": ""}}),
    ];
    for b in bad {
        let text = b.to_string();
        // SAFETY: the config is live for the call; no wake.
        let core = unsafe { plumcraft_ffi::plum_open(text.as_ptr(), text.len(), None, std::ptr::null_mut()) };
        assert_eq!(core, 0, "{b} opened");
    }
}
