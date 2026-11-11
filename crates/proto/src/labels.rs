//! Display labels for ports (B18, #59): a table that turns a raw port name (`max_array_width`)
//! into the label people see (`Max. width`). Core holds only the lookup; the names come from
//! data:
//!
//! - a craft pack's `[labels]` (its own nodes only);
//! - an application's table (the research software's application, for its own names);
//! - the user's `labels.toml`, beside the application's preferences, which wins.
//!
//! **The table**, in TOML or JSON: under `labels`, a string value labels that port name on every
//! node; a table value is a node kind, and labels that kind's ports only, before the general
//! entries:
//!
//! ```toml
//! [labels]
//! max_array_width = "Max. width"
//!
//! [labels."pdf.find"]
//! stop_at_limit = "Stop at the limit"
//! ```
//!
//! A label replaces the engine's own `label` for display; nothing that runs reads it. A file is
//! hostile input: an entry that doesn't read (not a string, empty, too long, a control
//! character) is skipped and named in [`Labels::skipped`], never fatal.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::catalogue::{Port, SchemaReply};

/// The longest label or key taken, in characters.
pub const MAX_LABEL: usize = 80;
/// The most entries one table keeps.
pub const MAX_ENTRIES: usize = 10_000;

/// One table of labels.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Labels {
    /// Port name → label, on every node.
    pub general: BTreeMap<String, String>,
    /// Node kind → port name → label.
    pub by_kind: BTreeMap<String, BTreeMap<String, String>>,
    /// What was skipped while reading, each as `key: why`.
    pub skipped: Vec<String>,
}

fn clean(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty() && t.chars().count() <= MAX_LABEL && !t.chars().any(char::is_control)).then(|| t.to_owned())
}

impl Labels {
    /// The table under `labels` in a JSON value (a parsed TOML or JSON file, or a pack's
    /// manifest). A value with no `labels` gives an empty table.
    pub fn from_value(v: &Value) -> Self {
        let mut out = Labels::default();
        match v.get("labels") {
            None | Some(Value::Null) => {}
            Some(Value::Object(m)) => out.read(m),
            Some(_) => out.skipped.push("labels: not a table".into()),
        }
        out
    }

    /// The table in a TOML file's text. A file that isn't TOML is an error.
    pub fn from_toml(text: &str) -> Result<Self, String> {
        let v: toml::Value = toml::from_str(text).map_err(|e| e.to_string())?;
        let v = serde_json::to_value(v).map_err(|e| e.to_string())?;
        Ok(Self::from_value(&v))
    }

    /// The table in a JSON file's text.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let v: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        Ok(Self::from_value(&v))
    }

    fn read(&mut self, m: &serde_json::Map<String, Value>) {
        let mut entries = 0usize;
        for (key, value) in m {
            let Some(k) = clean(key) else {
                self.skipped.push(format!("{}: not a port name or node kind", key.chars().take(MAX_LABEL).collect::<String>()));
                continue;
            };
            match value {
                Value::String(l) => match clean(l) {
                    Some(l) if entries < MAX_ENTRIES => {
                        entries += 1;
                        self.general.insert(k, l);
                    }
                    Some(_) => self.skipped.push(format!("{k}: more than {MAX_ENTRIES} entries")),
                    None => self.skipped.push(format!("{k}: a label is 1 to {MAX_LABEL} characters, with no control characters")),
                },
                Value::Object(ports) => {
                    for (port, l) in ports {
                        let (Some(p), Some(l)) = (clean(port), l.as_str().and_then(clean)) else {
                            self.skipped.push(format!(
                                "{k}.{}: a label is 1 to {MAX_LABEL} characters, with no control characters",
                                port.chars().take(MAX_LABEL).collect::<String>()
                            ));
                            continue;
                        };
                        if entries >= MAX_ENTRIES {
                            self.skipped.push(format!("{k}.{p}: more than {MAX_ENTRIES} entries"));
                            continue;
                        }
                        entries += 1;
                        self.by_kind.entry(k.clone()).or_default().insert(p, l);
                    }
                }
                _ => self.skipped.push(format!("{k}: a label (a string) or a node kind's table")),
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.general.is_empty() && self.by_kind.is_empty()
    }

    /// How many labels the table holds.
    pub fn len(&self) -> usize {
        self.general.len() + self.by_kind.values().map(BTreeMap::len).sum::<usize>()
    }

    /// `higher`'s entries over this table's.
    pub fn overlay(&mut self, higher: &Labels) {
        self.general.extend(higher.general.iter().map(|(k, v)| (k.clone(), v.clone())));
        for (kind, ports) in &higher.by_kind {
            self.by_kind.entry(kind.clone()).or_default().extend(ports.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
    }

    /// The label for port `port` of a node of kind `kind`: the kind's own entry, else the
    /// general one.
    pub fn label(&self, kind: &str, port: &str) -> Option<&str> {
        self.by_kind.get(kind).and_then(|m| m.get(port)).or_else(|| self.general.get(port)).map(String::as_str)
    }

    /// Give every port of node kind `kind` its label from the table, sub-ports included.
    pub fn apply_ports(&self, kind: &str, ports: &mut [Port]) {
        for p in ports {
            if let Some(l) = self.label(kind, &p.name) {
                p.label = Some(l.to_owned());
            }
            if let Some(options) = p.options.as_mut() {
                for subs in options.values_mut() {
                    self.apply_ports(kind, subs);
                }
            }
        }
    }

    /// Give every port in a catalogue its label from the table.
    pub fn apply(&self, catalog: &mut SchemaReply) {
        if self.is_empty() {
            return;
        }
        for n in &mut catalog.nodes {
            let kind = n.id.clone();
            self.apply_ports(&kind, &mut n.inputs);
            self.apply_ports(&kind, &mut n.outputs);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn port(name: &str, label: Option<&str>) -> Port {
        Port { name: name.into(), label: label.map(str::to_owned), ..Port::default() }
    }

    #[test]
    fn a_kinds_entry_wins_over_a_general_one() {
        let t = Labels::from_toml(
            r#"
            [labels]
            max_array_width = "Max. width"
            scale = "Scale"

            [labels."pdf.find"]
            scale = "Zoom"
            "#,
        )
        .unwrap();
        assert!(t.skipped.is_empty(), "{:?}", t.skipped);
        assert_eq!(t.label("vi.any", "max_array_width"), Some("Max. width"));
        assert_eq!(t.label("pdf.find", "scale"), Some("Zoom"));
        assert_eq!(t.label("vi.any", "scale"), Some("Scale"));
        assert_eq!(t.label("vi.any", "other"), None);
        assert_eq!(t.len(), 3);
    }

    #[test]
    fn a_higher_table_overrides_and_a_catalogue_takes_the_labels() {
        let mut low = Labels::from_value(&json!({"labels": {"a": "From the pack", "b": "B"}}));
        low.overlay(&Labels::from_value(&json!({"labels": {"a": "Mine"}})));
        let mut cat = SchemaReply::default();
        let mut sub = port("inner", None);
        sub.label = None;
        let mut with_options = port("mode", Some("Mode"));
        with_options.options = Some(BTreeMap::from([("x".to_owned(), vec![sub])]));
        cat.nodes.push(crate::catalogue::NodeDef {
            id: "k".into(),
            inputs: vec![port("a", Some("engine's")), port("c", Some("kept")), with_options],
            outputs: vec![port("b", None)],
            ..Default::default()
        });
        let mut t = low.clone();
        t.overlay(&Labels::from_value(&json!({"labels": {"k": {"inner": "Inner"}}})));
        t.apply(&mut cat);
        let n = &cat.nodes[0];
        assert_eq!(n.inputs[0].label.as_deref(), Some("Mine"));
        assert_eq!(n.inputs[1].label.as_deref(), Some("kept"), "a port with no entry keeps the engine's label");
        assert_eq!(n.inputs[2].options.as_ref().unwrap()["x"][0].label.as_deref(), Some("Inner"));
        assert_eq!(n.outputs[0].label.as_deref(), Some("B"));
    }

    #[test]
    fn hostile_entries_are_skipped() {
        let long = "x".repeat(MAX_LABEL + 1);
        let t = Labels::from_value(&json!({"labels": {
            "ok": "Fine",
            "num": 3,
            "empty": "  ",
            "ctl": "a\u{0007}b",
            "long": long,
            "": "no key",
            "kind": {"p": 1, "q": "Q"},
        }}));
        assert_eq!(t.label("x", "ok"), Some("Fine"));
        assert_eq!(t.label("kind", "q"), Some("Q"));
        assert_eq!(t.len(), 2);
        assert_eq!(t.skipped.len(), 6, "{:?}", t.skipped);
        assert!(Labels::from_value(&json!({"labels": [1]})).skipped.len() == 1);
        assert!(Labels::from_value(&json!({})).is_empty());
        assert!(Labels::from_toml("not = [toml").is_err());
        assert!(Labels::from_json("{").is_err());
    }
}
