//! An engine's status line (plumcraft #86): `texts.engine_status` drawn in the status bar under
//! the engine's name while the engine is connected, in its tone's colour, cut to a width cap with
//! the whole text on hover; `ui.inspect`'s `engineStatus`. And the bar's two-line cells (layout
//! B, the maintainer's "D17 B"): each cell's detail is a smaller line under its main line, so a
//! narrow window still shows the run and its Cancel button.

mod common;

use common::{add, catalog};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use plumcraft_proto::catalogue::texts;
use plumcraft_proto::session::EngineState;
use plumcraft_session::Session;
use plumcraft_ui::EditorApp;
use plumcraft_ui::status_cell::ENGINE_STATUS_WIDTH;
use serde_json::{Value, json};

const STATUS: &str = "GPU: Metal, Test GPU (Metal didn't open)";

fn connected() -> EngineState {
    EngineState::Connected { version: "2".into(), nodes: 3, caps: Default::default() }
}

/// An editor whose catalogue says `status` (and `tone`), connected unless `engine` says not.
fn editor(status: Option<&str>, tone: Option<&str>, engine: EngineState) -> (EditorApp, String) {
    let mut c = catalog();
    if let Some(s) = status {
        c.texts.insert(texts::ENGINE_STATUS.into(), s.into());
    }
    if let Some(t) = tone {
        c.texts.insert(texts::ENGINE_STATUS_TONE.into(), t.into());
    }
    let mut s = Session::new(c);
    let n = add(&mut s, "test.knobs", 40.0, 40.0);
    let mut app = EditorApp::new(s);
    app.engine = engine;
    (app, n)
}

fn harness_at(app: EditorApp, width: f32) -> Harness<'static, EditorApp> {
    let mut h = Harness::builder().with_size(egui::vec2(width, 700.0)).build_ui_state(|ui, app: &mut EditorApp| app.show(ui), app);
    h.run_steps(6);
    h
}

fn inspect(h: &mut Harness<'static, EditorApp>) -> Value {
    let ctx = h.ctx.clone();
    h.state_mut().control_now(&ctx, "ui.inspect", &json!({}))["result"].clone()
}

/// The colour and on-screen rect of the text drawn for `text` (the whole text, even when it was
/// cut short on screen).
fn drawn(h: &Harness<'static, EditorApp>, text: &str) -> Option<(egui::Color32, egui::Rect)> {
    fn find(shape: &egui::Shape, text: &str) -> Option<(egui::Color32, egui::Rect)> {
        match shape {
            egui::Shape::Text(t) if t.galley.text() == text => {
                let colour = t.galley.job.sections.first().map_or(t.fallback_color, |s| s.format.color);
                Some((colour, t.visual_bounding_rect()))
            }
            egui::Shape::Vec(v) => v.iter().find_map(|s| find(s, text)),
            _ => None,
        }
    }
    h.output().shapes.iter().find_map(|c| find(&c.shape, text))
}

/// Start a run that stays running (no engine answers it).
fn start(h: &mut Harness<'static, EditorApp>) {
    h.state_mut().session.connected = true;
    h.state_mut().session.execute("run.start", &json!({})).unwrap();
    let _ = h.state_mut().session.run.take_outbox();
    h.run_steps(3);
}

#[test]
fn an_engines_status_is_in_the_status_bar_and_in_ui_inspect() {
    let (app, _) = editor(Some(STATUS), Some("warn"), connected());
    let mut h = harness_at(app, 1400.0);
    h.get_by_label(STATUS);
    let i = inspect(&mut h);
    assert_eq!(i["engineStatus"], json!({"text": STATUS, "tone": "warn"}));
    // drawn in the theme's warning colour
    let warning = h.state().tokens.warning;
    assert_eq!(drawn(&h, STATUS).map(|d| d.0), Some(warning));
}

#[test]
fn the_tone_is_info_unless_it_says_warn_or_error() {
    for (tone, want) in [(None, "info"), (Some("info"), "info"), (Some("error"), "error"), (Some("loud"), "info"), (Some(""), "info")] {
        let (app, _) = editor(Some(STATUS), tone, connected());
        let mut h = harness_at(app, 1400.0);
        assert_eq!(inspect(&mut h)["engineStatus"]["tone"], want, "{tone:?}");
        let t = &h.state().tokens;
        let colour = match want {
            "error" => t.error,
            _ => t.text_dim,
        };
        assert_eq!(drawn(&h, STATUS).map(|d| d.0), Some(colour), "{tone:?}");
    }
}

#[test]
fn without_the_text_or_the_engine_nothing_new_is_shown() {
    // no text: nothing, and engineStatus is null
    let (app, _) = editor(None, Some("warn"), connected());
    let mut h = harness_at(app, 1400.0);
    assert!(inspect(&mut h)["engineStatus"].is_null());
    // an empty text is none
    let (app, _) = editor(Some(""), None, connected());
    let mut h = harness_at(app, 1400.0);
    assert!(inspect(&mut h)["engineStatus"].is_null());
    // not connected: the catalogue's text isn't shown
    let (app, _) = editor(Some(STATUS), None, EngineState::Off);
    let mut h = harness_at(app, 1400.0);
    assert!(h.query_by_label(STATUS).is_none());
    assert!(inspect(&mut h)["engineStatus"].is_null());
}

/// Review finding 3: a 300-character status in a 900-point window during a run is cut to the
/// cap, its hover has it all, and the run's Cancel button is still there and works.
#[test]
fn a_long_status_is_cut_short_and_cancel_still_shows_in_a_narrow_window() {
    let long = format!("GPU: OpenCL, Some GPU ({})", "the preferred backend didn't open, ".repeat(9));
    let long = long[..300].to_owned();
    let (app, _) = editor(Some(&long), Some("warn"), connected());
    let mut h = harness_at(app, 900.0);
    start(&mut h);
    assert!(h.state().session.run.running);
    let shown = h.get_by_label(&long).rect();
    assert!(shown.width() <= ENGINE_STATUS_WIDTH + 1.0, "cut to the cap: {shown:?}");
    let (_, text) = drawn(&h, &long).expect("drawn");
    assert!(text.width() <= ENGINE_STATUS_WIDTH + 1.0, "the drawn text too: {text:?}");
    // the whole text on hover
    h.hover_at(shown.center());
    h.run_steps(4);
    assert!(h.query_all_by_label(&long).count() >= 2, "the bar's line and its hover");
    // Cancel is on screen and stops the run
    let cancel = h.get_by_label("Cancel").rect();
    assert!(cancel.max.x <= 900.0 && cancel.max.y <= 700.0, "Cancel on screen: {cancel:?}");
    h.get_by_label("Cancel").click();
    h.run_steps(4);
    assert!(!h.state().session.run.running, "Cancel stopped the run");
}

/// Layout B (D17): at 900 points the engine status, document, run state and Cancel are all in
/// the window, and each cell's second line is under its first.
#[test]
fn each_cell_has_its_detail_under_it() {
    let (app, _) = editor(Some(STATUS), Some("info"), connected());
    let mut h = harness_at(app, 900.0);
    let window = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0));
    let doc_name = h.state().document_name();
    let doc = h.get_by_label_contains(&doc_name).rect();
    let size = h.get_by_label("1 nodes, 0 wires").rect();
    assert!(size.min.y >= doc.max.y - 1.0, "graph size under the document: {doc:?} {size:?}");
    assert!((size.min.x - doc.min.x).abs() < 1.0, "in the same cell");
    let status = h.get_by_label(STATUS).rect();
    let engine = h.get_by_label_contains("engine").rect();
    assert!(status.min.y >= engine.max.y - 1.0, "the status under the engine's name: {engine:?} {status:?}");
    // while running: the run's main line, Cancel, and its time under them
    start(&mut h);
    let running = h.get_by_label_contains("running: ").rect();
    let cancel = h.get_by_label("Cancel").rect();
    // the time alone, "0.0 s"
    let is_time = |l: &str| l.strip_suffix(" s").is_some_and(|n| n.parse::<f64>().is_ok());
    let secs = h.get_all_by_label_contains(" s").find(|n| n.value().is_some_and(|l| is_time(&l))).expect("the run's time").rect();
    assert!(secs.min.y >= running.max.y - 1.0, "the time under the run: {running:?} {secs:?}");
    for (what, r) in [("status", status), ("document", doc), ("run", running), ("Cancel", cancel)] {
        assert!(window.contains_rect(r), "{what} in the window: {r:?}");
        assert!(r.min.y > 600.0, "{what} in the bottom bar: {r:?}");
    }
}
