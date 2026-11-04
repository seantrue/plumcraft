//! Engine › Install Engine… (`engine.install`): the window offers the public engines, the
//! install runs off the UI thread with the status bar saying so, and when it ends the engine
//! list is read again and the person is told. The installer is a stand-in; the app's own runs
//! `uv tool install` and `register` (apps/plumcraft-editor).

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui_kittest::kittest::Queryable;
use plumcraft_ui::EditorApp;
use serde_json::json;

fn app(result: Result<String, String>) -> (EditorApp, Arc<AtomicUsize>, Arc<Mutex<Vec<String>>>) {
    let reloads = Arc::new(AtomicUsize::new(0));
    let asked = Arc::new(Mutex::new(Vec::new()));
    let (r, a) = (Arc::clone(&reloads), Arc::clone(&asked));
    let app = EditorApp::new(common::session())
        .with_reloader(Box::new(move || {
            r.fetch_add(1, Ordering::SeqCst);
            (Vec::new(), Vec::new())
        }))
        .with_installer(Arc::new(move |url: &str| {
            a.lock().unwrap().push(url.to_owned());
            std::thread::sleep(Duration::from_millis(150));
            result.clone()
        }));
    (app, reloads, asked)
}

fn until_done(h: &mut egui_kittest::Harness<'static, EditorApp>) {
    let start = Instant::now();
    while h.state().installing.is_some() {
        assert!(start.elapsed() < Duration::from_secs(10), "the install never ended");
        std::thread::sleep(Duration::from_millis(10));
        h.step();
    }
    h.run_steps(2);
}

#[test]
fn the_window_installs_a_public_engine_in_the_background_then_reloads() {
    let (app, reloads, asked) = app(Ok("installed and registered duckdb-engine from https://github.com/seantrue/plumcraft-duckdb-engine".into()));
    let mut h = common::harness(app);
    h.state_mut().invoke("engine.install");
    h.run_steps(3);
    h.get_by_label("Install an Engine");
    h.get_by_label("DuckDB: data files into tables, reshaped with SQL").click();
    h.run_steps(2);
    h.get_by_label("Install").click();
    h.run_steps(2);
    // running: said in the status bar, and a second install is refused
    assert!(h.state().installing.is_some());
    h.get_by_label("installing seantrue/plumcraft-duckdb-engine…");
    assert!(!h.state().enabled("engine.install"));
    until_done(&mut h);
    assert_eq!(*asked.lock().unwrap(), ["https://github.com/seantrue/plumcraft-duckdb-engine"]);
    assert_eq!(reloads.load(Ordering::SeqCst), 1, "the engine list is read again");
    let said = h.state().last_error.clone().unwrap_or_default();
    assert!(said.contains("installed and registered duckdb-engine") && said.contains("Engine menu offers it now"), "{said}");
    assert!(h.state().enabled("engine.install") && h.query_by_label("Install an Engine").is_none());
}

#[test]
fn a_failed_install_says_why_and_reloads_nothing() {
    let (app, reloads, _) = app(Err("uv tool install failed: no such repository".into()));
    let mut h = common::harness(app);
    let ctx = h.ctx.clone();
    let r = h.state_mut().control_now(&ctx, "command.run", &json!({"command": "engine.install", "params": {"repo": "someone/nothing"}}));
    assert_eq!(r["result"]["installing"], "https://github.com/someone/nothing", "{r}");
    until_done(&mut h);
    assert_eq!(reloads.load(Ordering::SeqCst), 0);
    let said = h.state().last_error.clone().unwrap_or_default();
    assert!(said.contains("someone/nothing") && said.contains("no such repository"), "{said}");
    // not a GitHub repository: refused at once
    let r = h.state_mut().control_now(&ctx, "command.run", &json!({"command": "engine.install", "params": {"repo": "not a repo"}}));
    assert_eq!(r["ok"], false, "{r}");
}
