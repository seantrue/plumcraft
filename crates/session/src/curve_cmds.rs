//! The `curves` value editor's commands (`docs/control-protocol.md`, the curves value
//! `plumcraft_proto::curves`): every edit an editor makes to a curves value is one of these, and
//! one undo step, so a native editor, the control channel and a script make the same edits. Each
//! acts on a node's curves input as `node.set` does (`Graph::set_value_checked`), keeping the
//! value's form (a JSON string stays a string, an object an object), and is refused for a port a
//! wire feeds.
//!
//! - `curves.edit {node, port?, channel, points, enabled?}`: one channel's points and on/off (a
//!   gesture: a point added, moved or removed, the channel turned on or off);
//! - `curves.reset {node, port?, channel}`: that channel back to the identity;
//! - `curves.preset {node, port?, channel, preset}`: a named preset's points;
//! - `curves.equalize {node, port?, channel}`: points at the quantiles of the channel's
//!   histogram, from what the last run sent the node's `curves` editor;
//! - `node.editorData {node, kind?}`: the value the node's editor edits and what the last run
//!   sent it (read only).
//!
//! `port` defaults to the input the node's `curves` editor names (`NodeDef.editor`). `channel` is
//! 0 (master) to 3 (blue), or its key (`master`, `red`, `green`, `blue`).

use plumcraft_graph::{NodeId, PortRef};
use plumcraft_proto::curves::{self, CHANNEL_KEYS, Curve, CurveSet, EDIT_MAX_POINTS, EQUALIZE_POINTS};
use serde_json::{Value, json};

use crate::commands::{CommandSpec, always, opt_str_param, str_param};
use crate::{Result, Session, SessionError};

/// The editor kind these commands edit.
pub const KIND: &str = "curves";

fn params(what: impl Into<String>) -> SessionError {
    SessionError::Params(what.into())
}

/// The node, its curves input, the curves now, and whether the value is kept as a string.
struct Target {
    node: NodeId,
    port: String,
    set: CurveSet,
    as_string: bool,
}

fn target(s: &Session, p: &Value) -> Result<Target> {
    let node = NodeId::from(str_param(p, "node")?);
    let n = s.graph().node(&node).ok_or_else(|| params(format!("no node {node}")))?;
    let def = s.catalog().node(&n.kind).ok_or_else(|| params(format!("{} isn't in the catalogue", n.kind)))?;
    let port = match opt_str_param(p, "port")? {
        Some(port) => port.to_owned(),
        None => def
            .editor
            .as_ref()
            .filter(|e| e.kind == KIND && !e.port.is_empty())
            .map(|e| e.port.clone())
            .ok_or_else(|| params(format!("{} names no curves editor: say which port", n.kind)))?,
    };
    let declared = def.inputs.iter().find(|q| q.name == port).ok_or_else(|| params(format!("{} has no input {port}", n.kind)))?;
    if s.graph().wire_into(&PortRef::new(node.clone(), port.clone())).is_some() {
        return Err(params(format!("{port} comes from a wire: there is no value here to edit")));
    }
    let current = plumcraft_graph::ports::current_value(def, n, &port, declared);
    let set = curves::parse_value(&current).map_err(params)?;
    let as_string = current.is_string() || current.is_null() || declared.ty == "STRING";
    Ok(Target { node, port, set, as_string })
}

/// Store `t.set` as one undo step named `label`; the value as stored.
fn store(s: &mut Session, label: &str, t: Target) -> Result<Value> {
    let text = curves::to_json(&t.set);
    let value = if t.as_string { Value::String(text) } else { serde_json::from_str(&text).map_err(|e| params(e.to_string()))? };
    let at = PortRef::new(t.node, t.port);
    s.edit(label, |g, catalog| Ok(g.set_value_checked(catalog, &at, value.clone())?))?;
    Ok(json!({"curves": value}))
}

/// `channel`: 0 to 3, or its key.
fn channel(p: &Value) -> Result<usize> {
    match p.get("channel") {
        Some(Value::Number(n)) => {
            n.as_u64().and_then(|k| usize::try_from(k).ok()).filter(|k| *k < 4).ok_or_else(|| params("channel is 0 (master) to 3 (blue)"))
        }
        Some(Value::String(k)) => CHANNEL_KEYS.iter().position(|c| c == k).ok_or_else(|| params(format!("channel {k:?}: one of {}", CHANNEL_KEYS.join(", ")))),
        _ => Err(params("\"channel\" is required: 0 (master) to 3 (blue), or its key")),
    }
}

/// A list of points, `[x, y]` or `{x, y}`, each in 0..1: at least two, at most
/// [`EDIT_MAX_POINTS`], sorted by x.
fn points(p: &Value) -> Result<Vec<(f64, f64)>> {
    let list = p.get("points").and_then(Value::as_array).ok_or_else(|| params("\"points\" is required: [[x, y], …]"))?;
    if list.len() < 2 || list.len() > EDIT_MAX_POINTS {
        return Err(params(format!("a curve has 2 to {EDIT_MAX_POINTS} points, not {}", list.len())));
    }
    let num = |v: Option<&Value>| v.and_then(Value::as_f64).filter(|f| f.is_finite() && (0.0..=1.0).contains(f));
    let mut out = Vec::with_capacity(list.len());
    for q in list {
        let (x, y) = match q {
            Value::Array(a) => (num(a.first()), num(a.get(1))),
            Value::Object(o) => (num(o.get("x")), num(o.get("y"))),
            _ => (None, None),
        };
        let (Some(x), Some(y)) = (x, y) else { return Err(params(format!("{q} isn't a point [x, y] in 0..1"))) };
        out.push((x, y));
    }
    out.sort_by(|a, b| a.0.total_cmp(&b.0));
    Ok(out)
}

fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    let mut t = target(s, p)?;
    let k = channel(p)?;
    let points = points(p)?;
    let enabled = match p.get("enabled") {
        None | Some(Value::Null) => t.set[k].enabled,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(params("enabled is true or false")),
    };
    t.set[k] = Curve { points, enabled };
    store(s, "Edit Curve", t)
}

fn reset(s: &mut Session, p: &Value) -> Result<Value> {
    let mut t = target(s, p)?;
    let k = channel(p)?;
    t.set[k] = curves::identity();
    store(s, "Reset Curve", t)
}

fn preset(s: &mut Session, p: &Value) -> Result<Value> {
    let mut t = target(s, p)?;
    let k = channel(p)?;
    let name = str_param(p, "preset")?;
    let points = curves::preset(name).ok_or_else(|| params(format!("no preset {name:?}: {}", curves::PRESETS.map(|p| p.0).join(", "))))?;
    t.set[k] = Curve { points, enabled: true };
    store(s, "Curve Preset", t)
}

/// The channels' histograms the last run sent the node's `curves` editor.
fn histograms(s: &Session, node: &NodeId) -> Vec<Vec<u64>> {
    s.run
        .node(&node.0)
        .and_then(|n| n.editor.get(KIND))
        .and_then(|d| d.get("histograms"))
        .and_then(Value::as_array)
        .map(|hs| {
            hs.iter().take(64).map(|h| h.as_array().map(|b| b.iter().take(65_536).map(|c| c.as_u64().unwrap_or(0)).collect()).unwrap_or_default()).collect()
        })
        .unwrap_or_default()
}

fn equalize(s: &mut Session, p: &Value) -> Result<Value> {
    let mut t = target(s, p)?;
    let k = channel(p)?;
    let hist = curves::tab_histogram(&histograms(s, &t.node), k);
    let points = curves::equalize_points(&hist, EQUALIZE_POINTS).ok_or_else(|| params("run it once: equalizing needs the channel's histogram"))?;
    t.set[k] = Curve { points, enabled: true };
    store(s, "Equalize Curve", t)
}

fn editor_data(s: &mut Session, p: &Value) -> Result<Value> {
    let node = NodeId::from(str_param(p, "node")?);
    let n = s.graph().node(&node).ok_or_else(|| params(format!("no node {node}")))?;
    let named = s.catalog().node(&n.kind).and_then(|d| d.editor.clone());
    let kind = match opt_str_param(p, "kind")? {
        Some(k) => k.to_owned(),
        None => named.as_ref().map(|e| e.kind.clone()).ok_or_else(|| params(format!("{} names no editor", n.kind)))?,
    };
    let port = named.as_ref().filter(|e| e.kind == kind).map(|e| e.port.clone());
    let value = port.as_deref().and_then(|p| {
        let def = s.catalog().node(&n.kind)?;
        let declared = def.inputs.iter().find(|q| q.name == p)?;
        Some(plumcraft_graph::ports::current_value(def, n, p, declared))
    });
    let wired = port.as_deref().is_some_and(|p| s.graph().wire_into(&PortRef::new(node.clone(), p.to_owned())).is_some());
    let data = s.run.node(&node.0).and_then(|r| r.editor.get(&kind)).cloned().unwrap_or(Value::Null);
    Ok(json!({"node": node, "kind": kind, "port": port, "value": value, "wired": wired, "data": data}))
}

pub(crate) fn specs() -> Vec<CommandSpec> {
    let spec = |id, label, params, run, journal| CommandSpec { id, label, menu: &[], shortcut: None, params, enabled: always, run, journal };
    vec![
        spec(
            "curves.edit",
            "Edit Curve",
            "{node, port?, channel: 0-3 | master | red | green | blue, points: [[x, y]] (2 to 64, in 0..1), enabled?} -> {curves}: one channel of a curves value (a point added, moved or removed; on or off), one undo step",
            edit as fn(&mut Session, &Value) -> Result<Value>,
            true,
        ),
        spec("curves.reset", "Reset Curve", "{node, port?, channel} -> {curves}: the channel back to a straight line, one undo step", reset, true),
        spec(
            "curves.preset",
            "Curve Preset",
            "{node, port?, channel, preset: Light S | Medium S | Strong S | Inverse S} -> {curves}: a preset's points on the channel, one undo step",
            preset,
            true,
        ),
        spec(
            "curves.equalize",
            "Equalize Curve",
            "{node, port?, channel} -> {curves}: points at the quantiles of the channel's histogram (from the node's last run), one undo step; refused before a run",
            equalize,
            true,
        ),
        spec(
            "node.editorData",
            "Editor Data",
            "{node, kind?} -> {node, kind, port, value, wired, data}: the value the node's editor edits (its port's, null when it names none) and what the node's last run sent that editor (NodeReport.editor[kind]; kind defaults to the one the node names), or data null",
            editor_data,
            false,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use plumcraft_proto::catalogue::SchemaReply;

    fn catalog() -> SchemaReply {
        serde_json::from_value(json!({"nodes": [
            {"id": "t.curve", "display": "Curve", "editor": {"kind": "curves", "port": "curves"},
             "inputs": [{"name": "curves", "type": "STRING", "default": curves::default_json()}], "outputs": [{"name": "out", "type": "STRING"}]},
            {"id": "t.obj", "display": "Curve Object", "editor": {"kind": "curves", "port": "tone"},
             "inputs": [{"name": "tone", "type": "JSON", "socket": "both", "default": {}}], "outputs": []},
            {"id": "t.src", "display": "Source", "inputs": [], "outputs": [{"name": "out", "type": "STRING"}]}
        ]}))
        .unwrap()
    }

    fn value(s: &Session, n: &str, port: &str) -> Value {
        s.graph().node(&NodeId::from(n)).and_then(|x| x.values.get(port).cloned()).unwrap_or(Value::Null)
    }

    #[test]
    fn each_edit_is_one_undo_step_and_keeps_the_values_form() {
        let mut s = Session::new(catalog());
        let n = s.execute("node.add", &json!({"kind": "t.curve"})).unwrap()["id"].as_str().unwrap().to_owned();
        let r = s.execute("curves.edit", &json!({"node": n, "channel": "red", "points": [[0, 0], [0.5, 0.8], [1, 1]]})).unwrap();
        assert!(r["curves"].is_string(), "a string port keeps a string");
        let set = curves::parse_value(&value(&s, &n, "curves")).unwrap();
        assert_eq!(set[1].points, vec![(0.0, 0.0), (0.5, 0.8), (1.0, 1.0)]);
        assert_eq!(s.undo_label(), Some("Edit Curve"));
        s.execute("curves.preset", &json!({"node": n, "channel": 0, "preset": "Strong S"})).unwrap();
        s.execute("curves.edit", &json!({"node": n, "channel": 0, "points": [[0, 0], [1, 1]], "enabled": false})).unwrap();
        assert!(!curves::parse_value(&value(&s, &n, "curves")).unwrap()[0].enabled);
        s.execute("curves.reset", &json!({"node": n, "channel": "red"})).unwrap();
        assert!(curves::parse_value(&value(&s, &n, "curves")).unwrap()[1].is_identity());
        // each gesture one step: undo walks them back one at a time
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(curves::parse_value(&value(&s, &n, "curves")).unwrap()[1].points.len(), 3);
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(curves::parse_value(&value(&s, &n, "curves")).unwrap()[0].enabled);
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(curves::parse_value(&value(&s, &n, "curves")).unwrap()[0].points, curves::identity().points);
        // an object port keeps an object
        let o = s.execute("node.add", &json!({"kind": "t.obj"})).unwrap()["id"].as_str().unwrap().to_owned();
        let r = s.execute("curves.preset", &json!({"node": o, "channel": "master", "preset": "Light S"})).unwrap();
        assert!(r["curves"].is_object() && value(&s, &o, "tone").is_object(), "{r}");
    }

    #[test]
    fn bad_edits_are_refused_and_change_nothing() {
        let mut s = Session::new(catalog());
        let n = s.execute("node.add", &json!({"kind": "t.curve"})).unwrap()["id"].as_str().unwrap().to_owned();
        let before = value(&s, &n, "curves");
        for bad in [
            json!({"node": n, "channel": 4, "points": [[0, 0], [1, 1]]}),
            json!({"node": n, "channel": "alpha", "points": [[0, 0], [1, 1]]}),
            json!({"node": n, "channel": 0, "points": [[0, 0]]}),
            json!({"node": n, "channel": 0, "points": [[0, 0], [1.5, 1]]}),
            json!({"node": n, "channel": 0, "points": [[0, 0], "x"]}),
            json!({"node": n, "channel": 0, "points": (0..65).map(|i| json!([f64::from(i) / 64.0, 0.5])).collect::<Vec<_>>()}),
            json!({"node": n, "channel": 0}),
            json!({"node": "nope", "channel": 0, "points": [[0, 0], [1, 1]]}),
            json!({"node": n, "port": "nope", "channel": 0, "points": [[0, 0], [1, 1]]}),
        ] {
            assert!(s.execute("curves.edit", &bad).is_err(), "{bad}");
        }
        assert!(s.execute("curves.preset", &json!({"node": n, "channel": 0, "preset": "nope"})).is_err());
        assert!(s.execute("curves.equalize", &json!({"node": n, "channel": 0})).unwrap_err().to_string().contains("run it once"));
        assert_eq!(value(&s, &n, "curves"), before);
        // a node naming no curves editor needs its port; a wired port has no value to edit
        let src = s.execute("node.add", &json!({"kind": "t.src"})).unwrap()["id"].as_str().unwrap().to_owned();
        assert!(s.execute("curves.reset", &json!({"node": src, "channel": 0})).unwrap_err().to_string().contains("names no curves editor"));
        s.execute("wire.connect", &json!({"from": {"node": src, "port": "out"}, "to": {"node": n, "port": "curves"}})).unwrap();
        assert!(s.execute("curves.reset", &json!({"node": n, "channel": 0})).unwrap_err().to_string().contains("comes from a wire"));
    }

    #[test]
    fn equalize_and_editor_data_read_the_last_run() {
        let mut s = Session::new(catalog());
        let n = s.execute("node.add", &json!({"kind": "t.curve"})).unwrap()["id"].as_str().unwrap().to_owned();
        assert_eq!(
            s.execute("node.editorData", &json!({"node": n})).unwrap(),
            json!({"node": n, "kind": "curves", "port": "curves", "value": curves::default_json(), "wired": false, "data": null})
        );
        let mut hist = vec![0u64; 256];
        hist[64..192].iter_mut().for_each(|c| *c = 10);
        s.run.nodes.entry(n.clone()).or_default().editor.insert("curves".into(), json!({"names": ["value"], "histograms": [hist], "live": true}));
        let d = s.execute("node.editorData", &json!({"node": n})).unwrap();
        assert_eq!(d["data"]["names"], json!(["value"]));
        s.execute("curves.equalize", &json!({"node": n, "channel": 1})).unwrap();
        let c = &curves::parse_value(&value(&s, &n, "curves")).unwrap()[1];
        assert_eq!(c.points.first().map(|p| p.1), Some(0.0));
        assert!((c.points[0].0 - 0.25).abs() < 1e-9, "the values start a quarter of the way in: {:?}", c.points);
        assert_eq!(s.undo_label(), Some("Equalize Curve"));
    }
}
