//! Display labels for ports (B18, #59; [`plumcraft_proto::labels`]). Layers, lowest first:
//!
//! 1. the engine's own `label` (and a craft pack's `[labels]`, which the craft adapter puts
//!    there for its own nodes);
//! 2. the application's table ([`LabelSources::app`]), e.g. one for the research
//!    software's names;
//! 3. the user's `labels.toml` ([`LabelSources::user_path`], beside the preferences), which the
//!    maintainer and Claude both edit as text.
//!
//! `labels.reload` reads the user's file again after an edit; `labels.show` lists what is in
//! effect. Neither is journaled: labels are only how ports look.

use std::path::{Path, PathBuf};

use plumcraft_proto::labels::Labels;
use serde_json::{Value, json};

use crate::commands::{CommandSpec, always, opt_str_param};
use crate::{Result, Session};

/// The largest user file read.
const MAX_FILE: u64 = 4 << 20;

/// Where labels come from, above the engines'.
#[derive(Debug, Clone, Default)]
pub struct LabelSources {
    /// The application's own table.
    pub app: Labels,
    /// The user's file, when there is one.
    pub user_path: Option<PathBuf>,
    /// What the user's file held when last read.
    pub user: Labels,
    /// Why the user's file couldn't be read, when it couldn't.
    pub user_error: Option<String>,
}

impl LabelSources {
    /// The application's table with the user's over it.
    pub fn combined(&self) -> Labels {
        let mut t = self.app.clone();
        t.overlay(&self.user);
        t
    }

    /// Read the user's file again. No file, or none there yet, is an empty table; one that
    /// isn't TOML keeps no labels and says why.
    pub fn read_user(&mut self) {
        self.user_error = None;
        self.user = match &self.user_path {
            None => Labels::default(),
            Some(p) => match read_file(p) {
                Ok(Some(t)) => t,
                Ok(None) => Labels::default(),
                Err(e) => {
                    self.user_error = Some(e);
                    Labels::default()
                }
            },
        };
    }
}

fn read_file(p: &Path) -> std::result::Result<Option<Labels>, String> {
    let meta = match std::fs::metadata(p) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.to_string()),
    };
    if !meta.is_file() || meta.len() > MAX_FILE {
        return Err(format!("not a file of at most {MAX_FILE} bytes"));
    }
    let text = std::fs::read_to_string(p).map_err(|e| e.to_string())?;
    Labels::from_toml(&text).map(Some)
}

impl Session {
    /// Use `app` as the application's table, and `user_path` as the user's file (read now).
    pub fn use_labels(&mut self, app: Labels, user_path: Option<PathBuf>) {
        self.labels.app = app;
        self.labels.user_path = user_path;
        self.labels.read_user();
        self.relabel();
    }
}

fn summary(s: &Session) -> Value {
    let l = &s.labels;
    json!({
        "path": l.user_path.as_ref().map(|p| p.display().to_string()),
        "user": l.user.len(),
        "app": l.app.len(),
        "skipped": l.app.skipped.iter().chain(&l.user.skipped).collect::<Vec<_>>(),
        "error": l.user_error,
    })
}

fn reload(s: &mut Session, _: &Value) -> Result<Value> {
    s.labels.read_user();
    s.relabel();
    Ok(summary(s))
}

fn show(s: &mut Session, p: &Value) -> Result<Value> {
    let kind = opt_str_param(p, "kind")?;
    let mut out = summary(s);
    let t = s.labels.combined();
    let labels: serde_json::Map<String, Value> = match kind {
        Some(k) => s
            .catalog()
            .node(k)
            .map(|d| d.inputs.iter().chain(&d.outputs).filter_map(|port| t.label(k, &port.name).map(|l| (port.name.clone(), json!(l)))).collect())
            .unwrap_or_default(),
        None => t.general.iter().map(|(k, v)| (k.clone(), json!(v))).chain(t.by_kind.iter().map(|(k, m)| (k.clone(), json!(m)))).collect(),
    };
    out["labels"] = Value::Object(labels);
    Ok(out)
}

pub(crate) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "labels.reload",
            label: "Reload Port Labels",
            menu: &[],
            shortcut: None,
            params: "{} -> {path, user, app, skipped, error}: read the user's labels.toml again (port name → label, or a node kind's table of them) and relabel every port",
            enabled: always,
            run: reload,
            journal: false,
        },
        CommandSpec {
            id: "labels.show",
            label: "Show Port Labels",
            menu: &[],
            shortcut: None,
            params: "{kind?} -> {path, user, app, skipped, error, labels}: the labels in effect above the engines' own, or those for one node kind's ports",
            enabled: always,
            run: show,
            journal: false,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use plumcraft_proto::catalogue::{NodeDef, Port, SchemaReply};

    fn catalog() -> SchemaReply {
        let port = |n: &str, l: Option<&str>| Port { name: n.into(), label: l.map(str::to_owned), ..Port::default() };
        SchemaReply {
            nodes: vec![NodeDef { id: "vi.pencil".into(), inputs: vec![port("max_array_width", None), port("scale", Some("Scale"))], ..Default::default() }],
            ..Default::default()
        }
    }

    fn label(s: &Session, port: &str) -> Option<String> {
        s.catalog().node("vi.pencil").and_then(|d| d.inputs.iter().find(|p| p.name == port)).and_then(|p| p.label.clone())
    }

    #[test]
    fn the_users_file_wins_and_reloads() {
        let dir = plumcraft_scratch::dir("labels");
        let file = dir.join("labels.toml");
        let mut s = Session::new(catalog());
        let app = Labels::from_value(&json!({"labels": {"max_array_width": "Max. width", "scale": "Size"}}));
        s.use_labels(app, Some(file.clone()));
        assert_eq!(label(&s, "max_array_width").as_deref(), Some("Max. width"), "no file yet: the application's");
        assert_eq!(label(&s, "scale").as_deref(), Some("Size"), "the application's over the engine's");

        std::fs::write(&file, "[labels.\"vi.pencil\"]\nmax_array_width = \"Widest\"\n").unwrap();
        let r = s.execute("labels.reload", &json!({})).unwrap();
        assert_eq!(r["user"], 1);
        assert_eq!(label(&s, "max_array_width").as_deref(), Some("Widest"));
        assert_eq!(s.execute("labels.show", &json!({"kind": "vi.pencil"})).unwrap()["labels"], json!({"max_array_width": "Widest", "scale": "Size"}));

        // A new catalogue keeps the labels; removing the file gives the engine's back.
        s.set_catalog(catalog());
        assert_eq!(label(&s, "max_array_width").as_deref(), Some("Widest"));
        std::fs::remove_file(&file).unwrap();
        s.use_labels(Labels::default(), Some(file.clone()));
        assert_eq!(label(&s, "scale").as_deref(), Some("Scale"));
        assert_eq!(label(&s, "max_array_width"), None);

        // A file that isn't TOML keeps no labels and says why.
        std::fs::write(&file, "not = [toml").unwrap();
        let r = s.execute("labels.reload", &json!({})).unwrap();
        assert!(r["error"].is_string(), "{r}");
        assert_eq!(label(&s, "scale").as_deref(), Some("Scale"));
    }
}
