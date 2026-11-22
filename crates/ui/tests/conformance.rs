//! The shell conformance suite (`plumcraft_editor_core::conformance`, N2 slice 4) on the egui
//! editor, through its control dispatcher (`EditorApp::handle_control`, what its `--control`
//! port serves), with the window drawn between requests as eframe would: the canvas measures
//! its own faces and reports them every frame, so the cases aim at what egui measured.

use std::sync::Arc;
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use egui_kittest::Harness;
use plumcraft_editor_core::conformance::{self, Driver};
use plumcraft_engine::host::Host;
use plumcraft_host::{Connection, InProc};
use plumcraft_session::Session;
use plumcraft_ui::EditorApp;
use plumcraft_ui::link::Link;
use serde_json::{Value, json};

/// The egui editor, linked to an engine of pack `.1` in-process.
struct Egui(Harness<'static, EditorApp>, &'static str);

fn linked(make: fn() -> plumcraft_engine::Engine) -> Link {
    let host = Host::start(Box::new(move || Ok(make())), None);
    host.wait_ready(Duration::from_secs(20));
    let conn: Arc<dyn Connection> = Arc::new(InProc(host));
    Link::start(conn, "in-process", None).unwrap()
}

fn editor(make: fn() -> plumcraft_engine::Engine, pack: &'static str) -> Egui {
    // the suite's engines folder, read and offered as the application offers the person's own
    // (B71: Reload Engine List and Install Engine… are the core's)
    let launcher = plumcraft_editor_core::engines::Launcher { folder: Some(conformance::suite_engines_folder()), ..Default::default() };
    assert_eq!(launcher.load_manifests(), Vec::<String>::new());
    let mut app = EditorApp::new(Session::new(make().schema())).with_launcher(&launcher).with_link(linked(make));
    app.session.engines.choices = launcher.choices();
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_ui_state(|ui, app: &mut EditorApp| app.show(ui), app);
    h.run_steps(6);
    Egui(h, pack)
}

impl Driver for Egui {
    fn name(&self) -> String {
        "the egui editor".into()
    }
    fn call(&mut self, method: &str, params: Value) -> Value {
        let (tx, rx) = channel();
        let ctx = self.0.ctx.clone();
        self.0.state_mut().handle_control(
            &ctx,
            method,
            &params,
            Box::new(move |v| {
                let _ = tx.send(v);
            }),
        );
        let t0 = Instant::now();
        loop {
            if let Ok(v) = rx.try_recv() {
                return v;
            }
            if t0.elapsed() > Duration::from_secs(30) {
                return serde_json::json!({"ok": false, "error": format!("{method}: no reply in 30 s")});
            }
            self.0.step();
        }
    }
    /// egui measures the faces it draws.
    fn reports(&self) -> bool {
        false
    }
    fn report_geometry(&mut self, _faces: &Value) -> Result<(), String> {
        Ok(())
    }
    /// The engine it was linked to. Its control port has no `media.get`.
    fn has(&self, capability: &str) -> bool {
        capability == "manifests" || capability.strip_prefix("engine:") == Some(self.1)
    }
    /// A frame, and a moment for the engine's events to arrive.
    fn pause(&mut self) {
        self.0.step();
        std::thread::sleep(Duration::from_millis(5));
    }
    /// Draw until every face the scene has is measured (the canvas rebuilds the frame after an
    /// edit and measures the frame after that), or 5 s.
    fn settle(&mut self) {
        let t0 = Instant::now();
        loop {
            self.0.step();
            let app = self.0.state();
            let faces = app.scene_now().nodes;
            if faces.iter().all(|f| app.doc.geometry.get(&f.id).is_some()) || t0.elapsed() > Duration::from_secs(5) {
                break;
            }
        }
        self.0.step();
    }
}

#[test]
fn the_suite_passes_on_the_egui_editor() {
    let mut d = editor(plumcraft_text::engine, "text");
    let out = conformance::run_all(&mut d).unwrap();
    assert!(out.iter().all(|o| o.error.is_none()), "{}", conformance::summary(&d.name(), &out));
    assert!(out.len() >= 15);
    let skipped: Vec<&str> = out.iter().filter(|o| o.skipped.is_some()).map(|o| o.name.as_str()).collect();
    // the array engine's cases run on a core opened on it (below)
    let arrays: Vec<String> = conformance::cases()
        .unwrap()
        .into_iter()
        // and those for Install Engine…'s form (`install`: egui has its own Install window, not the core's form)
        // and the audio engine's (below, on the editor linked to it)
        .filter(|c| {
            c["needs"].as_array().is_some_and(|n| {
                n.contains(&serde_json::json!("engine:array"))
                    || n.contains(&serde_json::json!("install"))
                    || n.contains(&serde_json::json!("engine:audio"))
                    || n.contains(&serde_json::json!("engine:ports"))
            })
        })
        .filter_map(|c| c["name"].as_str().map(str::to_owned))
        .collect();
    assert!(arrays.len() >= 5);
    assert_eq!(skipped, arrays.iter().map(String::as_str).collect::<Vec<_>>(), "{}", conformance::summary(&d.name(), &out));
}

/// The cases for the array engine's values (a picture), on the editor linked to it.
#[test]
fn the_array_cases_pass_on_the_egui_editor() {
    let mut d = editor(plumcraft_array::engine, "array");
    let cases: Vec<Value> =
        conformance::cases().unwrap().into_iter().filter(|c| c["needs"].as_array().is_some_and(|n| n.contains(&json!("engine:array")))).collect();
    let out: Vec<conformance::Outcome> = cases.iter().map(|c| conformance::run_case(&mut d, c)).collect();
    assert!(!out.is_empty() && out.iter().all(|o| o.error.is_none() && o.skipped.is_none()), "{}", conformance::summary(&d.name(), &out));
}

/// The audio engine's case on the editor linked to it in process (the egui editor itself, being
/// published, starts the audio engine as a process: its SoundCraft crates are git dependencies).
#[test]
fn the_audio_case_passes_on_the_egui_editor() {
    let mut d = editor(plumcraft_audio::engine, "audio");
    let cases: Vec<Value> =
        conformance::cases().unwrap().into_iter().filter(|c| c["needs"].as_array().is_some_and(|n| n.contains(&json!("engine:audio")))).collect();
    assert!(!cases.is_empty());
    let out: Vec<conformance::Outcome> = cases.iter().map(|c| conformance::run_case(&mut d, c)).collect();
    assert!(out.iter().all(|o| o.error.is_none() && o.skipped.is_none()), "{}", conformance::summary(&d.name(), &out));
}

/// The test engine's cases (the port and graph rows), on the editor linked to it in process.
#[test]
fn the_ports_cases_pass_on_the_egui_editor() {
    let mut d = editor(plumcraft_ports::engine, "ports");
    let cases: Vec<Value> =
        conformance::cases().unwrap().into_iter().filter(|c| c["needs"].as_array().is_some_and(|n| n.contains(&json!("engine:ports")))).collect();
    assert!(cases.len() >= 3);
    let out: Vec<conformance::Outcome> = cases.iter().map(|c| conformance::run_case(&mut d, c)).collect();
    assert!(out.iter().all(|o| o.error.is_none() && o.skipped.is_none()), "{}", conformance::summary(&d.name(), &out));
}
