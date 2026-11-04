//! Port addressing and the wire type rules.
//!
//! An input is addressed by name. Besides a node definition's own inputs there are two derived
//! kinds of address:
//!
//! - **Variadic slots:** a port with `variadic` takes wires only into its slots (`c1` … `c8`,
//!   from the template with `{n}` replaced by 1 ..= max), never into the port name itself.
//! - **Sub-ports of a choice:** a port with `options: {choice: [Port]}` exposes each option's ports
//!   as `"<port>.<sub>"`, valid only while the port's current value (the node's widget value, else
//!   the port default) selects that option.
//!
//! A definition's own inputs win over derived addresses, so dotted names are plain ports when a
//! definition declares them.
//!
//! Lifted from the research software's `research-graph` (`ports.rs`), without wire slices (no PlumCraft
//! executor applies them yet) and with the type rules taken from the engine's catalogue
//! ([`SchemaReply::accepts`]) instead of hard-coded.

use std::collections::BTreeSet;

use plumcraft_proto::catalogue::{NodeDef, Port, SchemaReply};
use serde_json::Value;

use crate::{GraphError, Node, NodeId, PortRef, Result, Slice};

/// The most slots a variadic port can have, whatever a (hostile) schema says.
pub const MAX_VARIADIC_SLOTS: u32 = 256;

/// How an input address was resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputKind {
    /// One of the definition's own inputs.
    Port,
    /// A slot of the variadic port `port`.
    Slot { port: String },
    /// A sub-port brought by `parent`'s current choice.
    SubPort { parent: String, choice: String },
}

/// An input address resolved against a node's definition and current values.
#[derive(Debug, Clone)]
pub struct ResolvedInput<'a> {
    /// The port description (for a slot, the variadic port; for a sub-port, the option's port).
    pub port: &'a Port,
    pub kind: InputKind,
}

/// The slot names of a variadic port (`c1` … `c8`); empty if `port` isn't variadic.
pub fn variadic_slots(port: &Port) -> Vec<String> {
    let Some(v) = &port.variadic else { return Vec::new() };
    if !v.names.is_empty() {
        return v.names.iter().take(MAX_VARIADIC_SLOTS as usize).cloned().collect();
    }
    let max = v.max.min(MAX_VARIADIC_SLOTS);
    (1..=max).map(|n| if v.template.contains("{n}") { v.template.replace("{n}", &n.to_string()) } else { format!("{}{n}", port.name) }).collect()
}

/// A JSON value as an option key: strings as themselves, anything else as its JSON text.
pub fn choice_key(v: &Value) -> String {
    v.as_str().map_or_else(|| v.to_string(), str::to_owned)
}

/// The value input `key` (of port `port`) has on `node`: its widget value; else, when a filling
/// choice ([`Port::fills`]) names it, the chosen value's entry; else the port default. What an
/// engine computes with for a port nobody set.
pub fn current_value(def: &NodeDef, node: &Node, key: &str, port: &Port) -> Value {
    node.values.get(key).or_else(|| def.filled_default(key, |n| node.values.get(n))).unwrap_or(&port.default).clone()
}

/// Drop the values of `node` that only repeat what a filling choice gives them, so those ports
/// follow the choice, also when it arrives on a wire. A value set away from the choice's stays.
/// Returns the ports dropped.
pub fn drop_filled(def: &NodeDef, node: &mut Node) -> Vec<String> {
    let same = |a: &Value, b: &Value| a == b || a.as_f64().zip(b.as_f64()).is_some_and(|(x, y)| x == y);
    let gone: Vec<String> = node
        .values
        .iter()
        .filter(|(k, v)| def.filled_default(k, |n| node.values.get(n).filter(|_| n != k.as_str())).is_some_and(|f| same(f, v)))
        .map(|(k, _)| k.clone())
        .collect();
    for k in &gone {
        node.values.remove(k);
    }
    gone
}

/// The inputs of `node` a canvas tucks under "more fields": those a filling choice with `shows`
/// doesn't use. A port shows when it is wired, is the choice itself, or holds a value other than
/// the choice's; else, with `active_when`, when that holds; else when the choice's `shows` lists
/// it. A node without such a choice still tucks a port whose `active_when` doesn't hold (the
/// parameters a SoundCraft plug-in choice brings, for the plug-ins not chosen).
pub fn tucked(def: &NodeDef, node: &Node, wired: impl Fn(&str) -> bool) -> BTreeSet<String> {
    let value = |p: &Port| current_value(def, node, &p.name, p);
    let Some(chooser) = def.inputs.iter().find(|p| p.fills.is_some() && p.shows.is_some()) else {
        return def
            .inputs
            .iter()
            .filter(|p| p.variadic.is_none() && p.has_widget() && !wired(&p.name))
            .filter(|p| p.active_when.as_ref().is_some_and(|when| !when.iter().all(|(k, vals)| def.input(k).is_none_or(|q| vals.contains(&value(q))))))
            .map(|p| p.name.clone())
            .collect();
    };
    let choice = active_choice(chooser, node);
    let filled = chooser.fills.as_ref().and_then(|f| f.get(&choice));
    let shows = chooser.shows.as_ref().and_then(|s| s.get(&choice));
    def.inputs
        .iter()
        .filter(|p| p.name != chooser.name && p.variadic.is_none() && p.has_widget() && !wired(&p.name))
        .filter(|p| filled.and_then(|f| f.get(&p.name)).is_none_or(|v| *v == value(p)))
        .filter(|p| match &p.active_when {
            Some(when) => !when.iter().all(|(k, vals)| def.input(k).is_none_or(|q| vals.contains(&value(q)))),
            None => !shows.is_some_and(|s| s.contains(&p.name)),
        })
        .map(|p| p.name.clone())
        .collect()
}

/// The choice currently selected by `port` on `node`: its widget value, else the port default.
pub fn active_choice(port: &Port, node: &Node) -> String {
    choice_key(node.values.get(&port.name).unwrap_or(&port.default))
}

/// Every input address `node` currently has, in definition order: own inputs (variadic ports
/// replaced by their slots), each followed by the sub-ports its current choice brings.
pub fn input_ports<'a>(def: &'a NodeDef, node: &Node) -> Vec<(String, &'a Port)> {
    let mut out = Vec::new();
    for p in &def.inputs {
        if p.variadic.is_some() {
            out.extend(variadic_slots(p).into_iter().map(|s| (s, p)));
            continue;
        }
        out.push((p.name.clone(), p));
        if let Some(opts) = &p.options
            && let Some(subs) = opts.get(&active_choice(p, node))
        {
            out.extend(subs.iter().map(|s| (format!("{}.{}", p.name, s.name), s)));
        }
    }
    out
}

/// Resolve input address `name` of node `id` (an instance of `def`).
pub fn resolve_input<'a>(def: &'a NodeDef, node: &Node, id: &NodeId, name: &str) -> Result<ResolvedInput<'a>> {
    let at = || PortRef::new(id.clone(), name);
    if let Some(p) = def.input(name) {
        if p.variadic.is_some() {
            return Err(GraphError::VariadicPort { at: at(), slots: variadic_slots(p).join(", ") });
        }
        return Ok(ResolvedInput { port: p, kind: InputKind::Port });
    }
    for p in &def.inputs {
        if p.variadic.is_some() && variadic_slots(p).iter().any(|s| s == name) {
            return Ok(ResolvedInput { port: p, kind: InputKind::Slot { port: p.name.clone() } });
        }
        let Some(opts) = &p.options else { continue };
        let Some(sub) = name.strip_prefix(p.name.as_str()).and_then(|r| r.strip_prefix('.')) else { continue };
        let active = active_choice(p, node);
        if let Some(sp) = opts.get(&active).and_then(|ps| ps.iter().find(|q| q.name == sub)) {
            return Ok(ResolvedInput { port: sp, kind: InputKind::SubPort { parent: p.name.clone(), choice: active } });
        }
        let needs: Vec<&str> = opts.iter().filter(|(_, ps)| ps.iter().any(|q| q.name == sub)).map(|(k, _)| k.as_str()).collect();
        if !needs.is_empty() {
            return Err(GraphError::InactiveSubPort { at: at(), parent: p.name.clone(), needs: needs.join(", "), now: active });
        }
    }
    Err(GraphError::UnknownPort { node: id.clone(), kind: def.id.clone(), dir: "input", port: name.to_owned() })
}

/// Check one wire's types and list-ness against the catalogue. `from_is_list` is whether the
/// source carries a list (its port is `list: true`, or its node is mapped over a list; see
/// [`crate::Graph::output_is_list`]).
///
/// Types follow the engine's conversions ([`SchemaReply::accepts`]: the same type always, plus
/// the engine's `compat` rules or the base defaults). A list feeds a `list: true` input whole,
/// or a `list: false` input item by item (the engine maps the node). A single value can't feed
/// a `list: true` input.
pub fn check_wire(catalog: &SchemaReply, from: &PortRef, out: &Port, from_is_list: bool, to: &PortRef, inp: &Port) -> Result<()> {
    check_sliced_wire(catalog, from, out, from_is_list, None, to, inp)
}

/// [`check_wire`] for a wire with an optional slice. A slice needs a list; a range keeps it a
/// list, and an index makes it one item, so the rules above apply to the result: an index into a
/// list feeds a single input without mapping, and can't feed a list input.
pub fn check_sliced_wire(catalog: &SchemaReply, from: &PortRef, out: &Port, from_is_list: bool, slice: Option<&Slice>, to: &PortRef, inp: &Port) -> Result<()> {
    if !catalog.accepts(&out.ty, &inp.ty) {
        return Err(GraphError::TypeMismatch { from: from.to_string(), from_ty: out.ty.clone(), to: to.to_string(), to_ty: inp.ty.clone() });
    }
    if let Some(s) = slice
        && !from_is_list
    {
        return Err(GraphError::BadSlice {
            from: from.to_string(),
            to: to.to_string(),
            reason: format!("[{s}] needs a list, but {from} gives a single value"),
        });
    }
    let carries_list = from_is_list && !slice.is_some_and(Slice::is_index);
    if inp.list && !carries_list {
        return Err(GraphError::ExpectsList { from: from.to_string(), to: to.to_string(), ty: inp.ty.clone() });
    }
    Ok(())
}

/// The sub-port addresses `parent`'s current choice brings.
pub(crate) fn active_subports(def: &NodeDef, node: &Node, parent: &str) -> BTreeSet<String> {
    let prefix = format!("{parent}.");
    input_ports(def, node).into_iter().map(|(n, _)| n).filter(|n| n.starts_with(&prefix)).collect()
}

/// Is `name` a sub-port address of `parent` under any of its choices?
pub(crate) fn is_subport_of(def: &NodeDef, parent: &str, name: &str) -> bool {
    def.input(parent).and_then(|p| p.options.as_ref()).is_some_and(|opts| {
        name.strip_prefix(parent).and_then(|r| r.strip_prefix('.')).is_some_and(|sub| opts.values().any(|ps| ps.iter().any(|q| q.name == sub)))
    })
}
