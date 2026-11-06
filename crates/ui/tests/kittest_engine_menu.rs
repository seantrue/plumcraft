//! The Engine menu's list of engines (#24): each engine is a submenu of the ways it can be
//! hosted, unless Settings › Engines › Pick Engines Automatically is on, when choosing an
//! engine starts it the fastest way there is.

mod common;

use common::{click, harness};
use egui_kittest::kittest::Queryable;
use plumcraft_session::Session;
use plumcraft_session::engine_cmds::{EngineChoice, EngineTarget};
use plumcraft_ui::EditorApp;

fn choice(engine: &str, label: &str, mode: &str, mode_label: &str) -> EngineChoice {
    EngineChoice { engine: engine.into(), mode: mode.into(), engine_label: label.into(), mode_label: mode_label.into() }
}

/// Text hosted in a process or in-process (listed slowest first), Audio over HTTP only.
fn editor(auto: bool) -> EditorApp {
    let mut s = Session::default();
    s.engines.choices = vec![
        choice("text", "Text", "process", "Rust · separate process"),
        choice("text", "Text", "inproc", "Rust · in-process"),
        choice("audio", "Audio", "http", "Python · HTTP"),
    ];
    s.engines.auto = auto;
    EditorApp::new(s)
}

/// The engine the editor tried to start (the test has no host, so it doesn't connect).
fn tried(h: &egui_kittest::Harness<'static, EditorApp>) -> Option<EngineTarget> {
    h.state().session.engines.current.clone()
}

fn text(mode: &str) -> Option<EngineTarget> {
    Some(EngineTarget::Choice { engine: "text".into(), mode: mode.into() })
}

#[test]
fn an_engine_offers_its_hostings_when_not_picking_automatically() {
    let mut h = harness(editor(false));
    click(&mut h, "Engine");
    h.run_steps(2);
    click(&mut h, "Text ⏵");
    h.run_steps(3);
    h.get_by_label("Rust · in-process");
    click(&mut h, "Rust · separate process");
    h.run_steps(3);
    assert_eq!(tried(&h), text("process"), "the hosting picked");
}

#[test]
fn picking_automatically_an_engine_starts_the_fastest_way() {
    let mut h = harness(editor(true));
    click(&mut h, "Engine");
    h.run_steps(2);
    assert!(h.query_by_label("Text ⏵").is_none(), "Text is no submenu");
    click(&mut h, "Text");
    h.run_steps(3);
    assert!(h.query_by_label("Rust · separate process").is_none(), "no list of hostings");
    assert!(h.query_by_label("Rust · in-process").is_none(), "no list of hostings");
    assert_eq!(tried(&h), text("inproc"), "the fastest hosting");
}
