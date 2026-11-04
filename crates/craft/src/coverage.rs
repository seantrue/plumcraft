//! Parameter coverage: which keys each curated node sends, which of its command's parameters it
//! never sends (hidden), and which of the craft's commands no node runs. [`node_keys`] is the one
//! place the sent keys are worked out: [`crate::build`]'s catalogue check uses it, and so does
//! [`coverage`] (and the `params_coverage` test), so the two can't drift.

use std::collections::BTreeSet;

use serde_json::Value;

use crate::manifest::{Kind, Manifest, NodeSpec};
use crate::{Catalogue, Report};

/// What one curated node runs and sends.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NodeKeys {
    /// Every command the node can run: `command` (once per choice of a template widget) and its
    /// action's command. Each must be in the catalogue.
    pub runs: Vec<String>,
    /// The command its keys go to: the open action's for an Open, the export's for an Export,
    /// else the first of `runs`.
    pub target: Option<String>,
    /// The keys the manifest makes it send (widgets, file sockets, fixed values), in order; the
    /// catalogue check requires the target to accept each (`*` merges any object).
    pub checked: Vec<String>,
    /// Everything it sends: `checked`, plus what the runner adds (the open or export action's
    /// path and params, `format` for the shared export, the export's option widgets, and the
    /// document id for a craft that names documents by id).
    pub sent: BTreeSet<String>,
}

/// The commands, target and keys of a curated node (as [`crate::nodes`] runs it).
pub fn node_keys(m: &Manifest, spec: &NodeSpec) -> NodeKeys {
    let mut runs: Vec<String> = Vec::new();
    if let Some(c) = &spec.command {
        let templated: Vec<_> = spec.widgets.iter().filter(|w| w.template).collect();
        if templated.is_empty() {
            runs.push(c.clone());
        } else {
            for w in &templated {
                for choice in w.choices.iter().flatten().filter_map(Value::as_str) {
                    runs.push(c.replace(&format!("{{{}}}", w.name), choice));
                }
            }
        }
    }
    if let Some(c) = spec.action.as_ref().and_then(|a| a.command.clone()) {
        runs.push(c);
    }
    let target = match spec.kind {
        Kind::Open => m.actions.open.command.clone(),
        Kind::Export => match (&spec.action, &spec.command) {
            (Some(a), _) => a.command.clone(),
            (None, Some(c)) => Some(c.clone()),
            (None, None) => m.actions.export.as_ref().and_then(|a| a.command.clone()),
        },
        _ => runs.first().cloned(),
    };
    let widgets = spec.widgets.iter().filter(|w| !w.template).map(|w| w.into.clone().unwrap_or_else(|| w.key.clone().unwrap_or_else(|| w.name.clone())));
    let files = spec.files.iter().map(|f| f.key.clone().unwrap_or_else(|| "path".into()));
    let checked: Vec<String> = widgets.chain(files).chain(spec.fixed.keys().cloned()).collect();
    let mut sent: BTreeSet<String> = checked.iter().cloned().collect();
    match spec.kind {
        Kind::Open => {
            let open = &m.actions.open;
            sent.insert(open.path.clone().unwrap_or_else(|| "path".into()));
            sent.extend(open.params.keys().cloned());
        }
        Kind::Export => {
            // The node's own action (with `command`, the export action's path key), or the
            // shared export, which is also sent `format`.
            let own = spec.action.as_ref();
            let shared = m.actions.export.as_ref();
            let path = match (own, &spec.command) {
                (Some(a), _) => a.path.clone(),
                (None, _) => shared.and_then(|a| a.path.clone()),
            };
            sent.insert(path.unwrap_or_else(|| "path".into()));
            if let Some(a) = own.or(if spec.command.is_none() { shared } else { None }) {
                sent.extend(a.params.keys().cloned());
            }
            if own.is_none() && spec.command.is_none() {
                sent.insert("format".into());
            }
            if spec.options_from.is_some() {
                sent.extend(spec.options.iter().cloned());
            }
        }
        Kind::New | Kind::Transform | Kind::Query => {}
    }
    if matches!(spec.kind, Kind::Transform | Kind::Query | Kind::Export)
        && let Some(id) = &m.doc.id_param
    {
        sent.insert(id.clone());
    }
    NodeKeys { runs, target, checked, sent }
}

/// One command a curated node runs, and the parameters it never sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeCoverage {
    pub node: String,
    pub command: String,
    /// The keys the command takes, or `None` when its description can't be read (unparsed) or
    /// it takes any key.
    pub accepted: Option<Vec<String>>,
    pub sent: BTreeSet<String>,
    /// `accepted` less `sent`, sorted; `None` when unparsed. Empty when the node merges a JSON
    /// object (`*`).
    pub hidden: Option<Vec<String>>,
}

/// A craft's parameter coverage.
#[derive(Debug, Clone, Default)]
pub struct Coverage {
    /// The size of the command catalogue.
    pub commands: usize,
    /// One row per (curated node, command it runs) whose command is in the catalogue.
    pub nodes: Vec<NodeCoverage>,
    /// (node, command) pairs whose command isn't in the catalogue.
    pub missing: Vec<(String, String)>,
    /// The catalogue commands some curated or generated node runs.
    pub run: BTreeSet<String>,
    /// The catalogue commands no node runs (the escape hatch aside).
    pub not_run: Vec<String>,
    /// How many of `not_run` take at least one parameter.
    pub not_run_with_params: usize,
    /// How many of `not_run` have a description that can't be read.
    pub not_run_unparsed: usize,
}

impl Coverage {
    /// The hidden parameters on every node command.
    pub fn hidden(&self) -> usize {
        self.nodes.iter().filter_map(|n| n.hidden.as_ref()).map(Vec::len).sum()
    }
    /// The node commands with at least one hidden parameter.
    pub fn with_hidden(&self) -> usize {
        self.nodes.iter().filter(|n| n.hidden.as_ref().is_some_and(|h| !h.is_empty())).count()
    }
    /// The node commands whose description can't be read.
    pub fn unparsed(&self) -> usize {
        self.nodes.iter().filter(|n| n.accepted.is_none()).count()
    }
}

/// The coverage of `m`'s curated nodes against the catalogue `report` read (build with
/// `include_excluded` to cover every node the pack lists).
pub fn coverage(m: &Manifest, report: &Report) -> Coverage {
    let cat = Catalogue { commands: report.commands.clone(), schemas: report.schemas.clone(), ..Default::default() };
    let mut cov = Coverage { commands: cat.commands.len(), ..Default::default() };
    for spec in &m.nodes {
        let keys = node_keys(m, spec);
        let targets: Vec<String> = match spec.kind {
            Kind::Open | Kind::Export => keys.target.iter().cloned().collect(),
            Kind::New | Kind::Transform | Kind::Query => keys.runs.clone(),
        };
        for c in keys.runs.iter().chain(&targets) {
            if cat.commands.contains_key(c) {
                cov.run.insert(c.clone());
            }
        }
        let any = keys.checked.iter().any(|k| k == "*");
        for t in targets {
            if !cat.commands.contains_key(&t) {
                cov.missing.push((spec.id.clone(), t));
                continue;
            }
            let accepted = crate::accepted(&cat, &t);
            let hidden = accepted.as_ref().map(|a| {
                let mut h: Vec<String> = if any { Vec::new() } else { a.iter().filter(|k| !keys.sent.contains(*k)).cloned().collect() };
                h.sort();
                h.dedup();
                h
            });
            cov.nodes.push(NodeCoverage { node: spec.id.clone(), command: t, accepted, sent: keys.sent.clone(), hidden });
        }
    }
    for g in &m.generated {
        if cat.commands.contains_key(&g.command) {
            cov.run.insert(g.command.clone());
        }
    }
    for c in cat.commands.keys().filter(|c| !cov.run.contains(*c)) {
        match crate::accepted(&cat, c) {
            Some(a) if !a.is_empty() => cov.not_run_with_params += 1,
            Some(_) => {}
            None => cov.not_run_unparsed += 1,
        }
        cov.not_run.push(c.clone());
    }
    cov
}
