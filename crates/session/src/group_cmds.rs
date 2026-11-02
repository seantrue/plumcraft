//! Group commands: create, ungroup, rename, promote, edit and unpromote a parameter, add and
//! remove an output, collapse and expand, move. Every command but `group.move` is an undo step.
//!
//! Groups are an editing overlay saved in the workflow file; engines run the flat graph and
//! ignore them ([`plumcraft_graph::groups`]).
//!
//! Lifted from the research software's `crates/engine/src/group_cmds.rs` (2026-10-27, the research software `8d96021`; plan
//! 10.3, E3). Names are any short text, not Python identifiers,
//! and `group.create` picks a fresh name (`group 1`, …) when none is given.

use plumcraft_graph::{GroupId, NodeId, PromotedEdit};
use serde_json::{Value, json};

use crate::commands::{
    CommandSpec, always, bool_param, group_param, id_list_param, node_list_param, opt_bool_param, opt_str_param, port_param, pos_param, str_param,
};
use crate::{Result, Session, SessionError};

fn has_groups(s: &Session) -> std::result::Result<(), String> {
    if s.graph().groups.is_empty() { Err("there are no groups".into()) } else { Ok(()) }
}

fn group_create(s: &mut Session, p: &Value) -> Result<Value> {
    let name = match opt_str_param(p, "name")? {
        Some(n) => n.to_owned(),
        None => s.graph().fresh_group_name(),
    };
    let nodes = node_list_param(p, "nodes")?;
    let groups: Vec<GroupId> = id_list_param(p, "groups")?.into_iter().map(GroupId).collect();
    let collapse = opt_bool_param(p, "collapsed", false)?;
    let id = s.edit("Group", |g, _| {
        let id = g.create_group(&name, &nodes, &groups)?;
        g.set_collapsed(&id, collapse)?;
        Ok(id)
    })?;
    Ok(json!({"id": id, "name": name}))
}

fn group_ungroup(s: &mut Session, p: &Value) -> Result<Value> {
    let id = group_param(p, "group")?;
    let removed = s.edit("Ungroup", |g, _| Ok(g.ungroup(&id)?))?;
    Ok(json!({"nodes": removed.nodes}))
}

fn group_rename(s: &mut Session, p: &Value) -> Result<Value> {
    let id = group_param(p, "group")?;
    let name = str_param(p, "name")?.to_owned();
    s.edit("Rename Group", |g, _| Ok(g.rename_group(&id, &name)?))?;
    Ok(Value::Null)
}

fn group_promote(s: &mut Session, p: &Value) -> Result<Value> {
    let id = group_param(p, "group")?;
    let port = port_param(p, "port")?;
    let label = match opt_str_param(p, "label")? {
        Some(l) => l.to_owned(),
        None => port.port.rsplit('.').next().unwrap_or(&port.port).to_owned(),
    };
    let doc = opt_str_param(p, "doc")?.unwrap_or_default().to_owned();
    s.edit("Promote", |g, catalog| Ok(g.promote(catalog, &id, port, &label, &doc)?))?;
    Ok(json!({"label": label}))
}

/// `min`/`max`: absent leaves the bound, null clears it, a number sets it.
fn bound_param(p: &Value, key: &str) -> Result<Option<Option<f64>>> {
    match p.get(key) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(v) => v.as_f64().map(|f| Some(Some(f))).ok_or_else(|| SessionError::Params(format!("\"{key}\" must be a number or null"))),
    }
}

fn group_edit_promoted(s: &mut Session, p: &Value) -> Result<Value> {
    let id = group_param(p, "group")?;
    let label = str_param(p, "label")?.to_owned();
    let edit = PromotedEdit {
        label: opt_str_param(p, "newLabel")?.map(str::to_owned),
        doc: opt_str_param(p, "doc")?.map(str::to_owned),
        min: bound_param(p, "min")?,
        max: bound_param(p, "max")?,
    };
    s.edit("Edit Parameter", |g, catalog| Ok(g.edit_promoted(catalog, &id, &label, &edit)?))?;
    let new = edit.label.unwrap_or(label);
    let promoted = s.graph().group(&id).and_then(|g| g.promoted.iter().find(|p| p.label == new).cloned());
    Ok(json!({"promoted": promoted}))
}

fn group_unpromote(s: &mut Session, p: &Value) -> Result<Value> {
    let id = group_param(p, "group")?;
    let label = str_param(p, "label")?.to_owned();
    let gone = s.edit("Unpromote", |g, _| Ok(g.unpromote(&id, &label)?))?;
    Ok(json!({"port": gone.port}))
}

fn group_output(s: &mut Session, p: &Value) -> Result<Value> {
    let id = group_param(p, "group")?;
    let from = port_param(p, "from")?;
    let name = match opt_str_param(p, "name")? {
        Some(n) => n.to_owned(),
        None => from.port.clone(),
    };
    s.edit("Add Group Output", |g, catalog| Ok(g.add_group_output(catalog, &id, from, &name)?))?;
    Ok(json!({"name": name}))
}

fn group_remove_output(s: &mut Session, p: &Value) -> Result<Value> {
    let id = group_param(p, "group")?;
    let name = str_param(p, "name")?.to_owned();
    let gone = s.edit("Remove Group Output", |g, _| Ok(g.remove_group_output(&id, &name)?))?;
    Ok(json!({"from": gone.from}))
}

fn group_checkpoint(s: &mut Session, p: &Value) -> Result<Value> {
    let id = group_param(p, "group")?;
    let node = NodeId::from(str_param(p, "node")?);
    let on = bool_param(p, "on")?;
    let label = if on { "Mark for Export" } else { "Unmark for Export" };
    s.edit(label, |g, catalog| Ok(g.set_checkpoint(catalog, &id, &node, on)?))?;
    Ok(Value::Null)
}

fn group_collapse(s: &mut Session, p: &Value) -> Result<Value> {
    let id = group_param(p, "group")?;
    let collapsed = bool_param(p, "collapsed")?;
    let label = if collapsed { "Collapse Group" } else { "Expand Group" };
    s.edit(label, |g, _| Ok(g.set_collapsed(&id, collapsed)?))?;
    Ok(Value::Null)
}

fn group_move(s: &mut Session, p: &Value) -> Result<Value> {
    let id = group_param(p, "group")?;
    let pos = pos_param(p, "pos")?.ok_or_else(|| SessionError::Params("\"pos\" is required".into()))?;
    s.graph_mut_untracked().move_group(&id, pos)?;
    s.mark_dirty();
    Ok(Value::Null)
}

pub(crate) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "group.create",
            label: "Group",
            menu: &["Group"],
            shortcut: Some("Cmd+G"),
            params: "{nodes: [node id], groups?: [group id], name?: string (default: a fresh \"group N\"), collapsed?: bool} -> {id, name} (all picked must sit in the same enclosing group, which becomes the parent; the UI groups the selection)",
            enabled: always,
            run: group_create,
            journal: true,
        },
        CommandSpec {
            id: "group.ungroup",
            label: "Ungroup",
            menu: &["Group"],
            shortcut: Some("Cmd+Shift+G"),
            params: "{group: group id} -> {nodes: [node id]} (members and nested groups move to the parent; the UI ungroups the selected collapsed group, or the selected nodes' innermost group)",
            enabled: has_groups,
            run: group_ungroup,
            journal: true,
        },
        CommandSpec {
            id: "group.rename",
            label: "Rename Group",
            menu: &[],
            shortcut: None,
            params: "{group: group id, name: string (1 to 200 characters, no control characters)}",
            enabled: has_groups,
            run: group_rename,
            journal: true,
        },
        CommandSpec {
            id: "group.promote",
            label: "Promote Input",
            menu: &[],
            shortcut: None,
            params: "{group: group id, port: {node, port}, label?: string (default: the port's name), doc?: string} -> {label} (an input of a member node; the collapsed group shows its widget)",
            enabled: has_groups,
            run: group_promote,
            journal: true,
        },
        CommandSpec {
            id: "group.editPromoted",
            label: "Edit Group Parameter",
            menu: &[],
            shortcut: None,
            params: "{group: group id, label, newLabel?: string, doc?: string, min?: number | null, max?: number | null} -> {promoted} (rename a promoted parameter, or set its doc or its range, inside the inner port's range; null clears a bound)",
            enabled: has_groups,
            run: group_edit_promoted,
            journal: true,
        },
        CommandSpec {
            id: "group.unpromote",
            label: "Unpromote Input",
            menu: &[],
            shortcut: None,
            params: "{group: group id, label} -> {port: {node, port}}",
            enabled: has_groups,
            run: group_unpromote,
            journal: true,
        },
        CommandSpec {
            id: "group.output",
            label: "Add Group Output",
            menu: &[],
            shortcut: None,
            params: "{group: group id, from: {node, port}, name?: string (default: the port's name)} -> {name} (an output of a member node; the collapsed group shows it first)",
            enabled: has_groups,
            run: group_output,
            journal: true,
        },
        CommandSpec {
            id: "group.removeOutput",
            label: "Remove Group Output",
            menu: &[],
            shortcut: None,
            params: "{group: group id, name} -> {from: {node, port}}",
            enabled: has_groups,
            run: group_remove_output,
            journal: true,
        },
        CommandSpec {
            id: "group.checkpoint",
            label: "Mark for Export",
            menu: &[],
            shortcut: None,
            params: "{group: group id, node: node id, on: bool} (marks a member for a pack's export, as a group's `checkpoints`; only node types the catalogue calls export.markable; a run ignores it)",
            enabled: has_groups,
            run: group_checkpoint,
            journal: true,
        },
        CommandSpec {
            id: "group.collapse",
            label: "Collapse Group",
            menu: &[],
            shortcut: None,
            params: "{group: group id, collapsed: bool} (collapsed: one node with the group's inputs, promoted widgets and outputs; false expands it)",
            enabled: has_groups,
            run: group_collapse,
            journal: true,
        },
        CommandSpec {
            id: "group.move",
            label: "Move Group",
            menu: &[],
            shortcut: None,
            params: "{group: group id, pos: [x, y]} (where the collapsed group's node sits; its members and nested groups move by the same offset; not an undo step)",
            enabled: has_groups,
            run: group_move,
            journal: false,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use plumcraft_graph::{Graph, NodeId};
    use plumcraft_proto::catalogue::SchemaReply;

    /// The text nodes the group tests use (the text engine's own, as a catalogue).
    fn catalog() -> SchemaReply {
        serde_json::from_value(json!({"v": 2, "nodes": [
            {"id": "text.literal", "inputs": [{"name": "text", "type": "STRING", "default": "", "socket": "widget"}], "outputs": [{"name": "text", "type": "TEXT"}]},
            {"id": "text.upper", "inputs": [{"name": "text", "type": "TEXT", "socket": "socket"}], "outputs": [{"name": "text", "type": "TEXT"}]},
            {"id": "text.words", "inputs": [{"name": "text", "type": "TEXT", "socket": "socket"}], "outputs": [{"name": "count", "type": "INT"}]},
            {"id": "text.slow", "inputs": [{"name": "text", "type": "TEXT", "socket": "socket"}, {"name": "seconds", "type": "FLOAT", "default": 1.0, "min": 0.0, "max": 60.0}],
             "outputs": [{"name": "text", "type": "TEXT"}]},
            {"id": "text.case", "inputs": [
                {"name": "text", "type": "TEXT", "socket": "socket"},
                {"name": "mode", "type": "ENUM", "default": "upper", "choices": ["upper", "title"],
                 "options": {"title": [{"name": "small", "type": "STRING", "default": "a,of"}]}}],
             "outputs": [{"name": "text", "type": "TEXT"}]}
        ]}))
        .unwrap()
    }

    fn session() -> Session {
        Session::new(catalog())
    }

    fn add(s: &mut Session, kind: &str) -> String {
        s.execute("node.add", &json!({"kind": kind})).unwrap()["id"].as_str().unwrap().to_owned()
    }

    #[test]
    fn a_checkpoint_is_one_undo_step_on_a_markable_member_and_goes_with_its_node() {
        let mut c = catalog();
        c.nodes.iter_mut().find(|n| n.id == "text.upper").unwrap().export.markable = true;
        let mut s = Session::new(c);
        let (up, words, out) = (add(&mut s, "text.upper"), add(&mut s, "text.words"), add(&mut s, "text.upper"));
        s.execute("group.create", &json!({"nodes": [up, words], "name": "g"})).unwrap();
        let marks = |s: &Session| s.graph().groups.values().next().map_or(0, |g| g.checkpoints.len());
        let refused = [
            json!({"group": "g1", "node": words, "on": true}),
            json!({"group": "g1", "node": out, "on": true}),
            json!({"group": "nope", "node": up, "on": true}),
            json!({"group": "g1", "node": up, "on": "yes"}),
            json!({"group": "g1", "on": true}),
        ];
        for bad in refused {
            assert!(s.execute("group.checkpoint", &bad).is_err(), "{bad}");
            assert_eq!(marks(&s), 0, "{bad}");
        }
        let e = s.execute("group.checkpoint", &json!({"group": "g1", "node": words, "on": true})).unwrap_err().to_string();
        assert!(e.contains("text.words"), "says which: {e}");

        let depth = s.undo_depth();
        s.execute("group.checkpoint", &json!({"group": "g1", "node": up, "on": true})).unwrap();
        assert_eq!((marks(&s), s.undo_depth(), s.undo_label()), (1, depth + 1, Some("Mark for Export")));
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(marks(&s), 0);
        s.execute("edit.redo", &json!({})).unwrap();
        assert_eq!(marks(&s), 1);

        // in the file, and back
        let text = s.graph().to_json().unwrap();
        assert!(text.contains("\"checkpoints\""), "{text}");
        assert_eq!(Graph::from_json(&text).unwrap().groups, s.graph().groups);

        s.execute("group.checkpoint", &json!({"group": "g1", "node": up, "on": false})).unwrap();
        assert_eq!(marks(&s), 0);
        assert!(!s.graph().to_json().unwrap().contains("\"checkpoints\""), "not written when empty");
        s.execute("group.checkpoint", &json!({"group": "g1", "node": up, "on": true})).unwrap();
        s.execute("node.remove", &json!({"nodes": [up]})).unwrap();
        assert_eq!(marks(&s), 0, "a removed node's mark goes with it");
        s.graph().check(s.catalog()).unwrap();
    }

    /// Every group command is one undo step, and undo/redo walk back and forth through them.
    #[test]
    fn undo_redo_across_group_commands() {
        let mut s = session();
        let slow = add(&mut s, "text.slow");
        let case = add(&mut s, "text.case");
        let words = add(&mut s, "text.words");
        let mut states: Vec<Graph> = vec![s.graph().clone()];
        let steps = [
            ("group.create", json!({"nodes": [slow, case, words], "name": "think"})),
            ("group.create", json!({"nodes": [words]})),
            ("group.rename", json!({"group": "g1", "name": "think hard"})),
            ("group.promote", json!({"group": "g1", "port": {"node": slow, "port": "seconds"}, "label": "delay"})),
            ("node.set", json!({"node": case, "port": "mode", "value": "title"})),
            ("group.promote", json!({"group": "g1", "port": {"node": case, "port": "mode.small"}, "doc": "kept small"})),
            ("group.output", json!({"group": "g1", "from": {"node": words, "port": "count"}, "name": "words"})),
            ("group.collapse", json!({"group": "g1", "collapsed": true})),
            ("group.editPromoted", json!({"group": "g1", "label": "small", "newLabel": "small words", "doc": "lower case"})),
            ("group.editPromoted", json!({"group": "g1", "label": "delay", "min": 0.5, "max": 4})),
            ("group.unpromote", json!({"group": "g1", "label": "delay"})),
            ("group.removeOutput", json!({"group": "g1", "name": "words"})),
            ("group.collapse", json!({"group": "g1", "collapsed": false})),
            ("group.ungroup", json!({"group": "g2"})),
        ];
        for (id, p) in &steps {
            s.execute(id, p).unwrap_or_else(|e| panic!("{id} {p}: {e}"));
            states.push(s.graph().clone());
        }
        assert_eq!(s.graph().group(&GroupId::from("g2")), None);
        let g1 = s.graph().group(&GroupId::from("g1")).unwrap();
        assert_eq!(g1.name, "think hard");
        assert!(!g1.collapsed && g1.nodes.contains(&NodeId::from(words.as_str())));
        assert_eq!(g1.promoted.iter().map(|p| p.label.as_str()).collect::<Vec<_>>(), ["small words"]);
        for want in states.iter().rev().skip(1) {
            s.execute("edit.undo", &Value::Null).unwrap();
            assert_eq!(s.graph(), want);
        }
        for want in states.iter().skip(1) {
            s.execute("edit.redo", &Value::Null).unwrap();
            assert_eq!(s.graph(), want);
        }
    }

    #[test]
    fn refused_group_edits_change_nothing() {
        let mut s = session();
        let a = add(&mut s, "text.slow");
        let b = add(&mut s, "text.upper");
        assert!(matches!(s.execute("group.ungroup", &json!({"group": "g1"})), Err(SessionError::Disabled { .. })));
        let r = s.execute("group.create", &json!({"nodes": [a]})).unwrap();
        assert_eq!(r["name"], "group 1", "a fresh name when none is given");
        let before = s.graph().clone();
        let depth = s.undo_depth();
        let bad = [
            ("group.create", json!({"nodes": [b], "name": "line\nbreak"})),
            ("group.create", json!({"nodes": [], "name": "empty"})),
            ("group.create", json!({"nodes": [b, "n99"], "name": "x"})),
            ("group.create", json!({"nodes": [b], "collapsed": "yes"})),
            ("group.rename", json!({"group": "g1", "name": "  "})),
            ("group.rename", json!({"group": "g7", "name": "ok"})),
            ("group.promote", json!({"group": "g1", "port": {"node": b, "port": "text"}, "label": "text"})),
            ("group.promote", json!({"group": "g1", "port": {"node": a, "port": "nope"}})),
            ("group.output", json!({"group": "g1", "from": {"node": a, "port": "nope"}, "name": "x"})),
            ("group.unpromote", json!({"group": "g1", "label": "nope"})),
            ("group.collapse", json!({"group": "g1"})),
            ("group.editPromoted", json!({"group": "g1", "label": "nope", "doc": "x"})),
        ];
        for (id, p) in &bad {
            let e = s.execute(id, p).unwrap_err();
            assert!(!e.to_string().is_empty());
            assert_eq!(s.graph(), &before, "{id} {p}");
        }
        assert_eq!(s.undo_depth(), depth);
        s.execute("group.promote", &json!({"group": "g1", "port": {"node": a, "port": "seconds"}})).unwrap();
        for bad in [json!({"min": 61}), json!({"min": -1}), json!({"min": 5, "max": 2}), json!({"min": "1"})] {
            let mut p = json!({"group": "g1", "label": "seconds"});
            p.as_object_mut().unwrap().extend(bad.as_object().unwrap().clone());
            assert!(s.execute("group.editPromoted", &p).is_err(), "{p}");
        }
    }

    #[test]
    fn group_move_is_not_an_undo_step() {
        let mut s = session();
        let a = add(&mut s, "text.upper");
        s.execute("group.create", &json!({"nodes": [a], "name": "one"})).unwrap();
        s.execute("group.move", &json!({"group": "g1", "pos": [10, 20]})).unwrap();
        assert_eq!(s.undo_label(), Some("Group"));
        assert!(s.is_dirty());
        assert_eq!(s.graph().group(&GroupId::from("g1")).unwrap().pos, [10.0, 20.0]);
        assert!(s.execute("group.move", &json!({"group": "g2", "pos": [1, 2]})).is_err());
    }

    /// P04 (2026-10-27): a collapsed group moved, then opened, came back where it was. Moving a
    /// group carries its members and nested groups by the same offset; collapsing puts its node
    /// at its members' top-left, wherever they were moved while it was open.
    #[test]
    fn a_group_moves_with_its_members_and_collapses_in_place() {
        let mut s = session();
        let pos = |s: &Session, n: &str| s.graph().node(&NodeId::from(n)).unwrap().pos;
        let gpos = |s: &Session, g: &str| s.graph().group(&GroupId::from(g)).unwrap().pos;
        let at = |s: &mut Session, kind: &str, p: [f32; 2]| s.execute("node.add", &json!({"kind": kind, "pos": p})).unwrap()["id"].as_str().unwrap().to_owned();
        let a = at(&mut s, "text.slow", [100.0, 50.0]);
        let b = at(&mut s, "text.upper", [400.0, 80.0]);
        let c = at(&mut s, "text.words", [700.0, 60.0]);
        let outside = at(&mut s, "text.literal", [0.0, 500.0]);
        s.execute("group.create", &json!({"nodes": [b], "name": "inner", "collapsed": true})).unwrap();
        s.execute("group.create", &json!({"nodes": [a, c], "groups": ["g1"], "name": "outer"})).unwrap();
        assert_eq!(gpos(&s, "g2"), [100.0, 50.0]);
        // open, a member moved: collapsing puts the node at the members' top-left now
        s.execute("node.move", &json!({"id": a, "pos": [150.0, 10.0]})).unwrap();
        s.execute("group.collapse", &json!({"group": "g2", "collapsed": true})).unwrap();
        assert_eq!(gpos(&s, "g2"), [150.0, 10.0]);
        let collapsed = s.graph().clone();
        let depth = s.undo_depth();
        // moved collapsed: everything inside goes along, the outside stays
        s.execute("group.move", &json!({"group": "g2", "pos": [250.0, 310.0]})).unwrap();
        assert_eq!((pos(&s, &a), pos(&s, &b), pos(&s, &c)), ([250.0, 310.0], [500.0, 380.0], [800.0, 360.0]));
        assert_eq!(gpos(&s, "g1"), [500.0, 380.0], "the nested group too");
        assert_eq!(pos(&s, &outside), [0.0, 500.0]);
        assert_eq!(s.undo_depth(), depth, "a move is not an undo step (as node.move)");
        // opened: the members are where the group now is
        s.execute("group.collapse", &json!({"group": "g2", "collapsed": false})).unwrap();
        assert_eq!(pos(&s, &a), [250.0, 310.0]);
        // and an open group's move carries them too
        s.execute("group.move", &json!({"group": "g2", "pos": [260.0, 300.0]})).unwrap();
        assert_eq!((pos(&s, &a), pos(&s, &c)), ([260.0, 300.0], [810.0, 350.0]));
        // undo walks back over the open, to the collapse (which put it at the moved place)
        s.execute("edit.undo", &Value::Null).unwrap();
        assert!(s.graph().group(&GroupId::from("g2")).unwrap().collapsed);
        s.execute("edit.undo", &Value::Null).unwrap();
        assert!(!s.graph().group(&GroupId::from("g2")).unwrap().collapsed);
        s.execute("edit.redo", &Value::Null).unwrap();
        // redo brings back the collapsed group as it was left: moved, its members with it
        assert!(s.graph().group(&GroupId::from("g2")).unwrap().collapsed);
        assert_eq!((gpos(&s, "g2"), pos(&s, &a), pos(&s, &b)), ([250.0, 310.0], [250.0, 310.0], [500.0, 380.0]));
        assert_ne!(s.graph(), &collapsed);
        // hostile: a move that would put a member off the canvas changes nothing
        s.execute("group.move", &json!({"group": "g2", "pos": [-3.0e38, 0.0]})).unwrap();
        let before = s.graph().clone();
        assert!(s.execute("group.move", &json!({"group": "g2", "pos": [3.0e38, 0.0]})).is_err(), "the offset overflows");
        assert_eq!(s.graph(), &before);
    }

    #[test]
    fn removing_nodes_updates_groups_and_undo_restores_them() {
        let mut s = session();
        let a = add(&mut s, "text.slow");
        let b = add(&mut s, "text.words");
        s.execute("group.create", &json!({"nodes": [a, b], "name": "one"})).unwrap();
        s.execute("group.output", &json!({"group": "g1", "from": {"node": b, "port": "count"}})).unwrap();
        s.execute("group.promote", &json!({"group": "g1", "port": {"node": a, "port": "seconds"}})).unwrap();
        s.execute("node.remove", &json!({"nodes": [b]})).unwrap();
        let g = s.graph().group(&GroupId::from("g1")).unwrap();
        assert!(g.outputs.is_empty() && g.promoted.len() == 1);
        s.execute("node.remove", &json!({"nodes": [a]})).unwrap();
        assert!(s.graph().groups.is_empty(), "an emptied group goes");
        s.execute("edit.undo", &Value::Null).unwrap();
        s.execute("edit.undo", &Value::Null).unwrap();
        assert_eq!(s.graph().group(&GroupId::from("g1")).unwrap().outputs.len(), 1);
        // all or nothing
        assert!(s.execute("node.remove", &json!({"nodes": [a, "n99"]})).is_err());
        assert!(s.graph().node(&NodeId::from(a.as_str())).is_some());
    }

    /// Duplicate keeps copies in their originals' groups; copy and paste lands at the top level
    /// (as the research software's do), and cut takes nodes out of their groups in one undo step.
    #[test]
    fn duplicate_copy_paste_and_cut_of_grouped_nodes() {
        let mut s = session();
        let a = add(&mut s, "text.literal");
        let u = add(&mut s, "text.upper");
        s.execute("wire.connect", &json!({"from": {"node": a, "port": "text"}, "to": {"node": u, "port": "text"}})).unwrap();
        s.execute("group.create", &json!({"nodes": [a, u], "name": "pair"})).unwrap();
        let r = s.execute("node.duplicate", &json!({"nodes": [a, u]})).unwrap();
        let copies: Vec<NodeId> = r["nodes"].as_array().unwrap().iter().map(|v| NodeId::from(v.as_str().unwrap())).collect();
        let pair = GroupId::from("g1");
        assert!(copies.iter().all(|c| s.graph().group_of(c) == Some(&pair)));
        let clip = s.execute("edit.copy", &json!({"nodes": [a, u]})).unwrap()["text"].as_str().unwrap().to_owned();
        let r = s.execute("edit.paste", &json!({"text": clip})).unwrap();
        for n in r["nodes"].as_array().unwrap() {
            assert_eq!(s.graph().group_of(&NodeId::from(n.as_str().unwrap())), None, "pasted at the top level");
        }
        let before = s.graph().clone();
        s.execute("edit.cut", &json!({"nodes": [a]})).unwrap();
        assert!(!s.graph().group(&pair).unwrap().nodes.contains(&NodeId::from(a.as_str())));
        s.execute("edit.undo", &Value::Null).unwrap();
        assert_eq!(s.graph(), &before);
    }

    #[test]
    fn groups_round_trip_through_save_and_open() {
        let dir = plumcraft_scratch::root().join("e3-groups");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("grouped.json");
        let mut s = session();
        let a = add(&mut s, "text.literal");
        let b = add(&mut s, "text.slow");
        let c = add(&mut s, "text.upper");
        s.execute("wire.connect", &json!({"from": {"node": a, "port": "text"}, "to": {"node": b, "port": "text"}})).unwrap();
        s.execute("wire.connect", &json!({"from": {"node": b, "port": "text"}, "to": {"node": c, "port": "text"}})).unwrap();
        let id = s.execute("group.create", &json!({"nodes": [b, c], "name": "think", "collapsed": true})).unwrap()["id"].as_str().unwrap().to_owned();
        s.execute("group.promote", &json!({"group": id, "port": {"node": b, "port": "seconds"}, "doc": "how long"})).unwrap();
        s.execute("group.output", &json!({"group": id, "from": {"node": c, "port": "text"}, "name": "shout"})).unwrap();
        s.execute("group.move", &json!({"group": id, "pos": [120, 80]})).unwrap();
        s.execute("graph.saveAs", &json!({"path": path})).unwrap();
        let mut t = session();
        t.execute("graph.open", &json!({"path": path})).unwrap();
        assert_eq!(t.graph().groups, s.graph().groups);
        assert_eq!(t.graph().next_group, s.graph().next_group);
        assert!(t.problems().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
