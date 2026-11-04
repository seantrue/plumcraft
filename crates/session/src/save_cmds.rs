//! Saving a node's output with its recipe: `run.save` asks the engine to write the file
//! (`output.save`, spec 15), as PNG, JPEG or whatever else its catalogue's `save_formats` list.
//! The Watched panel's and a result node's "Save Result…" and "Contact Sheet…" use it.
//!
//! The session can't reach the engine itself: the request waits in [`SaveState`]'s outbox, the
//! host sends it and hands the reply to [`SaveState::apply_reply`]. A save can take as long as
//! the graph does, so one is in flight at a time and the window stays live meanwhile.
//!
//! Lifted from the research software's `crates/engine/src/save_cmds.rs`. What a save writes, its options and
//! their defaults come from the catalogue: core knows no format.

use std::path::{Path, PathBuf};

use plumcraft_graph::NodeId;
use plumcraft_proto::catalogue::{Media, Port, SaveFormat, SchemaReply, scalars, texts};
use plumcraft_proto::error::NodeError;
use plumcraft_proto::run::{GraphRef, SaveReply, SaveRequest};
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::commands::{CommandSpec, opt_str_param, str_param};
use crate::{Result, Session, SessionError};

/// What a save asked for.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct SaveAsk {
    pub node: String,
    pub port: String,
    /// One item of a list; `None` saves every item as numbered files.
    pub index: Option<i64>,
    /// The file, absolute.
    pub path: String,
    /// Every item of a list on one picture, labelled with what varied.
    pub sheet: bool,
}

/// The save in flight, and the last outcome.
#[derive(Debug, Clone, Default)]
pub struct SaveState {
    /// The save awaiting its reply.
    pub pending: Option<SaveAsk>,
    /// The last save and its reply.
    pub last: Option<(SaveAsk, SaveReply)>,
    outbox: Vec<SaveRequest>,
}

impl SaveState {
    /// Requests for the engine (the host sends them).
    pub fn take_outbox(&mut self) -> Vec<SaveRequest> {
        if crate::commands::hooks().hold_runs.is_some_and(|hold| hold()) {
            return Vec::new();
        }
        std::mem::take(&mut self.outbox)
    }

    pub fn outbox_is_empty(&self) -> bool {
        self.outbox.is_empty()
    }

    /// The engine's reply, or a failure the host made into one.
    pub fn apply_reply(&mut self, reply: SaveReply) {
        let ask = self.pending.take().unwrap_or_default();
        self.last = Some((ask, reply));
    }

    /// The save couldn't be sent, or got no answer.
    pub fn fail(&mut self, why: &str) {
        self.apply_reply(SaveReply::failed(NodeError::engine(why)));
    }

    /// The state as JSON (`ui.inspect`): the pending save and the last outcome.
    pub fn summary(&self) -> Value {
        let last = self.last.as_ref().map(|(ask, r)| {
            json!({"request": ask, "ok": r.ok, "paths": r.paths, "recipe": r.recipe, "items": r.items, "labels": r.labels, "error": r.error.as_ref().map(|e| &e.message)})
        });
        json!({"pending": self.pending, "last": last})
    }
}

/// An extension as a catalogue may spell it, without its dot.
fn bare(ext: &str) -> &str {
    ext.trim_start_matches('.')
}

/// The format that writes files with `path`'s extension.
pub fn format_of<'a>(catalog: &'a SchemaReply, path: &Path) -> Option<&'a SaveFormat> {
    let ext = path.extension()?.to_str()?;
    catalog.save_formats.iter().find(|f| f.extensions.iter().any(|e| bare(e).eq_ignore_ascii_case(ext)))
}

/// Does some format write a value of kind `media`?
pub fn takes(catalog: &SchemaReply, media: Media) -> bool {
    catalog.save_formats.iter().any(|f| f.media.is_empty() || f.media.contains(&media))
}

/// The extension a bare name gets: the engine's first format's.
pub fn default_extension(catalog: &SchemaReply) -> Option<&str> {
    catalog.save_formats.first().and_then(|f| f.extensions.first()).map(|e| bare(e)).filter(|e| !e.is_empty())
}

/// A save path as the engine gets it: `~` and relative paths resolved against `base` (the
/// workflow's folder, or the folder the app started in), and `extension` added to a bare name.
pub fn save_path(path: &str, base: Option<&Path>, extension: Option<&str>) -> PathBuf {
    let expanded = crate::paths::expand_user(path.trim());
    let anchored = if expanded.is_absolute() { expanded } else { base.map_or_else(|| expanded.clone(), |b| b.join(&expanded)) };
    match extension {
        Some(e) if anchored.extension().is_none() => anchored.with_extension(e),
        _ => anchored,
    }
}

/// The file name offered for a save: the node's title without the catalogue's result prefix,
/// made safe for a file name, with the list item's number when there is one.
pub fn suggested_name(catalog: &SchemaReply, title: &str, index: Option<i64>) -> String {
    let prefix = catalog.text(texts::RESULT_PREFIX).unwrap_or_default().trim();
    let stem = title.trim();
    let stem = if prefix.is_empty() { stem } else { [prefix, prefix.trim_end_matches(':')].iter().find_map(|p| stem.strip_prefix(p)).unwrap_or(stem).trim() };
    let safe: String = stem.chars().map(|c| if c == '/' || c == ':' || c == '\\' || c.is_control() { '-' } else { c }).collect();
    let safe = safe.trim_matches(|c: char| c == '.' || c == '-' || c.is_whitespace());
    let name = if safe.is_empty() { "result" } else { safe };
    let numbered = index.map_or_else(|| name.to_owned(), |i| format!("{name}-{i:02}"));
    match default_extension(catalog) {
        Some(e) => format!("{numbered}.{e}"),
        None => numbered,
    }
}

/// `value` against the option's declaration: its choices, its type and its range.
fn check_option(port: &Port, value: &Value) -> std::result::Result<(), String> {
    let name = &port.name;
    if let Some(choices) = port.choices.as_ref().filter(|c| !c.is_empty())
        && !choices.contains(value)
    {
        return Err(format!("{name:?} must be one of {choices:?}, not {value}"));
    }
    if port.ty == scalars::INT || port.ty == scalars::FLOAT {
        let n = value.as_f64().filter(|n| port.ty == scalars::FLOAT || n.fract() == 0.0).ok_or_else(|| format!("{name:?} must be a {} number", port.ty))?;
        if port.min.is_some_and(|m| n < m) || port.max.is_some_and(|m| n > m) {
            return Err(format!("{name:?} must be from {} to {}, not {n}", port.min.unwrap_or(f64::MIN), port.max.unwrap_or(f64::MAX)));
        }
    }
    Ok(())
}

/// `options`, checked against what `format` declares. With no format (an extension the catalogue
/// doesn't list) the engine is the one to refuse.
fn options(p: &Value, format: Option<&SaveFormat>) -> Result<Map<String, Value>> {
    let given = match p.get("options") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(o)) => o.clone(),
        Some(_) => return Err(SessionError::Params("\"options\" must be an object".into())),
    };
    if let Some(f) = format {
        for (k, v) in &given {
            let Some(port) = f.options.iter().find(|o| o.name == *k) else {
                return Err(SessionError::Params(format!("{} has no option {k:?}", f.label)));
            };
            check_option(port, v).map_err(SessionError::Params)?;
        }
    }
    Ok(given)
}

fn can_save(s: &Session) -> std::result::Result<(), String> {
    if !s.connected {
        return Err("the engine is not connected".into());
    }
    if s.catalog().save_formats.is_empty() {
        return Err("this engine doesn't save files".into());
    }
    if s.graph().nodes.is_empty() {
        return Err("the graph is empty".into());
    }
    if s.save.pending.is_some() {
        return Err("a save is in progress".into());
    }
    Ok(())
}

/// `run.save {node, port?, index?, sheet?, path, options?}`.
fn save(s: &mut Session, p: &Value) -> Result<Value> {
    let node = NodeId::from(str_param(p, "node")?);
    let Some(def) = s.graph().node(&node).and_then(|n| s.catalog().node(&n.kind)) else {
        return Err(plumcraft_graph::GraphError::UnknownNode(node).into());
    };
    let port = match opt_str_param(p, "port")? {
        Some(port) => port.to_owned(),
        None => crate::run_cmds::default_output(s, &node).ok_or_else(|| SessionError::Params(format!("{node} has no outputs")))?,
    };
    if def.output(&port).is_none() {
        return Err(SessionError::Params(format!("{node} has no output {port:?}")));
    }
    let index = match p.get("index") {
        None | Some(Value::Null) => None,
        Some(v) => Some(v.as_i64().filter(|i| *i >= 0).ok_or_else(|| SessionError::Params("\"index\" must be a whole number".into()))?),
    };
    let sheet = crate::commands::opt_bool_param(p, "sheet", false)?;
    if sheet && index.is_some() {
        return Err(SessionError::Params("\"sheet\" takes every item: leave \"index\" out".into()));
    }
    let raw = str_param(p, "path")?;
    if raw.trim().is_empty() {
        return Err(SessionError::Params("\"path\" names where to save".into()));
    }
    let base = crate::paths::base_folder(s.path());
    let path = save_path(raw, base.as_deref(), default_extension(s.catalog()));
    let format = format_of(s.catalog(), &path);
    let options = options(p, format)?;
    let format = format.map(|f| f.id.clone()).unwrap_or_default();
    let path = path.to_string_lossy().into_owned();
    // The run's own graph, so what a run computed is found again.
    let graph = crate::convert::runnable(s, base.as_deref());
    let graph = serde_json::to_value(&graph).map_err(|e| SessionError::Params(e.to_string()))?;
    // a Convert's value is its last step's
    let (node, port) = {
        let (n, p) = s.run.convert.out(&node.0, &port);
        (NodeId(n), p)
    };
    if s.run.review_due {
        s.run.ask_review();
        return Err(SessionError::Params("this workflow came from elsewhere: review it before saving a result (graph.review, then graph.trust)".into()));
    }
    let ask = SaveAsk { node: node.0.clone(), port: port.clone(), index, path: path.clone(), sheet };
    s.save.outbox.push(SaveRequest {
        graph: GraphRef::Inline { graph },
        node: node.0,
        port,
        index,
        path: Some(path),
        format,
        options,
        sheet,
        ..Default::default()
    });
    s.save.pending = Some(ask.clone());
    Ok(json!({"pending": ask}))
}

pub(crate) fn specs() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: "run.save",
        label: "Save Result…",
        menu: &[],
        shortcut: None,
        params: "{node, port? (default: its first output), index? (one item of a list; default: every item, numbered), sheet? (true: every item of a list on one picture, each labelled with what varied), path (relative to the workflow's folder; the first format's extension added to a bare name), options? (the format's, from the catalogue's save_formats)} -> {pending} (the engine writes the file, its recipe included, by the extension; the reply arrives later: ui.inspect's save)",
        enabled: can_save,
        run: save,
        journal: false,
    }]
}

#[cfg(test)]
mod tests {
    use plumcraft_proto::catalogue::{SaveFormat, SchemaReply};

    use super::*;

    fn formats() -> Vec<SaveFormat> {
        serde_json::from_value(json!([
            {"id": "pic", "label": "Picture", "extensions": ["pic"], "media": ["ndarray"],
             "options": [{"name": "dither", "type": "ENUM", "choices": ["none", "some"]}]},
            {"id": "rough", "label": "Rough", "extensions": [".rgh", "rough"],
             "options": [{"name": "quality", "type": "INT", "min": 1, "max": 100}]}]))
        .unwrap()
    }

    fn catalog() -> SchemaReply {
        let mut c: SchemaReply = serde_json::from_value(json!({"nodes": [
            {"id": "t.out", "display": "Out", "outputs": [{"name": "picture", "type": "ARRAY"}, {"name": "note", "type": "TEXT"}]},
            {"id": "t.sink", "display": "Sink"}]}))
        .unwrap();
        c.save_formats = formats();
        c.texts.insert(texts::RESULT_PREFIX.into(), "Out:".into());
        c
    }

    fn session() -> (Session, String) {
        let mut s = Session::new(catalog());
        s.connected = true;
        let id = s.execute("node.add", &json!({"kind": "t.out"})).unwrap()["id"].as_str().unwrap().to_owned();
        (s, id)
    }

    #[test]
    fn a_save_goes_out_and_its_reply_comes_back() {
        let (mut s, id) = session();
        for bad in [
            json!({"node": "nope", "path": "a.pic"}),
            json!({"node": id, "port": "nope", "path": "a.pic"}),
            json!({"node": id, "path": ""}),
            json!({"node": id, "path": "  "}),
            json!({"path": "a.pic"}),
            json!({"node": id, "path": "a.pic", "index": -1}),
            json!({"node": id, "path": "a.pic", "index": "two"}),
            json!({"node": id, "path": "a.pic", "sheet": "yes"}),
            json!({"node": id, "path": "a.pic", "sheet": true, "index": 1}),
            json!({"node": id, "path": "a.pic", "options": 3}),
            json!({"node": id, "path": "a.pic", "options": {"dither": "lots"}}),
            json!({"node": id, "path": "a.pic", "options": {"quality": 50}}),
            json!({"node": id, "path": "a.rgh", "options": {"quality": 0}}),
            json!({"node": id, "path": "a.rgh", "options": {"quality": 50.5}}),
            json!({"node": id, "path": "a.rgh", "options": {"quality": "best"}}),
        ] {
            assert!(s.execute("run.save", &bad).is_err(), "{bad}");
        }
        assert!(s.save.take_outbox().is_empty() && s.save.pending.is_none(), "nothing was sent");
        let r = s.execute("run.save", &json!({"node": id, "path": "prints/one", "index": 2, "options": {"dither": "some"}})).unwrap();
        assert_eq!(r["pending"]["port"], "picture", "the default port is the first output");
        assert!(s.execute("run.save", &json!({"node": id, "path": "b.pic"})).is_err(), "one at a time");
        let out = s.save.take_outbox();
        assert_eq!(out.len(), 1);
        let body = &out[0];
        assert_eq!((body.node.as_str(), body.port.as_str(), body.index, body.format.as_str(), body.sheet), (id.as_str(), "picture", Some(2), "pic", false));
        assert_eq!(body.options["dither"], "some");
        let path = PathBuf::from(body.path.clone().unwrap());
        assert!(path.is_absolute() && path.ends_with("prints/one.pic"), "{}", path.display());
        let GraphRef::Inline { graph } = &body.graph else { panic!("inline") };
        assert!(graph["nodes"][&id].is_object());
        assert!(s.journal().iter().all(|e| e.id != "run.save"), "saves aren't journaled");
        s.save.apply_reply(SaveReply { ok: true, paths: vec![path.display().to_string()], recipe: true, ..Default::default() });
        let (ask, reply) = s.save.last.clone().unwrap();
        assert!(reply.ok && reply.recipe && ask.index == Some(2) && s.save.pending.is_none());
        assert_eq!(s.save.summary()["last"]["ok"], true);
        // the next save goes, and a failure is data
        s.execute("run.save", &json!({"node": id, "path": "/abs/rough.rough", "options": {"quality": 80}})).unwrap();
        assert_eq!(s.save.take_outbox()[0].format, "rough", "any of a format's extensions, its dot or not");
        s.save.fail("the engine went away");
        assert_eq!(s.save.summary()["last"]["error"], "the engine went away");
        assert!(s.save.pending.is_none());
    }

    #[test]
    fn an_extension_the_catalogue_doesnt_list_is_the_engines_to_refuse() {
        let (mut s, id) = session();
        s.execute("run.save", &json!({"node": id, "path": "a.gif", "options": {"anything": 1}})).unwrap();
        let out = s.save.take_outbox();
        assert_eq!(out[0].format, "");
        assert_eq!(out[0].options["anything"], 1);
    }

    #[test]
    fn a_sheet_is_asked_for_with_the_whole_list() {
        let (mut s, id) = session();
        let r = s.execute("run.save", &json!({"node": id, "path": "grid", "sheet": true})).unwrap();
        assert_eq!(r["pending"]["sheet"], true);
        let body = s.save.take_outbox().pop().unwrap();
        assert!(body.sheet && body.index.is_none());
    }

    #[test]
    fn it_needs_an_engine_that_saves_and_something_to_save() {
        let (mut s, id) = session();
        s.connected = false;
        assert!(s.execute("run.save", &json!({"node": id, "path": "a.pic"})).unwrap_err().to_string().contains("not connected"));
        let mut none = Session::new(SchemaReply { save_formats: Vec::new(), ..catalog() });
        none.connected = true;
        let id = none.execute("node.add", &json!({"kind": "t.out"})).unwrap()["id"].as_str().unwrap().to_owned();
        assert!(none.execute("run.save", &json!({"node": id, "path": "a.pic"})).unwrap_err().to_string().contains("doesn't save"));
        let mut empty = Session::new(catalog());
        empty.connected = true;
        assert!(empty.execute("run.save", &json!({"node": "n", "path": "a.pic"})).unwrap_err().to_string().contains("empty"));
    }

    #[test]
    fn only_what_a_format_takes_is_offered() {
        let c = catalog();
        assert!(takes(&c, Media::Ndarray) && takes(&c, Media::Table), "Rough takes anything");
        let c = SchemaReply { save_formats: formats().into_iter().take(1).collect(), ..catalog() };
        assert!(takes(&c, Media::Ndarray) && !takes(&c, Media::Table));
        assert!(!takes(&SchemaReply::default(), Media::Ndarray));
    }

    #[test]
    fn save_paths_resolve_against_the_base_folder() {
        let base = Path::new("/work/flows");
        assert_eq!(save_path("prints/a.png", Some(base), Some("png")), PathBuf::from("/work/flows/prints/a.png"));
        assert_eq!(save_path("a", Some(base), Some("pic")), PathBuf::from("/work/flows/a.pic"));
        assert_eq!(save_path("/elsewhere/b.jpg", Some(base), Some("png")), PathBuf::from("/elsewhere/b.jpg"));
        assert_eq!(save_path(" c.tif ", None, None), PathBuf::from("c.tif"));
        if let Some(home) = crate::paths::home_dir() {
            assert_eq!(save_path("~/d.png", Some(base), None), home.join("d.png"));
        }
    }

    #[test]
    fn suggested_names_come_from_the_title() {
        let c = catalog();
        assert_eq!(suggested_name(&c, "Out: pencil", None), "pencil.pic");
        assert_eq!(suggested_name(&c, "Out", None), "result.pic");
        assert_eq!(suggested_name(&c, "  a/b:c  ", Some(3)), "a-b-c-03.pic");
        assert_eq!(suggested_name(&c, "...", None), "result.pic");
        assert_eq!(suggested_name(&c, "Pencil", Some(0)), "Pencil-00.pic");
        assert_eq!(suggested_name(&SchemaReply::default(), "Out: pencil", None), "Out- pencil", "no prefix, no format: no assumptions");
    }
}
