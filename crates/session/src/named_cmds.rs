//! Named ports from outside the editor (`docs/research/23-named-ports.md` section 4): setting a
//! graph's parameters by name (`--set NAME=VALUE`), and listing them (`--params`). Any
//! application's `run` uses these, so they agree.
//!
//! - **A bare `NAME`** is looked up among the graph's named ports. **`NODE.PORT`** stays the
//!   escape hatch for a port nobody named; `NODE` is a node id, or a title only one node has.
//! - **A value is read as the port's type:** INT, FLOAT, BOOL (`true`/`false`), an ENUM's choice
//!   (one it offers), STRING as written; any other type as JSON. Ranges are the named port's own
//!   when it has one, else the port's.
//! - **Paths:** a relative path given here is the caller's, so it is made absolute against
//!   `cwd`. Apply these after the workflow's own anchoring (`paths::anchor_workflow`), which
//!   makes the file's relative paths absolute against its folder.
//! - Any failure names the assignment and what the graph has, before anything runs.

use std::path::Path;

use plumcraft_graph::{Graph, NodeId, PortRef, resolve_input};
use plumcraft_proto::catalogue::{Port, SchemaReply};
use serde_json::{Value, json};

/// One `--set` argument split: `NAME=VALUE` or `NODE.PORT=VALUE`.
pub fn split_set(arg: &str) -> Result<(String, String), String> {
    let (k, v) = arg.split_once('=').ok_or_else(|| format!("--set {arg:?}: expected NAME=VALUE or NODE.PORT=VALUE"))?;
    let k = k.trim();
    if k.is_empty() {
        return Err(format!("--set {arg:?}: no name before '='"));
    }
    Ok((k.to_owned(), v.to_owned()))
}

/// The input a `--set` key names.
fn target(graph: &Graph, catalog: &SchemaReply, key: &str) -> Result<(PortRef, Option<(f64, f64)>), String> {
    if let Some(n) = graph.named_port(key) {
        return Ok((n.port.clone(), Some((n.min.unwrap_or(f64::NEG_INFINITY), n.max.unwrap_or(f64::INFINITY)))));
    }
    let Some((node, port)) = key.split_once('.') else {
        let names: Vec<&str> = graph.named.iter().map(|n| n.name.as_str()).collect();
        return Err(if names.is_empty() {
            format!("no port is named {key:?}: this graph names none (use NODE.PORT)")
        } else {
            format!("no port is named {key:?}; the graph has {}", names.join(", "))
        });
    };
    let id = NodeId(node.to_owned());
    let id = if graph.nodes.contains_key(&id) {
        id
    } else {
        let titled: Vec<&NodeId> = graph.nodes.iter().filter(|(_, n)| n.title.as_deref() == Some(node)).map(|(k, _)| k).collect();
        match titled.as_slice() {
            [one] => (*one).clone(),
            [] => return Err(format!("{key}: no node {node:?}")),
            _ => return Err(format!("{key}: {} nodes are titled {node:?}; use the node's id", titled.len())),
        }
    };
    let _ = catalog;
    Ok((PortRef::new(id, port), None))
}

/// `text` read as `port`'s value.
fn read_value(port: &Port, text: &str, range: Option<(f64, f64)>, cwd: &Path) -> Result<Value, String> {
    let (lo, hi) = range.unwrap_or((f64::NEG_INFINITY, f64::INFINITY));
    let lo = lo.max(port.min.unwrap_or(f64::NEG_INFINITY));
    let hi = hi.min(port.max.unwrap_or(f64::INFINITY));
    let in_range = |x: f64| {
        if x < lo || x > hi { Err(format!("{x} is outside {}..{}", fmt_bound(lo), fmt_bound(hi))) } else { Ok(()) }
    };
    match port.ty.as_str() {
        "INT" => {
            let n: i64 = text.trim().parse().map_err(|_| format!("{text:?} is not a whole number"))?;
            in_range(n as f64)?;
            Ok(json!(n))
        }
        "FLOAT" => {
            let x: f64 = text.trim().parse().ok().filter(|x: &f64| x.is_finite()).ok_or_else(|| format!("{text:?} is not a number"))?;
            in_range(x)?;
            Ok(json!(x))
        }
        "BOOL" => match text.trim() {
            "true" => Ok(json!(true)),
            "false" => Ok(json!(false)),
            _ => Err(format!("{text:?} is not true or false")),
        },
        "ENUM" => {
            let choices = port.choices.as_deref().unwrap_or_default();
            choices.iter().find(|c| c.as_str() == Some(text) || serde_json::from_str::<Value>(text).is_ok_and(|v| &v == *c)).cloned().ok_or_else(|| {
                format!(
                    "{text:?} isn't one of its choices: {}",
                    choices.iter().map(|c| c.as_str().map_or_else(|| c.to_string(), str::to_owned)).collect::<Vec<_>>().join(", ")
                )
            })
        }
        "STRING" if port.path.is_some() && !text.is_empty() => {
            let p = Path::new(text);
            Ok(json!(if p.is_absolute() { p.to_path_buf() } else { cwd.join(p) }.display().to_string()))
        }
        "STRING" => Ok(json!(text)),
        _ => serde_json::from_str(text).map_err(|e| format!("{text:?} is not JSON for a {}: {e}", port.ty)),
    }
}

fn fmt_bound(x: f64) -> String {
    if x.is_infinite() { "".into() } else { x.to_string() }
}

/// Apply `sets` (`[(key, value)]`) to `graph`: each value stored as its port's widget value.
/// Nothing is changed unless every one applies.
pub fn apply_sets(graph: &mut Graph, catalog: &SchemaReply, sets: &[(String, String)], cwd: &Path) -> Result<(), String> {
    let mut g = graph.clone();
    for (key, text) in sets {
        let (at, range) = target(&g, catalog, key)?;
        let node = g.nodes.get(&at.node).ok_or_else(|| format!("{key}: no node {}", at.node))?;
        let def = catalog.node(&node.kind).ok_or_else(|| format!("{key}: {} is a {:?}, which no engine here offers", at.node, node.kind))?;
        let port = resolve_input(def, node, &at.node, &at.port).map_err(|e| format!("{key}: {e}"))?.port.clone();
        if !port.has_widget() {
            return Err(format!("{key}: {at} takes a wire only, so it holds no value to set"));
        }
        if g.wires.iter().any(|w| w.to == at) {
            return Err(format!("{key}: {at} is wired; its wire would win over the value"));
        }
        let value = read_value(&port, text, range, cwd).map_err(|e| format!("--set {key}={text}: {e}"))?;
        g.set_value_checked(catalog, &at, value).map_err(|e| format!("{key}: {e}"))?;
    }
    *graph = g;
    Ok(())
}

/// The graph's parameters, in order: each name, its port, type, value now (or the port's
/// default), range or choices, and doc. What `--params` prints.
pub fn params(graph: &Graph, catalog: &SchemaReply) -> Vec<Value> {
    graph
        .named
        .iter()
        .map(|n| {
            let node = graph.nodes.get(&n.port.node);
            let port =
                node.and_then(|node| catalog.node(&node.kind).and_then(|d| resolve_input(d, node, &n.port.node, &n.port.port).ok()).map(|r| r.port.clone()));
            let value =
                node.and_then(|node| node.values.get(&n.port.port).cloned()).or_else(|| port.as_ref().map(|p| p.default.clone())).unwrap_or(Value::Null);
            let mut v = json!({"name": n.name, "port": n.port.to_string(), "type": port.as_ref().map(|p| p.ty.clone()), "value": value});
            if let Some(p) = &port {
                let (lo, hi) = (n.min.or(p.min), n.max.or(p.max));
                if lo.is_some() || hi.is_some() {
                    v["range"] = json!([lo, hi]);
                }
                if let Some(c) = &p.choices {
                    v["choices"] = json!(c);
                }
            }
            if !n.doc.is_empty() {
                v["doc"] = json!(n.doc);
            }
            v
        })
        .collect()
}

/// [`params`] as lines for a terminal.
pub fn params_text(graph: &Graph, catalog: &SchemaReply) -> String {
    let rows = params(graph, catalog);
    if rows.is_empty() {
        return "this graph names no ports (set them as NODE.PORT=VALUE)\n".into();
    }
    let mut out = String::new();
    for r in rows {
        let s = |k: &str| r.get(k).and_then(Value::as_str).unwrap_or("");
        out.push_str(&format!("{}  ({}, {})  = {}", s("name"), s("type"), s("port"), r["value"]));
        if let Some(c) = r.get("choices") {
            out.push_str(&format!("  choices {c}"));
        }
        if let Some(rg) = r.get("range") {
            out.push_str(&format!("  range {rg}"));
        }
        out.push('\n');
        if !s("doc").is_empty() {
            out.push_str(&format!("    {}\n", s("doc")));
        }
    }
    out
}

// ---- the editor's commands (section 3) --------------------------------------------------

use crate::commands::{CommandSpec, always, opt_str_param, port_param, str_param};
use crate::{Result as SResult, Session, SessionError};

fn has_named(s: &Session) -> std::result::Result<(), String> {
    if s.graph().named.is_empty() { Err("no port is named".into()) } else { Ok(()) }
}

/// `graph.port.name {port: {node, port}, name, doc?}`.
fn port_name(s: &mut Session, p: &Value) -> SResult<Value> {
    let (port, name, doc) = (port_param(p, "port")?, str_param(p, "name")?, opt_str_param(p, "doc")?.unwrap_or(""));
    s.edit("Name Port", |g, catalog| Ok(g.name_port(catalog, port.clone(), name, doc)?))?;
    let named = s.graph().name_of_port(&port).unwrap_or(name).to_owned();
    Ok(json!({"name": named, "port": port.to_string()}))
}

/// `graph.port.unname {name}`.
fn port_unname(s: &mut Session, p: &Value) -> SResult<Value> {
    let name = str_param(p, "name")?;
    let gone = s.edit("Remove Port Name", |g, _| Ok(g.unname_port(name)?))?;
    Ok(json!({"name": gone.name, "port": gone.port.to_string()}))
}

/// `graph.port.rename {name, to}`.
fn port_rename(s: &mut Session, p: &Value) -> SResult<Value> {
    let (name, to) = (str_param(p, "name")?, str_param(p, "to")?);
    s.edit("Rename Port", |g, _| Ok(g.rename_port(name, to)?))?;
    Ok(json!({"name": plumcraft_graph::named::check_port_name(to).unwrap_or_else(|_| to.to_owned())}))
}

/// `graph.port.doc {name, doc}`.
fn port_doc(s: &mut Session, p: &Value) -> SResult<Value> {
    let (name, doc) = (str_param(p, "name")?, str_param(p, "doc")?);
    s.edit("Describe Port", |g, _| Ok(g.set_port_doc(name, doc)?))?;
    Ok(Value::Null)
}

/// `graph.port.reorder {names}`.
fn port_reorder(s: &mut Session, p: &Value) -> SResult<Value> {
    let names: Vec<String> = match p.get("names") {
        Some(Value::Array(a)) => a.iter().map(|v| v.as_str().map(str::to_owned)).collect::<Option<_>>(),
        _ => None,
    }
    .ok_or_else(|| SessionError::Params("\"names\" must be a list of the named ports' names".into()))?;
    s.edit("Reorder Parameters", |g, _| Ok(g.reorder_named(&names)?))?;
    Ok(Value::Null)
}

/// `view.params {open?}`: open or close the Parameters window; without `open`, toggle it.
fn view_params(s: &mut Session, p: &Value) -> SResult<Value> {
    let open = match p.get("open") {
        None | Some(Value::Null) => !s.views.graph_params,
        Some(v) => v.as_bool().ok_or_else(|| SessionError::Params("\"open\" must be true or false".into()))?,
    };
    s.views.graph_params = open;
    Ok(json!({"open": open}))
}

/// `graph.params {}`: the graph's parameters, as `--params` lists them.
fn graph_params(s: &mut Session, _: &Value) -> SResult<Value> {
    Ok(json!({"params": params(s.graph(), s.catalog())}))
}

pub(crate) fn specs() -> Vec<CommandSpec> {
    let spec = |id, label, params, run, enabled: crate::commands::Enabled, journal| CommandSpec {
        id,
        label,
        menu: &[],
        shortcut: None,
        params,
        enabled,
        run,
        journal,
    };
    vec![
        spec(
            "graph.port.name",
            "Name This Port…",
            "{port: {node, port}, name, doc?} -> {name, port}: make input `port` one of the graph's parameters, set from outside by name (`run --set NAME=VALUE`). The name is 1 to 40 characters, no '=' or '.', unique; the port must hold a value (have a widget) and not be wired; a named port can't be wired until its name is removed. One undo step; what runs is unchanged",
            port_name,
            always,
            true,
        ),
        spec(
            "graph.port.unname",
            "Remove Port Name",
            "{name} -> {name, port}: the port stops being a parameter; its value stays",
            port_unname,
            has_named,
            true,
        ),
        spec("graph.port.rename", "Rename Port…", "{name, to} -> {name}", port_rename, has_named, true),
        spec("graph.port.doc", "Describe Port…", "{name, doc} -> null: the parameter's one-line description (`--params` shows it)", port_doc, has_named, true),
        spec(
            "graph.port.reorder",
            "Reorder Parameters",
            "{names: [every name once]} -> null: the order the Parameters list and `--params` show",
            port_reorder,
            has_named,
            true,
        ),
        spec(
            "graph.params",
            "Parameters",
            "{} -> {params: [{name, port, type, value, range?, choices?, doc?}]}: the graph's named ports, in order",
            graph_params,
            always,
            false,
        ),
        CommandSpec {
            id: "view.params",
            label: "Parameters…",
            menu: &["View"],
            shortcut: None,
            params: "{open?: bool (default: toggle)} -> {open}: the Parameters window: the graph's named ports, their values and docs, with rename, remove, reorder and a jump to each node",
            enabled: always,
            run: view_params,
            journal: false,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use plumcraft_proto::catalogue::{NodeDef, PathHint, Socket};

    fn catalog() -> SchemaReply {
        let p = |name: &str, ty: &str| Port { name: name.into(), ty: ty.into(), socket: Some(Socket::Both), ..Port::default() };
        let mut scale = p("scale", "FLOAT");
        (scale.min, scale.max) = (Some(0.0), Some(10.0));
        let mut mode = p("mode", "ENUM");
        mode.choices = Some(vec![json!("fast"), json!("good")]);
        let mut path = p("path", "STRING");
        path.path = Some(PathHint::default());
        SchemaReply {
            nodes: vec![NodeDef {
                id: "t.n".into(),
                inputs: vec![scale, mode, path, p("count", "INT"), p("on", "BOOL"), p("text", "STRING"), p("pts", "JSON")],
                outputs: vec![Port { name: "out".into(), ty: "STRING".into(), ..Port::default() }],
                ..Default::default()
            }],
            ..Default::default()
        }
    }

    fn graph() -> (Graph, NodeId) {
        let c = catalog();
        let mut g = Graph::new();
        let a = g.add_node(c.node("t.n").unwrap(), [0.0, 0.0]);
        g.name_port(&c, PortRef::new(a.clone(), "scale"), "scales", "How big").unwrap();
        g.name_port(&c, PortRef::new(a.clone(), "path"), "input", "The picture").unwrap();
        (g, a)
    }

    fn sets(s: &[&str]) -> Vec<(String, String)> {
        s.iter().map(|a| split_set(a).unwrap()).collect()
    }

    #[test]
    fn values_land_by_name_or_by_node_and_port() {
        let c = catalog();
        let (mut g, a) = graph();
        let cwd = Path::new("/work");
        apply_sets(
            &mut g,
            &c,
            &sets(&["scales=2.5", "input=pics/in.jpg", &format!("{a}.mode=good"), &format!("{a}.count=7"), &format!("{a}.on=true"), &format!("{a}.pts=[1,2]")]),
            cwd,
        )
        .unwrap();
        let v = &g.nodes[&a].values;
        assert_eq!(
            (v["scale"].clone(), v["mode"].clone(), v["count"].clone(), v["on"].clone(), v["pts"].clone()),
            (json!(2.5), json!("good"), json!(7), json!(true), json!([1, 2]))
        );
        assert_eq!(v["path"], json!("/work/pics/in.jpg"), "a relative path is the caller's");
        apply_sets(&mut g, &c, &sets(&["input=/abs/x.png"]), cwd).unwrap();
        assert_eq!(g.nodes[&a].values["path"], json!("/abs/x.png"));
        // a title only one node has
        g.nodes.get_mut(&a).unwrap().title = Some("Load".into());
        apply_sets(&mut g, &c, &sets(&["Load.text=hello"]), cwd).unwrap();
        assert_eq!(g.nodes[&a].values["text"], json!("hello"));
    }

    #[test]
    fn a_bad_assignment_changes_nothing_and_says_why() {
        let c = catalog();
        let (mut g, a) = graph();
        let before = g.clone();
        for (bad, says) in [
            ("nope=1", "scales, input"),
            ("scales=11", "outside"),
            ("scales=x", "not a number"),
            (&format!("{a}.mode=slow") as &str, "fast, good"),
            (&format!("{a}.on=yes"), "true or false"),
            (&format!("{a}.count=1.5"), "whole number"),
            ("n99.text=x", "no node"),
        ] {
            let e = apply_sets(&mut g, &c, &sets(&["scales=3", bad]), Path::new("/")).unwrap_err();
            assert!(e.contains(says), "{bad}: {e}");
            assert_eq!(g.nodes, before.nodes, "{bad}: nothing changed");
        }
        assert!(split_set("noequals").is_err() && split_set("=3").is_err());
    }

    #[test]
    fn params_list_the_surface() {
        let c = catalog();
        let (g, _) = graph();
        let p = params(&g, &c);
        assert_eq!(p.len(), 2);
        assert_eq!(
            (p[0]["name"].clone(), p[0]["type"].clone(), p[0]["range"].clone(), p[0]["doc"].clone()),
            (json!("scales"), json!("FLOAT"), json!([0.0, 10.0]), json!("How big"))
        );
        let t = params_text(&g, &c);
        assert!(t.contains("scales  (FLOAT") && t.contains("The picture"), "{t}");
        assert!(params_text(&Graph::new(), &c).contains("names no ports"));
    }
}

#[cfg(test)]
mod command_tests {
    use super::*;

    #[test]
    fn ports_are_named_through_commands_and_undo() {
        let mut s = Session::new(plumcraft_text::engine().schema());
        let a = s.execute("node.add", &json!({"kind": "text.literal"})).unwrap()["id"].as_str().unwrap().to_owned();
        let r = s.execute("graph.port.name", &json!({"port": {"node": a, "port": "text"}, "name": " words ", "doc": "What to say"})).unwrap();
        assert_eq!(r["name"], "words");
        assert_eq!(s.execute("graph.params", &json!({})).unwrap()["params"][0]["doc"], "What to say");
        s.execute("graph.port.rename", &json!({"name": "words", "to": "story"})).unwrap();
        s.execute("graph.port.doc", &json!({"name": "story", "doc": "The tale"})).unwrap();
        assert_eq!(s.graph().named_port("story").unwrap().doc, "The tale");
        assert!(s.execute("graph.port.name", &json!({"port": {"node": a, "port": "text"}, "name": "again"})).is_err(), "named twice");
        assert!(s.execute("graph.port.rename", &json!({"name": "story", "to": "a.b"})).is_err());
        s.execute("edit.undo", &json!({})).unwrap();
        assert_eq!(s.graph().named_port("story").unwrap().doc, "What to say", "undo takes back the doc");
        s.execute("graph.port.unname", &json!({"name": "story"})).unwrap();
        assert!(s.graph().named.is_empty());
        s.execute("edit.undo", &json!({})).unwrap();
        assert!(s.graph().named_port("story").is_some());
        assert!(s.execute("graph.port.reorder", &json!({"names": ["story"]})).is_ok());
        assert!(s.execute("graph.port.reorder", &json!({"names": []})).is_err());
    }
}
