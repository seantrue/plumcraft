//! B151 on the C ABI: About says which build this is. `app.about`'s `build` carries this library's
//! stamp (`shell`), the application's when its library set one (`app::set_build`), and the native
//! program's from the config (`app.built`), with the line About shows; a stamp a shell sent that
//! isn't in form reads "unknown", and one that isn't an object is left out.

mod common;

use common::{Client, store_root};
use serde_json::{Value, json};

fn about(c: &mut Client) -> Value {
    c.run("app.about", json!({}))["build"].clone()
}

#[test]
fn about_says_which_build_this_is() {
    // the application's own stamp, set before the first core opens, as its entry would
    plumcraft_ffi::app::set_build(plumcraft_build_stamp::Stamp::from_env("03d315b", "yes", "2026-11-24T21:45:10Z"));
    let ui = json!({"commit": "22f5884", "dirty": false, "date": "2099-01-02T03:04:05Z"});
    let mut c = Client::open(&json!({"store_root": store_root("build-stamp"), "app": {"name": "Thing", "version": "1.0", "built": ui}}));
    let b = about(&mut c);
    // this library's stamp: a commit (or unknown), and a date in form
    let shell = &b["shell"];
    let commit = shell["commit"].as_str().unwrap();
    assert!(commit == "unknown" || (commit.len() >= 4 && commit.chars().all(|ch| ch.is_ascii_hexdigit())), "{b}");
    assert!(shell["dirty"].is_boolean() || shell["dirty"].is_null(), "{b}");
    assert_eq!(shell["date"].as_str().map(str::len), Some(20), "{b}");
    assert_eq!(b["app"], json!({"commit": "03d315b", "dirty": true, "date": "2026-11-24T21:45:10Z"}));
    assert_eq!(b["ui"], ui);
    // the line: the newest date (the native program's here), the shell, the app; the ui's
    // commit only where it differs from the shell's
    let line = b["line"].as_str().unwrap();
    assert!(line.starts_with("Built 2099-01-02 03:04 UTC · shell "), "{line}");
    assert!(line.contains(" · app 03d315b+changes"), "{line}");
    assert_eq!(line.contains(" · ui 22f5884"), commit != "22f5884" || shell["dirty"] != json!(false), "{line}");
    // nothing personal in it
    assert!(!line.contains('/') && !b.to_string().contains(std::env::var("HOME").unwrap_or_else(|_| "/nowhere-at-all".into()).as_str()), "{b}");
    c.close();

    // a native program's stamp that isn't in form: unknown; not an object: left out
    let mut c = Client::open(&json!({"store_root": store_root("build-stamp-bad"), "app": {"built": {"commit": "../../x", "dirty": "yes", "date": 7}}}));
    assert_eq!(about(&mut c)["ui"], json!({"commit": "unknown", "dirty": null, "date": "unknown"}));
    c.close();
    let mut c = Client::open(&json!({"store_root": store_root("build-stamp-none"), "app": {"built": "22f5884"}}));
    let b = about(&mut c);
    assert!(b["ui"].is_null() && b["line"].as_str().is_some_and(|l| !l.contains(" · ui ")), "{b}");
    c.close();
}
