//! Net label commands (`plumcraft_graph::nets`): draw a wire through a named net, as a schematic
//! draws a net label, and back. Labels are only how wires are drawn, so none of these changes
//! what runs.
//!
//! A net is named by `name` and, for one local to a group, `group` (its id); results name a net
//! the same way, `{name, group}`.
//!
//! Lifted from the research software's `crates/engine/src/net_cmds.rs` (2026-10-31, the research software `8480c03`).

use std::collections::BTreeMap;

use plumcraft_graph::{GroupId, NetKey, PortRef};
use serde_json::{Value, json};

use crate::commands::{CommandSpec, always, port_param, str_param};
use crate::{Result, Session, SessionError};

fn has_nets(s: &Session) -> std::result::Result<(), String> {
    if s.graph().has_nets() { Ok(()) } else { Err("no wire is drawn through a net label".into()) }
}

/// `{name, group?}`: a net, at the top level or local to a group.
fn key_param(p: &Value) -> Result<NetKey> {
    let name = str_param(p, "name")?;
    let group = match p.get("group") {
        None | Some(Value::Null) => None,
        Some(Value::String(g)) => Some(GroupId(g.clone())),
        Some(_) => return Err(SessionError::Params("\"group\" must be a group id or null".into())),
    };
    Ok(NetKey::new(group, name))
}

fn key_json(k: &NetKey) -> Value {
    json!({"name": k.name, "group": k.group})
}

/// `at`: `[dx, y]` (whole graph units, rounded) or null (beside the pin).
fn offset_param(p: &Value) -> Result<Option<[i32; 2]>> {
    let bad = || SessionError::Params(format!("\"at\" must be [dx, dy] within ±{} or null", plumcraft_graph::MAX_LABEL_OFFSET));
    match p.get("at") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(a)) => {
            let [x, y] = a.as_slice() else { return Err(bad()) };
            let c = |v: &Value| {
                v.as_f64().filter(|f| f.is_finite() && f.abs() <= f64::from(plumcraft_graph::MAX_LABEL_OFFSET)).map(|f| f.round() as i32).ok_or_else(bad)
            };
            Ok(Some([c(x)?, c(y)?]))
        }
        Some(_) => Err(bad()),
    }
}

/// `wire.label {to, name}`: draw the wire into `to` through net `name`.
fn wire_label(s: &mut Session, p: &Value) -> Result<Value> {
    let (to, name) = (port_param(p, "to")?, str_param(p, "name")?);
    let key = s.edit("Draw as Label", |g, _| Ok(g.label_wire(&to, name)?))?;
    Ok(key_json(&key))
}

/// `wire.unlabel {to}`: draw it as a plain wire again.
fn wire_unlabel(s: &mut Session, p: &Value) -> Result<Value> {
    let to = port_param(p, "to")?;
    let key = s.edit("Draw as Wire", |g, _| Ok(g.unlabel_wire(&to)?))?;
    Ok(key.as_ref().map_or(json!({"name": null}), key_json))
}

/// Each node's Tidy column, for "wires spanning at least n columns".
fn columns(s: &Session) -> BTreeMap<plumcraft_graph::NodeId, usize> {
    let mut g = s.graph().clone();
    let (ids, layout) = crate::layout_cmds::tidy(&mut g, s.catalog(), &[], &BTreeMap::new(), &BTreeMap::new());
    ids.into_iter().zip(layout.column).collect()
}

/// `output.label {from, name, minColumns?}`: draw every wire from `from` (or those spanning at
/// least `minColumns` of Tidy's columns) through net `name`.
fn output_label(s: &mut Session, p: &Value) -> Result<Value> {
    let (from, name) = (port_param(p, "from")?, str_param(p, "name")?);
    let min = match p.get("minColumns") {
        None | Some(Value::Null) => 0,
        Some(v) => v.as_u64().ok_or_else(|| SessionError::Params("\"minColumns\" must be a whole number".into()))?,
    };
    let cols = if min > 0 { columns(s) } else { BTreeMap::new() };
    let span = |w: &plumcraft_graph::Wire| {
        let (a, b) = (cols.get(&w.from.node).copied().unwrap_or(0), cols.get(&w.to.node).copied().unwrap_or(0));
        b.saturating_sub(a) as u64 >= min
    };
    let (key, tos) = s.edit("Label All Uses", |g, _| Ok(g.label_output(&from, name, span)?))?;
    let mut out = key_json(&key);
    out["wires"] = json!(tos);
    Ok(out)
}

/// `net.rename {name, group?, to}`.
fn net_rename(s: &mut Session, p: &Value) -> Result<Value> {
    let (key, to) = (key_param(p)?, str_param(p, "to")?);
    let key = s.edit("Rename Net", |g, _| Ok(g.rename_net(&key, to)?))?;
    Ok(key_json(&key))
}

/// `net.connect {name, group?, to}`: wire input `to` from the net's driver, drawn through it.
fn net_connect(s: &mut Session, p: &Value) -> Result<Value> {
    let (key, to) = (key_param(p)?, port_param(p, "to")?);
    let replaced = s.edit("Connect to Net", |g, catalog| Ok(g.connect_net(catalog, &key, &to)?))?;
    Ok(json!({"replaced": replaced.as_ref().map(crate::graph_cmds::wire_json)}))
}

/// `net.move {name, group?, at}`: move the driver's label (not an undo step, as moving a node
/// isn't).
fn net_move(s: &mut Session, p: &Value) -> Result<Value> {
    let (key, at) = (key_param(p)?, offset_param(p)?);
    s.graph_mut_untracked().move_net_label(&key, at)?;
    s.mark_dirty();
    Ok(Value::Null)
}

/// `wire.labelAt {to, at}`: move a wire's receiving label (not an undo step).
fn wire_label_at(s: &mut Session, p: &Value) -> Result<Value> {
    let (to, at): (PortRef, _) = (port_param(p, "to")?, offset_param(p)?);
    s.graph_mut_untracked().move_wire_label(&to, at)?;
    s.mark_dirty();
    Ok(Value::Null)
}

pub(crate) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "wire.label",
            label: "Draw as Label",
            menu: &[],
            shortcut: None,
            params: "{to: {node, port}, name} -> {name, group} (draws the wire into `to` as a net label at each end; creates net `name` on the wire's output, local to the innermost group holding both ends (group: its id, null at the top level); refuses a name another output's net holds where this one would be seen; one undo step; what runs is unchanged)",
            enabled: always,
            run: wire_label,
            journal: true,
        },
        CommandSpec {
            id: "wire.unlabel",
            label: "Draw as Wire",
            menu: &[],
            shortcut: None,
            params: "{to: {node, port}} -> {name, group}: the net it was drawn through, or {name: null} (the net goes with its last wire; one undo step)",
            enabled: always,
            run: wire_unlabel,
            journal: true,
        },
        CommandSpec {
            id: "output.label",
            label: "Label All Uses",
            menu: &[],
            shortcut: None,
            params: "{from: {node, port}, name, minColumns?: n} -> {name, group, wires: [to]} (draws every wire from the output through net `name`, or only those spanning at least n of Tidy's columns; one undo step)",
            enabled: always,
            run: output_label,
            journal: true,
        },
        CommandSpec {
            id: "net.rename",
            label: "Rename Net",
            menu: &[],
            shortcut: None,
            params: "{name, group?, to} -> {name, group} (group: the id of the group the net is local to; refuses a name a net it is seen with has; one undo step)",
            enabled: has_nets,
            run: net_rename,
            journal: true,
        },
        CommandSpec {
            id: "net.connect",
            label: "Connect to Net",
            menu: &[],
            shortcut: None,
            params: "{name, group?, to: {node, port}} -> {replaced: wire|null} (wires the input from the net's driver, drawn through the net, type-checked as wire.connect; a pin outside the net's group takes the net out to the group holding both; one undo step)",
            enabled: has_nets,
            run: net_connect,
            journal: true,
        },
        CommandSpec {
            id: "net.move",
            label: "Move Net Label",
            menu: &[],
            shortcut: None,
            params: "{name, group?, at: [dx, dy] | null} (the driver's label, as an offset from its output pin; null: beside it; not an undo step)",
            enabled: has_nets,
            run: net_move,
            journal: false,
        },
        CommandSpec {
            id: "wire.labelAt",
            label: "Move Wire Label",
            menu: &[],
            shortcut: None,
            params: "{to: {node, port}, at: [dx, dy] | null} (the wire's receiving label, as an offset from the input pin; null: beside it; not an undo step)",
            enabled: has_nets,
            run: wire_label_at,
            journal: false,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use plumcraft_graph::NodeId;

    fn session() -> Session {
        Session::new(plumcraft_text::engine().schema())
    }

    fn add(s: &mut Session, kind: &str) -> String {
        s.execute("node.add", &json!({"kind": kind})).unwrap()["id"].as_str().unwrap().to_owned()
    }

    fn wire(s: &mut Session, from: &str, out: &str, to: &str, inp: &str) {
        s.execute("wire.connect", &json!({"from": {"node": from, "port": out}, "to": {"node": to, "port": inp}})).unwrap();
    }

    /// literal -> upper -> upper -> join, and the literal straight into the join too (a
    /// 3-column wire). Returns (literal, first upper, second upper, join).
    fn chain(s: &mut Session) -> (String, String, String, String) {
        let img = add(s, "text.literal");
        let lab = add(s, "text.upper");
        let grad = add(s, "text.upper");
        let edge = add(s, "text.join");
        wire(s, &img, "text", &lab, "text");
        wire(s, &lab, "text", &grad, "text");
        wire(s, &grad, "text", &edge, "a");
        wire(s, &img, "text", &edge, "b");
        (img, lab, grad, edge)
    }

    #[test]
    fn label_commands_are_undo_steps_and_change_nothing_that_runs() {
        let mut s = session();
        let (img, lab, _, edge) = chain(&mut s);
        let before = s.graph().clone();
        assert!(s.execute("net.rename", &json!({"name": "a", "to": "b"})).is_err(), "disabled with no nets");
        let r = s.execute("output.label", &json!({"from": {"node": img, "port": "text"}, "name": "picture"})).unwrap();
        assert_eq!(r["wires"].as_array().unwrap().len(), 2);
        assert_eq!(s.undo_label(), Some("Label All Uses"));
        // the graph the engine reads is the same, apart from how wires are drawn
        let strip = |g: &plumcraft_graph::Graph| g.wires.iter().map(|w| (w.from.clone(), w.to.clone())).collect::<Vec<_>>();
        assert_eq!(strip(s.graph()), strip(&before));
        assert_eq!(s.graph().nodes, before.nodes);
        s.execute("net.rename", &json!({"name": "picture", "to": "rgb"})).unwrap();
        s.execute("wire.unlabel", &json!({"to": {"node": lab, "port": "text"}})).unwrap();
        assert_eq!(s.graph().net_wires(&NetKey::top("rgb")).count(), 1);
        s.execute("wire.labelAt", &json!({"to": {"node": edge, "port": "b"}, "at": [-80.4, 3]})).unwrap();
        assert_eq!(s.graph().wire_into(&PortRef::new(edge.as_str(), "b")).unwrap().label_at, Some([-80, 3]));
        assert_eq!(s.undo_label(), Some("Draw as Wire"), "moving a label isn't an undo step");
        for _ in 0..3 {
            s.execute("edit.undo", &Value::Null).unwrap();
        }
        assert!(!s.graph().has_nets());
        assert_eq!(s.graph().wires, before.wires);
    }

    #[test]
    fn min_columns_labels_only_the_long_wires() {
        let mut s = session();
        let (img, lab, _, edge) = chain(&mut s);
        let r = s.execute("output.label", &json!({"from": {"node": img, "port": "text"}, "name": "picture", "minColumns": 2})).unwrap();
        assert_eq!(r["wires"], json!([{"node": edge, "port": "b"}]));
        assert!(s.graph().wire_into(&PortRef::new(lab.as_str(), "text")).unwrap().label.is_none());
        // none that long: refused, nothing changed
        assert!(s.execute("output.label", &json!({"from": {"node": lab, "port": "text"}, "name": "g", "minColumns": 5})).is_err());
        assert_eq!(s.graph().nets.len(), 1);
    }

    #[test]
    fn net_connect_wires_by_name_and_bad_params_are_errors() {
        let mut s = session();
        let (img, lab, _, _) = chain(&mut s);
        s.execute("wire.label", &json!({"to": {"node": lab, "port": "text"}, "name": "picture"})).unwrap();
        let grad2 = add(&mut s, "text.upper");
        let r = s.execute("net.connect", &json!({"name": "picture", "to": {"node": grad2, "port": "text"}})).unwrap();
        assert_eq!(r["replaced"], Value::Null);
        let w = s.graph().wire_into(&PortRef::new(grad2.as_str(), "text")).unwrap();
        assert_eq!((&w.from.node, w.label.as_deref()), (&NodeId::from(img.as_str()), Some("picture")));
        for (id, p) in [
            ("wire.label", json!({"to": {"node": lab, "port": "text"}, "name": ""})),
            ("wire.label", json!({"to": {"node": lab, "port": "text"}})),
            ("net.connect", json!({"name": "nope", "to": {"node": grad2, "port": "text"}})),
            ("net.move", json!({"name": "picture", "at": [1e9, 0]})),
            ("net.move", json!({"name": "picture", "at": "x"})),
            ("wire.labelAt", json!({"to": {"node": grad2, "port": "nope"}, "at": [0, 0]})),
            ("output.label", json!({"from": {"node": img, "port": "text"}, "name": "p", "minColumns": -1})),
        ] {
            assert!(s.execute(id, &p).is_err(), "{id} {p}");
        }
        s.execute("net.move", &json!({"name": "picture", "at": [20, -12]})).unwrap();
        assert_eq!(s.graph().nets["picture"].at, Some([20, -12]));
        s.execute("net.move", &json!({"name": "picture", "at": null})).unwrap();
        assert_eq!(s.graph().nets["picture"].at, None);
    }

    #[test]
    fn a_net_in_a_group_is_named_by_its_group() {
        let mut s = session();
        let (img, lab, grad, edge) = chain(&mut s);
        s.execute("group.create", &json!({"nodes": [img, lab, grad], "name": "inner"})).unwrap();
        let r = s.execute("wire.label", &json!({"to": {"node": lab, "port": "text"}, "name": "picture"})).unwrap();
        assert_eq!(r, json!({"name": "picture", "group": "g1"}));
        // named without its group, it isn't found
        assert!(s.execute("net.rename", &json!({"name": "picture", "to": "rgb"})).is_err());
        let r = s.execute("net.rename", &json!({"name": "picture", "group": "g1", "to": "rgb"})).unwrap();
        assert_eq!(r, json!({"name": "rgb", "group": "g1"}));
        s.execute("net.move", &json!({"name": "rgb", "group": "g1", "at": [10, 0]})).unwrap();
        // a pin outside the group takes the net out to the top level
        s.execute("wire.disconnect", &json!({"to": {"node": edge, "port": "b"}})).unwrap();
        s.execute("net.connect", &json!({"name": "rgb", "group": "g1", "to": {"node": edge, "port": "b"}})).unwrap();
        assert_eq!(s.graph().net_wires(&NetKey::top("rgb")).count(), 2);
        assert_eq!(s.graph().nets["rgb"].at, Some([10, 0]), "the driver's tag kept its place");
        assert!(s.execute("net.connect", &json!({"name": "rgb", "group": 7, "to": {"node": edge, "port": "b"}})).is_err());
    }

    #[test]
    fn a_paste_brings_its_nets_renamed() {
        let mut s = session();
        let (img, lab, _, edge) = chain(&mut s);
        s.execute("output.label", &json!({"from": {"node": img, "port": "text"}, "name": "picture"})).unwrap();
        let text = s.execute("edit.copy", &json!({"nodes": [img, lab, edge]})).unwrap()["text"].as_str().unwrap().to_owned();
        s.execute("edit.paste", &json!({"text": text})).unwrap();
        assert_eq!(s.graph().net_wires(&NetKey::top("picture")).count(), 2);
        assert_eq!(s.graph().net_wires(&NetKey::top("picture_2")).count(), 2);
    }
}
