//! SVG, a core type, in the editor: an SVG the array engine makes (Array to SVG) shows in the
//! Watched panel drawn by our own rasteriser, and on its wire as a picture, though the value
//! reached the editor only as bytes.

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::add;
use egui_kittest::Harness;
use plumcraft_engine::host::Host;
use plumcraft_host::{Connection, InProc};
use plumcraft_session::Session;
use plumcraft_ui::EditorApp;
use plumcraft_ui::link::Link;
use serde_json::{Value, json};

fn until(h: &mut Harness<'static, EditorApp>, what: &str, done: impl Fn(&mut Harness<'static, EditorApp>) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !done(h) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
}

fn inspect(h: &mut Harness<'static, EditorApp>) -> Value {
    let ctx = h.ctx.clone();
    h.state_mut().control_now(&ctx, "ui.inspect", &Value::Null)["result"].clone()
}

#[test]
fn an_svg_shows_in_the_panel_and_on_its_wire() {
    let mut s = Session::new(plumcraft_array::engine().schema());
    let c = add(&mut s, "array.constant", 0.0, 0.0);
    s.execute("node.set", &json!({"node": c, "port": "value", "value": 0.5})).unwrap();
    let svg = add(&mut s, "array.array_to_svg", 400.0, 0.0);
    let r = add(&mut s, "array.rasterize_svg", 800.0, 0.0);
    s.execute("wire.connect", &json!({"from": {"node": c, "port": "constant"}, "to": {"node": svg, "port": "image"}})).unwrap();
    s.execute("wire.connect", &json!({"from": {"node": svg, "port": "svg"}, "to": {"node": r, "port": "svg"}})).unwrap();
    let host = Host::start(Box::new(|| Ok(plumcraft_array::engine())), None);
    host.wait_ready(Duration::from_secs(5));
    let conn: Arc<dyn Connection> = Arc::new(InProc(host));
    let app = EditorApp::new(s).with_link(Link::start(conn, "in-process", None).unwrap());
    let mut h = common::harness(app);
    h.state_mut().try_run("run.watch", &json!({"node": svg, "port": "svg"})).unwrap();
    h.state_mut().start_run_if_enabled();
    // The panel: the SVG drawn by us (state ready, a raster), not a fallback.
    until(&mut h, "the panel's SVG", |h| inspect(h)["watched"]["view"]["state"] == "ready");
    let w = inspect(&mut h)["watched"].clone();
    assert_eq!(w["media"], "vector", "{w}");
    assert!(w["view"]["raster"].is_array(), "{w}");
    // The wire: its preview is the SVG viewer, with a raster of its own.
    let style = h.ctx.global_style();
    let catalog = h.state().session.catalog().clone();
    let mids = h.state().canvas.wire_midpoints(&style, &catalog);
    let (at, ..) = mids.iter().find(|(_, f, _)| f.node.0 == svg).cloned().unwrap();
    h.hover_at(at);
    until(&mut h, "the wire's SVG preview", |h| inspect(h)["wirePreview"]["pane"]["state"]["raster"].is_array());
    let p = inspect(&mut h)["wirePreview"].clone();
    assert_eq!(p["port"], format!("{svg}.svg"));
    assert_eq!(p["pane"]["viewer"], "svg", "{p}");
}
