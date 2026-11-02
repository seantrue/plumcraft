//! Graph editing commands: new graph, add/remove/duplicate/move/set/rename nodes, connect and
//! disconnect wires. Lifted from the research software's `graph_cmds.rs` without wire routes.

use plumcraft_graph::{Graph, GraphError, NodeCache, NodeId, NodeView, PortRef, Wire};
use plumcraft_proto::catalogue::SchemaReply;
use serde_json::{Value, json};

use crate::commands::{CommandSpec, always, node_list_param, opt_bool_param, opt_str_param, port_param, pos_param, str_param};
use crate::{Result, Session, SessionError};

fn has_schema(s: &Session) -> std::result::Result<(), String> {
    if s.catalog().nodes.is_empty() { Err("no engine catalogue loaded (is the engine running?)".into()) } else { Ok(()) }
}

/// `discard: true` lets `graph.new` and `graph.open` drop unsaved changes; without it they
/// refuse ([`SessionError::Unsaved`]). The UI asks first, then passes it.
pub(crate) fn check_discard(s: &Session, p: &Value) -> Result<()> {
    let discard = match p.get("discard") {
        None | Some(Value::Null) => false,
        Some(v) => v.as_bool().ok_or_else(|| SessionError::Params("\"discard\" must be true or false".into()))?,
    };
    if s.is_dirty() && !discard { Err(SessionError::Unsaved) } else { Ok(()) }
}

fn graph_new(s: &mut Session, p: &Value) -> Result<Value> {
    check_discard(s, p)?;
    s.edit("New Graph", |g, _| {
        *g = Graph::new();
        Ok(Value::Null)
    })?;
    s.start_afresh();
    s.forget_path();
    Ok(Value::Null)
}

pub(crate) fn wire_json(w: &Wire) -> Value {
    match &w.slice {
        Some(s) => json!({"from": w.from, "to": w.to, "slice": s}),
        None => json!({"from": w.from, "to": w.to}),
    }
}

fn node_add(s: &mut Session, p: &Value) -> Result<Value> {
    let kind = str_param(p, "kind")?;
    let pos = pos_param(p, "pos")?.unwrap_or([0.0, 0.0]);
    let order: Vec<String> = s.engines.engine_ids().into_iter().map(str::to_owned).collect();
    let id = s.edit("Add Node", |g, catalog| {
        let def = catalog.node(kind).ok_or_else(|| GraphError::UnknownKind(kind.to_owned()))?;
        let id = g.add_node(def, pos);
        // Several engines (spec 8.2, decision E4): a kind more than one offers is pinned to the
        // one that runs it now, so connecting another later can't move it; and the workflow
        // records the engines it was made with.
        if def.engines.len() > 1 {
            g.set_engine(&id, def.engines.first().cloned())?;
        }
        if !def.engines.is_empty() && g.engines.is_empty() {
            g.engines = order;
        }
        Ok(id)
    })?;
    Ok(json!({"id": id}))
}

/// `{nodes: [ids]}`, or the older single `{id}`.
fn nodes_or_id(p: &Value) -> Result<Vec<NodeId>> {
    let nodes = node_list_param(p, "nodes")?;
    if !nodes.is_empty() {
        return Ok(nodes);
    }
    match p.get("id") {
        Some(_) => Ok(vec![NodeId::from(str_param(p, "id")?)]),
        None => Err(SessionError::Params("\"nodes\" must be a non-empty list of node ids".into())),
    }
}

fn node_remove(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = nodes_or_id(p)?;
    let label = if ids.len() == 1 { "Remove Node" } else { "Remove Nodes" };
    s.edit(label, |g, _| {
        let mut removed = Vec::new();
        for id in &ids {
            if !removed.contains(id) {
                g.remove_node(id)?;
                removed.push(id.clone());
            }
        }
        Ok(json!({"removed": removed}))
    })
}

fn node_duplicate(s: &mut Session, p: &Value) -> Result<Value> {
    let ids = nodes_or_id(p)?;
    let offset = pos_param(p, "offset")?.unwrap_or([40.0, 40.0]);
    let keep_inputs = match p.get("keepInputs") {
        None | Some(Value::Null) => false,
        Some(v) => v.as_bool().ok_or_else(|| SessionError::Params("\"keepInputs\" must be true or false".into()))?,
    };
    let pairs = s.edit("Duplicate", |g, _| Ok(g.duplicate_nodes(&ids, offset, keep_inputs)?))?;
    let map: serde_json::Map<String, Value> = pairs.iter().map(|(old, new)| (old.0.clone(), Value::from(new.0.clone()))).collect();
    Ok(json!({"nodes": pairs.iter().map(|(_, new)| new).collect::<Vec<_>>(), "map": map}))
}

fn node_move(s: &mut Session, p: &Value) -> Result<Value> {
    let id = NodeId::from(str_param(p, "id")?);
    let pos = pos_param(p, "pos")?.ok_or_else(|| SessionError::Params("\"pos\" is required".into()))?;
    s.graph_mut_untracked().move_node(&id, pos)?;
    s.mark_dirty();
    Ok(Value::Null)
}

/// `node.view {nodes, view}`: show nodes in full, without their previews, or collapsed. A view
/// change, saved with the workflow, but not an undo step (as moving a node isn't).
fn node_view(s: &mut Session, p: &Value) -> Result<Value> {
    let nodes = if p.get("nodes").is_some() { node_list_param(p, "nodes")? } else { vec![NodeId::from(str_param(p, "id")?)] };
    let view = match str_param(p, "view")? {
        "full" => NodeView::Full,
        "compact" => NodeView::Compact,
        "collapsed" => NodeView::Collapsed,
        other => return Err(SessionError::Params(format!("view {other:?}: use full, compact or collapsed"))),
    };
    if let Some(missing) = nodes.iter().find(|n| s.graph().node(n).is_none()) {
        return Err(GraphError::UnknownNode(missing.clone()).into());
    }
    for n in &nodes {
        s.graph_mut_untracked().set_view(n, view)?;
    }
    s.mark_dirty();
    Ok(json!({"nodes": nodes}))
}

/// `node.fields {node, all}`: show every field of a node, or tuck those its choice doesn't use
/// under "more fields". A view change, saved with the workflow, but not an undo step.
fn node_fields(s: &mut Session, p: &Value) -> Result<Value> {
    let id = NodeId::from(str_param(p, "node")?);
    let all = p.get("all").and_then(Value::as_bool).ok_or_else(|| SessionError::Params("\"all\" (true or false) is required".into()))?;
    s.graph_mut_untracked().set_all_fields(&id, all)?;
    s.mark_dirty();
    Ok(json!({"node": id, "all": all}))
}

/// `node.parameters {node?}`: open the parameter editor on a node, or close it with no node.
fn node_parameters(s: &mut Session, p: &Value) -> Result<Value> {
    let node = opt_str_param(p, "node")?.map(NodeId::from);
    if let Some(missing) = node.as_ref().filter(|n| s.graph().node(n).is_none()) {
        return Err(GraphError::UnknownNode(missing.clone()).into());
    }
    s.views.parameters = node.clone();
    Ok(json!({"node": node}))
}

/// `node.editor {node, open?}`: open or close the editor the node names (`NodeDef.editor`). An
/// application registers what draws it (`plumcraft_ui::value_editors`); a value on a wire isn't
/// the node's to edit.
fn node_editor(s: &mut Session, p: &Value) -> Result<Value> {
    let id = NodeId::from(str_param(p, "node")?);
    if !opt_bool_param(p, "open", true)? {
        s.views.editors.retain(|n| *n != id);
        return Ok(json!({"node": id, "open": false}));
    }
    let node = s.graph().node(&id).ok_or_else(|| GraphError::UnknownNode(id.clone()))?;
    let Some(editor) = s.catalog().node(&node.kind).and_then(|d| d.editor.as_ref()) else {
        return Err(SessionError::Params(format!("{} names no editor", node.kind)));
    };
    let (kind, port) = (editor.kind.clone(), editor.port.clone());
    if s.graph().wire_into(&PortRef::new(id.clone(), port.clone())).is_some() {
        return Err(SessionError::Params(format!("{port} comes from a wire: there is no value here to edit")));
    }
    if !s.views.editors.contains(&id) {
        s.views.editors.push(id.clone());
    }
    Ok(json!({"node": id, "kind": kind, "port": port, "open": true}))
}

/// `node.cache {nodes, cache}`: whether the engine persists the nodes' results (spec 14.5):
/// `auto` (when slow), `always` or `never`. An undo step; it doesn't change what they compute.
fn node_cache(s: &mut Session, p: &Value) -> Result<Value> {
    let nodes = if p.get("nodes").is_some() { node_list_param(p, "nodes")? } else { vec![NodeId::from(str_param(p, "id")?)] };
    let cache = match str_param(p, "cache")? {
        "auto" => NodeCache::Auto,
        "always" => NodeCache::Always,
        "never" => NodeCache::Never,
        other => return Err(SessionError::Params(format!("cache {other:?}: use auto, always or never"))),
    };
    if let Some(missing) = nodes.iter().find(|n| s.graph().node(n).is_none()) {
        return Err(GraphError::UnknownNode(missing.clone()).into());
    }
    s.edit("Cache Setting", |g, _| {
        for n in &nodes {
            g.set_cache(n, cache)?;
        }
        Ok(())
    })?;
    Ok(json!({"nodes": nodes}))
}

/// The longest engine id `node.engine` takes.
pub const MAX_ENGINE_ID: usize = 128;

/// `node.engine {nodes, engine}`: pin the nodes to an engine that offers their kind (`null`
/// unpins: whichever does). An undo step; the nodes go stale.
fn node_engine(s: &mut Session, p: &Value) -> Result<Value> {
    let nodes = if p.get("nodes").is_some() { node_list_param(p, "nodes")? } else { vec![NodeId::from(str_param(p, "id")?)] };
    let engine = match p.get("engine") {
        None | Some(Value::Null) => None,
        Some(Value::String(e)) if !e.is_empty() && e.chars().count() <= MAX_ENGINE_ID && !e.chars().any(char::is_control) => Some(e.clone()),
        Some(_) => return Err(SessionError::Params("\"engine\" is an engine id, or null to unpin".into())),
    };
    for n in &nodes {
        let node = s.graph().node(n).ok_or_else(|| GraphError::UnknownNode(n.clone()))?;
        let def = s.catalog().node(&node.kind).ok_or_else(|| SessionError::Params(format!("{n}: no engine connected has {}", node.kind)))?;
        // Only an engine that offers the kind: a merged catalogue lists them; a single
        // engine's catalogue offers its own.
        if let Some(e) = &engine {
            let offered: Vec<&str> =
                if def.engines.is_empty() { vec![s.catalog().engine.id.as_str()] } else { def.engines.iter().map(String::as_str).collect() };
            if !offered.contains(&e.as_str()) {
                return Err(SessionError::Params(format!("{n}: {e} doesn't offer {} (these do: {})", node.kind, offered.join(", "))));
            }
        }
    }
    s.edit("Engine", |g, _| {
        for n in &nodes {
            g.set_engine(n, engine.clone())?;
        }
        Ok(())
    })?;
    Ok(json!({"nodes": nodes, "engine": engine}))
}

fn node_set(s: &mut Session, p: &Value) -> Result<Value> {
    let at = PortRef::new(str_param(p, "node")?, str_param(p, "port")?);
    let value = p.get("value").cloned().ok_or_else(|| SessionError::Params("\"value\" is required".into()))?;
    let out = s.edit("Set Value", |g, catalog| Ok(g.set_value_checked(catalog, &at, value)?))?;
    Ok(json!({
        "disconnected": out.disconnected.iter().map(wire_json).collect::<Vec<_>>(),
        "dropped": out.dropped,
        "added": out.added,
        "unpromoted": out.unpromoted.iter().map(|(g, l)| json!({"group": g, "label": l})).collect::<Vec<_>>(),
    }))
}

fn node_title(s: &mut Session, p: &Value) -> Result<Value> {
    let id = NodeId::from(str_param(p, "node")?);
    let title = str_param(p, "title")?.trim().to_owned();
    if title.chars().count() > plumcraft_graph::MAX_TITLE_CHARS {
        return Err(SessionError::Params(format!("a title has at most {} characters", plumcraft_graph::MAX_TITLE_CHARS)));
    }
    if title.chars().any(char::is_control) {
        return Err(SessionError::Params("a title can't contain control characters".into()));
    }
    s.edit("Rename Node", |g, _| {
        let node = g.nodes.get_mut(&id).ok_or_else(|| GraphError::UnknownNode(id.clone()))?;
        // Blank: back to the type name (and its serial).
        node.title = (!title.is_empty()).then_some(title);
        Ok(())
    })?;
    Ok(json!({"title": crate::node_title(s.graph(), s.catalog(), &id)}))
}

fn wire_connect(s: &mut Session, p: &Value) -> Result<Value> {
    let (from, to) = (port_param(p, "from")?, port_param(p, "to")?);
    let (replaced, seeded) = s.edit("Connect", |g, catalog| {
        let replaced = g.connect(catalog, from.clone(), to.clone())?;
        let seeded = seed_choices(g, catalog, &from, &to, None);
        Ok((replaced, seeded))
    })?;
    Ok(json!({"replaced": replaced.as_ref().map(wire_json), "seeded": seeded}))
}

/// The choices of input `at`, as its Vary Choices text would list them (`a, b, c`), when it
/// has any.
fn choices_text(g: &Graph, catalog: &SchemaReply, at: &PortRef) -> Option<String> {
    let node = g.node(&at.node)?;
    let def = catalog.node(&node.kind)?;
    let port = plumcraft_graph::resolve_input(def, node, &at.node, &at.port).ok()?.port;
    let choices = port.choices.as_ref().filter(|c| !c.is_empty())?;
    Some(choices.iter().map(|c| c.as_str().map_or_else(|| c.to_string(), str::to_owned)).collect::<Vec<_>>().join(", "))
}

/// A Vary Choices node wired into an input with choices: seed its `values` text with them,
/// so the list can be read and pruned on the node. Only a blank text is seeded, or one that
/// still reads as the choices of `old_to` (the input the wire came from), so a pruned list is
/// kept. Returns the text set.
fn seed_choices(g: &mut Graph, catalog: &SchemaReply, from: &PortRef, to: &PortRef, old_to: Option<&PortRef>) -> Option<String> {
    let node = g.node(&from.node)?;
    if !catalog.node(&node.kind)?.flags.vary_choices {
        return None;
    }
    let current = node.values.get("values").and_then(Value::as_str).unwrap_or_default().trim().to_owned();
    let untouched = current.is_empty() || old_to.and_then(|o| choices_text(g, catalog, o)).is_some_and(|old| old == current);
    if !untouched {
        return None;
    }
    let text = choices_text(g, catalog, to)?;
    if text == current {
        return None;
    }
    g.set_value(&PortRef::new(from.node.clone(), "values"), Value::String(text.clone())).ok()?;
    Some(text)
}

fn wire_disconnect(s: &mut Session, p: &Value) -> Result<Value> {
    let to = port_param(p, "to")?;
    let wire = s.edit("Disconnect", |g, _| Ok(g.disconnect(&to)?))?;
    Ok(json!({"wire": wire_json(&wire)}))
}

/// `wire.slice {to, slice}`: set the slice on the wire into `to`; `null` or `""` clears it.
fn wire_slice(s: &mut Session, p: &Value) -> Result<Value> {
    let to = port_param(p, "to")?;
    let slice = match p.get("slice") {
        None | Some(Value::Null) => None,
        Some(Value::String(t)) if t.trim().is_empty() => None,
        Some(Value::String(t)) => Some(t.clone()),
        Some(_) => return Err(SessionError::Params("\"slice\" must be a string such as \"1:-1:2\" or \"-1\", or null to clear".into())),
    };
    let label = if slice.is_some() { "Slice Wire" } else { "Clear Slice" };
    let old = s.edit(label, |g, catalog| Ok(g.set_slice(catalog, &to, slice.as_deref())?))?;
    let now = s.graph().wire_into(&to).and_then(|w| w.slice.clone());
    Ok(json!({"slice": now, "previous": old}))
}

/// Move the wire into `to` so it feeds `newTo` instead, in one undo step. Its slice goes with it
/// when it still fits the new input; otherwise the wire moves without it. Bend points and a net
/// label belong to the old route, and stay behind.
fn wire_reroute(s: &mut Session, p: &Value) -> Result<Value> {
    let (to, new_to) = (port_param(p, "to")?, port_param(p, "newTo")?);
    let (old, replaced, slice_dropped) = s.edit("Reroute Wire", |g, catalog| {
        let old = g.disconnect(&to)?;
        let moved = old.slice.clone().and_then(|slice| {
            let mut trial = g.clone();
            trial.connect_sliced(catalog, old.from.clone(), new_to.clone(), Some(slice)).ok().map(|r| (trial, r))
        });
        let out = match moved {
            Some((trial, replaced)) => {
                *g = trial;
                (old, replaced, false)
            }
            None => {
                let replaced = g.connect(catalog, old.from.clone(), new_to.clone())?;
                let dropped = old.slice.is_some();
                (old, replaced, dropped)
            }
        };
        // a Vary Choices text still reading as the old input's choices follows the wire
        seed_choices(g, catalog, &out.0.from, &new_to, Some(&to));
        Ok(out)
    })?;
    Ok(json!({"from": old.from, "replaced": replaced.as_ref().map(wire_json), "sliceDropped": if slice_dropped { old.slice } else { None }}))
}

/// `wire.route {to, via: [[x, y], ...]}`: the wire's bend points (graph units; empty: none),
/// lifted from the research software. One undo step; nothing goes stale (only how the wire looks).
fn wire_route(s: &mut Session, p: &Value) -> Result<Value> {
    let to = port_param(p, "to")?;
    let bad = || SessionError::Params("\"via\" must be a list of [x, y] numbers".into());
    let via: Vec<[i32; 2]> = match p.get("via") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(points)) if points.len() <= plumcraft_graph::MAX_VIA => points
            .iter()
            .map(|pt| {
                let [x, y] = pt.as_array().map(Vec::as_slice).ok_or_else(bad)? else { return Err(bad()) };
                let c = |v: &Value| {
                    v.as_f64().filter(|f| f.is_finite() && f.abs() <= f64::from(plumcraft_graph::MAX_VIA_COORD)).map(|f| f.round() as i32).ok_or_else(bad)
                };
                Ok([c(x)?, c(y)?])
            })
            .collect::<Result<_>>()?,
        Some(Value::Array(_)) => return Err(SessionError::Params(format!("at most {} bend points", plumcraft_graph::MAX_VIA))),
        Some(_) => return Err(bad()),
    };
    let label = if via.is_empty() { "Straighten Wire" } else { "Route Wire" };
    s.edit(label, |g, _| Ok(g.set_via(&to, via)?))?;
    Ok(json!({"via": s.graph().wire_into(&to).map(|w| w.via.clone())}))
}

pub(crate) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "graph.new",
            label: "New Graph",
            menu: &["File"],
            shortcut: Some("Cmd+N"),
            params: "{discard?: bool} (an empty graph with no file; refused over unsaved changes unless discard is true: the UI asks first. One undo step)",
            enabled: always,
            run: graph_new,
            journal: true,
        },
        CommandSpec {
            id: "node.add",
            label: "Add Node",
            menu: &[],
            shortcut: None,
            params: "{kind: NodeDef id, pos?: [x, y]} -> {id}",
            enabled: has_schema,
            run: node_add,
            journal: true,
        },
        CommandSpec {
            id: "node.remove",
            label: "Remove Nodes",
            menu: &[],
            shortcut: None,
            params: "{nodes: [node id]} (or {id}) -> {removed: [node id]} (their wires and group entries go too; a group left empty goes)",
            enabled: always,
            run: node_remove,
            journal: true,
        },
        CommandSpec {
            id: "node.duplicate",
            label: "Duplicate",
            menu: &["Edit"],
            shortcut: Some("Cmd+D"),
            params: "{nodes: [node id], offset?: [x, y] (default [40, 40]), keepInputs?: bool} -> {nodes: [new id], map: {old: new}} (wires among them are copied; with keepInputs, wires from outside too; each copy joins its original's innermost group)",
            enabled: always,
            run: node_duplicate,
            journal: true,
        },
        CommandSpec {
            id: "node.title",
            label: "Rename Node",
            menu: &[],
            shortcut: None,
            params: "{node: node id, title: string} -> {title} (blank goes back to the type name; the title shown is returned. Nodes titled by their type name show a serial only when there is more than one)",
            enabled: always,
            run: node_title,
            journal: true,
        },
        CommandSpec {
            id: "node.fields",
            label: "Show All Fields",
            menu: &[],
            shortcut: None,
            params: "{node: node id, all: bool} (true: every field; false: the fields its choice doesn't use under \"more fields\") -> {node, all}",
            enabled: always,
            run: node_fields,
            journal: false,
        },
        CommandSpec {
            id: "node.parameters",
            label: "Edit Parameters…",
            menu: &[],
            shortcut: None,
            params: "{node?: node id} (none: close) -> {node}: every input of one node in a window of its own",
            enabled: always,
            run: node_parameters,
            journal: false,
        },
        CommandSpec {
            id: "node.editor",
            label: "Edit Value…",
            menu: &[],
            shortcut: None,
            params: "{node: node id, open?: bool (default true)} -> {node, open, kind?, port?}: the editor the node names (NodeDef.editor) in a window of its own; refused for a node that names none, or a port on a wire",
            enabled: always,
            run: node_editor,
            journal: false,
        },
        CommandSpec {
            id: "node.view",
            label: "Node View",
            menu: &[],
            shortcut: None,
            params: "{nodes: [node id] (or {id}), view: full | compact | collapsed} (compact: without the preview; not journaled)",
            enabled: always,
            run: node_view,
            journal: false,
        },
        CommandSpec {
            id: "node.cache",
            label: "Cache Result",
            menu: &[],
            shortcut: None,
            params: "{nodes: [node id] (or {id}), cache: auto | always | never} -> {nodes}. Whether the engine keeps the nodes' results in its store across restarts (spec 14.5): auto keeps those whose own work took the engine's threshold (1 s by default), unless writing them costs about as much",
            enabled: always,
            run: node_cache,
            journal: true,
        },
        CommandSpec {
            id: "node.engine",
            label: "Run On Engine",
            menu: &[],
            shortcut: None,
            params: "{nodes: [node id] (or {id}), engine: engine id | null} -> {nodes, engine}. With several engines connected, the engine that runs the nodes (one connected that offers their kind; null: the first in the workflow's engines that does). An undo step; the nodes go stale",
            enabled: always,
            run: node_engine,
            journal: true,
        },
        CommandSpec {
            id: "node.move",
            label: "Move Node",
            menu: &[],
            shortcut: None,
            params: "{id: node id, pos: [x, y]} (not journaled)",
            enabled: always,
            run: node_move,
            journal: false,
        },
        CommandSpec {
            id: "node.set",
            label: "Set Value",
            menu: &[],
            shortcut: None,
            params: "{node: node id, port: input name or sub-port \"<port>.<sub>\", value: JSON} -> {disconnected: [wire], dropped: [port], added: [port], unpromoted: [{group, label}]} (a choice change takes away the old choice's sub-ports, and their promotions)",
            enabled: always,
            run: node_set,
            journal: true,
        },
        CommandSpec {
            id: "wire.connect",
            label: "Connect",
            menu: &[],
            shortcut: None,
            params: "{from: {node, port}, to: {node, port | variadic slot | active sub-port}} -> {replaced: wire|null}",
            enabled: always,
            run: wire_connect,
            journal: true,
        },
        CommandSpec {
            id: "wire.slice",
            label: "Slice Wire",
            menu: &[],
            shortcut: None,
            params: "{to: {node, port}, slice: \"start:stop:step\" (Python: 1:-1:2, :-1, ::2) | \"index\" (-1) | null (clear)} -> {slice, previous} (a range keeps a list a list; an index takes one item; refused with the reason when invalid; one undo step)",
            enabled: always,
            run: wire_slice,
            journal: true,
        },
        CommandSpec {
            id: "wire.reroute",
            label: "Reroute Wire",
            menu: &[],
            shortcut: None,
            params: "{to: {node, port} (the wire's input end now), newTo: {node, port}} -> {from, replaced, sliceDropped} (one undo step; the slice moves too when it fits the new input)",
            enabled: always,
            run: wire_reroute,
            journal: true,
        },
        CommandSpec {
            id: "wire.route",
            label: "Route Wire",
            menu: &[],
            shortcut: None,
            params: "{to: {node, port}, via: [[x, y], ...]} -> {via} (bend points the wire is drawn through, graph units, at most 32; [] straightens it; one undo step; nothing goes stale)",
            enabled: always,
            run: wire_route,
            journal: true,
        },
        CommandSpec {
            id: "wire.disconnect",
            label: "Disconnect",
            menu: &[],
            shortcut: None,
            params: "{to: {node, port}} -> {wire: {from, to}}",
            enabled: always,
            run: wire_disconnect,
            journal: true,
        },
    ]
}
