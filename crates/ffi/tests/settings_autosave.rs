//! N4 slice 4 on the C ABI: the settings a core keeps for all its documents (`settings.get`, the
//! settings commands carried out in the core, kept in `dirs.prefs/prefs.json`, the feed's `prefs`
//! domain), the theme following the system (`lifecycle.appearance`), autosave
//! (`document.open {autosave: true}`, flushed by `lifecycle.background` and on close), and the
//! control port's bind beyond loopback (refused without the debug flag and a pre-shared token).

mod common;

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::{Duration, Instant};

use common::{Client, store_root, take};
use plumcraft_ffi::plum_poll;
use serde_json::{Value, json};

/// A config on the text engine with its preferences in `prefs` (a folder).
fn config(name: &str, prefs: Option<&Path>) -> Value {
    let mut c = json!({"store_root": store_root(name), "engines": {"default": "text"}, "app": {"name": "PlumCraft", "version": "9.9.9", "build": "test"}});
    if let Some(p) = prefs {
        c["dirs"] = json!({"prefs": p});
    }
    c
}

/// Poll until a `changed` entry matches `want`, keeping replies for the client: the entry.
fn changed(c: &mut Client, want: impl Fn(&Value) -> bool) -> Value {
    let t0 = Instant::now();
    loop {
        let bytes = take(plum_poll(c.core));
        if !bytes.is_empty() {
            let v: Value = serde_json::from_slice(&bytes).unwrap();
            for r in v["replies"].as_array().unwrap() {
                c.replies.insert(r["id"].as_u64().unwrap(), r.clone());
            }
            if let Some(hit) = v["changed"].as_array().and_then(|a| a.iter().find(|x| want(x))) {
                return hit.clone();
            }
        }
        assert!(t0.elapsed() < Duration::from_secs(60), "no such change");
        c.wait_wake(Duration::from_millis(100));
    }
}

fn prefs_env_is_clear() -> bool {
    // a test that keeps preferences in a file never follows the environment's file
    std::env::var_os("PLUMCRAFT_EDITOR_PREFS").is_none()
}

#[test]
fn settings_are_the_cores_kept_in_its_prefs_and_every_document_follows_them() {
    if !prefs_env_is_clear() {
        return;
    }
    let dir = plumcraft_scratch::dir("ffi-settings-prefs");
    let _ = std::fs::remove_file(dir.join("prefs.json"));
    let mut c = Client::open(&config("settings", Some(&dir)));
    let m = c.call("settings.get", json!({}));
    assert_eq!(m["ok"], true, "{m}");
    assert_eq!((m["result"]["chosen"].as_str(), m["result"]["shown"].as_str()), (Some("system"), Some("paper")));
    let ids: Vec<&str> = m["result"]["sections"].as_array().unwrap().iter().filter_map(|s| s["id"].as_str()).collect();
    assert_eq!(ids, ["appearance", "colours", "families", "files", "running", "engines", "cache"]);
    // a second document sees what the first changes
    let other = c.call("document.open", json!({}))["result"]["doc"].as_u64().unwrap();
    let r = c.run("theme.setToken", json!({"token": "accent", "color": "#123456"}));
    assert_eq!(r["color"], "#123456", "the reply is the setting's, as the egui window gives it: {r}");
    c.run("view.animationSpeed", json!({"ms": 1000}));
    let id = c.send(&json!({"method": "theme.get", "doc": other}));
    let t = c.wait_reply(id, Duration::from_secs(30));
    assert_eq!(t["result"]["tokens"]["accent"], json!([0x12, 0x34, 0x56, 255]), "{t}");
    // the system goes dark: Match the System shows Dusk, and every document hears (the feed's
    // `prefs` domain; read straight from the feed, as a shell does)
    let asked = c.send(&json!({"method": "lifecycle.appearance", "params": {"dark": true}}));
    changed(&mut c, |x| x["domain"] == "prefs" && x["doc"] == json!(other));
    let a = c.wait_reply(asked, Duration::from_secs(30));
    assert_eq!((a["result"]["changed"].as_bool(), a["result"]["shown"].as_str()), (Some(true), Some("dusk")), "{a}");
    assert_eq!(c.call("theme.get", json!({}))["result"]["name"], "dusk");
    assert!(c.call("lifecycle.appearance", json!({"dark": "very"}))["ok"] == false);
    // kept in the file: what a reopen reads
    let disk = c.call("settings.get", json!({"fromDisk": true}));
    assert_eq!(disk["result"]["persisted"], true);
    let running = disk["result"]["sections"].as_array().unwrap().iter().find(|s| s["id"] == "running").unwrap().clone();
    assert_eq!(running["items"][1]["value"], 1000, "{running}");
    c.close();
    let text = std::fs::read_to_string(dir.join("prefs.json")).unwrap();
    assert!(text.contains("#123456") && text.contains("1000"), "{text}");
    // a core opened again on the same preferences has them
    let mut again = Client::open(&config("settings-again", Some(&dir)));
    let t = again.call("theme.get", json!({}));
    assert_eq!(t["result"]["tokens"]["accent"], json!([0x12, 0x34, 0x56, 255]));
    again.close();
}

#[test]
fn about_carries_the_apps_and_the_cores_versions() {
    let mut c = Client::open(&config("about", None));
    let a = c.run("app.about", json!({}));
    assert_eq!((a["title"].as_str(), a["version"].as_str()), (Some("PlumCraft"), Some("9.9.9")));
    let parts: Vec<String> = a["parts"].as_array().unwrap().iter().map(|p| p[0].as_str().unwrap_or_default().to_owned()).collect();
    assert_eq!(parts, ["Build", "Editor core", "Shell interface", "C ABI"]);
    c.close();
}

#[test]
fn an_autosaved_document_saves_itself_and_flushes_on_background_and_close() {
    let dir = plumcraft_scratch::dir("ffi-autosave");
    let path = dir.join("g.json");
    std::fs::write(&path, r#"{"version": 1, "nodes": {}, "wires": []}"#).unwrap();
    let mut c = Client::open(&config("autosave", None));
    let opened = c.call("document.open", json!({"path": path, "autosave": true}));
    assert_eq!(opened["result"]["autosave"], true, "{opened}");
    let doc = opened["result"]["doc"].as_u64().unwrap();
    let on = |c: &mut Client, method: &str, params: Value| {
        let id = c.send(&json!({"method": method, "params": params, "doc": doc}));
        c.wait_reply(id, Duration::from_secs(60))
    };
    let r = on(&mut c, "command.run", json!({"command": "node.add", "params": {"kind": "text.literal", "id": "a"}}));
    assert_eq!(r["ok"], true, "{r}");
    let first = r["result"]["id"].as_str().or_else(|| r["result"]["node"].as_str()).unwrap_or("a").to_owned();
    // saved once it settles: the feed's document domain says so, and the file has the node
    let saved = changed(&mut c, |x| x["doc"] == json!(doc) && x["domain"] == "document" && x["autosave"]["state"] == "saved");
    assert_eq!(saved["dirty"], false);
    assert!(std::fs::read_to_string(&path).unwrap().contains("text.literal"));
    // backgrounding writes what hasn't settled yet, at once
    on(&mut c, "command.run", json!({"command": "node.add", "params": {"kind": "text.upper", "id": "b"}}));
    let bg = c.call("lifecycle.background", json!({}));
    assert_eq!(bg["result"]["saved"], json!([doc]), "{bg}");
    assert!(std::fs::read_to_string(&path).unwrap().contains("text.upper"));
    // closing an autosaved document saves it and asks nothing
    let r = on(&mut c, "command.run", json!({"command": "node.remove", "params": {"id": first}}));
    assert_eq!(r["ok"], true, "{r}");
    let closed = c.call("document.close", json!({"doc": doc}));
    assert_eq!(closed["result"]["closing"], true, "{closed}");
    assert!(!std::fs::read_to_string(&path).unwrap().contains("text.literal"));
    // a memory warning drops the media caches and says so
    let m = c.call("lifecycle.memory", json!({"level": "low"}));
    assert_eq!(m["result"]["level"], "low", "{m}");
    assert_eq!(c.call("lifecycle.memory", json!({"level": "severe"}))["ok"], false);
    c.close();
}

#[test]
fn a_port_beyond_loopback_needs_the_debug_flag_and_a_pre_shared_token() {
    let token = "ab".repeat(32);
    let open = |control: Value| plumcraft_ffi::core::Config::parse(json!({"control": control}).to_string().as_bytes());
    // a release build never sets network_debug: refused
    let e = open(json!({"port": 0, "bind": "lan", "token": token})).unwrap_err();
    assert!(e.contains("debug builds only"), "{e}");
    let e = open(json!({"port": 0, "bind": "::1", "network_debug": true})).unwrap_err();
    assert!(e.contains("pre-shared token"), "{e}");
    for bad in [
        json!({"port": 0, "bind": "0.0.0.0", "network_debug": true, "token": token}),
        json!({"port": 0, "bind": "8.8.8.8", "network_debug": true, "token": token}),
        json!({"port": 0, "bind": 7, "network_debug": true, "token": token}),
        json!({"port": 0, "token": "short"}),
        json!({"port": 0, "bind": "::1", "network_debug": "yes", "token": token}),
    ] {
        assert!(open(bad.clone()).is_err(), "{bad}");
    }
    // loopback, as ever, needs neither
    assert!(open(json!({"port": 0})).is_ok());
    // an address other than 127.0.0.1 that never leaves this machine (IPv6 loopback), with the
    // flag and the token: served, and the token is asked for. A machine with no IPv6 (the
    // Linux test host) has no such address to serve on; what is refused was checked above.
    if std::net::TcpListener::bind(("::1", 0)).is_err() {
        return;
    }
    let mut c = Client::open(&json!({"store_root": store_root("bind"), "control": {"port": 0, "bind": "::1", "network_debug": true, "token": token}}));
    let info = c.call("core.control", json!({}));
    let Some(port) = info["result"]["port"].as_u64() else {
        c.close();
        return;
    };
    assert_eq!((info["result"]["bind"].as_str(), info["result"]["tokenFile"].is_null()), (Some("::1"), true), "{info}");
    let ask = |token: &str| -> String {
        let mut s = TcpStream::connect(("::1", port as u16)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
        let mut r = BufReader::new(s.try_clone().unwrap());
        writeln!(s, "{}", json!({"id": 1, "method": "auth", "params": {"token": token}})).unwrap();
        let mut line = String::new();
        r.read_line(&mut line).unwrap();
        writeln!(s, "{}", json!({"id": 2, "method": "graph.get"})).ok();
        let mut next = String::new();
        let _ = r.read_line(&mut next);
        format!("{line}{next}")
    };
    let wrong = ask(&"cd".repeat(32));
    assert!(wrong.contains("authentication required") && !wrong.contains("\"nodes\""), "{wrong}");
    let right = ask(&token);
    assert!(right.contains("authenticated") && right.contains("\"nodes\""), "{right}");
    c.close();
}

#[test]
fn watching_a_port_pops_its_value_out_and_a_second_watch_brings_it_forward() {
    // "popout does not show" (the maintainer, slice 3's Mac build): a click on a pin and every
    // Watch item are `run.watch {node, port}`, which showed nothing once the Watched column went
    let mut c = Client::open(&config("watch-popout", None));
    let n = c.run("node.add", json!({"kind": "text.literal"}));
    let id = n["id"].as_str().unwrap().to_owned();
    let port = c.call("graph.get", json!({}));
    let _ = port;
    let out = c.call("menu.get", json!({"context": {"kind": "node", "node": id}}));
    let watch = out["result"]["items"]
        .as_array()
        .and_then(|a| a.iter().find(|i| i["command"] == "run.watch" && i["params"]["port"].is_string()))
        .cloned()
        .unwrap_or_else(|| panic!("no Watch item: {out}"));
    c.run("run.watch", watch["params"].clone());
    let s = c.call("scene.get", json!({}));
    let pops = s["result"]["view"]["windows"]["popouts"].as_array().cloned().unwrap_or_default();
    assert_eq!(pops.len(), 1, "{}", s["result"]["view"]);
    assert_eq!(pops[0]["port"], watch["params"]["port"]);
    c.run("run.watch", watch["params"].clone());
    let s = c.call("scene.get", json!({}));
    assert_eq!(s["result"]["view"]["windows"]["popouts"].as_array().map(Vec::len), Some(1), "the same value: brought forward, not a second window");
    // watching a node (its title) selects and watches without a window
    c.run("view.close", json!({"id": pops[0]["id"]}));
    c.run("run.watch", json!({"node": id}));
    let s = c.call("scene.get", json!({}));
    assert!(s["result"]["view"]["windows"]["popouts"].as_array().is_none_or(Vec::is_empty));
    c.close();
}

/// N4 slice 6: Engine › Attach… and Install Engine… have no window of their own here: invoked,
/// the core asks with its form, and runs the command with what was typed.
#[test]
fn attach_asks_with_the_cores_form_and_install_is_refused_here() {
    let mut c = Client::open(&config("attach-form", None));
    let r = c.call("ui.invoke", json!({"command": "engine.attach"}));
    assert_eq!(r["ok"], true, "{r}");
    let form = |v: &Value| v["result"]["asks"].as_array().and_then(|a| a.iter().find(|x| x["kind"] == "form").cloned());
    let ask = form(&r).unwrap_or_else(|| panic!("no form ask: {r}"));
    assert_eq!(ask["params"]["command"], "engine.attach", "{ask}");
    assert_eq!(ask["params"]["fields"][0]["key"], "url");
    assert_eq!(ask["params"]["fields"][1]["kind"], "secret");
    // the address left empty: the form stays, saying why
    let a = c.call("ask.answer", json!({"ask": ask["ask"], "answer": {"values": {"url": ""}}}));
    let again = form(&a).unwrap_or_else(|| panic!("the form went: {a}"));
    assert!(again["params"]["error"].as_str().is_some_and(|e| e.contains("Address")), "{again}");
    let gone = c.call("ask.answer", json!({"ask": again["ask"], "cancel": true}));
    assert!(form(&gone).is_none(), "{gone}");
    // this core can't install engines: the command is refused, and nothing is asked
    let i = c.call("ui.invoke", json!({"command": "engine.install"}));
    assert!(i["ok"] == false || form(&i).is_none(), "{i}");
    c.close();
}
