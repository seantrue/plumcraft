//! The viewer's tools through the real UI, on a test engine whose arrays are known to the byte:
//! the pixel probe, 1:1 and nearest-neighbour zoom, Compare, plots of tables and the sheet. Nothing
//! here knows the engine: it runs the arrays and images any engine sends.
//!
//! Every number asserted is what the bytes hold, read from the same source the test wrote, and
//! the probe's is checked against what the 8-bit picture would have said at that pixel.

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::click_at;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use plumcraft_engine::host::Host;
use plumcraft_engine::{Array, Blob, BlobSource, Cx, Data, Engine, Inputs, List, Node, Outputs};
use plumcraft_host::{Connection, InProc};
use plumcraft_proto::ProtoError;
use plumcraft_proto::catalogue::{NodeDef, Port, core_types};
use plumcraft_proto::data::{BlobMeta, DataRef, Form};
use plumcraft_proto::error::NodeError;
use plumcraft_proto::session::{EngineInfo, EngineState};
use plumcraft_session::Session;
use plumcraft_ui::EditorApp;
use plumcraft_ui::link::Link;
use serde_json::{Value, json};

/// Rows and columns of the known arrays.
const H: usize = 12;
const W: usize = 16;

/// A value of [`ramp`] at (row, col): no two alike, and none of them a whole 8-bit level.
fn ramp_at(r: usize, c: usize) -> f32 {
    (r * W + c) as f32 * 0.00517 + 0.00011
}

fn f32s(v: impl IntoIterator<Item = f32>) -> Vec<u8> {
    v.into_iter().flat_map(f32::to_le_bytes).collect()
}

fn ramp() -> Data {
    let v = (0..H).flat_map(|r| (0..W).map(move |c| ramp_at(r, c)));
    Data::Array(Array::new("<f4", vec![H as u64, W as u64], f32s(v)))
}

/// [`ramp`] with two pixels changed, by known amounts.
fn nudged() -> Data {
    let v = (0..H).flat_map(|r| {
        (0..W).map(move |c| match (r, c) {
            (2, 3) => ramp_at(r, c) + 0.5,
            (7, 9) => ramp_at(r, c) - 0.25,
            _ => ramp_at(r, c),
        })
    });
    Data::Array(Array::new("<f4", vec![H as u64, W as u64], f32s(v)))
}

/// A 3-channel value, each channel different.
fn rgb() -> Data {
    let v = (0..H).flat_map(|r| (0..W).flat_map(move |c| [ramp_at(r, c), 0.25, 0.5 + 0.001 * c as f32]));
    Data::Array(Array { names: vec!["hue".into(), "sat".into(), "val".into()], ..Array::new("<f4", vec![H as u64, W as u64, 3], f32s(v)) })
}

/// 0 and 1 in a checkerboard: every block's edge is the largest step there is.
fn checker() -> Data {
    let v = (0..H).flat_map(|r| (0..W).map(move |c| ((r + c) % 2) as f32));
    Data::Array(Array::new("<f4", vec![H as u64, W as u64], f32s(v)))
}

/// One bright pixel in a dark array larger than a picture is sent: only the array has it.
fn big() -> Data {
    let (h, w) = (BIG.0, BIG.1);
    let mut v = vec![0f32; h * w];
    v[BIG_AT.0 * w + BIG_AT.1] = 1.0;
    Data::Array(Array::new("<f4", vec![h as u64, w as u64], f32s(v)))
}
const BIG: (usize, usize) = (1500, 2000);
const BIG_AT: (usize, usize) = (700, 1300);

/// A table of points.
fn points() -> Data {
    let rows = [[2.0, 3.0], [10.0, 4.0], [6.0, 9.0], [1.0, 1.0]];
    Data::Array(Array { names: vec!["x".into(), "y".into()], table: true, ..Array::new("<f4", vec![4, 2], f32s(rows.into_iter().flatten())) })
}

/// A table of segments.
fn segments() -> Data {
    let rows = [[0.0, 0.0, 10.0, 10.0], [10.0, 0.0, 0.0, 10.0]];
    Data::Array(Array {
        names: ["x0", "y0", "x1", "y1"].map(String::from).to_vec(),
        table: true,
        ..Array::new("<f4", vec![2, 4], f32s(rows.into_iter().flatten()))
    })
}

/// A list of three different pictures, labelled.
fn list() -> Data {
    let item = |k: usize| Data::Array(Array::new("<f4", vec![6, 8], f32s((0..48).map(move |i| (i * (k + 1)) as f32 / 150.0))));
    Data::List(List { items: (0..3).map(item).collect(), axes: Vec::new(), labels: vec!["soft".into(), "plain".into(), "hard".into()] })
}

/// A 16-bit grey PNG: an 8-bit reading of it would be wrong.
fn png16() -> Data {
    let img = image::ImageBuffer::<image::Luma<u16>, Vec<u16>>::from_fn(W as u32, H as u32, |x, y| image::Luma([40000 + (y as u16) * 100 + x as u16]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    img.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    let bytes = bytes.into_inner();
    let meta = BlobMeta { media: "image/png".into(), size: bytes.len() as u64, form: Form::Image { width: W as u32, height: H as u32 }, aliases: Vec::new() };
    Data::Blob(Box::new(Blob { handle: plumcraft_proto::Handle::raw(&bytes), meta, source: BlobSource::Bytes(Arc::from(bytes)), publish: true, thumb: None }))
}

/// A node that outputs a fixed value.
struct Fixed {
    id: &'static str,
    ty: &'static str,
    list: bool,
    make: fn() -> Data,
}

impl Node for Fixed {
    fn def(&self) -> NodeDef {
        NodeDef {
            id: self.id.into(),
            display: self.id.into(),
            category: vec!["Testing".into()],
            family: "test".into(),
            outputs: vec![Port { name: "out".into(), ty: self.ty.into(), list: self.list, ..Default::default() }],
            ..Default::default()
        }
    }

    fn run(&self, _: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
        Ok(Outputs::from([("out".to_owned(), (self.make)())]))
    }
}

fn engine() -> Engine {
    let info = EngineInfo { id: "known".into(), name: "Known".into(), version: "1".into(), salt: "known".into(), ..Default::default() };
    let mut e = Engine::new(info).with_wire_type(core_types::array()).with_wire_type(core_types::image());
    let a = core_types::ARRAY;
    for (id, ty, list, make) in [
        ("t.ramp", a, false, ramp as fn() -> Data),
        ("t.nudged", a, false, nudged),
        ("t.rgb", a, false, rgb),
        ("t.checker", a, false, checker),
        ("t.big", a, false, big),
        ("t.points", a, false, points),
        ("t.segments", a, false, segments),
        ("t.list", a, true, list),
        ("t.png16", core_types::IMAGE, false, png16),
    ] {
        e = e.with_node(Box::new(Fixed { id, ty, list, make }));
    }
    e
}

/// The connection as a remote engine with no `blob.range` looks: pictures arrive, full arrays
/// don't.
struct FullsGone(InProc);

impl Connection for FullsGone {
    fn call(&self, method: &str, params: Value) -> Result<Value, ProtoError> {
        self.0.call(method, params)
    }

    fn state(&self) -> EngineState {
        self.0.state()
    }

    fn resolve(&self, r: &DataRef) -> Option<plumcraft_store::Blob> {
        let picture = matches!(&r.meta.form, Form::Array { dtype, .. } if dtype == "|u1");
        picture.then(|| self.0.resolve(r)).flatten()
    }
}

/// One node of each kind, not wired: each is watched in turn.
fn app_with(wrap: impl FnOnce(InProc) -> Arc<dyn Connection>) -> (EditorApp, Vec<String>) {
    let host = Host::start(Box::new(|| Ok(engine())), None);
    host.wait_ready(Duration::from_secs(5));
    let catalog = serde_json::from_value(host.schema(None).unwrap()).unwrap();
    let mut s = Session::new(catalog);
    let kinds = ["t.ramp", "t.nudged", "t.rgb", "t.checker", "t.big", "t.points", "t.segments", "t.list", "t.png16"];
    let ids = kinds
        .iter()
        .enumerate()
        .map(|(i, k)| s.execute("node.add", &json!({"kind": k, "pos": [40 + 260 * (i % 3), 40 + 240 * (i / 3)]})).unwrap()["id"].as_str().unwrap().to_owned())
        .collect();
    (EditorApp::new(s).with_link(Link::start(wrap(InProc(host)), "in-process", None).unwrap()), ids)
}

fn editor() -> (Harness<'static, EditorApp>, Vec<String>) {
    let (app, ids) = app_with(|c| Arc::new(c));
    (common::harness(app), ids)
}

/// The editor as `ui.screenshot`'s fallback draws it: the real offscreen picture, kept in
/// `PLUMCRAFT_SHOT_DIR` when that is set.
fn picture(h: &mut Harness<'static, EditorApp>, name: &str) -> image::RgbaImage {
    let ctx = h.ctx.clone();
    let shot = plumcraft_ui::offscreen::render(h.state_mut(), egui::vec2(1400.0, 800.0), 1.0, Some(&ctx)).expect("an offscreen picture");
    let [w, ht] = shot.size;
    let img = image::RgbaImage::from_raw(w as u32, ht as u32, shot.pixels.iter().flat_map(|p| p.to_srgba_unmultiplied()).collect()).unwrap();
    if let Some(dir) = std::env::var_os("PLUMCRAFT_SHOT_DIR") {
        img.save(std::path::Path::new(&dir).join(format!("{name}.png"))).unwrap();
    }
    img
}

fn cmd(h: &mut Harness<'static, EditorApp>, id: &str, p: Value) -> Value {
    let r = h.state_mut().try_run(id, &p).unwrap_or_else(|e| panic!("{id}: {e}"));
    h.run_steps(2);
    r
}

/// Step until `done` (10 s at most).
fn until(h: &mut Harness<'static, EditorApp>, what: &str, done: impl Fn(&Harness<'static, EditorApp>) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !done(h) {
        assert!(Instant::now() < deadline, "timed out waiting for {what}: {}", inspect(h)["watched"]);
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    h.run_steps(3);
}

fn inspect(h: &Harness<'static, EditorApp>) -> Value {
    h.state().inspect(&h.ctx)
}

fn idle(h: &Harness<'static, EditorApp>) -> bool {
    let s = h.state();
    !s.session.run.running && s.session.run.outbox_is_empty()
}

/// Watch `node`, run, and wait for the value and its bytes.
fn watch(h: &mut Harness<'static, EditorApp>, node: &str) {
    cmd(h, "run.watch", json!({"node": node}));
    until(h, "the value", |h| idle(h) && h.state().session.run.panel().and_then(|(_, d)| d).is_some_and(|d| d.node == node));
    h.run_steps(4);
}

/// The panel's picture on screen and the array pixel (row, col) in it.
fn pixel(h: &Harness<'static, EditorApp>, r: usize, c: usize, rows: usize, cols: usize) -> egui::Pos2 {
    let p = inspect(h)["watched"]["view"]["tools"]["pictures"][0].clone();
    let p: Vec<f32> = serde_json::from_value(p).unwrap();
    let image = egui::Rect::from_min_max(egui::pos2(p[0], p[1]), egui::pos2(p[2], p[3]));
    let (x, y) = (image.width() / cols as f32, image.height() / rows as f32);
    egui::pos2(image.min.x + (c as f32 + 0.5) * x, image.min.y + (r as f32 + 0.5) * y)
}

/// What the 8-bit picture of `v` says, for a value from 0 to 1.
fn eight_bit(v: f32) -> f32 {
    (v * 255.0).round() / 255.0
}

#[test]
fn the_probe_reads_the_arrays_own_number_at_the_hovered_pixel() {
    let (mut h, ids) = editor();
    watch(&mut h, &ids[0]);
    let (r, c) = (5, 7);
    let want = ramp_at(r, c);
    assert!((0.0..=1.0).contains(&want), "{want}");
    assert_ne!(eight_bit(want).to_string(), want.to_string(), "the picture would read a different number here");
    let at = pixel(&h, r, c, H, W);
    h.hover_at(at);
    h.run_steps(3);
    // the label a person reads
    h.get_by_label(&format!("row {r} · col {c}: value {want}"));
    // and the control channel, against the bytes it came from
    let read = inspect(&h)["watched"]["view"]["tools"]["hover"].clone();
    assert_eq!(read["values"], json!([want.to_string()]), "{read}");
    assert_eq!(read["dtype"], "<f4");
    let full = h.state().watched.full.clone().unwrap();
    let at = (r * W + c) * 4;
    assert_eq!(f32::from_le_bytes(full.bytes[at..at + 4].try_into().unwrap()), want, "the test's own expectation is the bytes'");
    // Another pixel reads another number: the stride is right.
    let at = pixel(&h, 11, 15, H, W);
    h.hover_at(at);
    h.run_steps(3);
    h.get_by_label(&format!("row 11 · col 15: value {}", ramp_at(11, 15)));
}

#[test]
fn a_click_pins_the_probe_and_the_command_does_the_same() {
    let (mut h, ids) = editor();
    watch(&mut h, &ids[0]);
    assert!(h.query_by_label("Hover the picture to read values; click to pin.").is_some());
    let at = pixel(&h, 3, 4, H, W);
    click_at(&mut h, at);
    assert_eq!(h.state().session.views.tools.probe, Some((3, 4)));
    let want = ramp_at(3, 4);
    assert_eq!(inspect(&h)["watched"]["view"]["tools"]["probe"]["values"], json!([want.to_string()]));
    h.hover_at(egui::pos2(300.0, 500.0));
    h.run_steps(3);
    h.get_by_label(&format!("pinned row 3 · col 4: value {want}"));
    // a second click unpins
    let at = pixel(&h, 3, 4, H, W);
    click_at(&mut h, at);
    assert_eq!(h.state().session.views.tools.probe, None);
    // By command, as the control channel and MCP do it, and outside the array it says so.
    cmd(&mut h, "viewer.probe", json!({"row": 9, "col": 2}));
    assert_eq!(inspect(&h)["watched"]["view"]["tools"]["probe"]["values"], json!([ramp_at(9, 2).to_string()]));
    cmd(&mut h, "viewer.probe", json!({"row": 900, "col": 2}));
    h.get_by_label_contains("outside (16 × 12)");
    cmd(&mut h, "viewer.probe", json!({}));
    assert_eq!(inspect(&h)["watched"]["view"]["tools"]["probe"], Value::Null);
    assert!(h.state_mut().try_run("viewer.probe", &json!({"row": 1})).is_err(), "row without col");
}

#[test]
fn every_channel_reads_under_its_own_name() {
    let (mut h, ids) = editor();
    watch(&mut h, &ids[2]);
    cmd(&mut h, "viewer.probe", json!({"row": 2, "col": 3}));
    let (hue, val) = (ramp_at(2, 3), 0.5 + 0.001 * 3.0);
    let read = inspect(&h)["watched"]["view"]["tools"]["probe"].clone();
    assert_eq!(read["names"], json!(["hue", "sat", "val"]));
    assert_eq!(read["text"], format!("hue {hue} · sat 0.25 · val {}", val as f32));
}

/// A value whose bytes can't be read here says so: no number, least of all the picture's.
#[test]
fn a_value_that_isnt_here_says_so_and_shows_no_number() {
    let (app, ids) = app_with(|c| Arc::new(FullsGone(c)));
    let mut h = common::harness(app);
    watch(&mut h, &ids[0]);
    // the picture came, so hover works, and the panel's value is `missing`
    assert!(h.state().watched.full.is_none());
    assert!(!inspect(&h)["watched"]["view"]["pictures"].as_array().unwrap().is_empty());
    let at = pixel(&h, 5, 7, H, W);
    h.hover_at(at);
    h.run_steps(3);
    let read = inspect(&h)["watched"]["view"]["tools"]["hover"].clone();
    assert!(read.get("values").is_none(), "{read}");
    assert!(read["unavailable"].as_str().unwrap().contains("blob.range"), "{read}");
    h.get_by_label_contains("row 5 · col 7: no exact values here");
}

#[test]
fn an_images_probe_reads_its_own_16_bits() {
    let (mut h, ids) = editor();
    watch(&mut h, &ids[8]);
    until(&mut h, "the decoded image", |h| inspect(h)["watched"]["view"]["state"] == "ready");
    h.run_steps(3);
    let at = pixel(&h, 4, 6, H, W);
    h.hover_at(at);
    until(&mut h, "the numbers", |h| inspect(h)["watched"]["view"]["tools"]["hover"].get("values").is_some());
    let want = 40000 + 4 * 100 + 6;
    assert_eq!(inspect(&h)["watched"]["view"]["tools"]["hover"]["values"], json!([want.to_string()]));
    assert_eq!(inspect(&h)["watched"]["view"]["tools"]["hover"]["dtype"], "<u2");
    h.get_by_label_contains(&format!("label {want}"));
}

/// Screen pixels in the panel's picture at array pixel (r, c), offset by (dx, dy) from its centre.
fn colour_at(img: &image::RgbaImage, at: egui::Pos2, ppp: f32) -> [u8; 3] {
    let p = img.get_pixel((at.x * ppp) as u32, (at.y * ppp) as u32).0;
    [p[0], p[1], p[2]]
}

#[test]
fn zoomed_in_one_image_pixel_is_one_sharp_block() {
    let (mut h, ids) = editor();
    watch(&mut h, &ids[3]);
    cmd(&mut h, "run.view", json!({"view": "clip"}));
    until(&mut h, "the clip view", idle);
    cmd(&mut h, "viewer.zoom", json!({"to": 24, "at": [6, 8]}));
    h.run_steps(4);
    let tools = inspect(&h)["watched"]["view"]["tools"].clone();
    assert_eq!(tools["pixelsPerArrayPixel"], 24.0, "{tools}");
    assert_eq!(tools["arrayPixelsPerBlock"], 1.0, "{tools}");
    let img = picture(&mut h, "nearest");
    let ppp = 1.0;
    // Across two blocks, screen pixel by screen pixel: two colours and one step, no ramp.
    let centre = pixel(&h, 6, 8, H, W);
    let (black, white) = ([0u8; 3], [255u8; 3]);
    assert_eq!(colour_at(&img, centre, ppp), black, "(6, 8) holds 0");
    let mut steps = Vec::new();
    let mut last = black;
    for dx in 1..100 {
        let c = colour_at(&img, centre + egui::vec2(dx as f32, 0.0), ppp);
        assert!(c == black || c == white, "{dx}: interpolated {c:?}");
        if c != last {
            steps.push(dx);
        }
        last = c;
    }
    assert_eq!(steps.len(), 4, "block by block: {steps:?}");
    assert!(steps.windows(2).all(|w| (23..=25).contains(&(w[1] - w[0]))), "24 screen pixels each, to the pixel the edge lands on: {steps:?}");
    // 1:1 is one array pixel per screen pixel.
    cmd(&mut h, "viewer.zoom", json!({"to": "1:1"}));
    h.run_steps(4);
    let scale = inspect(&h)["watched"]["view"]["tools"]["pixelsPerArrayPixel"].as_f64().unwrap();
    assert!((scale - 1.0).abs() < 1e-4, "{scale}");
    cmd(&mut h, "viewer.zoom", json!({"to": "fit"}));
    h.run_steps(4);
    assert!(inspect(&h)["watched"]["view"]["zoom"].as_f64().unwrap() == 1.0);
}

/// 1:1 on an array larger than the picture sent: the engine draws one the array's size, so a
/// block is one array pixel, and the lone bright pixel the small picture averaged away shows.
#[test]
fn zoom_brings_a_picture_as_large_as_the_array() {
    let (mut h, ids) = editor();
    watch(&mut h, &ids[4]);
    let sent = |h: &Harness<'static, EditorApp>| inspect(h)["watched"]["view"]["pictures"][0]["size"].clone();
    assert_eq!(sent(&h), json!([1024, 768]), "sent at the default size");
    assert_eq!(inspect(&h)["watched"]["px"], 1024);
    let r = cmd(&mut h, "viewer.zoom", json!({"to": 32, "at": [BIG_AT.0, BIG_AT.1]}));
    assert!(r["run_id"].is_string(), "{r}");
    until(&mut h, "the larger picture", |h| idle(h) && sent(h) == json!([2000, 1500]));
    assert_eq!(inspect(&h)["watched"]["px"], 2000);
    h.run_steps(4);
    let tools = inspect(&h)["watched"]["view"]["tools"].clone();
    assert_eq!((tools["arrayPixelsPerBlock"].as_f64(), tools["pixelsPerArrayPixel"].as_f64()), (Some(1.0), Some(32.0)), "{tools}");
    let img = picture(&mut h, "detail");
    let ppp = 1.0;
    let at = pixel(&h, BIG_AT.0, BIG_AT.1, BIG.0, BIG.1);
    let [r, ..] = colour_at(&img, at + egui::vec2(4.0, 4.0), ppp);
    let [dark, ..] = colour_at(&img, at + egui::vec2(40.0, 0.0), ppp);
    assert!(r > 200 && dark < 40, "the bright pixel at its block: {r} on {dark}");
    // Fit doesn't rerun; the next run sends the small picture again.
    let r = cmd(&mut h, "viewer.zoom", json!({"to": "fit"}));
    assert!(r.get("run_id").is_none(), "{r}");
    assert_eq!(inspect(&h)["watched"]["px"], 1024);
}

/// Scrolling in, past where the small picture's pixels show as blocks, asks for the larger one.
#[test]
fn scrolling_in_past_the_picture_asks_for_detail_once() {
    let (mut h, ids) = editor();
    watch(&mut h, &ids[4]);
    let at = pixel(&h, BIG_AT.0, BIG_AT.1, BIG.0, BIG.1);
    h.hover_at(at);
    h.run_steps(2);
    assert_eq!(inspect(&h)["watched"]["px"], 1024, "fitted: nothing more is wanted");
    for _ in 0..30 {
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, 120.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.run_steps(1);
    }
    h.run_steps(30);
    until(&mut h, "the larger picture", |h| idle(h) && inspect(h)["watched"]["px"] == 2000);
    let tools = inspect(&h)["watched"]["view"]["tools"].clone();
    assert_eq!(tools["arrayPixelsPerBlock"], 1.0, "a block is a pixel now: {tools}");
}

fn compare_result(h: &Harness<'static, EditorApp>) -> Value {
    inspect(h)["watched"]["compare"]["result"].clone()
}

/// Pin two known arrays and read their difference from the buttons a person presses.
#[test]
fn the_difference_of_two_known_arrays_is_exact() {
    let (mut h, ids) = editor();
    watch(&mut h, &ids[0]);
    h.get_by_label("Pin A").click();
    h.run_steps(3);
    assert!(inspect(&h)["watched"]["compare"].is_object());
    watch(&mut h, &ids[1]);
    h.get_by_label("Pin B").click();
    h.run_steps(3);
    h.get_by_label("|A−B|").click();
    h.run_steps(3);
    until(&mut h, "the difference", |h| compare_result(h)["state"] == "ready");
    let r = compare_result(&h);
    let (a, b) = (ramp_at(2, 3), ramp_at(7, 9));
    let (d1, d2) = ((f64::from(a + 0.5) - f64::from(a)).abs(), (f64::from(b - 0.25) - f64::from(b)).abs());
    assert_eq!(r["max"].as_f64(), Some(d1.max(d2)), "{r}");
    assert_eq!(r["differing"], 2);
    assert_eq!(r["elements"], H * W);
    assert_eq!(r["mean"].as_f64(), Some((d1 + d2) / (H * W) as f64));
    assert_eq!(r["byHandle"], false);
    h.get_by_label_contains("2 of 192 elements differ");
    // The probe reads A, B and their difference at a changed pixel.
    cmd(&mut h, "viewer.probe", json!({"row": 2, "col": 3}));
    let read = inspect(&h)["watched"]["compare"]["probe"].clone();
    assert_eq!(read["a"]["values"], json!([a.to_string()]));
    assert_eq!(read["b"]["values"], json!([(a + 0.5).to_string()]));
    assert_eq!(read["difference"], format!("value {}", f64::from(a) - f64::from(a + 0.5)));
    // Unpin goes back to the value alone.
    h.get_by_label("Unpin").click();
    h.run_steps(4);
    assert_eq!(inspect(&h)["watched"]["compare"], Value::Null);
    assert!(!h.state().session.views.compare.on);
}

/// A value against itself: from the handle alone, and again by reading every element.
#[test]
fn the_difference_of_an_array_with_itself_is_exactly_zero() {
    let (mut h, ids) = editor();
    watch(&mut h, &ids[2]);
    cmd(&mut h, "viewer.pin", json!({"slot": "a"}));
    cmd(&mut h, "viewer.compare", json!({"how": "difference"}));
    until(&mut h, "the difference", |h| compare_result(h)["state"] == "ready");
    let r = compare_result(&h);
    assert_eq!((r["byHandle"].as_bool(), r["max"].as_f64(), r["differing"].as_u64()), (Some(true), Some(0.0), Some(0)), "{r}");
    assert_eq!(r["elements"], H * W * 3);
    h.get_by_label_contains("A and B are identical");
    // Per channel: three black pictures, each named.
    cmd(&mut h, "viewer.compare", json!({"diff": "channels"}));
    until(&mut h, "per channel", |h| compare_result(h)["pictures"].as_array().is_some_and(|p| p.len() == 3));
    assert_eq!(compare_result(&h)["pictures"][1]["caption"], "|A−B| sat");
    assert_eq!(compare_result(&h)["pictures"][1]["top"], 0.0);
}

#[test]
fn different_shapes_say_so_and_side_and_split_still_show_both() {
    let (mut h, ids) = editor();
    watch(&mut h, &ids[0]);
    cmd(&mut h, "viewer.pin", json!({"slot": "a"}));
    watch(&mut h, &ids[2]);
    cmd(&mut h, "viewer.pin", json!({"slot": "b"}));
    cmd(&mut h, "viewer.compare", json!({"how": "difference"}));
    until(&mut h, "the refusal", |h| compare_result(h)["state"] == "error");
    assert_eq!(compare_result(&h)["error"], "A is 12×16 and B is 12×16×3: the shapes differ");
    cmd(&mut h, "viewer.compare", json!({"how": "side"}));
    h.run_steps(3);
    assert_eq!(inspect(&h)["watched"]["compare"]["tools"]["pictures"].as_array().unwrap().len(), 2, "A and B, side by side");
    cmd(&mut h, "viewer.compare", json!({"how": "split", "split": 0.25}));
    h.run_steps(3);
    let cmp = inspect(&h)["watched"]["compare"].clone();
    assert_eq!(cmp["tools"]["pictures"].as_array().unwrap().len(), 1, "one picture, divided");
    assert_eq!(cmp["split"], 0.25);
    // the divider follows a drag
    let handle = h.state().compare.split_handle.expect("the divider");
    let to = handle.center() + egui::vec2(60.0, 0.0);
    h.hover_at(handle.center());
    h.run_steps(2);
    h.drag_at(handle.center());
    h.run_steps(2);
    h.hover_at(to);
    h.run_steps(2);
    h.drop_at(to);
    h.run_steps(3);
    let after = inspect(&h)["watched"]["compare"]["split"].as_f64().unwrap();
    assert!(after > 0.3, "{after}");
}

/// Two items of one list are two values: each pin keeps its own through a run.
#[test]
fn a_and_b_can_be_two_items_of_one_list() {
    let (mut h, ids) = editor();
    watch(&mut h, &ids[7]);
    cmd(&mut h, "viewer.pin", json!({"slot": "a"}));
    cmd(&mut h, "run.item", json!({"index": 2}));
    until(&mut h, "item 2", |h| idle(h) && h.state().session.run.panel().and_then(|(_, d)| d).is_some_and(|d| d.index == 2));
    cmd(&mut h, "viewer.pin", json!({"slot": "b"}));
    cmd(&mut h, "viewer.compare", json!({"how": "difference"}));
    h.state_mut().start_run_if_enabled();
    until(&mut h, "both", |h| idle(h) && compare_result(h)["state"] == "ready");
    let at = 47.0f64;
    let (a, b) = (f64::from(at as f32 / 150.0), f64::from(at as f32 * 3.0 / 150.0));
    assert_eq!(compare_result(&h)["max"].as_f64(), Some((a - b).abs()), "{}", compare_result(&h));
}

/// A pin goes with its node; nothing is left comparing a value that's gone.
#[test]
fn deleting_a_pinned_node_drops_the_pin() {
    let (mut h, ids) = editor();
    watch(&mut h, &ids[0]);
    cmd(&mut h, "viewer.pin", json!({"slot": "a"}));
    watch(&mut h, &ids[1]);
    assert!(h.state().session.views.compare.on);
    cmd(&mut h, "node.remove", json!({"nodes": [ids[0]]}));
    h.run_steps(4);
    assert!(!h.state().session.views.compare.on);
    assert_eq!(inspect(&h)["watched"]["compare"], Value::Null);
    h.state_mut().start_run_if_enabled();
    until(&mut h, "the run", idle);
    assert_eq!(inspect(&h)["lastError"], Value::Null);
}

#[test]
fn a_table_plots_its_points_and_segments() {
    let (mut h, ids) = editor();
    watch(&mut h, &ids[5]);
    assert_eq!(h.state().panel_pane.current(), Some("array.plot"));
    assert_eq!(h.state().panel_pane.inspect()["state"], json!({"kind": "plot", "drawn": 4, "rows": 4, "marks": "points", "sameScale": true}));
    h.get_by_label("4 points of 4 rows");
    for tab in ["Plot", "Values", "Bytes"] {
        h.get_by_label(tab);
    }
    watch(&mut h, &ids[6]);
    assert_eq!(h.state().panel_pane.inspect()["state"]["marks"], "segments");
    h.get_by_label("2 segments of 2 rows");
}

/// The plot draws: its points are on screen, in the accent colour.
#[test]
fn a_plots_marks_are_on_screen() {
    let (mut h, ids) = editor();
    watch(&mut h, &ids[5]);
    h.run_steps(4);
    let img = picture(&mut h, "plot");
    let accent = h.state().tokens.accent;
    let near = |a: u8, b: u8| a.abs_diff(b) < 30;
    let panel_x = h.ctx.content_rect().max.x - 340.0;
    let marks = img
        .enumerate_pixels()
        .filter(|(x, y, p)| *x as f32 >= panel_x && *y > 120 && near(p[0], accent.r()) && near(p[1], accent.g()) && near(p[2], accent.b()))
        .count();
    assert!(marks >= 4 * 8, "four points' worth of accent pixels, got {marks}");
}

#[test]
fn the_sheet_is_a_labelled_grid_and_a_click_shows_that_item() {
    let (mut h, ids) = editor();
    watch(&mut h, &ids[7]);
    let panel_left = h.get_by_label("Watched").rect().min.x;
    h.get_by_label("Sheet").click();
    h.run_steps(3);
    until(&mut h, "the sheet", |h| idle(h) && h.state().panel_pane.current() == Some("items"));
    h.run_steps(4);
    assert_eq!(inspect(&h)["watched"]["sheet"], true);
    let d = h.state().session.run.panel().and_then(|(_, d)| d).cloned().unwrap();
    assert!(d.sheet.iter().all(|c| c.thumb.is_some()), "every item came with its picture");
    for label in ["soft", "plain", "hard"] {
        h.query_all_by_label(label).next().unwrap_or_else(|| panic!("{label}"));
    }
    // The grid wraps in the panel: it doesn't widen it (eleven planes once took the whole window).
    assert_eq!(h.get_by_label("Watched").rect().min.x, panel_left);
    let y = |h: &Harness<'static, EditorApp>, l: &str| h.query_all_by_label(l).map(|n| n.rect().min.y).fold(0.0, f32::max);
    assert!(y(&h, "hard") > y(&h, "soft"), "the third item is on a row of its own, in a panel this wide");
    // A click shows that item alone.
    let hard = h.query_all_by_label("hard").max_by(|a, b| a.rect().min.y.total_cmp(&b.rect().min.y)).unwrap().rect().center();
    click_at(&mut h, egui::pos2(hard.x, hard.y));
    until(&mut h, "item 3", |h| idle(h) && h.state().session.run.panel().and_then(|(_, d)| d).is_some_and(|d| d.index == 2));
    assert_eq!(inspect(&h)["watched"]["sheet"], false);
    assert_ne!(h.state().panel_pane.current(), Some("items"));
    h.run_steps(3);
    assert!(h.query_by_label("item 3 of 3").is_some());
    assert!(h.state_mut().try_run("viewer.sheet", &json!({"on": "yes"})).is_err());
}

/// A pop-out has its own probe and zoom, by the same commands.
#[test]
fn a_popout_has_its_own_probe_and_zoom() {
    let (mut h, ids) = editor();
    watch(&mut h, &ids[0]);
    let id = cmd(&mut h, "view.popout", json!({"node": ids[0]}))["id"].as_u64().unwrap();
    until(&mut h, "the pop-out", |h| idle(h) && h.state().popouts.iter().any(|w| w.pane.display().is_some()));
    cmd(&mut h, "viewer.probe", json!({"row": 1, "col": 2, "popout": id}));
    cmd(&mut h, "viewer.zoom", json!({"to": 8, "popout": id}));
    h.run_steps(4);
    let state = inspect(&h)["popouts"][0]["pane"]["state"].clone();
    assert_eq!(state["tools"]["probe"]["values"], json!([ramp_at(1, 2).to_string()]), "{state}");
    assert_eq!(state["tools"]["pixelsPerArrayPixel"], 8.0, "{state}");
    assert_eq!(inspect(&h)["watched"]["view"]["tools"]["probe"], Value::Null, "the panel keeps its own");
    // A click on the pop-out's picture pins the pop-out's probe, and its zoom button is its own.
    cmd(&mut h, "viewer.zoom", json!({"to": "fit", "popout": id}));
    h.run_steps(4);
    let r: Vec<f32> = serde_json::from_value(inspect(&h)["popouts"][0]["pane"]["state"]["tools"]["pictures"][0].clone()).unwrap();
    let at = egui::pos2(r[0] + (r[2] - r[0]) * 2.5 / W as f32, r[1] + (r[3] - r[1]) * 6.5 / H as f32);
    click_at(&mut h, at);
    assert_eq!(h.state().session.views.popouts[0].tools.probe, Some((6, 2)));
    assert_eq!(h.state().session.views.tools.probe, None, "not the panel's");
}

/// Every tool is a command with an id and label, listed with its state, and a disabled one says
/// why; a new graph leaves nothing pinned.
#[test]
fn the_tools_are_commands_and_a_new_graph_forgets_them() {
    let (mut h, ids) = editor();
    let entry = |h: &Harness<'static, EditorApp>, id: &str| plumcraft_session::commands::list_entry(&h.state().session, plumcraft_session::find(id).unwrap());
    for id in ["viewer.pin", "viewer.unpin", "viewer.compare", "viewer.probe", "viewer.zoom", "viewer.sheet"] {
        let e = entry(&h, id);
        assert!(!e["label"].as_str().unwrap().is_empty());
        assert_eq!(e["enabled"], false, "{e}");
        assert!(!e["disabledReason"].as_str().unwrap().is_empty(), "{e}");
    }
    watch(&mut h, &ids[0]);
    cmd(&mut h, "viewer.pin", json!({}));
    cmd(&mut h, "viewer.pin", json!({}));
    assert!(h.state().session.run.entry(plumcraft_session::run::WatchRole::PinB).is_some(), "the second goes to B");
    cmd(&mut h, "viewer.probe", json!({"row": 1, "col": 1}));
    cmd(&mut h, "graph.new", json!({"discard": true}));
    h.run_steps(4);
    assert!(!h.state().session.views.compare.on);
    assert_eq!(h.state().session.views.tools.probe, None);
    assert_eq!(inspect(&h)["watched"], Value::Null);
}

/// Pictures of each compare view, for a person to look at (`PLUMCRAFT_SHOT_DIR`), and a check
/// that A, B and the difference are what's on screen.
#[test]
fn the_compare_views_show_what_they_say() {
    let (mut h, ids) = editor();
    watch(&mut h, &ids[0]);
    cmd(&mut h, "viewer.pin", json!({"slot": "a"}));
    watch(&mut h, &ids[3]);
    cmd(&mut h, "viewer.pin", json!({"slot": "b"}));
    cmd(&mut h, "run.view", json!({"view": "clip"}));
    until(&mut h, "the views", idle);
    cmd(&mut h, "viewer.compare", json!({"how": "side"}));
    h.run_steps(4);
    let rects =
        |h: &Harness<'static, EditorApp>| -> Vec<Vec<f32>> { serde_json::from_value(inspect(h)["watched"]["compare"]["tools"]["pictures"].clone()).unwrap() };
    let img = picture(&mut h, "compare-side");
    let side = rects(&h);
    assert_eq!(side.len(), 2);
    let at = |r: &Vec<f32>, row: f32, col: f32| egui::pos2(r[0] + (r[2] - r[0]) * (col + 0.5) / W as f32, r[1] + (r[3] - r[1]) * (row + 0.5) / H as f32);
    // A is the ramp, B the checker: (0, 1) is white in B, near black in A's first row.
    assert!(colour_at(&img, at(&side[1], 0.0, 1.0), 1.0)[0] > 200, "B's white square");
    assert!(colour_at(&img, at(&side[0], 0.0, 1.0), 1.0)[0] < 30, "A's dark first row");
    cmd(&mut h, "viewer.compare", json!({"how": "split", "split": 0.5}));
    h.run_steps(4);
    let img = picture(&mut h, "compare-split");
    let split = rects(&h);
    // Left of the divider is A, right of it B.
    assert!(colour_at(&img, at(&split[0], 0.0, 1.0), 1.0)[0] < 30, "left is A");
    assert!(colour_at(&img, at(&split[0], 0.0, 13.0), 1.0)[0] > 200, "right is B");
    cmd(&mut h, "viewer.compare", json!({"how": "difference"}));
    until(&mut h, "the difference", |h| compare_result(h)["state"] == "ready");
    let img = picture(&mut h, "compare-diff");
    let diff = rects(&h);
    let r = compare_result(&h);
    // |A−B| is largest at a white square of the checker with A dark, and stretched to white.
    let top = r["pictures"][0]["top"].as_f64().unwrap();
    assert!(colour_at(&img, at(&diff[0], 0.0, 1.0), 1.0)[0] > 240, "|0 − 1| at (0, 1) of top {top}");
}
