//! FR1: a value a node names an editor for (`NodeDef.editor`) promoted to a group's face, as a
//! row that opens the editor. The public test engine's `ports.curve` names one (`ports.curve` on
//! `curve`) that no application registers, as on the native shells: the promoted row shows the
//! member's short form, and a tap on it is the member's own `node.editor`, which opens the core's
//! value window (the value as JSON), the only editor these shells have.

use plumcraft_editor_core::document::{Document, execute};
use plumcraft_editor_core::gesture;
use plumcraft_session::Session;
use serde_json::{Value, json};

fn gesture(d: &mut Document, g: Value) -> Value {
    let sc = plumcraft_editor_core::control::Shell::scene(d).unwrap();
    d.publish_scene(sc);
    let snap = std::sync::Arc::clone(&d.snapshot);
    let req = gesture::parse(&g).unwrap();
    let plan = {
        let f = gesture::Facts { graph: d.session.graph(), selection: d.selection.get(), drag: &d.drag };
        gesture::plan(&f, &snap, &req).unwrap()
    };
    let mut commands = Vec::new();
    for (c, p) in plan.commands {
        let r = execute(d, c, &p);
        commands.push(json!({"command": c, "params": p, "ok": r.is_ok(), "error": r.err()}));
    }
    json!({"commands": commands})
}

#[test]
fn a_promoted_editor_value_opens_the_members_editor_from_the_groups_face() {
    let mut d = Document::new(Session::new(plumcraft_ports::engine().schema()));
    let n = execute(&mut d, "node.add", &json!({"kind": "ports.curve", "pos": [0, 0]})).unwrap()["id"].as_str().unwrap().to_owned();
    // promoted by the same command and menu item as any input
    let g = execute(&mut d, "group.create", &json!({"nodes": [n], "name": "Shapes"})).unwrap()["id"].as_str().unwrap().to_owned();
    // the node's menu offers it under Promote input, as any input (the item is the command run next)
    let items = plumcraft_editor_core::menu::context(
        &plumcraft_editor_core::menu::Parts::of(&d.session),
        &plumcraft_editor_core::menu::Facts::default(),
        &plumcraft_editor_core::menu::Context::Node { node: plumcraft_graph::NodeId::from(n.as_str()) },
    );
    let text = serde_json::to_string(&items).unwrap();
    assert!(text.contains(r#""command":"group.promote""#) && text.contains(r#""port":"curve""#), "Promote input › curve: {text}");
    execute(&mut d, "group.promote", &json!({"group": g, "port": {"node": n, "port": "curve"}, "label": "curve"})).unwrap();
    execute(&mut d, "group.collapse", &json!({"group": g, "collapsed": true})).unwrap();
    let sc = plumcraft_editor_core::control::Shell::scene(&d).unwrap();
    let proxy = sc.nodes.iter().find(|f| f.id == format!("group:{g}")).expect("the group's face");
    let row = &proxy.inputs[0];
    assert_eq!((row.key.as_str(), row.node.as_str(), row.source.as_str()), ("curve", n.as_str(), "readonly"));
    assert_eq!(row.text.as_deref(), Some("curve: [[0.0,0.0],[0.5,0.6],[1.0,1.0]]"), "the member's short form");
    let e = row.editor.as_ref().expect("a row that opens the editor");
    assert_eq!((e.kind.as_str(), e.label.as_str()), ("ports.curve", "Edit Value…"));
    // the shell measured the face, the row's caption as what a tap lands on
    execute(
        &mut d,
        "geometry.set",
        &json!({"faces": [{"node": format!("group:{g}"), "size": [200, 90], "header": 28, "pins": {"in": [43], "out": [43]},
                           "rows": [[10, 32, 120, 54]]}]}),
    )
    .unwrap();
    let tap = gesture(&mut d, json!({"kind": "tap", "at": [60, 43], "slop": 2}));
    let ran: Vec<(String, Value)> =
        tap["commands"].as_array().unwrap().iter().map(|c| (c["command"].as_str().unwrap().to_owned(), c["params"].clone())).collect();
    assert_eq!(ran.last(), Some(&("node.editor".to_owned(), json!({"node": n}))), "{tap}");
    assert!(tap["commands"].as_array().unwrap().iter().all(|c| c["ok"] == true), "{tap}");
    // what it opened: the core's value window on the member's value
    let w = plumcraft_editor_core::reports::get(&d.session, &json!({"window": "editor", "node": n})).unwrap();
    assert_eq!((w["kind"].as_str(), w["port"].as_str()), (Some("ports.curve"), Some("curve")));
}

/// FR1 merged with B133's Browse: on one node with a path row and an editor value, and on its
/// collapsed group with both promoted, each tap goes where it should: the Browse square
/// `row.browse`, the path's field `row.edit`, the editor value's row `node.editor`.
#[test]
fn browse_field_and_editor_rows_each_take_their_own_tap() {
    let catalog = serde_json::from_value(json!({"nodes": [{"id": "t.both", "display": "Both",
        "editor": {"kind": "t.dial", "port": "curve"},
        "inputs": [{"name": "file", "type": "STRING", "default": "", "path": {"mode": "open", "filters": []}},
                   {"name": "curve", "type": "x.t.curve", "socket": "both", "default": {"pts": [0, 1]}}],
        "outputs": [{"name": "out", "type": "STRING"}]}]}))
    .unwrap();
    let mut d = Document::new(Session::new(catalog));
    let n = execute(&mut d, "node.add", &json!({"kind": "t.both", "pos": [0, 0]})).unwrap()["id"].as_str().unwrap().to_owned();
    // measured as a shell does: the path field's rect, the editor row's caption
    let measure = |d: &mut Document, face: &str| {
        execute(
            d,
            "geometry.set",
            &json!({"faces": [{"node": face, "size": [260, 90], "header": 28, "pins": {"in": [43, 65], "out": [43]},
                                                    "rows": [[100, 34, 250, 52], [10, 56, 120, 74]]}]}),
        )
        .unwrap();
    };
    let last = |t: &Value| t["commands"].as_array().and_then(|c| c.last()).map(|c| (c["command"].as_str().unwrap_or("").to_owned(), c["ok"] == true));
    measure(&mut d, &n);
    // the Browse square: the right end of the field, its side the field's height (18)
    // (row.browse is invoked, as `ui.gesture` does; this harness runs the plan's commands
    // directly, so only where the tap went is checked for it)
    assert_eq!(last(&gesture(&mut d, json!({"kind": "tap", "at": [241, 43], "slop": 1}))).map(|c| c.0), Some("row.browse".into()));
    assert_eq!(last(&gesture(&mut d, json!({"kind": "tap", "at": [150, 43], "slop": 1}))), Some(("row.edit".into(), true)));
    execute(&mut d, "row.edit", &json!({"open": false})).ok();
    assert_eq!(last(&gesture(&mut d, json!({"kind": "tap", "at": [60, 65], "slop": 1}))), Some(("node.editor".into(), true)));
    // collapsed, both promoted (in that order: rows 0 and 1 of the group's face)
    let g = execute(&mut d, "group.create", &json!({"nodes": [n], "name": "Both"})).unwrap()["id"].as_str().unwrap().to_owned();
    execute(&mut d, "group.promote", &json!({"group": g, "port": {"node": n, "port": "file"}, "label": "file"})).unwrap();
    execute(&mut d, "group.promote", &json!({"group": g, "port": {"node": n, "port": "curve"}, "label": "curve"})).unwrap();
    execute(&mut d, "group.collapse", &json!({"group": g, "collapsed": true})).unwrap();
    measure(&mut d, &format!("group:{g}"));
    // (row.browse is invoked, as `ui.gesture` does; this harness runs the plan's commands
    // directly, so only where the tap went is checked for it)
    assert_eq!(last(&gesture(&mut d, json!({"kind": "tap", "at": [241, 43], "slop": 1}))).map(|c| c.0), Some("row.browse".into()));
    assert_eq!(last(&gesture(&mut d, json!({"kind": "tap", "at": [150, 43], "slop": 1}))), Some(("row.edit".into(), true)));
    execute(&mut d, "row.edit", &json!({"open": false})).ok();
    let t = gesture(&mut d, json!({"kind": "tap", "at": [60, 65], "slop": 1}));
    assert_eq!(last(&t), Some(("node.editor".into(), true)), "{t}");
    assert_eq!(t["commands"].as_array().unwrap().last().unwrap()["params"], json!({"node": n}), "the member's");
}

/// B173: a node naming the core's mask editor on a path input (an application's "edit an
/// intermediate" node): its row is a path field with a Browse button and a caption drawn as a
/// link. A shell measures the row from its caption to its field's end; a tap on the caption opens
/// the editor (before, the caption was not part of the row and a tap only selected the node), the
/// Browse square still browses; the node's menu offers "Edit Mask…" (before, the C ABI's shells
/// passed no editors, so the item was never there), and the row says the same words.
#[test]
fn a_path_row_naming_the_mask_editor_opens_it_by_its_caption_and_the_menu() {
    let catalog = serde_json::from_value(json!({"nodes": [{"id": "t.edit", "display": "Edit",
        "editor": {"kind": "mask", "port": "mask"},
        "inputs": [{"name": "image", "type": "ARRAY", "socket": "socket"},
                   {"name": "mask", "type": "STRING", "default": "", "path": {"mode": "save", "filters": []}}],
        "outputs": [{"name": "out", "type": "ARRAY"}]}]}))
    .unwrap();
    let mut d = Document::new(Session::new(catalog));
    let n = execute(&mut d, "node.add", &json!({"kind": "t.edit", "pos": [0, 0]})).unwrap()["id"].as_str().unwrap().to_owned();
    let sc = plumcraft_editor_core::control::Shell::scene(&d).unwrap();
    let row = sc.nodes.iter().find(|f| f.id == n).unwrap().inputs.iter().find(|r| r.key == "mask").unwrap().clone();
    let e = row.editor.as_ref().expect("the row opens the editor");
    assert_eq!((e.kind.as_str(), e.label.as_str()), ("mask", "Edit Mask…"), "the core's own editor's words");
    assert!(row.widget.is_some(), "and keeps its path field");
    // the menu, as a native shell's (the C ABI document's facts)
    let facts = plumcraft_editor_core::control::Shell::menu_facts(&d);
    let items = plumcraft_editor_core::menu::context(
        &plumcraft_editor_core::menu::Parts::of(&d.session),
        &facts,
        &plumcraft_editor_core::menu::Context::Node { node: plumcraft_graph::NodeId::from(n.as_str()) },
    );
    let item = items.iter().find(|i| i.command.as_deref() == Some("node.editor")).expect("Edit Mask… in the node's menu");
    assert_eq!(item.label, "Edit Mask…");
    // measured as the Apple and Windows shells now do: the caption (x 10) to the field's end (250)
    execute(
        &mut d,
        "geometry.set",
        &json!({"faces": [{"node": n, "size": [260, 90], "header": 28, "pins": {"in": [43, 65], "out": [43]},
                           "rows": [null, [10, 56, 250, 74]]}]}),
    )
    .unwrap();
    let last = |t: &Value| t["commands"].as_array().and_then(|c| c.last()).map(|c| (c["command"].as_str().unwrap_or("").to_owned(), c["ok"] == true));
    let t = gesture(&mut d, json!({"kind": "tap", "at": [30, 65], "slop": 1}));
    assert_eq!(last(&t), Some(("node.editor".into(), true)), "the caption: {t}");
    execute(&mut d, "node.editor", &json!({"node": n, "open": false})).ok();
    assert_eq!(last(&gesture(&mut d, json!({"kind": "tap", "at": [241, 65], "slop": 1}))).map(|c| c.0), Some("row.browse".into()), "the Browse square");
    // wired, the value isn't the editor's: no item
    let mut d2 = Document::new(Session::new(
        serde_json::from_value(json!({"nodes": [{"id": "t.edit", "display": "Edit",
        "editor": {"kind": "mask", "port": "mask"},
        "inputs": [{"name": "mask", "type": "STRING", "default": "", "socket": "both"}], "outputs": [{"name": "out", "type": "STRING"}]},
        {"id": "t.src", "display": "Src", "inputs": [], "outputs": [{"name": "out", "type": "STRING"}]}]}))
        .unwrap(),
    ));
    let a = execute(&mut d2, "node.add", &json!({"kind": "t.src", "pos": [0, 0]})).unwrap()["id"].as_str().unwrap().to_owned();
    let b = execute(&mut d2, "node.add", &json!({"kind": "t.edit", "pos": [300, 0]})).unwrap()["id"].as_str().unwrap().to_owned();
    execute(&mut d2, "wire.connect", &json!({"from": {"node": a, "port": "out"}, "to": {"node": b, "port": "mask"}})).unwrap();
    let facts = plumcraft_editor_core::control::Shell::menu_facts(&d2);
    let items = plumcraft_editor_core::menu::context(
        &plumcraft_editor_core::menu::Parts::of(&d2.session),
        &facts,
        &plumcraft_editor_core::menu::Context::Node { node: plumcraft_graph::NodeId::from(b.as_str()) },
    );
    assert!(!items.iter().any(|i| i.command.as_deref() == Some("node.editor")));
}
