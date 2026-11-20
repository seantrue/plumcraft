//! The shell conformance suite (`plumcraft_editor_core::conformance`, N2 slice 4) on the egui
//! editor, through its control dispatcher (`EditorApp::handle_control`, what its `--control`
//! port serves), with the window drawn between requests as eframe would: the canvas measures
//! its own faces and reports them every frame, so the cases aim at what egui measured.

use std::sync::mpsc::channel;
use std::time::{Duration, Instant};

use egui_kittest::Harness;
use plumcraft_editor_core::conformance::{self, Driver};
use plumcraft_session::Session;
use plumcraft_ui::EditorApp;
use serde_json::Value;

struct Egui(Harness<'static, EditorApp>);

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
    let app = EditorApp::new(Session::new(plumcraft_text::engine().schema()));
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 900.0)).build_ui_state(|ui, app: &mut EditorApp| app.show(ui), app);
    h.run_steps(6);
    let mut d = Egui(h);
    let out = conformance::run_all(&mut d).unwrap();
    assert!(out.iter().all(|o| o.error.is_none()), "{}", conformance::summary(&d.name(), &out));
    assert!(out.len() >= 15);
}
