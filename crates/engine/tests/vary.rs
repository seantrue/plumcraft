//! Lists in the executor (spec 7.6 rule 2) and the Vary nodes: a list into an input that takes
//! one value runs the node once per item; lists zip along a shared axis and form a product over
//! different ones; Vary Range 2D's outputs zip; Vary Choices gets the choices it feeds; errors,
//! cancel and the memo.

use plumcraft_engine::data::Plane;
use plumcraft_engine::exec::{self, Memo};
use plumcraft_engine::{CancelToken, Cx, Data, Emit, Engine, Inputs, Node, Outputs};
use plumcraft_proto::PortValue;
use plumcraft_proto::catalogue::{Media, NodeDef, Port, WireType};
use plumcraft_proto::error::NodeError;
use plumcraft_proto::event::{NodeState, StatusRecord};
use plumcraft_proto::run::{RunReply, RunRequest};
use plumcraft_proto::session::EngineInfo;
use serde_json::{Value, json};

/// `a × b`, and `mode` (an ENUM) as text: one value each.
struct Mul;

impl Node for Mul {
    fn def(&self) -> NodeDef {
        let num = |n: &str| Port { name: n.into(), ty: "FLOAT".into(), default: json!(1.0), ..Default::default() };
        NodeDef {
            id: "t.mul".into(),
            inputs: vec![
                num("a"),
                num("b"),
                Port {
                    name: "mode".into(),
                    ty: "ENUM".into(),
                    default: json!("plain"),
                    choices: Some(vec![json!("plain"), json!("bold"), json!("loud")]),
                    ..Default::default()
                },
            ],
            outputs: vec![
                Port { name: "out".into(), ty: "FLOAT".into(), ..Default::default() },
                Port { name: "mode".into(), ty: "STRING".into(), ..Default::default() },
            ],
            ..Default::default()
        }
    }

    fn run(&self, i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
        let f = |k: &str| i.get(k).and_then(Data::json).and_then(Value::as_f64).unwrap_or(f64::NAN);
        if f("a") < 0.0 {
            return Err(NodeError::new(NodeError::CONFIG, "a is negative"));
        }
        Ok(Outputs::from([
            ("out".to_owned(), Data::Json(json!(f("a") * f("b")))),
            ("mode".to_owned(), i.get("mode").cloned().unwrap_or(Data::Json(Value::Null))),
        ]))
    }
}

/// The sum of a whole list.
struct Sum;

impl Node for Sum {
    fn def(&self) -> NodeDef {
        NodeDef {
            id: "t.sum".into(),
            inputs: vec![Port { name: "xs".into(), ty: "FLOAT".into(), list: true, ..Default::default() }],
            outputs: vec![Port { name: "sum".into(), ty: "FLOAT".into(), ..Default::default() }],
            ..Default::default()
        }
    }

    fn run(&self, i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
        let xs = i.get("xs").and_then(Data::list).ok_or_else(|| NodeError::new(NodeError::CONFIG, "xs is not a list"))?;
        let s: f64 = xs.items.iter().filter_map(Data::json).filter_map(Value::as_f64).sum();
        Ok(Outputs::from([("sum".to_owned(), Data::Json(json!(s)))]))
    }
}

fn engine() -> Engine {
    let info = EngineInfo { id: "t".into(), salt: "t".into(), ..Default::default() };
    Engine::new(info)
        .with_store(None)
        .with_wire_type(WireType { id: "FLOAT".into(), media: Media::Scalar, ..Default::default() })
        .with_node(Box::new(Mul))
        .with_node(Box::new(Sum))
        .with_building_blocks()
}

#[derive(Default)]
struct Log(Vec<StatusRecord>);

impl Emit for Log {
    fn status(&mut self, rec: StatusRecord) {
        self.0.push(rec);
    }
}

fn go_with(
    e: &Engine,
    memo: &mut Memo,
    nodes: Value,
    wires: &[(&str, &str, &str, &str)],
    targets: &[&str],
    watch: Value,
    cancel: &CancelToken,
) -> (RunReply, Vec<StatusRecord>) {
    // `port|slice` puts a slice on the wire
    let wire = |(a, p, b, q): &(&str, &str, &str, &str)| {
        let (port, slice) = q.split_once('|').map_or((*q, None), |(q, s)| (q, Some(s)));
        let mut w = json!({"from": {"node": a, "port": p}, "to": {"node": b, "port": port}});
        if let Some(s) = slice {
            w["slice"] = json!(s);
        }
        w
    };
    let wires: Vec<Value> = wires.iter().map(wire).collect();
    let req: RunRequest = serde_json::from_value(json!({
        "run_id": "r", "generation": 1,
        "graph": {"kind": "inline", "graph": {"version": 1, "nodes": nodes, "wires": wires}},
        "targets": targets, "watch": watch,
    }))
    .unwrap();
    let plane = Plane { store: None, shm: false, urls: false, max_inline: 1 << 20 };
    let mut log = Log::default();
    let reply = exec::run(e, &req, &mut log, cancel, "test", memo, &plane);
    (reply, log.0)
}

fn go(nodes: Value, wires: &[(&str, &str, &str, &str)], targets: &[&str]) -> RunReply {
    go_with(&engine(), &mut Memo::default(), nodes, wires, targets, json!([]), &CancelToken::default()).0
}

/// A target's output as plain JSON (lists as arrays).
fn out(r: &RunReply, node: &str, port: &str) -> Value {
    fn plain(v: &PortValue) -> Value {
        match v {
            PortValue::Json { value } => value.clone(),
            PortValue::List { items } => Value::Array(items.iter().map(plain).collect()),
            other => json!(format!("{other:?}")),
        }
    }
    r.nodes.get(node).and_then(|n| n.outputs.get(port)).map(plain).unwrap_or(Value::Null)
}

#[test]
fn a_vary_list_runs_a_node_once_per_value() {
    let r = go(
        json!({"v": {"kind": "vary.float", "values": {"values": "1, 2, 3"}}, "m": {"kind": "t.mul", "values": {"b": 10}}}),
        &[("v", "values", "m", "a")],
        &["v", "m"],
    );
    assert!(r.ok, "{r:?}");
    assert_eq!(out(&r, "v", "values"), json!([1.0, 2.0, 3.0]));
    assert_eq!(out(&r, "m", "out"), json!([10.0, 20.0, 30.0]));
    // every output of a mapped node is a list, single inputs broadcast
    assert_eq!(out(&r, "m", "mode"), json!(["plain", "plain", "plain"]));
}

#[test]
fn two_vary_nodes_form_every_combination_and_one_zips_with_itself() {
    let nodes = json!({"a": {"kind": "vary.int", "values": {"values": "1, 2"}}, "b": {"kind": "vary.float", "values": {"values": "10, 20, 30"}}, "m": {"kind": "t.mul"}});
    let r = go(nodes.clone(), &[("a", "values", "m", "a"), ("b", "values", "m", "b")], &["m"]);
    // the first varies slowest
    assert_eq!(out(&r, "m", "out"), json!([10.0, 20.0, 30.0, 20.0, 40.0, 60.0]));
    let r = go(nodes, &[("b", "values", "m", "a"), ("b", "values", "m", "b")], &["m"]);
    assert_eq!(out(&r, "m", "out"), json!([100.0, 400.0, 900.0]), "one list on two inputs zips");
    // a mapped node's list meets its source again downstream: they share the axis, so zip
    let nodes = json!({"b": {"kind": "vary.float", "values": {"values": "1, 2, 3"}}, "m": {"kind": "t.mul", "values": {"b": 2}}, "n": {"kind": "t.mul"}});
    let r = go(nodes, &[("b", "values", "m", "a"), ("m", "out", "n", "a"), ("b", "values", "n", "b")], &["n"]);
    assert_eq!(out(&r, "n", "out"), json!([2.0, 8.0, 18.0]));
}

/// Spec 7.6 rule 3: a range keeps a list a list, an index takes one item, and either is part of
/// the memo key.
#[test]
fn a_slice_picks_from_the_list_a_wire_carries() {
    let nodes =
        || json!({"v": {"kind": "vary.float", "values": {"values": "1, 2, 3, 4"}}, "m": {"kind": "t.mul", "values": {"b": 10}}, "s": {"kind": "t.sum"}});
    let r = go(nodes(), &[("v", "values", "m", "a|1:")], &["m"]);
    assert_eq!(out(&r, "m", "out"), json!([20.0, 30.0, 40.0]));
    let r = go(nodes(), &[("v", "values", "m", "a|-1")], &["m"]);
    assert_eq!(out(&r, "m", "out"), json!(40.0), "an index is one item: nothing to map over");
    let r = go(nodes(), &[("v", "values", "s", "xs|::2")], &["s"]);
    assert_eq!(out(&r, "s", "sum"), json!(4.0), "a range into a list input");
    let r = go(nodes(), &[("v", "values", "m", "a| 1 : -1 ")], &["m"]);
    assert_eq!(out(&r, "m", "out"), json!([20.0, 30.0]));
    // a slice's mapping keeps its order
    let r = go(nodes(), &[("v", "values", "m", "a|::-1")], &["m"]);
    assert_eq!(out(&r, "m", "out"), json!([40.0, 30.0, 20.0, 10.0]));
    for (bad, why) in [("a|9", "out of range"), ("a|5:", "is empty"), ("a|x", "use start:stop:step"), ("a|3:1", "always empty")] {
        let r = go(nodes(), &[("v", "values", "m", bad)], &["m"]);
        let n = &r.nodes["m"];
        assert_eq!(n.state, NodeState::Error, "{bad}");
        let e = n.error.as_ref().unwrap();
        assert!(e.kind == NodeError::CONFIG && e.message.contains(why), "{bad}: {e:?}");
    }
    let single = json!({"m": {"kind": "t.mul"}, "n": {"kind": "t.mul"}});
    let r = go(single, &[("m", "out", "n", "a|0")], &["n"]);
    assert!(r.nodes["n"].error.as_ref().unwrap().message.contains("needs a list"));
}

#[test]
fn a_slice_is_part_of_the_memo_key() {
    let (e, mut memo) = (engine(), Memo::default());
    let nodes = || json!({"v": {"kind": "vary.float", "values": {"values": "1, 2, 3"}}, "m": {"kind": "t.mul"}});
    let mut run = |q: &str| go_with(&e, &mut memo, nodes(), &[("v", "values", "m", q)], &["m"], json!([]), &CancelToken::default()).0;
    assert_eq!(run("a|1:").nodes["m"].state, NodeState::Done);
    assert_eq!(run("a|1:").nodes["m"].state, NodeState::Cached);
    assert_eq!(run("a| 1 : ").nodes["m"].state, NodeState::Cached, "the same slice, canonically");
    let r = run("a|:1");
    assert_eq!((r.nodes["m"].state, out(&r, "m", "out")), (NodeState::Done, json!([1.0])));
}

/// A plan key misses and the content key finds it: two different makers of the same value feed
/// the same node, and the second doesn't run. An array input isn't hashed unless the node asks.
#[test]
fn the_same_bytes_from_elsewhere_are_found_by_content() {
    let (e, mut memo) = (engine(), Memo::default());
    let run = |memo: &mut Memo, a: f64, b: f64| {
        let nodes = json!({"p": {"kind": "t.mul", "values": {"a": a, "b": b}}, "m": {"kind": "t.mul", "values": {"b": 10}}});
        go_with(&e, memo, nodes, &[("p", "out", "m", "a")], &["m"], json!([]), &CancelToken::default()).0
    };
    let first = run(&mut memo, 2.0, 3.0);
    assert_eq!(first.stats.as_ref().unwrap().computed, 2);
    // 6 again, made differently: p runs, m doesn't
    let second = run(&mut memo, 3.0, 2.0);
    let s = second.stats.as_ref().unwrap();
    assert_eq!((s.computed, s.from_content, s.from_memory), (1, 1, 0));
    let m = &second.nodes["m"];
    assert_eq!(m.state, NodeState::Cached);
    let info = m.memo.as_ref().unwrap();
    assert_eq!(info.source.as_deref(), Some("content"));
    assert!(info.content_key.is_some() && info.computed_ms.is_some());
    assert_eq!(out(&second, "m", "out"), json!(60.0));
    // and now its plan key is known
    let third = run(&mut memo, 3.0, 2.0);
    assert_eq!(third.stats.as_ref().unwrap().from_memory, 2);
    // a different value is a different result
    let other = run(&mut memo, 3.0, 3.0);
    assert_eq!((other.nodes["m"].state, out(&other, "m", "out")), (NodeState::Done, json!(90.0)));
}

#[test]
fn a_list_input_takes_the_list_whole() {
    let r = go(
        json!({"r": {"kind": "vary.range", "values": {"start": 0, "stop": 1, "num": 5}}, "s": {"kind": "t.sum"}}),
        &[("r", "values", "s", "xs")],
        &["r", "s"],
    );
    assert_eq!(out(&r, "r", "values"), json!([0.0, 0.25, 0.5, 0.75, 1.0]));
    assert_eq!(out(&r, "s", "sum"), json!(2.5));
}

#[test]
fn vary_range_2d_and_3d_zip_their_coordinates() {
    let nodes = json!({"p": {"kind": "vary.range2d", "values": {"start_x": 0, "stop_x": 1, "start_y": 10, "stop_y": 20, "num": 3}}, "m": {"kind": "t.mul"}});
    let r = go(nodes, &[("p", "x", "m", "a"), ("p", "y", "m", "b")], &["p", "m"]);
    assert_eq!(out(&r, "p", "x"), json!([0.0, 0.5, 1.0]));
    assert_eq!(out(&r, "p", "y"), json!([10.0, 15.0, 20.0]));
    assert_eq!(out(&r, "m", "out"), json!([0.0, 7.5, 20.0]), "points along the line, not every combination");
    // grid: every combination, x fastest
    let nodes = json!({"p": {"kind": "vary.range2d", "values": {"start_x": 1, "stop_x": 2, "start_y": 10, "stop_y": 20, "num": 2, "grid": true}}, "m": {"kind": "t.mul"}});
    let r = go(nodes, &[("p", "x", "m", "a"), ("p", "y", "m", "b")], &["p", "m"]);
    assert_eq!(out(&r, "p", "x"), json!([1.0, 2.0, 1.0, 2.0]));
    assert_eq!(out(&r, "p", "y"), json!([10.0, 10.0, 20.0, 20.0]));
    assert_eq!(out(&r, "m", "out"), json!([10.0, 20.0, 20.0, 40.0]));
    let r = go(json!({"p": {"kind": "vary.range3d", "values": {"num": 3, "grid": true, "endpoint": false}}}), &[], &["p"]);
    let z = out(&r, "p", "z");
    assert_eq!(z.as_array().map(Vec::len), Some(27));
    let x = out(&r, "p", "x");
    assert_eq!(x.as_array().and_then(|a| a.get(..3)).map(<[Value]>::to_vec), Some(vec![json!(0.0), json!(1.0 / 3.0), json!(2.0 / 3.0)]));
    let r = go(json!({"p": {"kind": "vary.range3d", "values": {"num": 11, "grid": true}}}), &[], &["p"]);
    assert!(!r.ok && r.nodes["p"].error.as_ref().is_some_and(|e| e.message.contains("1000")), "{r:?}");
}

#[test]
fn vary_choices_takes_the_choices_of_the_enum_it_feeds() {
    let nodes = |values: &str| json!({"c": {"kind": "vary.choices", "values": {"values": values}}, "m": {"kind": "t.mul"}});
    let r = go(nodes(""), &[("c", "values", "m", "mode")], &["c", "m"]);
    assert_eq!(out(&r, "m", "mode"), json!(["plain", "bold", "loud"]));
    let r = go(nodes("loud, bold"), &[("c", "values", "m", "mode")], &["m"]);
    assert_eq!(out(&r, "m", "mode"), json!(["loud", "bold"]));
    let r = go(nodes("shouty"), &[("c", "values", "m", "mode")], &["m"]);
    assert!(r.nodes["c"].error.as_ref().is_some_and(|e| e.message.contains("not among")), "{r:?}");
    let r = go(nodes(""), &[], &["c"]);
    assert!(r.nodes["c"].error.as_ref().is_some_and(|e| e.message.contains("blank")), "{r:?}");
}

#[test]
fn an_item_that_fails_fails_the_node_and_names_the_item() {
    let r = go(json!({"v": {"kind": "vary.float", "values": {"values": "1, -2, 3"}}, "m": {"kind": "t.mul"}}), &[("v", "values", "m", "a")], &["m"]);
    let e = r.nodes["m"].error.clone().unwrap();
    assert_eq!(e.kind, "config");
    assert!(e.message.starts_with("item 2 of 3:"), "{e:?}");
    let r = go(json!({"v": {"kind": "vary.float", "values": {"values": "1:5/0"}}}), &[], &["v"]);
    assert_eq!(r.nodes["v"].state, NodeState::Error);
}

#[test]
fn a_watched_list_shows_one_item_with_a_sheet_of_labels() {
    let nodes = json!({"a": {"kind": "vary.int", "values": {"values": "1, 2"}}, "b": {"kind": "vary.float", "values": {"values": "0.5, 0.25"}}, "m": {"kind": "t.mul"}});
    let wires = [("a", "values", "m", "a"), ("b", "values", "m", "b")];
    let (r, log) = go_with(&engine(), &mut Memo::default(), nodes, &wires, &["m"], json!([{"node": "m", "port": "out", "index": -1}]), &CancelToken::default());
    let d = &r.watch[0];
    assert_eq!((d.items, d.index), (4, 3));
    assert_eq!(d.value, Some(json!(0.5)));
    assert!(d.summary.starts_with("item 4 of 4 (a 2 · b 0.25)"), "{}", d.summary);
    let labels: Vec<&str> = d.sheet.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, ["a 1 · b 0.5", "a 1 · b 0.25", "a 2 · b 0.5", "a 2 · b 0.25"]);
    // a node mapped over a mapped node's list keeps what made each item, not its value
    let nodes = json!({"v": {"kind": "vary.float", "values": {"values": "1, 2"}}, "m": {"kind": "t.mul", "values": {"b": 3}}, "n": {"kind": "t.mul"}});
    let (r, _) = go_with(
        &engine(),
        &mut Memo::default(),
        nodes,
        &[("v", "values", "m", "a"), ("m", "out", "n", "a")],
        &["n"],
        json!([{"node": "n", "port": "out"}]),
        &CancelToken::default(),
    );
    let labels: Vec<&str> = r.watch[0].sheet.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, ["a 1", "a 2"]);
    // progress per item, scaled into the whole
    let fractions: Vec<f64> = log.iter().filter(|r| r.node_id == "m" && r.state == NodeState::Progress).filter_map(|r| r.fraction).collect();
    assert!(!fractions.is_empty() && fractions.iter().all(|f| (0.0..1.0).contains(f)), "{fractions:?}");
}

#[test]
fn a_mapped_node_is_memoised_whole_and_cancel_stops_between_items() {
    let e = engine();
    let mut memo = Memo::default();
    let nodes = json!({"v": {"kind": "vary.float", "values": {"values": "1, 2, 3"}}, "m": {"kind": "t.mul"}});
    let wires = [("v", "values", "m", "a")];
    let (first, _) = go_with(&e, &mut memo, nodes.clone(), &wires, &["m"], json!([]), &CancelToken::default());
    let (again, _) = go_with(&e, &mut memo, nodes.clone(), &wires, &["m"], json!([]), &CancelToken::default());
    assert_eq!(again.nodes["m"].state, NodeState::Cached);
    assert_eq!(out(&again, "m", "out"), out(&first, "m", "out"));
    let cancel = CancelToken::default();
    cancel.cancel();
    let (r, _) = go_with(&e, &mut Memo::default(), nodes, &wires, &["m"], json!([]), &cancel);
    assert!(!r.ok);
}

// --- Vary Range 2D's tuples, Zip, Outer Join, Unzip -------------------------------------

#[test]
fn vary_range_2d_gives_points_as_tuples_that_unzip_back() {
    let nodes = json!({"p": {"kind": "vary.range2d", "values": {"start_x": 0, "stop_x": 1, "start_y": 10, "stop_y": 20, "num": 3}}, "u": {"kind": "seq.unzip"}, "m": {"kind": "t.mul"}});
    let r = go(nodes, &[("p", "xy", "u", "tuples"), ("u", "a", "m", "a"), ("u", "b", "m", "b")], &["p", "u", "m"]);
    assert!(r.ok, "{r:?}");
    assert_eq!(out(&r, "p", "xy"), json!([[0.0, 10.0], [0.5, 15.0], [1.0, 20.0]]));
    assert_eq!(out(&r, "u", "a"), json!([0.0, 0.5, 1.0]));
    assert_eq!(out(&r, "m", "out"), json!([0.0, 7.5, 20.0]), "unzipped parts zip downstream");
    let r = go(json!({"p": {"kind": "vary.range3d", "values": {"num": 2}}}), &[], &["p"]);
    assert_eq!(out(&r, "p", "xyz"), json!([[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]]));
}

#[test]
fn zip_pairs_in_order_and_outer_join_makes_every_combination() {
    let nodes = |kind: &str, b: &str| json!({"a": {"kind": "vary.int", "values": {"values": "1, 2"}}, "b": {"kind": "vary.string", "values": {"values": b}}, "z": {"kind": kind}, "u": {"kind": "seq.unzip"}});
    let wires = [("a", "values", "z", "a"), ("b", "values", "z", "b"), ("z", "tuples", "u", "tuples")];
    let r = go(nodes("seq.zip", "x, y"), &wires, &["z", "u"]);
    assert_eq!(out(&r, "z", "tuples"), json!([[1, "x"], [2, "y"]]));
    assert_eq!(out(&r, "u", "b"), json!(["x", "y"]));
    let r = go(nodes("seq.zip", "x, y, z"), &wires, &["z"]);
    assert!(r.nodes["z"].error.as_ref().is_some_and(|e| e.message.contains("differ in length")), "{r:?}");
    let r = go(nodes("seq.outer", "x, y, z"), &wires, &["z", "u"]);
    assert_eq!(out(&r, "z", "tuples"), json!([[1, "x"], [1, "y"], [1, "z"], [2, "x"], [2, "y"], [2, "z"]]));
    assert_eq!(out(&r, "u", "a"), json!([1, 1, 1, 2, 2, 2]));
    // a single value goes with every item
    let r = go(
        json!({"a": {"kind": "vary.int", "values": {"values": "1, 2, 3"}}, "z": {"kind": "seq.zip"}, "t": {"kind": "t.mul", "values": {"a": 7}}}),
        &[("a", "values", "z", "a"), ("t", "out", "z", "b")],
        &["z"],
    );
    assert_eq!(out(&r, "z", "tuples"), json!([[1, 7.0], [2, 7.0], [3, 7.0]]));
    // tuples are labelled by their parts
    let (r, _) = go_with(
        &engine(),
        &mut Memo::default(),
        nodes("seq.outer", "x, y"),
        &wires,
        &["z"],
        json!([{"node": "z", "port": "tuples", "index": 1}]),
        &CancelToken::default(),
    );
    let labels: Vec<&str> = r.watch[0].sheet.iter().map(|c| c.label.as_str()).collect();
    assert_eq!(labels, ["a 1 · b x", "a 1 · b y", "a 2 · b x", "a 2 · b y"]);
    assert_eq!(r.watch[0].value, Some(json!([1, "y"])));
}

// --- Select, Find, Load and Write Files --------------------------------------------------

fn tmp(name: &str) -> std::path::PathBuf {
    let d = plumcraft_scratch::root().join(format!("files-{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("sub")).unwrap();
    d
}

#[test]
fn find_files_lists_a_folder_and_notices_a_new_file() {
    let d = tmp("find");
    for (name, body) in [("b.txt", "bee"), ("a.TXT", "ay"), ("c.png", "x"), (".hidden.txt", "h"), ("sub/d.txt", "dee")] {
        std::fs::write(d.join(name), body).unwrap();
    }
    let e = engine();
    let mut memo = Memo::default();
    let find =
        |recursive: bool| json!({"f": {"kind": "file.find", "values": {"folder": d, "pattern": "*.txt", "recursive": recursive}}, "l": {"kind": "file.load"}});
    let wires = [("f", "files", "l", "file")];
    let (r, _) = go_with(&e, &mut memo, find(false), &wires, &["f", "l"], json!([{"node": "l", "port": "value"}]), &CancelToken::default());
    assert!(r.ok, "{r:?}");
    let files = out(&r, "f", "files");
    assert_eq!(files, json!([d.join("a.TXT"), d.join("b.txt")]));
    assert_eq!(r.watch[0].items, 2);
    assert_eq!(r.watch[0].sheet.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(), ["a", "b"]);
    let (r, _) = go_with(&e, &mut memo, find(true), &wires, &["f"], json!([]), &CancelToken::default());
    assert_eq!(out(&r, "f", "files").as_array().map(Vec::len), Some(3));
    // unchanged: cached; a new file: found
    let (r, _) = go_with(&e, &mut memo, find(false), &wires, &["f"], json!([]), &CancelToken::default());
    assert_eq!(r.nodes["f"].state, NodeState::Cached);
    std::fs::write(d.join("e.txt"), "e").unwrap();
    let (r, _) = go_with(&e, &mut memo, find(false), &wires, &["f"], json!([]), &CancelToken::default());
    assert_eq!(r.nodes["f"].state, NodeState::Done);
    assert_eq!(out(&r, "f", "files").as_array().map(Vec::len), Some(3));
    let r = go(json!({"f": {"kind": "file.find", "values": {"folder": d, "pattern": "*.nope"}}}), &[], &["f"]);
    assert!(r.nodes["f"].error.as_ref().is_some_and(|e| e.message.contains("no files match")));
}

#[test]
fn select_load_and_write_files_name_each_file_by_what_made_it() {
    let d = tmp("write");
    std::fs::write(d.join("corgi.svg"), "<svg/>").unwrap();
    std::fs::write(d.join("plum.svg"), "<svg></svg>").unwrap();
    let outdir = d.join("out");
    let files = format!("{}\n{}", d.join("corgi.svg").display(), d.join("plum.svg").display());
    let nodes = |overwrite: bool| {
        json!({"s": {"kind": "file.select", "values": {"files": files}}, "l": {"kind": "file.load"},
               "w": {"kind": "file.write", "values": {"folder": outdir, "basename": "copy", "overwrite": overwrite}}})
    };
    let wires = [("s", "files", "l", "file"), ("l", "value", "w", "values")];
    let r = go(nodes(false), &wires, &["w"]);
    assert!(r.ok, "{r:?}");
    assert_eq!(out(&r, "w", "files"), json!([outdir.join("copy_corgi.svg"), outdir.join("copy_plum.svg")]));
    assert_eq!(std::fs::read_to_string(outdir.join("copy_plum.svg")).unwrap(), "<svg></svg>");
    // the same again: nothing to replace
    assert!(go(nodes(false), &wires, &["w"]).ok);
    // a different file there: refused unless overwrite
    std::fs::write(outdir.join("copy_plum.svg"), "changed").unwrap();
    let r = go(nodes(false), &wires, &["w"]);
    assert!(r.nodes["w"].error.as_ref().is_some_and(|e| e.message.contains("overwrite")), "{r:?}");
    assert_eq!(std::fs::read_to_string(outdir.join("copy_plum.svg")).unwrap(), "changed", "nothing written");
    assert!(go(nodes(true), &wires, &["w"]).ok);
    assert_eq!(std::fs::read_to_string(outdir.join("copy_plum.svg")).unwrap(), "<svg></svg>");
    // a swept setting names the files too; plain values are written as text or JSON
    let r = go(
        json!({"v": {"kind": "vary.float", "values": {"values": "0.5, 2"}}, "m": {"kind": "t.mul", "values": {"b": 2}}, "w": {"kind": "file.write", "values": {"folder": outdir, "basename": "m"}}}),
        &[("v", "values", "m", "a"), ("m", "out", "w", "values")],
        &["w"],
    );
    assert_eq!(out(&r, "w", "files"), json!([outdir.join("m_a-0.5.json"), outdir.join("m_a-2.json")]));
    assert_eq!(std::fs::read_to_string(outdir.join("m_a-2.json")).unwrap(), "4.0");
    let r = go(json!({"s": {"kind": "file.select", "values": {"files": ""}}}), &[], &["s"]);
    assert!(r.nodes["s"].error.as_ref().is_some_and(|e| e.message.contains("no files chosen")));
}
