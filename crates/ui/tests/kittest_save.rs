//! Save Result… and Contact Sheet…, driven as a person would, down to the file: the buttons on a
//! result node and in the Watched panel ask where, and the real host's worker gives the engine
//! core the request. The core here writes what it was asked for, so a test reads the file back.

mod common;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::{add, click_at, click_in_node, harness};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use plumcraft_engine::data::Plane;
use plumcraft_engine::host::Host;
use plumcraft_engine::{CancelToken, Emit, Engine, EngineCore};
use plumcraft_host::{Connection, InProc};
use plumcraft_proto::catalogue::{Media, SaveFormat, SchemaReply};
use plumcraft_proto::display::{Display, Rendition};
use plumcraft_proto::error::NodeError;
use plumcraft_proto::event::NodeState;
use plumcraft_proto::run::{GraphRef, NodeReport, RunReply, RunRequest, SaveReply, SaveRequest};
use plumcraft_proto::session::{EngineInfo, EngineState};
use plumcraft_session::Session;
use plumcraft_ui::link::Link;
use plumcraft_ui::prefs;
use plumcraft_ui::{EditorApp, FileDialogs};
use serde_json::{Value, json};

/// A catalogue with a result node, a list source and a table source, and one format. Nothing here
/// says "png": an engine brings its own.
fn catalog() -> SchemaReply {
    serde_json::from_value(json!({
        "engine": {"id": "core", "name": "Core"},
        "wire_types": [{"id": "ARRAY", "media": "ndarray"}, {"id": "x.t.grid", "media": "table"}],
        "texts": {"result_prefix": "Output:"},
        "save_formats": [{"id": "note", "label": "Note", "extensions": ["note"], "media": ["ndarray"],
                          "options": [{"name": "quality", "type": "INT", "min": 1, "max": 9}]}],
        "nodes": [
            {"id": "t.make", "display": "Make", "category": ["T"], "outputs": [{"name": "picture", "type": "ARRAY"}, {"name": "grid", "type": "x.t.grid"}]},
            {"id": "t.result", "display": "Output", "category": ["T"], "flags": {"result": true},
             "inputs": [{"name": "picture", "type": "ARRAY", "socket": "socket"}], "outputs": [{"name": "picture", "type": "ARRAY"}]}]}))
    .unwrap()
}

/// How a run ends: how many items its result has.
const ITEMS: u64 = 3;

/// A core that "computes" every node, shows every watched value as a list of pictures, and
/// writes each save as a file naming what it was asked.
#[derive(Default)]
struct Writing {
    saves: Arc<Mutex<u32>>,
    last_cancelled: bool,
}

impl EngineCore for Writing {
    fn run(&mut self, req: RunRequest, _: &Plane<'_>, _: &mut dyn Emit, cancel: &CancelToken) -> RunReply {
        self.last_cancelled = cancel.is_cancelled();
        let GraphRef::Inline { graph } = &req.graph else { panic!("inline") };
        let mut reply = RunReply { run_id: req.run_id.clone(), generation: req.generation, ok: true, ..Default::default() };
        let thumb =
            Rendition::Rgba8 { data: Default::default(), w: 4, h: 4, role: "view".into(), caption: String::new(), summary: String::new(), items: ITEMS };
        for id in graph["nodes"].as_object().unwrap().keys() {
            reply.nodes.insert(id.clone(), NodeReport { state: NodeState::Done, thumb: Some(thumb.clone()), ..Default::default() });
        }
        reply.watch = req
            .watch
            .iter()
            .map(|w| {
                let media = if w.port == "grid" { Media::Table } else { Media::Ndarray };
                Display { node: w.node.clone(), port: w.port.clone(), media, items: ITEMS, index: w.index, summary: "a value".into(), ..Default::default() }
            })
            .collect();
        reply
    }

    fn release_scope(&mut self, _: &str) {
        *self.saves.lock().unwrap() += 100;
    }

    fn save(&mut self, req: SaveRequest, _: &Plane<'_>, _: &CancelToken) -> SaveReply {
        *self.saves.lock().unwrap() += 1;
        let path = req.path.clone().unwrap();
        match req.node.as_str() {
            _ if path.contains("slow") => std::thread::sleep(Duration::from_millis(400)),
            _ if path.contains("crash") => panic!("asked to"),
            _ if path.contains("refused") => return SaveReply::failed(NodeError::new(NodeError::CONFIG, "save RGB or one channel")),
            _ => {}
        }
        let GraphRef::Inline { graph } = &req.graph else { panic!("inline") };
        let wrote = json!({"node": req.node, "port": req.port, "index": req.index, "sheet": req.sheet, "format": req.format, "options": req.options, "nodes": graph["nodes"].as_object().unwrap().len()});
        std::fs::create_dir_all(Path::new(&path).parent().unwrap()).unwrap();
        std::fs::write(&path, wrote.to_string()).unwrap();
        SaveReply { ok: true, paths: vec![path], recipe: !req.sheet, items: if req.sheet { ITEMS } else { 0 }, ..Default::default() }
    }
}

/// What the dialog was offered: the formats' labels and the suggested file.
type Offered = Arc<Mutex<Vec<(Vec<String>, PathBuf)>>>;

/// A save dialog: what it was offered, and what it answers.
struct Dialog {
    offered: Offered,
    answer: Option<PathBuf>,
}

impl FileDialogs for Dialog {
    fn pick_path(&mut self, _: &plumcraft_proto::catalogue::PathHint, _: &str) -> Option<PathBuf> {
        None
    }
    fn save_output(&mut self, formats: &[SaveFormat], suggested: &Path) -> Option<PathBuf> {
        self.offered.lock().unwrap().push((formats.iter().map(|f| f.label.clone()).collect(), suggested.to_path_buf()));
        self.answer.clone()
    }
}

struct Rig {
    h: Harness<'static, EditorApp>,
    result: String,
    make: String,
    dir: PathBuf,
    offered: Offered,
    saves: Arc<Mutex<u32>>,
}

/// Make → Output ("Output: pencil"), saved beside `dir`, on a hosted core; run once.
fn rig(name: &str, answer: Option<&str>) -> Rig {
    let dir = plumcraft_scratch::dir(&format!("kittest-save-{name}"));
    let saves = Arc::new(Mutex::new(0));
    let core_saves = saves.clone();
    let factory = move || {
        let c = core_saves.clone();
        let info = EngineInfo { id: "core".into(), name: "Core".into(), ..Default::default() };
        let make = move || -> Result<Box<dyn EngineCore>, String> { Ok(Box::new(Writing { saves: c.clone(), last_cancelled: false })) };
        Ok(Engine::new(info).with_store(None).with_cap("output.save").with_core(catalog(), Box::new(make)))
    };
    let host = Host::start(Box::new(factory), None);
    assert!(matches!(host.wait_ready(Duration::from_secs(5)), EngineState::Connected { .. }));
    let conn: Arc<dyn Connection> = Arc::new(InProc(host));
    let mut s = Session::new(catalog());
    let make = add(&mut s, "t.make", 20.0, 40.0);
    let result = add(&mut s, "t.result", 360.0, 40.0);
    s.execute("wire.connect", &json!({"from": {"node": make, "port": "picture"}, "to": {"node": result, "port": "picture"}})).unwrap();
    s.execute("node.title", &json!({"node": result, "title": "Output: pencil"})).unwrap();
    s.execute("graph.saveAs", &json!({"path": dir.join("flow.json")})).unwrap();
    let offered = Arc::new(Mutex::new(Vec::new()));
    let mut app = EditorApp::new(s).with_link(Link::start(conn, "in-process", None).unwrap());
    app.dialogs = Box::new(Dialog { offered: offered.clone(), answer: answer.map(|a| dir.join(a)) });
    let mut h = harness(app);
    h.state_mut().run("run.start", &json!({})).unwrap();
    pump(&mut h, |h| !h.state().session.run.running);
    h.run_steps(6);
    Rig { h, result, make, dir, offered, saves }
}

/// Frames, with time for the engine, until `done`.
fn pump(h: &mut Harness<'static, EditorApp>, done: impl Fn(&Harness<'static, EditorApp>) -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    while !done(h) {
        assert!(Instant::now() < until, "timed out; last error {:?}, save {}", h.state().last_error, h.state().session.save.summary());
        std::thread::sleep(Duration::from_millis(5));
        h.run_steps(1);
    }
}

fn saved(h: &mut Harness<'static, EditorApp>) {
    pump(h, |h| h.state().session.save.pending.is_none() && h.state().session.save.last.is_some());
    h.run_steps(3);
}

/// Where the Watched panel starts, in a 1400 wide window. (A result node's own buttons have the
/// same labels; their rects are in graph coordinates, well to the left of it.)
const PANEL_X: f32 = 1000.0;

fn panel_click(h: &mut Harness<'static, EditorApp>, label: &str) {
    let at = h.query_all_by_label(label).map(|n| n.rect()).find(|r| r.left() > PANEL_X).unwrap_or_else(|| panic!("no {label} in the panel")).center();
    click_at(h, at);
}

fn wrote(path: &Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))).unwrap()
}

#[test]
fn a_result_nodes_button_asks_where_and_the_engine_writes_the_file() {
    let mut r = rig("result", Some("prints:pencil.note"));
    click_in_node(&mut r.h, "Save Result…");
    saved(&mut r.h);
    let offered = r.offered.lock().unwrap().clone();
    assert_eq!(offered.len(), 1, "{:?}", r.h.state().last_error);
    assert_eq!(offered[0].0, ["Note"], "the engine's formats");
    assert_eq!(offered[0].1, r.dir.join("pencil.note"), "the title without the catalogue's prefix, in the workflow's folder");
    let file = r.dir.join("prints/pencil.note");
    let got = wrote(&file);
    assert_eq!(got["node"], r.result);
    assert_eq!(got["port"], "picture", "a result's first output");
    assert_eq!((&got["index"], &got["sheet"]), (&Value::Null, &json!(false)), "every item");
    assert_eq!(got["format"], "note");
    assert_eq!(got["nodes"], 2, "with the graph the run had");
    assert!(r.h.query_by_label_contains("saved prints/pencil.note").is_some(), "the status bar says so");
    assert_eq!(r.h.state().session.save.summary()["last"]["ok"], true, "and what ui.inspect reads");
}

/// The save folder lasted a session; now the next run's dialog starts in it, by the preferences.
#[test]
fn the_folder_a_save_went_to_is_where_the_next_run_starts() {
    let mut first = rig("prefs-first", Some("prints/a.note"));
    std::fs::create_dir_all(first.dir.join("prints")).unwrap();
    let file = first.dir.join("prefs.json");
    first.h.state_mut().prefs = prefs::Store::at(file.clone());
    click_in_node(&mut first.h, "Save Result…");
    saved(&mut first.h);
    assert!(first.dir.join("prints/a.note").is_file());
    let prints = first.dir.join("prints");
    drop(first);

    let mut next = rig("prefs-second", Some("b.note"));
    next.h.state_mut().prefs = prefs::Store::at(file);
    click_in_node(&mut next.h, "Save Result…");
    saved(&mut next.h);
    assert_eq!(next.offered.lock().unwrap()[0].1, prints.join("pencil.note"), "not the workflow's folder: {:?}", next.dir);
}

/// File › New in the window: the engine hears the old graph's life is over.
#[test]
fn a_new_graph_tells_the_engine_the_last_one_is_over() {
    let mut r = rig("release", None);
    r.h.state_mut().run("graph.new", &json!({"discard": true})).unwrap();
    pump(&mut r.h, |_| *r.saves.lock().unwrap() == 100);
}

#[test]
fn a_list_offers_a_contact_sheet() {
    let mut r = rig("sheet", Some("sheet.note"));
    click_in_node(&mut r.h, "Contact Sheet…");
    saved(&mut r.h);
    assert_eq!(r.offered.lock().unwrap()[0].1, r.dir.join("pencil sheet.note"));
    let got = wrote(&r.dir.join("sheet.note"));
    assert_eq!((&got["sheet"], &got["index"]), (&json!(true), &Value::Null));
    assert!(r.h.query_by_label_contains("saved a sheet of 3 as").is_some());
    assert!(r.h.query_by_label_contains("without its recipe").is_some(), "the reply said it has none");
}

#[test]
fn the_watched_panel_saves_the_item_shown_and_only_what_a_format_takes() {
    let mut r = rig("panel", Some("item.note"));
    let (make, h) = (r.make.clone(), &mut r.h);
    h.state_mut().run("run.watch", &json!({"node": make, "port": "picture"})).unwrap();
    pump(h, |h| !h.state().session.run.running && h.state().session.run.panel().is_some_and(|(_, d)| d.is_some()));
    h.run_steps(4);
    h.state_mut().run("run.item", &json!({"index": 2})).unwrap();
    pump(h, |h| !h.state().session.run.running && h.state().session.run.panel().is_some_and(|(w, d)| w.index == 2 && d.is_some()));
    h.run_steps(4);
    panel_click(h, "Save Result…");
    saved(h);
    assert_eq!(r.offered.lock().unwrap()[0].1, r.dir.join("Make-02.note"), "numbered by the item");
    let got = wrote(&r.dir.join("item.note"));
    assert_eq!((&got["node"], &got["port"], &got["index"]), (&json!(make), &json!("picture"), &json!(2)));
    panel_click(h, "Contact Sheet…");
    saved(h);
    assert!(wrote(&r.dir.join("item.note"))["sheet"] == json!(true));
    // a table is no value for this engine's only format
    h.state_mut().run("run.watch", &json!({"node": make, "port": "grid"})).unwrap();
    pump(h, |h| !h.state().session.run.running && h.state().session.run.panel().is_some_and(|(w, d)| w.port == "grid" && d.is_some()));
    h.run_steps(4);
    let in_panel = h.query_all_by_label("Save Result…").filter(|n| n.rect().left() > PANEL_X).count();
    assert_eq!(in_panel, 0, "a Table isn't a Note");
}

#[test]
fn a_cancelled_dialog_saves_nothing() {
    let mut r = rig("cancel", None);
    click_in_node(&mut r.h, "Save Result…");
    r.h.run_steps(4);
    assert_eq!(r.offered.lock().unwrap().len(), 1);
    assert_eq!(*r.saves.lock().unwrap(), 0);
    assert!(r.h.state().session.save.pending.is_none() && r.h.state().session.save.last.is_none());
}

#[test]
fn the_window_stays_live_during_a_slow_save_and_one_goes_at_a_time() {
    let mut r = rig("slow", Some("slow.note"));
    click_in_node(&mut r.h, "Save Result…");
    assert!(r.h.state().session.save.pending.is_some(), "still going: the frame didn't wait for it");
    r.h.run_steps(2);
    assert!(r.h.query_by_label_contains("saving ").is_some());
    let e = r.h.state_mut().try_run("run.save", &json!({"node": r.result, "path": "other.note"})).unwrap_err();
    assert!(e.contains("a save is in progress"), "{e}");
    assert_eq!(*r.saves.lock().unwrap(), 1);
    // an edit goes through meanwhile
    r.h.state_mut().run("node.title", &json!({"node": r.make, "title": "Still here"})).unwrap();
    saved(&mut r.h);
    assert_eq!(wrote(&r.dir.join("slow.note"))["node"], r.result);
}

#[test]
fn a_refusal_and_a_crash_say_not_saved_and_the_engine_carries_on() {
    let mut r = rig("bad", Some("refused.note"));
    click_in_node(&mut r.h, "Save Result…");
    saved(&mut r.h);
    assert!(r.h.query_by_label_contains("not saved: save RGB or one channel").is_some());
    assert!(!r.dir.join("refused.note").exists());
    r.h.state_mut().run("run.save", &json!({"node": r.result, "path": "crash.note"})).unwrap();
    saved(&mut r.h);
    assert!(r.h.query_by_label_contains("not saved: the engine crashed").is_some());
    pump(&mut r.h, |h| h.state().engine.is_connected());
    r.h.state_mut().run("run.save", &json!({"node": r.result, "path": "after.note"})).unwrap();
    saved(&mut r.h);
    assert_eq!(wrote(&r.dir.join("after.note"))["port"], "picture", "a rebuilt core saves");
}

#[test]
fn an_option_the_format_lists_reaches_the_engine_and_others_dont() {
    let mut r = rig("options", None);
    let (result, h) = (r.result.clone(), &mut r.h);
    assert!(h.state_mut().try_run("run.save", &json!({"node": result, "path": "a.note", "options": {"quality": 11}})).is_err());
    assert!(h.state_mut().try_run("run.save", &json!({"node": result, "path": "a.note", "options": {"colour": 1}})).is_err());
    h.state_mut().run("run.save", &json!({"node": result, "path": "a", "options": {"quality": 7}})).unwrap();
    saved(h);
    let got = wrote(&r.dir.join("a.note"));
    assert_eq!(got["options"]["quality"], 7, "and a bare name takes the first format's extension");
}

#[test]
fn nothing_is_offered_before_a_run_or_by_an_engine_with_no_formats() {
    let mut s = Session::new(catalog());
    add(&mut s, "t.result", 60.0, 60.0);
    let mut h = harness(EditorApp::new(s));
    h.run_steps(4);
    assert!(h.query_by_label("Save Result…").is_none(), "hasn't run");
    // the same node, run, on an engine with no save formats
    let mut c = catalog();
    c.save_formats.clear();
    let mut s = Session::new(c);
    s.connected = true;
    let id = add(&mut s, "t.result", 60.0, 60.0);
    let req = s.execute("run.start", &json!({})).unwrap();
    let _ = s.run.take_outbox();
    let mut reply = RunReply { run_id: req["run_id"].as_str().unwrap().to_owned(), generation: s.run.generation, ok: true, ..Default::default() };
    reply.nodes.insert(id, NodeReport { state: NodeState::Done, ..Default::default() });
    assert!(s.run.apply_reply(&reply, Instant::now()));
    let mut h = harness(EditorApp::new(s));
    h.run_steps(4);
    assert!(h.query_by_label("Save Result…").is_none());
}
