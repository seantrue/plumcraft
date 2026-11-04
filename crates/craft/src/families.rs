//! Node families generated from the command list (`[[generate_commands]]`): one transform node
//! per matching command, its widgets read from the command's own params description, so the
//! craft's big command groups (PhotoCraft's filters and adjustments, EffectCraft's layer
//! commands) are nodes with widgets rather than only the escape hatch.
//!
//! Safety: a generated command matches the block's `commands`, and neither its `deny` nor the
//! escape hatch's `deny`; parameters in `drop_keys` (file paths, process state) are never
//! widgets. What still fails with its defaults goes in the pack's `exclude` after the smoke test.
//!
//! Two params dialects are read: the prose one `grammar::doc_keys` reads (`{layers?, op: a|b}`),
//! and PhotoCraft's JSON-like one (`{"radius":0.1..1000=1,"mode":"a|b"="a","invert":bool}`),
//! read here by [`json_keys`] until `grammar` reads it (fold it in there at merge).

use std::collections::{BTreeMap, BTreeSet};

use plumcraft_proto::catalogue::{NodeDef, Port};
use serde_json::{Map, Value};

use crate::manifest::{GenerateCommandsSpec, Kind, Manifest, WidgetSpec};
use crate::nodes::{Plan, WidgetPlan};
use crate::{Catalogue, JSON, grammar};

/// `id` matches a pattern: equal, or the pattern ends in `.` and `id` starts with it.
pub(crate) fn matches(id: &str, pats: &[String]) -> bool {
    pats.iter().any(|p| id == p || (p.ends_with('.') && id.starts_with(p.as_str())))
}

/// Every command a curated node can run (templated commands expanded): left to those nodes.
pub(crate) fn curated_commands(m: &Manifest) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for n in &m.nodes {
        let Some(c) = &n.command else { continue };
        let templated: Vec<&WidgetSpec> = n.widgets.iter().filter(|w| w.template).collect();
        if templated.is_empty() {
            out.entry(c.clone()).or_insert_with(|| n.id.clone());
        }
        for w in templated {
            for choice in w.choices.iter().flatten().filter_map(Value::as_str) {
                out.entry(c.replace(&format!("{{{}}}", w.name), choice)).or_insert_with(|| n.id.clone());
            }
        }
    }
    out
}

/// The commands a block generates nodes for, each with its node id, or why it is left out (a
/// curated node runs it, or has its id). Sorted by command id.
pub(crate) fn family(m: &Manifest, g: &GenerateCommandsSpec, cat: &Catalogue) -> Vec<(String, String, Result<(), String>)> {
    let curated = curated_commands(m);
    let curated_ids: BTreeSet<&str> = m.nodes.iter().map(|n| n.id.as_str()).chain(m.run_node.iter().map(|r| r.id.as_str())).collect();
    let hatch_deny = m.run_node.as_ref().map(|r| r.deny.as_slice()).unwrap_or_default();
    cat.commands
        .keys()
        .filter(|c| matches(c, &g.commands) && !matches(c, &g.deny) && !matches(c, hatch_deny))
        .map(|c| {
            let id = format!("{}{c}", g.id_prefix);
            let why = match curated.get(c) {
                Some(by) => Err(format!("curated node {by} runs {c}")),
                None if curated_ids.contains(id.as_str()) => Err(format!("a curated node is called {id}")),
                None => Ok(()),
            };
            (c.clone(), id, why)
        })
        .collect()
}

/// One parameter read from a description.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Key {
    pub name: String,
    /// Not required by the craft (`?`, or in the JSON dialect no default given): unset, the craft's default applies.
    pub optional: bool,
    /// What the description says of it (the tooltip).
    pub text: String,
    /// The port read from it; `None`: leave it to `widget_port`'s inference.
    pub port: Option<grammar::Inferred>,
}

/// The keys of a description in either dialect; `Err` when it reads as neither.
pub fn keys(doc: &str) -> Result<Vec<Key>, String> {
    let d = doc.trim_start();
    if d.starts_with("{\"") || d.starts_with("{}") {
        return json_keys(d);
    }
    let parsed = grammar::doc_keys(d)?;
    if !parsed.more.is_empty() {
        return Err("it imports another command's keys (…)".into());
    }
    let mut out: Vec<Key> = Vec::new();
    for (name, optional, text) in parsed.keys {
        if out.iter().any(|k| k.name == name) {
            continue;
        }
        let mut inf = grammar::infer(text);
        // A required choice with no default given: the first.
        if !optional && inf.ty == "ENUM" && inf.default.is_null() {
            inf.default = inf.choices.first().map_or(Value::Null, |c| Value::from(c.as_str()));
        }
        // A required key stays required, default or not: with none the user must give it (the
        // node fails until then, rather than leaving out a key the craft needs).
        out.push(Key { name: name.to_owned(), optional, text: text.trim_start_matches(['?', ':', ' ']).to_owned(), port: (!optional).then_some(inf) });
    }
    Ok(out)
}

/// The end of the object `s` starts with (`s[0] == '{'`): the index of its `}`.
fn object_end(s: &str) -> Option<usize> {
    let (mut depth, mut quote) = (0usize, false);
    for (i, c) in s.char_indices() {
        match c {
            '"' => quote = !quote,
            _ if quote => {}
            '{' | '[' | '(' => depth += 1,
            '}' | ']' | ')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// PhotoCraft's dialect: `{"key":spec,…} prose`, a spec being `lo..hi=default`, `"a|b"="a"`,
/// `bool=true`, `json`, `color=#ffffff`, `u32=0`, `px=0`, `[…]`, with `?` for optional.
pub fn json_keys(doc: &str) -> Result<Vec<Key>, String> {
    let end = object_end(doc).ok_or("the params object isn't closed")?;
    let body = doc.get(1..end).unwrap_or_default();
    let mut out: Vec<Key> = Vec::new();
    for entry in grammar::split_top(body, ',').into_iter().map(str::trim).filter(|e| !e.is_empty()) {
        let rest = entry.strip_prefix('"').ok_or_else(|| format!("`{}` doesn't start with a quoted key", entry.chars().take(40).collect::<String>()))?;
        let (name, spec) = rest.split_once('"').ok_or("an unclosed key")?;
        let spec = spec.trim_start().strip_prefix(':').ok_or_else(|| format!("{name}: no `:`"))?.trim();
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return Err(format!("{name:?} is not a parameter name"));
        }
        if out.iter().any(|k| k.name == name) {
            continue;
        }
        let (inf, optional) = json_spec(spec);
        out.push(Key { name: name.to_owned(), optional: optional || inf.default.is_null(), text: spec.to_owned(), port: Some(inf) });
    }
    Ok(out)
}

/// One spec of the JSON-like dialect, and whether it is marked optional.
fn json_spec(spec: &str) -> (grammar::Inferred, bool) {
    let parts = grammar::split_top(spec, '=');
    let (ty_text, default_text) = match parts.as_slice() {
        [t] => (t.trim(), None),
        [t, d, ..] => (t.trim(), Some(d.trim())),
        [] => ("", None),
    };
    let optional = ty_text.ends_with('?') || default_text.is_some_and(|d| d.ends_with('?'));
    let t = ty_text.trim_end_matches('?').trim();
    let mut inf = grammar::Inferred::default();
    let lower = t.to_ascii_lowercase();
    let default = default_text.map(|d| d.trim_end_matches('?')).map(|d| serde_json::from_str::<Value>(d).unwrap_or_else(|_| Value::from(d)));
    if let Some(inner) = t.strip_prefix('"').and_then(|x| x.strip_suffix('"')).filter(|x| x.contains('|') && !x.contains('"')) {
        let choices: Vec<String> = inner.split('|').map(str::to_owned).collect();
        if choices.iter().all(|c| !c.is_empty() && c.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')) {
            inf.ty = "ENUM".into();
            inf.default = default.filter(|d| d.as_str().is_some_and(|s| choices.iter().any(|c| c == s))).unwrap_or(Value::Null);
            inf.choices = choices;
            return (inf, optional);
        }
    }
    let range = t.split_once("..").and_then(|(a, b)| Some((a, b, a.trim().parse::<f64>().ok()?, b.trim().parse::<f64>().ok()?)));
    if lower == "bool" {
        inf.ty = "BOOL".into();
        inf.default = default.filter(Value::is_boolean).unwrap_or(Value::Null);
    } else if lower == "json" || t.starts_with('[') || t.starts_with('{') || lower.contains('[') {
        inf.ty = JSON.into();
        inf.default = default.filter(|d| d.is_array() || d.is_object()).unwrap_or(Value::Null);
    } else if lower == "color" || lower.starts_with("\"#") || lower.starts_with('#') {
        inf.ty = "STRING".into();
        inf.default = default.filter(Value::is_string).unwrap_or(Value::Null);
    } else if lower == "u32" {
        inf.ty = "INT".into();
        inf.min = Some(0.0);
        inf.max = Some(f64::from(u32::MAX));
        inf.default = default.filter(Value::is_u64).unwrap_or(Value::Null);
    } else if ["px", "mm", "deg", "pt", "%", "number"].contains(&lower.as_str()) {
        inf.ty = "FLOAT".into();
        inf.default = default.filter(Value::is_number).unwrap_or(Value::Null);
    } else if let Some((a, b, lo, hi)) = range {
        let int = !a.contains('.') && !b.contains('.') && default.as_ref().is_none_or(|d| d.is_i64() || d.is_u64());
        inf.ty = if int { "INT" } else { "FLOAT" }.into();
        if lo <= hi && lo.is_finite() && hi.is_finite() {
            inf.min = Some(lo);
            inf.max = Some(hi);
        }
        inf.default = default.filter(Value::is_number).unwrap_or(Value::Null);
    } else {
        // text, an index or id, a document: a string the craft reads its own way
        inf.ty = "STRING".into();
        inf.default = default.filter(Value::is_string).unwrap_or(Value::Null);
    }
    (inf, optional)
}

/// A generated node's NodeDef and how it runs, from its command's description.
pub(crate) fn node(m: &Manifest, g: &GenerateCommandsSpec, command: &str, id: &str, cat: &Catalogue) -> Result<(NodeDef, Plan), String> {
    let doc = cat.commands.get(command).ok_or_else(|| format!("{} has no command {command}", m.name))?;
    let keys = match cat.schemas.get(command) {
        // a JSON Schema: widget_port reads each property itself
        Some(schema) => {
            let required: Vec<&str> =
                schema.get("required").and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
            let props = schema.get("properties").and_then(Value::as_object);
            props.into_iter().flat_map(|p| p.keys()).map(|k| Key { name: k.clone(), optional: !required.contains(&k.as_str()), ..Default::default() }).collect()
        }
        None => keys(doc).map_err(|e| format!("{command}: its params don't read: {e}"))?,
    };
    let mut inputs = vec![super::socket("doc", &m.doc.wire)];
    if let Some(sel) = &g.select {
        inputs.extend(super::select_ports(sel));
    }
    let mut widgets = Vec::new();
    for k in keys {
        if g.drop_keys.contains(&k.name) || inputs.iter().any(|p| p.name == k.name) {
            continue;
        }
        let mut w = WidgetSpec { name: k.name.clone(), optional: k.optional, ..Default::default() };
        if let Some(inf) = &k.port {
            w.ty = (!inf.ty.is_empty()).then(|| inf.ty.clone());
            w.min = inf.min;
            w.max = inf.max;
            w.choices = (!inf.choices.is_empty()).then(|| inf.choices.iter().map(|c| Value::from(c.as_str())).collect());
            w.default = if k.optional { Value::Null } else { inf.default.clone() };
        }
        if !k.text.is_empty() {
            w.tooltip = Some(k.text.chars().take(300).collect());
        }
        if let Some(v) = g.defaults.get(&k.name) {
            w.default = v.clone();
            w.optional = false;
        }
        let port: Port = super::widget_port(&w, Some(command), cat)?;
        widgets.push(WidgetPlan {
            name: k.name.clone(),
            key: k.name.clone(),
            into: None,
            optional: w.optional,
            template: false,
            allowed: None,
            ..Default::default()
        });
        inputs.push(port);
    }
    let (label, menu) = cat.info.get(command).cloned().unwrap_or_default();
    let label = if label.is_empty() { command.to_owned() } else { label.trim_end_matches(['…', '.']).trim().to_owned() };
    let mut category = vec![m.name.clone()];
    if !g.family.is_empty() {
        category.push(g.family.clone());
    }
    category.extend(menu.into_iter().skip(1).filter(|s| !s.is_empty() && *s != g.family));
    let def = NodeDef {
        id: id.to_owned(),
        display: label,
        category,
        description: format!("{command}: {}", doc.chars().take(400).collect::<String>()),
        inputs,
        outputs: vec![
            Port { name: "doc".into(), ty: m.doc.wire.clone(), ..Default::default() },
            Port { name: "result".into(), ty: JSON.into(), ..Default::default() },
        ],
        family: m.id.clone(),
        memo: super::memo(g.pure, false),
        x: BTreeMap::from([(format!("{}.command", m.id), Value::from(command))]),
        ..Default::default()
    };
    let plan = Plan {
        kind: Kind::Transform,
        command: Some(command.to_owned()),
        select: g.select.is_some(),
        widgets,
        files: Vec::new(),
        fixed: Map::new(),
        outputs: Vec::new(),
        export: None,
        export_action: None,
        timeout: None,
    };
    Ok((def, plan))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::Manifest;
    use serde_json::json;

    const PACK: &str = r##"
id = "toy"
name = "Toy"
command = ["toy-cli", "mcp"]

[tools]
list = "command_list"
run = { tool = "command_run", id = "id", params = "params" }

[actions]
open = { tool = "open", path = "path" }
save = { tool = "save", path = "path" }

[doc]
wire = "toy.doc"
ext = "toy"
media = "application/x-toy"
app = "toy"

[run_node]
id = "t.run"
display = "Run"
allow = ["filter.", "layer."]
deny = ["filter.secret"]

[[node]]
id = "t.blur"
display = "Blur"
kind = "transform"
command = "filter.blur.gaussian"

[[generate_commands]]
commands = ["filter.", "layer.addMarker", "layer.transform"]
deny = ["filter.last"]
id_prefix = "t."
family = "Filter"
drop_keys = ["mapPath"]
defaults = { layers = ["#1"] }
"##;

    fn catalogue() -> Catalogue {
        let mut cat = Catalogue::default();
        for (id, label, menu, doc) in [
            ("filter.blur.gaussian", "Gaussian Blur…", vec!["Filter", "Blur"], r#"{"radius":0.1..1000=1}"#),
            (
                "filter.blur.box",
                "Box Blur…",
                vec!["Filter", "Blur"],
                r#"{"radius":1..2000=1,"quality":"draft|good|best"="good","mono":bool,"seed":u32=0,"pins":json,"mapPath":text}"#,
            ),
            ("filter.blur.average", "Average", vec!["Filter", "Blur"], "{}"),
            ("filter.last", "Last Filter", vec!["Filter"], "{}"),
            ("filter.secret", "Secret", vec!["Filter"], "{}"),
            ("filter.broken", "Broken", vec!["Filter"], "a prose sentence"),
            ("layer.addMarker", "Add Marker", vec!["Layer"], "{layers?, time?, comment?}"),
            ("layer.transform", "Transform", vec!["Layer"], "{layers?, op: reset|center|fit}"),
            ("layer.delete", "Delete", vec!["Layer"], "{layers?}"),
        ] {
            cat.commands.insert(id.into(), doc.into());
            cat.info.insert(id.into(), (label.into(), menu.into_iter().map(str::to_owned).collect()));
        }
        cat
    }

    #[test]
    fn a_family_is_its_commands_less_the_denied_and_the_curated() {
        let m = Manifest::parse(PACK).unwrap();
        let cat = catalogue();
        let g = &m.generated_commands[0];
        let fam = family(&m, g, &cat);
        let ids: Vec<(&str, bool)> = fam.iter().map(|(_, id, ok)| (id.as_str(), ok.is_ok())).collect();
        assert_eq!(
            ids,
            [
                ("t.filter.blur.average", true),
                ("t.filter.blur.box", true),
                ("t.filter.blur.gaussian", false), // curated t.blur runs it
                ("t.filter.broken", true),
                ("t.layer.addMarker", true),
                ("t.layer.transform", true),
            ],
            "filter.last (the block's deny), filter.secret (the hatch's deny) and layer.delete (not matched) are left out"
        );
        assert!(fam.iter().any(|(_, _, w)| w.as_ref().is_err_and(|e| e.contains("t.blur"))));
    }

    #[test]
    fn widgets_come_from_the_description() {
        let m = Manifest::parse(PACK).unwrap();
        let cat = catalogue();
        let g = &m.generated_commands[0];
        let (def, plan) = node(&m, g, "filter.blur.box", "t.filter.blur.box", &cat).unwrap();
        assert_eq!(def.display, "Box Blur");
        assert_eq!(def.category, ["Toy", "Filter", "Blur"]);
        let port = |n: &str| def.inputs.iter().find(|p| p.name == n).cloned().unwrap_or_else(|| panic!("no {n}"));
        let names: Vec<&str> = def.inputs.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["doc", "radius", "quality", "mono", "seed", "pins"], "mapPath is dropped");
        let r = port("radius");
        assert_eq!((r.ty.as_str(), r.min, r.max, r.default.clone(), r.optional), ("INT", Some(1.0), Some(2000.0), json!(1), false));
        let q = port("quality");
        assert_eq!((q.ty.as_str(), q.default.clone()), ("ENUM", json!("good")));
        assert_eq!(q.choices, Some(vec![json!("draft"), json!("good"), json!("best")]));
        let mono = port("mono");
        assert_eq!((mono.ty.as_str(), mono.optional), ("BOOL", true), "no default: unset, the craft's applies");
        assert_eq!(port("seed").max, Some(f64::from(u32::MAX)));
        assert_eq!(port("pins").ty, "JSON");
        assert_eq!(plan.command.as_deref(), Some("filter.blur.box"));
        assert!(!plan.select);
        assert_eq!(plan.widgets.iter().filter(|w| w.optional).count(), 2, "mono and pins");

        // the prose dialect, with the pack's defaults
        let (def, plan) = node(&m, g, "layer.transform", "t.layer.transform", &cat).unwrap();
        let op = def.inputs.iter().find(|p| p.name == "op").unwrap();
        assert_eq!((op.ty.as_str(), op.default.clone(), op.optional), ("ENUM", json!("reset"), false), "a required choice defaults to the first");
        let layers = def.inputs.iter().find(|p| p.name == "layers").unwrap();
        assert_eq!((layers.default.clone(), layers.optional), (json!(["#1"]), false));
        assert!(plan.widgets.iter().all(|w| !w.optional));
        let (def, _) = node(&m, g, "layer.addMarker", "t.layer.addMarker", &cat).unwrap();
        assert!(def.inputs.iter().filter(|p| p.name == "time" || p.name == "comment").all(|p| p.optional));

        // no parameters, and a description that doesn't read
        let (def, _) = node(&m, g, "filter.blur.average", "t.filter.blur.average", &cat).unwrap();
        assert_eq!(def.inputs.len(), 1);
        assert!(node(&m, g, "filter.broken", "t.filter.broken", &cat).unwrap_err().contains("don't read"));
    }

    #[test]
    fn the_json_dialect_reads_photocrafts_specs() {
        let k = json_keys(
            r#"{"amount":0.1..400=12.5,"distribution":"uniform|gaussian","seed":u32=0,"color":color=#ffffff,"depthChannel":index|name?,"focalLength":mm=0,"target":"pixels"|"mask"?=Channels panel target} (prose)"#,
        )
        .unwrap();
        let by = |n: &str| k.iter().find(|x| x.name == n).unwrap().clone();
        let a = by("amount").port.unwrap();
        assert_eq!((a.ty.as_str(), a.min, a.max, a.default), ("FLOAT", Some(0.1), Some(400.0), json!(12.5)));
        assert!(by("distribution").optional && by("distribution").port.unwrap().ty == "ENUM");
        assert_eq!(by("color").port.unwrap().default, json!("#ffffff"));
        assert!(by("depthChannel").optional);
        assert_eq!(by("focalLength").port.unwrap().ty, "FLOAT");
        assert!(by("target").optional);
        assert_eq!(json_keys("{}").unwrap(), []);
    }

    #[test]
    fn hostile_descriptions_never_panic() {
        for s in [
            "{",
            "{\"",
            "{\"a\"",
            "{\"a\":",
            "{\"a\":=",
            "{\"a\":..=}",
            "{\"a\":\"|\"}",
            "{\"é\":1}",
            "{\"a\":1e999..2=3}",
            "{\"a\":[[[}",
            "}",
            "",
            "{a?|",
            "{…}",
        ] {
            let _ = keys(s);
            let _ = json_keys(s);
        }
    }
}
