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
