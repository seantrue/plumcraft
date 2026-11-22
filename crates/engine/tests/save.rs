//! Save Result in the Rust SDK (`output.save`, spec 15; the maintainer, 2026-11-22: "Make the save
//! an basic feature"): an engine whose nodes output core types saves them with no code of its
//! own, in the formats the SDK gives each type; it adds, replaces and takes away formats, and
//! gives a node its own list. A save names a file (`data.path`) or gets its files back as blobs;
//! a list saves as numbered files or one item; options are checked; a failure is data.

use std::time::Duration;

use plumcraft_engine::host::Host;
use plumcraft_engine::save::{Format, option};
use plumcraft_engine::{Array, Cx, Data, Engine, Inputs, List, Node, Outputs};
use plumcraft_proto::catalogue::{Media, NodeDef, NodeFlags, Port, core_types, scalars};
use plumcraft_proto::error::NodeError;
use plumcraft_proto::run::SaveReply;
use plumcraft_proto::session::{EngineInfo, EngineState};
use serde_json::{Value, json};

struct Make {
    id: &'static str,
    ty: &'static str,
    value: fn(&Inputs) -> Result<Data, NodeError>,
    formats: Option<Vec<String>>,
}

impl Node for Make {
    fn def(&self) -> NodeDef {
        NodeDef {
            id: self.id.into(),
            display: self.id.into(),
            inputs: vec![Port { name: "word".into(), ty: scalars::STRING.into(), default: json!("plum"), ..Default::default() }],
            outputs: vec![Port { name: "value".into(), ty: self.ty.into(), ..Default::default() }],
            flags: NodeFlags { result: true, ..Default::default() },
            save_formats: self.formats.clone(),
            ..Default::default()
        }
    }
    fn run(&self, i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
        Ok([("value".to_owned(), (self.value)(i)?)].into_iter().collect())
    }
}

fn word(i: &Inputs) -> String {
    i.get("word").and_then(Data::as_str).unwrap_or_default().to_owned()
}

fn engine() -> Engine {
    let shout = Format::new("shout", "Shouted Text", &["shout"], &[Media::Text], "text/plain", |v, cx| {
        let s = v.as_str().ok_or_else(|| NodeError::new(NodeError::CONFIG, "Shouted Text saves text"))?;
        let mark = cx.options.get("mark").and_then(Value::as_str).unwrap_or("!");
        Ok(format!("{}{mark}", s.to_uppercase()).into_bytes())
    })
    .with_option(Port { choices: Some(vec![json!("!"), json!("?")]), ..option("mark", "Mark", scalars::ENUM, json!("!")) });
    Engine::new(EngineInfo { id: "t".into(), name: "T".into(), version: "1".into(), salt: "t".into(), ..Default::default() })
        .with_store(None)
        .with_wire_type(core_types::text())
        .with_wire_type(core_types::table())
        .with_wire_type(core_types::array())
        .with_node(Box::new(Make { id: "t.text", ty: core_types::TEXT, value: |i| Ok(Data::Json(json!(word(i)))), formats: None }))
        .with_node(Box::new(Make {
            id: "t.loud",
            ty: core_types::TEXT,
            value: |i| Ok(Data::Json(json!(word(i)))),
            formats: Some(vec!["shout".into(), "txt".into()]),
        }))
        .with_node(Box::new(Make {
            id: "t.list",
            ty: core_types::TEXT,
            value: |i| {
                Ok(Data::List(List {
                    items: vec![Data::Json(json!(word(i))), Data::Json(json!("b"))],
                    axes: Vec::new(),
                    labels: vec!["one".into(), "two".into()],
                }))
            },
            formats: None,
        }))
        .with_node(Box::new(Make {
            id: "t.table",
            ty: core_types::TABLE,
            value: |_| {
                Ok(Data::Json(json!({"columns": [{"name": "n", "type": "INT", "values": [1, 2]}, {"name": "s", "type": "STRING", "values": ["a,b", "c"]}]})))
            },
            formats: None,
        }))
        .with_node(Box::new(Make {
            id: "t.grey",
            ty: core_types::ARRAY,
            value: |_| Ok(Data::Array(Array::new("|u1", vec![1, 2], vec![7u8, 9]))),
            formats: None,
        }))
        .with_node(Box::new(Make { id: "t.fail", ty: core_types::TEXT, value: |_| Err(NodeError::new(NodeError::OP, "it broke")), formats: None }))
        .with_node(Box::new(Make {
            id: "t.pics",
            ty: core_types::ARRAY,
            value: |_| {
                let pic = |v: u8| Data::Array(Array::new("|u1", vec![2, 2], vec![v; 4]));
                Ok(Data::List(List { items: vec![pic(10), pic(200), pic(90)], axes: Vec::new(), labels: vec!["a 1".into(), "a 2".into(), "a 3".into()] }))
            },
            formats: None,
        }))
        .with_save_format(shout)
}

/// Contact Sheet… on an SDK engine (decision 2A): every picture of a list on one PNG, the
/// labels in the reply; a format that doesn't make sheets refuses. And the workflow's name and
/// graph hash go into the formats with room for them (`stamped`), nothing else.
#[test]
fn a_list_of_pictures_saves_as_one_contact_sheet_with_the_workflows_name() {
    let h = host(engine);
    let dir = plumcraft_scratch::dir("sdk-save-sheet");
    let info = json!({"name": "Flow", "hash": "sha256:0123"});
    let r = save(&h, "t.pics", json!({"path": dir.join("sheet.png"), "sheet": true, "graph_info": info}));
    assert!(r.ok && r.items == 3 && r.labels == ["a 1", "a 2", "a 3"] && r.paths.len() == 1 && r.stamped && !r.recipe, "{r:?}");
    let png = std::fs::read(dir.join("sheet.png")).unwrap();
    let img = image::load_from_memory(&png).unwrap().to_rgb8();
    // two columns, two rows of 2-pixel cells, 8-pixel gaps
    assert_eq!((img.width(), img.height()), (2 * 2 + 3 * 8, 2 * 2 + 3 * 8));
    assert_eq!((img.get_pixel(8, 8).0, img.get_pixel(18, 8).0, img.get_pixel(8, 18).0), ([10; 3], [200; 3], [90; 3]));
    assert!(png.windows(15).any(|w| w == b"PlumCraft graph"), "the name and hash, informational");
    assert!(save(&h, "t.pics", json!({"path": dir.join("sheet.jpg"), "sheet": true})).ok, "JPEG makes sheets too");
    assert!(save(&h, "t.pics", json!({"path": dir.join("sheet.npy"), "sheet": true})).error.unwrap().message.contains("sheet"));
    let txt = save(&h, "t.text", json!({"path": dir.join("w.txt"), "graph_info": info}));
    assert!(txt.ok && !txt.stamped && std::fs::read(dir.join("w.txt")).unwrap() == b"plum", "text has no room: as it was");
}

fn host(e: fn() -> Engine) -> Host {
    let h = Host::start(Box::new(move || Ok(e())), None);
    assert!(matches!(h.wait_ready(Duration::from_secs(5)), EngineState::Connected { .. }));
    h
}

fn graph(kind: &str) -> Value {
    json!({"kind": "inline", "graph": {"version": 1, "nodes": {"n": {"kind": kind, "values": {"word": "plum"}}}, "wires": []}})
}

fn save(h: &Host, kind: &str, extra: Value) -> SaveReply {
    let mut p = json!({"graph": graph(kind), "node": "n", "port": "value"});
    for (k, v) in extra.as_object().cloned().unwrap_or_default() {
        p[k] = v;
    }
    serde_json::from_value(h.dispatch("output.save", p).unwrap()).unwrap()
}

fn bytes_of(r: &SaveReply, i: usize) -> Vec<u8> {
    plumcraft_store::read_inline(r.files[i].data.as_ref().unwrap()).unwrap().bytes.to_vec()
}

#[test]
fn the_catalogue_lists_the_defaults_for_its_types_and_its_own_and_hello_says_it_saves() {
    let s = engine().schema();
    let ids: Vec<&str> = s.save_formats.iter().map(|f| f.id.as_str()).collect();
    assert_eq!(ids, ["txt", "csv", "json", "png", "jpeg", "npy", "shout"], "TEXT, TABLE and ARRAY's defaults in order, then its own");
    let node = |id: &str| s.nodes.iter().find(|n| n.id == id).unwrap().clone();
    let offered = |id: &str| s.save_formats_for(&node(id), "value").iter().map(|f| f.id.clone()).collect::<Vec<_>>();
    assert_eq!(offered("t.text"), ["txt", "shout"], "a TEXT output: every format that takes text");
    assert_eq!(offered("t.loud"), ["shout", "txt"], "a node's own list, its order");
    assert_eq!(offered("t.table"), ["csv", "json"]);
    assert_eq!(offered("t.grey"), ["png", "jpeg", "npy"]);
    assert!(engine().caps().has("output.save"));
    let none = engine().without_default_save_formats().without_save_format("shout");
    assert!(none.schema().save_formats.is_empty() && !none.caps().has("output.save"), "no formats: no output.save");
    let replaced = engine().with_save_format(Format::new("txt", "Plain", &["text"], &[Media::Text], "text/plain", |_, _| Ok(b"x".to_vec())));
    assert_eq!(replaced.schema().save_formats.iter().find(|f| f.id == "txt").map(|f| f.label.as_str()), Some("Plain"));
}

#[test]
fn a_save_writes_the_file_the_request_names() {
    let h = host(engine);
    let dir = plumcraft_scratch::dir("sdk-save-path");
    let txt = dir.join("word.txt");
    let r = save(&h, "t.text", json!({"path": txt}));
    assert!(r.ok && r.paths == [txt.display().to_string()] && !r.recipe && r.items == 1, "{r:?}");
    assert_eq!(std::fs::read(&txt).unwrap(), b"plum");
    // by extension, by id, and with an option
    let loud = dir.join("word.shout");
    let r = save(&h, "t.loud", json!({"path": loud, "options": {"mark": "?"}}));
    assert!(r.ok, "{r:?}");
    assert_eq!(std::fs::read(&loud).unwrap(), b"PLUM?");
    let r = save(&h, "t.loud", json!({"path": dir.join("noext"), "format": ""}));
    assert!(r.ok && std::fs::read(dir.join("noext")).unwrap() == b"PLUM!", "no extension, no format: the node's first, its option's default");
    let csv = dir.join("t.csv");
    assert!(save(&h, "t.table", json!({"path": csv})).ok);
    assert_eq!(std::fs::read_to_string(&csv).unwrap(), "n,s\n1,\"a,b\"\n2,c\n");
    let png = dir.join("grey.png");
    assert!(save(&h, "t.grey", json!({"path": png})).ok);
    assert_eq!(&std::fs::read(&png).unwrap()[..8], b"\x89PNG\r\n\x1a\n");
    let npy = dir.join("grey.npy");
    assert!(save(&h, "t.grey", json!({"path": npy, "format": "npy"})).ok);
    let b = std::fs::read(&npy).unwrap();
    assert!(b.starts_with(b"\x93NUMPY") && b.ends_with(&[7u8, 9]));
    // no temporary file is left beside them
    assert!(std::fs::read_dir(&dir).unwrap().flatten().all(|e| !e.file_name().to_string_lossy().starts_with('.')));
}

#[test]
fn a_list_saves_as_numbered_files_or_one_item() {
    let h = host(engine);
    let dir = plumcraft_scratch::dir("sdk-save-list");
    let r = save(&h, "t.list", json!({"path": dir.join("w.txt")}));
    assert!(r.ok && r.items == 2 && r.labels == ["one", "two"], "{r:?}");
    assert_eq!(r.paths, [dir.join("w-00.txt").display().to_string(), dir.join("w-01.txt").display().to_string()]);
    assert_eq!(std::fs::read(dir.join("w-01.txt")).unwrap(), b"b");
    let r = save(&h, "t.list", json!({"path": dir.join("one.txt"), "index": 1}));
    assert!(r.ok && r.paths == [dir.join("one.txt").display().to_string()]);
    assert_eq!(std::fs::read(dir.join("one.txt")).unwrap(), b"b");
    let past = save(&h, "t.list", json!({"path": dir.join("x.txt"), "index": 2}));
    assert_eq!(past.error.unwrap().kind, NodeError::CONFIG);
    let sheet = save(&h, "t.list", json!({"path": dir.join("s.txt"), "sheet": true}));
    assert!(!sheet.ok && sheet.error.unwrap().message.contains("sheet"));
}

#[test]
fn without_a_path_the_files_come_back_as_blobs() {
    let h = host(engine);
    let r = save(&h, "t.loud", json!({"format": "shout"}));
    assert!(r.ok && r.paths.is_empty() && r.files.len() == 1, "{r:?}");
    assert_eq!(r.files[0].name, "result.shout");
    assert_eq!(bytes_of(&r, 0), b"PLUM!");
    let r = save(&h, "t.list", json!({}));
    assert_eq!(r.files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(), ["result-00.txt", "result-01.txt"]);
    assert_eq!(bytes_of(&r, 1), b"b");
}

#[test]
fn what_cant_be_saved_is_refused_as_data() {
    let h = host(engine);
    let dir = plumcraft_scratch::dir("sdk-save-refused");
    let kind = |r: SaveReply| r.error.map(|e| (e.kind, e.message)).unwrap_or_default();
    for (k, extra, want) in [
        ("t.text", json!({"format": "csv"}), "doesn't save as"),
        ("t.text", json!({"path": dir.join("a.wav")}), "doesn't save .wav"),
        ("t.loud", json!({"path": dir.join("a.shout"), "options": {"mark": "."}}), "must be one of"),
        ("t.loud", json!({"path": dir.join("a.shout"), "options": {"volume": 11}}), "no option"),
        ("t.text", json!({"node": "nope"}), "isn't a node"),
        ("t.text", json!({"port": "nope"}), "no output"),
        ("t.text", json!({"index": 1}), "one value"),
    ] {
        let (kind, message) = kind(save(&h, k, extra.clone()));
        assert_eq!(kind, NodeError::CONFIG, "{extra}: {message}");
        assert!(message.contains(want), "{extra}: {message}");
    }
    let failed = save(&h, "t.fail", json!({"path": dir.join("f.txt")}));
    assert_eq!(failed.error.map(|e| (e.kind, e.message)), Some((NodeError::OP.to_owned(), "it broke".to_owned())), "the node's own error");
    assert!(!dir.join("f.txt").exists());
    // a folder in the way of the file
    std::fs::create_dir_all(dir.join("taken.txt")).unwrap();
    assert!(!save(&h, "t.text", json!({"path": dir.join("taken.txt")})).ok);
}

#[test]
fn a_save_after_a_run_reuses_its_value() {
    use std::sync::atomic::{AtomicU32, Ordering};
    static RUNS: AtomicU32 = AtomicU32::new(0);
    struct Counted;
    impl Node for Counted {
        fn def(&self) -> NodeDef {
            NodeDef { id: "c.n".into(), outputs: vec![Port { name: "value".into(), ty: core_types::TEXT.into(), ..Default::default() }], ..Default::default() }
        }
        fn run(&self, _: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
            RUNS.fetch_add(1, Ordering::SeqCst);
            Ok([("value".to_owned(), Data::Json(json!("once")))].into_iter().collect())
        }
    }
    let h = host(|| Engine::new(EngineInfo { id: "c".into(), salt: "c".into(), ..Default::default() }).with_store(None).with_node(Box::new(Counted)));
    let g = json!({"kind": "inline", "graph": {"nodes": {"n": {"kind": "c.n"}}, "wires": []}});
    let ack = h.dispatch("run", json!({"run_id": "r1", "graph": g, "targets": ["n"]})).unwrap();
    assert_eq!(ack["accepted"], true);
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while h.get("r1").map(|s| s.reply.is_none()).unwrap_or(true) && std::time::Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    let r: SaveReply = serde_json::from_value(h.dispatch("output.save", json!({"graph": g, "node": "n"})).unwrap()).unwrap();
    assert!(r.ok && r.files[0].name == "result.txt", "{r:?}");
    assert_eq!(RUNS.load(Ordering::SeqCst), 1, "the save took the run's value from the memo");
}

/// Hostile input (spec 18): every field of a save swapped for every kind of junk, and the graph
/// shaped wrong, is a protocol error or a failed save, never a panic, and the engine saves after.
#[test]
fn a_hostile_save_never_panics_and_the_engine_carries_on() {
    let h = host(engine);
    let seed = json!({"graph": graph("t.loud"), "node": "n", "port": "value", "index": null, "format": "shout",
                      "options": {"mark": "?"}, "sheet": false, "inputs": {}, "trusted_roots": [], "recipe_graph": null});
    let junk = [json!(null), json!(-1), json!(1.5e308), json!("x"), json!([]), json!({}), json!(true), json!(u64::MAX), json!({"kind": "inline", "graph": 7})];
    for key in seed.as_object().unwrap().keys() {
        for j in &junk {
            let mut p = seed.clone();
            p[key] = j.clone();
            if let Ok(v) = h.dispatch("output.save", p) {
                assert!(serde_json::from_value::<SaveReply>(v).is_ok(), "{key}: a reply");
            }
        }
    }
    for g in [
        json!({"nodes": {"n": {"kind": 7}}}),
        json!({"nodes": {"n": {"kind": "t.loud", "values": []}}, "wires": [{"from": {"node": "n"}}]}),
        json!({"nodes": []}),
    ] {
        let r = h.dispatch("output.save", json!({"graph": {"kind": "inline", "graph": g}, "node": "n"}));
        assert!(r.is_err() || r.is_ok_and(|v| v["ok"] == false), "a broken graph is no save");
    }
    assert!(save(&h, "t.loud", json!({"format": "shout"})).ok, "and the engine saves after");
    assert!(matches!(h.state(), EngineState::Connected { .. }));
}
