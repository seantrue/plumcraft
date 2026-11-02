//! Expand a command node into its graph (F24): `node.expand` replaces it, in one undo step, with
//! the fragment the catalogue's `expansions` gives for its type ([`Expansion`], spec 7.5).
//!
//! - fragment nodes get fresh ids, sit relative to the command, and keep its engine;
//! - whatever fed the command's mapped inputs now feeds the fragment's, slices kept;
//! - the command's setting values go to the fragment's widgets; a setting fed by a wire is wired
//!   instead, and one the command took from its bundle (which wins over its own widget) is copied
//!   from that bundle;
//! - the command's consumers read the fragment's output; a wire that can't be carried over is
//!   dropped and reported, never silently;
//! - the new nodes join the command's group, whose promotions and outputs follow them; with
//!   `group` they also get their own group, its parameters and outputs from the mapping, or as
//!   the expansion's `group` block says.
//!
//! Lifted from the research software's `crates/engine/src/expand_cmds.rs` (`8480c03`). Its group names are
//! Python identifiers, for plugin export; here a group is named for what it shows. Unlike it,
//! a command alone in its group keeps that group, and promotions on the command follow it.

use std::collections::{BTreeMap, BTreeSet};

use plumcraft_graph::{Graph, GraphError, GroupId, MAX_TITLE_CHARS, NodeId, PortRef, PromotedEdit, Wire, groups, ports};
use plumcraft_proto::catalogue::SchemaReply;
use plumcraft_proto::expansion::Expansion;
use serde_json::{Value, json};

use crate::bundle_cmds::{static_bundle_value, upstream_has};
use crate::commands::{CommandSpec, str_param};
use crate::{Result, Session, SessionError, display_name};

/// Can a node of type `kind` be expanded?
pub fn has_expansion(catalog: &SchemaReply, kind: &str) -> bool {
    catalog.expansions.contains_key(kind)
}

/// The expansion of node type `kind`, read and checked.
pub fn expansion_of(catalog: &SchemaReply, kind: &str) -> Result<Expansion> {
    let v = catalog.expansions.get(kind).ok_or_else(|| SessionError::Params(format!("{kind} has no expansion")))?;
    let e = Expansion::parse(v).map_err(|why| SessionError::Params(format!("the engine's expansion of {kind} is malformed: {why}")))?;
    if !e.command.is_empty() && e.command != kind {
        return Err(SessionError::Params(format!("the expansion for {kind} says it is for {}", e.command)));
    }
    Ok(e)
}

/// `base`, or `base 2`, `base 3`, … : the first not in `taken`.
fn unique_name(base: &str, taken: &BTreeSet<String>) -> String {
    if !taken.contains(base) {
        return base.to_owned();
    }
    (2u32..).map(|i| format!("{base} {i}")).find(|n| !taken.contains(n)).unwrap_or_else(|| base.to_owned())
}

/// A wire that couldn't be carried over, and why.
#[derive(Debug, Clone, PartialEq)]
pub struct Dropped {
    pub wire: Wire,
    pub reason: String,
}

/// What [`expand_node`] did.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Expanded {
    /// Fragment local id -> new node id.
    pub nodes: BTreeMap<String, NodeId>,
    pub group: Option<GroupId>,
    pub dropped: Vec<Dropped>,
    /// Settings whose value came from the command's bundle (copied in).
    pub from_bundle: Vec<String>,
    /// Settings its bundle carries whose value isn't known without a run (wired upstream): the
    /// widget's value was used.
    pub unresolved: Vec<String>,
    /// A group's parameters and results on the command that have no place in the fragment.
    pub lost: Vec<String>,
}

/// The node `local` of the fragment, as added.
fn local<'a>(nodes: &'a BTreeMap<String, NodeId>, name: &str) -> Result<&'a NodeId> {
    nodes.get(name).ok_or_else(|| SessionError::Params(format!("the expansion names a missing node {name:?}")))
}

fn local_port(nodes: &BTreeMap<String, NodeId>, name: &str, port: &str) -> Result<PortRef> {
    Ok(PortRef::new(local(nodes, name)?.clone(), port))
}

/// The values the command's mapped settings have: from its bundle, else its own. A wired setting
/// has none, it is wired through.
fn setting_values(g: &Graph, catalog: &SchemaReply, id: &NodeId, exp: &Expansion, out: &mut Expanded) -> Result<BTreeMap<String, Value>> {
    let cmd = g.node(id).ok_or_else(|| GraphError::UnknownNode(id.clone()))?;
    let def = catalog.node(&cmd.kind).ok_or_else(|| GraphError::UnknownKind(cmd.kind.clone()))?;
    let mut values = BTreeMap::new();
    for name in exp.settings.keys() {
        if g.wire_into(&PortRef::new(id.clone(), name.clone())).is_some() {
            continue;
        }
        let port = def.input(name);
        let param = port.and_then(|p| p.param_ref.as_ref());
        let v = match param.and_then(|p| static_bundle_value(g, catalog, id, p)) {
            Some(v) => {
                out.from_bundle.push(name.clone());
                Some(v)
            }
            None => {
                if param.is_some_and(|p| upstream_has(g, catalog, id, p)) {
                    out.unresolved.push(name.clone());
                }
                match port {
                    Some(p) => Some(ports::current_value(def, cmd, name, p)),
                    None => cmd.values.get(name).cloned(),
                }
            }
        };
        if let Some(v) = v {
            values.insert(name.clone(), v);
        }
    }
    Ok(values)
}

/// Add the fragment's nodes, in the command's group and on its engine.
fn add_fragment(g: &mut Graph, catalog: &SchemaReply, id: &NodeId, exp: &Expansion, out: &mut Expanded) -> Result<()> {
    let cmd = g.node(id).cloned().ok_or_else(|| GraphError::UnknownNode(id.clone()))?;
    let container = g.group_of(id).cloned();
    for (name, n) in &exp.nodes {
        let def = catalog.node(&n.kind).ok_or_else(|| GraphError::UnknownKind(n.kind.clone()))?;
        let new = g.add_node(def, [cmd.pos[0] + n.pos[0], cmd.pos[1] + n.pos[1]]);
        for (k, v) in &n.values {
            g.set_value_checked(catalog, &PortRef::new(new.clone(), k.clone()), v.clone())?;
        }
        if let Some(node) = g.nodes.get_mut(&new) {
            node.engine.clone_from(&cmd.engine);
            // a title as node.title would take it; anything else is left off
            node.title = n
                .title
                .as_deref()
                .map(str::trim)
                .filter(|t| !t.is_empty() && t.chars().count() <= MAX_TITLE_CHARS && !t.chars().any(char::is_control))
                .map(str::to_owned);
        }
        out.nodes.insert(name.clone(), new);
    }
    if let Some(grp) = container.and_then(|gid| g.groups.get_mut(&gid)) {
        grp.nodes.extend(out.nodes.values().cloned());
    }
    Ok(())
}

/// Point the groups' parameters and results on the command at the fragment, before the command
/// goes and takes them with it. A setting with several targets follows the first.
fn move_group_ports(g: &mut Graph, id: &NodeId, exp: &Expansion, nodes: &BTreeMap<String, NodeId>, lost: &mut Vec<String>) -> Result<()> {
    let target = |port: &str| exp.settings.get(port).or_else(|| exp.inputs.get(port)).and_then(|t| t.first());
    for grp in g.groups.values_mut() {
        for p in grp.promoted.iter_mut().filter(|p| &p.port.node == id) {
            match target(&p.port.port) {
                Some((n, port)) => p.port = local_port(nodes, n, port)?,
                None => lost.push(format!("{}: {}", grp.name, p.label)),
            }
        }
        for o in grp.outputs.iter_mut().filter(|o| &o.from.node == id) {
            match exp.outputs.get(&o.from.port) {
                Some((n, port)) => o.from = local_port(nodes, n, port)?,
                None => lost.push(format!("{}: {}", grp.name, o.name)),
            }
        }
    }
    Ok(())
}

/// The group the fragment came from, or one made from the mapping.
fn make_group(g: &mut Graph, catalog: &SchemaReply, kind: &str, exp: &Expansion, name: &str, out: &Expanded) -> Result<GroupId> {
    let members: Vec<NodeId> = out.nodes.values().cloned().collect();
    let gid = g.create_group(name, &members, &[])?;
    let nodes = &out.nodes;
    if let Some(block) = &exp.group {
        for pr in &block.promoted {
            g.promote(catalog, &gid, local_port(nodes, &pr.port.node, &pr.port.port)?, &pr.label, &pr.doc)?;
            if pr.min.is_some() || pr.max.is_some() {
                let edit = PromotedEdit { min: Some(pr.min), max: Some(pr.max), ..Default::default() };
                g.edit_promoted(catalog, &gid, &pr.label, &edit)?;
            }
        }
        for o in &block.outputs {
            g.add_group_output(catalog, &gid, local_port(nodes, &o.from.node, &o.from.port)?, &o.name)?;
        }
        if !block.checkpoints.is_empty() {
            // as the group was stored: a mark needs nothing of the catalogue to come back
            let marked = block.checkpoints.iter().map(|n| local(nodes, n).cloned()).collect::<Result<Vec<_>>>()?;
            if let Some(grp) = g.groups.get_mut(&gid) {
                grp.checkpoints.extend(marked);
            }
        }
        return Ok(gid);
    }
    let def = catalog.node(kind);
    let mut labels = BTreeSet::new();
    for (port, targets) in &exp.settings {
        let Some((n, p)) = targets.first() else { continue };
        let label = unique_name(port.rsplit('.').next().unwrap_or(port), &labels);
        let doc = def.and_then(|d| d.input(port)).and_then(|p| p.tooltip.clone()).unwrap_or_default();
        g.promote(catalog, &gid, local_port(nodes, n, p)?, &label, &doc)?;
        labels.insert(label);
    }
    let mut names = BTreeSet::new();
    for (port, (n, p)) in &exp.outputs {
        let name = unique_name(port, &names);
        g.add_group_output(catalog, &gid, local_port(nodes, n, p)?, &name)?;
        names.insert(name);
    }
    Ok(gid)
}

/// Replace command node `id` with its expansion (see the module docs). With `group`, the new
/// nodes go in a new group of that name.
pub fn expand_node(g: &mut Graph, catalog: &SchemaReply, id: &NodeId, group: Option<&str>) -> Result<Expanded> {
    let kind = g.node(id).map(|n| n.kind.clone()).ok_or_else(|| GraphError::UnknownNode(id.clone()))?;
    let exp = expansion_of(catalog, &kind)?;
    if let Some(name) = group {
        groups::check_name("group name", name)?;
    }
    let incoming: Vec<Wire> = g.wires.iter().filter(|w| &w.to.node == id).cloned().collect();
    let outgoing: Vec<Wire> = g.wires.iter().filter(|w| &w.from.node == id).cloned().collect();
    let into = |port: &str| incoming.iter().find(|w| w.to.port == port);
    let mut out = Expanded::default();
    let values = setting_values(g, catalog, id, &exp, &mut out)?;

    // The fragment joins the command's group first: its last member going would remove it.
    add_fragment(g, catalog, id, &exp, &mut out)?;
    let mut lost = Vec::new();
    move_group_ports(g, id, &exp, &out.nodes, &mut lost)?;
    out.lost = lost;
    g.remove_node(id)?;

    // Its own wires, slices kept.
    for w in &exp.wires {
        let (from, to) = (local_port(&out.nodes, &w.from.node, &w.from.port)?, local_port(&out.nodes, &w.to.node, &w.to.port)?);
        g.connect_sliced(catalog, from, to, w.slice.clone())?;
    }

    // Settings: wired through, or the command's value set on each target widget.
    for (port, targets) in &exp.settings {
        for (n, p) in targets {
            let to = local_port(&out.nodes, n, p)?;
            match (into(port), values.get(port)) {
                (Some(w), _) => {
                    g.connect_sliced(catalog, w.from.clone(), to, w.slice.clone())?;
                }
                (None, Some(v)) => {
                    g.set_value_checked(catalog, &to, v.clone())?;
                }
                (None, None) => {}
            }
        }
    }

    // Inputs: what fed the command feeds the fragment.
    for (port, targets) in &exp.inputs {
        let Some(w) = into(port) else { continue };
        for (n, p) in targets {
            g.connect_sliced(catalog, w.from.clone(), local_port(&out.nodes, n, p)?, w.slice.clone())?;
        }
    }
    for w in incoming.iter().filter(|w| !exp.inputs.contains_key(&w.to.port) && !exp.settings.contains_key(&w.to.port)) {
        out.dropped.push(Dropped { wire: w.clone(), reason: format!("the expansion has no place for {}", w.to.port) });
    }

    // Outputs: the command's consumers now read the fragment's output.
    for w in &outgoing {
        let Some((n, p)) = exp.outputs.get(&w.from.port) else {
            out.dropped.push(Dropped { wire: w.clone(), reason: format!("the expansion has no output for {}", w.from.port) });
            continue;
        };
        if let Err(e) = g.connect_sliced(catalog, local_port(&out.nodes, n, p)?, w.to.clone(), w.slice.clone()) {
            out.dropped.push(Dropped { wire: w.clone(), reason: e.to_string() });
        }
    }

    if let Some(name) = group {
        out.group = Some(make_group(g, catalog, &kind, &exp, name, &out)?);
    }
    Ok(out)
}

fn has_expandable(s: &Session) -> std::result::Result<(), String> {
    if s.graph().nodes.values().any(|n| has_expansion(s.catalog(), &n.kind)) { Ok(()) } else { Err("no node in the graph has an expansion".into()) }
}

fn node_expand(s: &mut Session, p: &Value) -> Result<Value> {
    let id = NodeId::from(str_param(p, "node")?);
    let kind = s.graph().node(&id).map(|n| n.kind.clone()).ok_or_else(|| GraphError::UnknownNode(id.clone()))?;
    let title = display_name(s.catalog(), &kind);
    let group = match p.get("group") {
        None | Some(Value::Null | Value::Bool(false)) => None,
        Some(Value::Bool(true)) => Some(title.clone()),
        Some(Value::String(name)) => Some(name.clone()),
        Some(_) => return Err(SessionError::Params("\"group\" must be true, false or a group name".into())),
    };
    let out = s.edit(&format!("Expand {title}"), |g, catalog| expand_node(g, catalog, &id, group.as_deref()))?;
    // what the panel and pop-outs watch of the command, they watch of what stands for it
    if let Ok(exp) = expansion_of(s.catalog(), &kind) {
        for (port, (n, p)) in &exp.outputs {
            if let Ok(to) = local_port(&out.nodes, n, p) {
                s.run.move_watches(&PortRef::new(id.clone(), port.clone()), &to);
            }
        }
    }
    Ok(json!({
        "nodes": out.nodes,
        "group": out.group,
        "dropped": out.dropped.iter().map(|d| json!({"from": d.wire.from, "to": d.wire.to, "reason": d.reason})).collect::<Vec<_>>(),
        "fromBundle": out.from_bundle,
        "unresolved": out.unresolved,
        "lost": out.lost,
    }))
}

pub(crate) fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "node.expand",
        label: "Expand",
        menu: &["Node"],
        shortcut: None,
        params: "{node: command node id, group?: bool | name} -> {nodes: {local: id}, group, dropped: [{from, to, reason}], fromBundle: [setting], unresolved: [setting], lost: [text]} (replaces the command with the graph the catalogue's expansions give for its type, in one undo step; group: true names the group after the command)",
        enabled: has_expandable,
        run: node_expand,
        journal: true,
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A "steps" command: `scales` -> `blur` (with a slice), `edge` -> `mix`. No `vi.` here: the
    /// catalogue decides what expands.
    fn catalog() -> SchemaReply {
        serde_json::from_value(json!({
            "wire_types": [{"id": "x.t.tune", "bundle": true}, {"id": "x.t.pic", "media": "ndarray"}],
            "nodes": [
                {"id": "t.src", "display": "Source", "inputs": [], "outputs": [{"name": "pic", "type": "x.t.pic"}, {"name": "many", "type": "STRING", "list": true}]},
                {"id": "t.cmd", "display": "Steps", "flags": {"bundle_port": "tuning"},
                 "inputs": [{"name": "image", "type": "x.t.pic", "socket": "socket"},
                            {"name": "tuning", "type": "x.t.tune", "socket": "socket", "optional": true},
                            {"name": "steps.scales", "type": "STRING", "default": "1,2,3", "socket": "both", "param_ref": {"group": "steps", "name": "scales"}},
                            {"name": "steps.gain", "type": "FLOAT", "default": 1.0, "socket": "both", "tooltip": "How much", "param_ref": {"group": "steps", "name": "gain"}},
                            {"name": "odd", "type": "x.t.pic", "socket": "socket", "optional": true}],
                 "outputs": [{"name": "images", "type": "x.t.pic"}, {"name": "labels", "type": "STRING"}, {"name": "tuning", "type": "x.t.tune"}]},
                {"id": "t.scales", "display": "Scales", "inputs": [{"name": "scales", "type": "STRING", "default": "", "socket": "both"}],
                 "outputs": [{"name": "scales", "type": "STRING", "list": true}]},
                {"id": "t.blur", "display": "Blur",
                 "inputs": [{"name": "image", "type": "x.t.pic", "socket": "socket", "optional": true},
                            {"name": "scale", "type": "STRING", "default": "", "socket": "both"},
                            {"name": "gain", "type": "FLOAT", "default": 0.0, "socket": "both"}],
                 "outputs": [{"name": "value", "type": "x.t.pic"}]},
                {"id": "t.mix", "display": "Mix", "inputs": [{"name": "a", "type": "x.t.pic", "socket": "socket", "optional": true}],
                 "outputs": [{"name": "result", "type": "x.t.pic"}]},
                {"id": "t.view", "display": "View", "inputs": [{"name": "array", "type": "x.t.pic", "socket": "socket", "optional": true},
                                                                {"name": "text", "type": "STRING", "socket": "socket", "optional": true}], "outputs": []},
            ],
            "expansions": {"t.cmd": {
                "command": "t.cmd",
                "nodes": {"scales": {"kind": "t.scales", "values": {"scales": ""}, "pos": [0, 420]},
                          "blur": {"kind": "t.blur", "values": {"gain": 3.0}, "pos": [380, 0], "title": "the blur"},
                          "mix": {"kind": "t.mix", "pos": [800, 420]}},
                "wires": [{"from": {"node": "scales", "port": "scales"}, "to": {"node": "blur", "port": "scale"}, "slice": ":-1"},
                          {"from": {"node": "blur", "port": "value"}, "to": {"node": "mix", "port": "a"}}],
                "inputs": {"image": [["blur", "image"]]},
                "settings": {"steps.scales": [["scales", "scales"]], "steps.gain": [["blur", "gain"]]},
                "outputs": {"images": ["mix", "result"]},
                "note": "Scales -> Blur -> Mix"}}}))
        .unwrap()
    }

    fn add(s: &mut Session, kind: &str, pos: [f32; 2]) -> String {
        s.execute("node.add", &json!({"kind": kind, "pos": pos})).unwrap()["id"].as_str().unwrap().to_owned()
    }

    fn connect(s: &mut Session, from: (&str, &str), to: (&str, &str)) {
        s.execute("wire.connect", &json!({"from": {"node": from.0, "port": from.1}, "to": {"node": to.0, "port": to.1}})).unwrap();
    }

    fn pr(n: &str, p: &str) -> PortRef {
        PortRef::new(n, p)
    }

    /// source -> Steps (scales "9,8") -> view; labels -> view.
    fn command(s: &mut Session) -> (String, String, String) {
        let src = add(s, "t.src", [0.0, 0.0]);
        let cmd = add(s, "t.cmd", [400.0, 100.0]);
        let view = add(s, "t.view", [900.0, 100.0]);
        connect(s, (&src, "pic"), (&cmd, "image"));
        connect(s, (&cmd, "images"), (&view, "array"));
        connect(s, (&cmd, "labels"), (&view, "text"));
        s.execute("node.set", &json!({"node": cmd, "port": "steps.scales", "value": "9,8"})).unwrap();
        (src, cmd, view)
    }

    fn ids(r: &Value) -> BTreeMap<String, String> {
        serde_json::from_value(r["nodes"].clone()).unwrap()
    }

    #[test]
    fn a_command_is_replaced_by_its_graph_in_one_undo_step() {
        let mut s = Session::new(catalog());
        let (src, cmd, view) = command(&mut s);
        let before = s.graph().clone();
        let r = s.execute("node.expand", &json!({"node": cmd})).unwrap();
        assert_eq!(s.undo_label(), Some("Expand Steps"));
        let g = s.graph();
        assert!(g.node(&NodeId::from(cmd.as_str())).is_none(), "the command is gone");
        let ids = ids(&r);
        assert_eq!(ids.keys().map(String::as_str).collect::<Vec<_>>(), ["blur", "mix", "scales"]);
        for fresh in ids.values() {
            assert!(![&src, &cmd, &view].contains(&fresh), "fresh ids, not {fresh}");
        }
        let node = |k: &str| g.node(&NodeId::from(ids[k].as_str())).unwrap();
        assert_eq!(node("scales").pos, [400.0, 520.0], "relative to the command");
        assert_eq!(node("blur").values["gain"], json!(1.0), "the command's value over the fragment's");
        assert_eq!(node("scales").values["scales"], json!("9,8"));
        assert_eq!(node("blur").title.as_deref(), Some("the blur"));
        let wire = |from: PortRef, to: PortRef| g.wires.iter().find(|w| w.from == from && w.to == to).cloned();
        assert_eq!(wire(pr(&ids["scales"], "scales"), pr(&ids["blur"], "scale")).unwrap().slice.as_deref(), Some(":-1"));
        assert!(wire(pr(&src, "pic"), pr(&ids["blur"], "image")).is_some(), "what fed it feeds the fragment");
        assert!(wire(pr(&ids["mix"], "result"), pr(&view, "array")).is_some(), "its consumer reads the fragment");
        assert!(g.wire_into(&pr(&view, "text")).is_none());
        let dropped = r["dropped"].as_array().unwrap();
        assert_eq!(dropped.len(), 1, "{dropped:?}");
        assert_eq!(dropped[0]["from"]["port"], "labels");
        assert!(dropped[0]["reason"].as_str().unwrap().contains("no output for labels"));
        assert_eq!(g.wires.len(), 2 + 1 + 1);
        assert!(s.problems().is_empty(), "{:?}", s.problems());
        s.execute("edit.undo", &Value::Null).unwrap();
        assert_eq!(s.graph(), &before, "one undo step restores it");
        s.execute("edit.redo", &Value::Null).unwrap();
        assert_eq!(s.graph().nodes.len(), before.nodes.len() - 1 + 3);
    }

    #[test]
    fn a_setting_can_come_by_wire_or_from_the_bundle() {
        let mut s = Session::new(catalog());
        let src = add(&mut s, "t.src", [0.0, 0.0]);
        let up = add(&mut s, "t.cmd", [0.0, 300.0]);
        let cmd = add(&mut s, "t.cmd", [300.0, 0.0]);
        s.execute("node.set", &json!({"node": up, "port": "steps.gain", "value": 7.0})).unwrap();
        s.execute("node.set", &json!({"node": cmd, "port": "steps.gain", "value": 2.0})).unwrap();
        connect(&mut s, (&up, "tuning"), (&cmd, "tuning"));
        connect(&mut s, (&src, "many"), (&cmd, "steps.scales"));
        let r = s.execute("node.expand", &json!({"node": cmd})).unwrap();
        assert_eq!(r["fromBundle"], json!(["steps.gain"]));
        let ids = ids(&r);
        let g = s.graph();
        assert_eq!(g.node(&NodeId::from(ids["blur"].as_str())).unwrap().values["gain"], json!(7.0), "the bundle's, not the widget's 2");
        assert_eq!(g.wire_into(&pr(&ids["scales"], "scales")).unwrap().from, pr(&src, "many"), "wired through");
        assert_eq!(r["dropped"].as_array().unwrap().len(), 1, "tuning isn't mapped, and that is said: {r}");
    }

    #[test]
    fn a_bundle_value_known_only_at_run_time_is_reported() {
        let mut s = Session::new(catalog());
        let src = add(&mut s, "t.src", [0.0, 0.0]);
        let up = add(&mut s, "t.cmd", [0.0, 300.0]);
        let cmd = add(&mut s, "t.cmd", [300.0, 0.0]);
        connect(&mut s, (&up, "tuning"), (&cmd, "tuning"));
        connect(&mut s, (&src, "many"), (&up, "steps.scales"));
        let r = s.execute("node.expand", &json!({"node": cmd})).unwrap();
        assert_eq!(r["unresolved"], json!(["steps.scales"]));
    }

    #[test]
    fn into_a_group_it_gets_the_parameters_and_outputs_from_the_mapping() {
        let mut s = Session::new(catalog());
        let (_, cmd, view) = command(&mut s);
        s.execute("group.create", &json!({"nodes": [cmd, view], "name": "outer"})).unwrap();
        let r = s.execute("node.expand", &json!({"node": cmd, "group": true})).unwrap();
        let gid = GroupId::from(r["group"].as_str().unwrap());
        let g = s.graph();
        let grp = g.group(&gid).unwrap();
        assert_eq!(grp.name, "Steps", "the catalogue's name");
        assert_eq!(grp.parent, Some(GroupId::from("g1")), "nested in the command's group");
        assert_eq!(grp.nodes.len(), 3);
        let labels: Vec<_> = grp.promoted.iter().map(|p| (p.label.as_str(), p.doc.as_str())).collect();
        assert_eq!(labels, [("gain", "How much"), ("scales", "")]);
        assert_eq!(grp.outputs.iter().map(|o| o.name.as_str()).collect::<Vec<_>>(), ["images"]);
        assert!(s.problems().is_empty());
        let back = Graph::from_json(&g.to_json().unwrap()).unwrap();
        assert_eq!(back.groups, g.groups, "and it round-trips");
        assert_eq!(back.wires, g.wires);
    }

    #[test]
    fn a_command_alone_in_a_group_leaves_it_there_with_its_promotions() {
        let mut s = Session::new(catalog());
        let (_, cmd, _) = command(&mut s);
        let gid = s.execute("group.create", &json!({"nodes": [cmd], "name": "solo"})).unwrap()["id"].as_str().unwrap().to_owned();
        s.execute("group.promote", &json!({"group": gid, "port": {"node": cmd, "port": "steps.gain"}, "label": "Gain"})).unwrap();
        s.execute("group.promote", &json!({"group": gid, "port": {"node": cmd, "port": "odd"}, "label": "Odd"})).unwrap();
        s.execute("group.output", &json!({"group": gid, "from": {"node": cmd, "port": "images"}, "name": "Out"})).unwrap();
        let r = s.execute("node.expand", &json!({"node": cmd})).unwrap();
        let ids = ids(&r);
        let grp = s.graph().group(&GroupId::from(gid.as_str())).expect("the group stayed");
        assert_eq!(grp.nodes.len(), 3);
        assert_eq!(grp.promoted.len(), 1, "Odd has no place");
        assert_eq!(grp.promoted[0].port, pr(&ids["blur"], "gain"));
        assert_eq!(grp.outputs[0].from, pr(&ids["mix"], "result"));
        assert_eq!(r["lost"], json!(["solo: Odd"]));
        assert!(s.problems().is_empty(), "{:?}", s.problems());
    }

    #[test]
    fn the_fragment_stays_on_the_commands_engine() {
        let mut s = Session::new(catalog());
        let (_, cmd, _) = command(&mut s);
        s.graph_mut_untracked().set_engine(&NodeId::from(cmd.as_str()), Some("one".into())).unwrap();
        let r = s.execute("node.expand", &json!({"node": cmd})).unwrap();
        for id in ids(&r).values() {
            assert_eq!(s.graph().node(&NodeId::from(id.as_str())).unwrap().engine.as_deref(), Some("one"));
        }
    }

    #[test]
    fn a_watch_on_the_command_follows_its_output_and_one_with_no_place_goes() {
        let mut s = Session::new(catalog());
        let (_, cmd, _) = command(&mut s);
        s.connected = true;
        s.execute("run.watch", &json!({"node": cmd, "port": "images"})).unwrap();
        let popout = s.execute("view.popout", &json!({"node": cmd, "port": "labels"})).unwrap()["id"].as_u64().unwrap();
        let r = s.execute("node.expand", &json!({"node": cmd})).unwrap();
        let w = s.run.panel().expect("still watching").0;
        assert_eq!((w.node.as_str(), w.port.as_str()), (ids(&r)["mix"].as_str(), "result"));
        assert!(s.views.popout(popout).is_none(), "labels has no counterpart");
        assert!(s.run.watch.iter().all(|w| w.node != cmd), "and nothing watches what isn't there");
    }

    #[test]
    fn a_stored_group_comes_back_as_it_was() {
        let mut c = catalog();
        c.expansions.get_mut("t.cmd").unwrap()["group"] = json!({
            "promoted": [{"port": {"node": "blur", "port": "gain"}, "label": "strength", "doc": "How hard", "min": 0.0, "max": 9.0}],
            "checkpoints": ["blur"],
            "outputs": [{"from": {"node": "mix", "port": "result"}, "name": "picture"}]});
        let mut s = Session::new(c);
        let (_, cmd, _) = command(&mut s);
        let r = s.execute("node.expand", &json!({"node": cmd, "group": "mine"})).unwrap();
        let ids = ids(&r);
        let grp = s.graph().group(&GroupId::from(r["group"].as_str().unwrap())).unwrap();
        assert_eq!(grp.name, "mine");
        assert_eq!(grp.promoted.len(), 1);
        assert_eq!(
            (grp.promoted[0].label.as_str(), grp.promoted[0].doc.as_str(), grp.promoted[0].min, grp.promoted[0].max),
            ("strength", "How hard", Some(0.0), Some(9.0))
        );
        assert_eq!(grp.outputs[0].name, "picture");
        assert_eq!(grp.checkpoints.iter().map(|n| n.0.clone()).collect::<Vec<_>>(), [ids["blur"].clone()], "kept for what reads them");
    }

    #[test]
    fn refused_expansions_change_nothing() {
        let mut s = Session::new(catalog());
        let src = add(&mut s, "t.src", [0.0, 0.0]);
        assert!(matches!(s.execute("node.expand", &json!({"node": src})), Err(SessionError::Disabled { .. })), "no command in the graph");
        let cmd = add(&mut s, "t.cmd", [0.0, 0.0]);
        let before = s.graph().clone();
        for p in [json!({"node": src}), json!({"node": "n99"}), json!({"node": cmd, "group": "  "}), json!({"node": cmd, "group": 3}), json!({})] {
            assert!(s.execute("node.expand", &p).is_err(), "{p}");
            assert_eq!(s.graph(), &before, "{p}");
        }
        let mut c = s.catalog().clone();
        for bad in [
            json!(null),
            json!([]),
            json!({"nodes": {}}),
            json!({"nodes": {"a": {"kind": "nope"}}}),
            json!({"nodes": {"a": {"kind": "t.blur", "values": {"no_port": 1}}}}),
            json!({"nodes": {"a": {"kind": "t.blur"}}, "wires": [{"from": {"node": "zz", "port": "x"}, "to": {"node": "a", "port": "image"}}]}),
            json!({"nodes": {"a": {"kind": "t.blur"}}, "wires": [{"from": {"node": "a", "port": "value"}, "to": {"node": "a", "port": "image"}}]}),
            json!({"nodes": {"a": {"kind": "t.blur"}}, "outputs": {"images": ["a", "no_port"]}, "group": {"outputs": [{"from": {"node": "a", "port": "no_port"}, "name": "n"}]}}),
            json!({"command": "t.other", "nodes": {"a": {"kind": "t.blur"}}}),
        ] {
            c.expansions.insert("t.cmd".into(), bad.clone());
            s.set_catalog(c.clone());
            let group = bad.get("group").is_some();
            assert!(s.execute("node.expand", &json!({"node": cmd, "group": group})).is_err(), "{bad}");
            assert_eq!(s.graph(), &before, "{bad}");
        }
    }

    #[test]
    fn names_do_not_collide() {
        let taken: BTreeSet<String> = ["a".to_owned(), "a 2".to_owned()].into();
        assert_eq!(unique_name("a", &taken), "a 3");
        assert_eq!(unique_name("b", &taken), "b");
    }
}
