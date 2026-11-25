//! Editors for a node's own value: what an application draws for a value this editor has no
//! widget for. A node names one in its catalogue entry (`NodeDef.editor`, `{kind, port}`), a run
//! may send it data (`NodeReport.editor`, by kind), and the application registers what draws that
//! kind, in the style of [`crate::viewers::register`]. Core knows the kind only as a string.
//!
//! - [`EditorKind`]: a kind, the words for its button and window, what to say of a value, and how
//!   to make a fresh [`ValueEditor`].
//! - [`for_port`]: the parameter editor gives a port its button. A port of a type core doesn't
//!   know gets only that; one it knows keeps its widget beside the button.
//! - [`offered`]: the node's footer and menu offer it too, unless the port is on a wire.
//! - [`ValueEditors`]: the windows open, one a node, following `views.editors` (`node.editor`).
//!
//! An editor changes nothing itself: [`ValueEditor::show`] returns commands by id, its own or
//! `node.set`, so an edit is a journal entry and an undo step, and every run of it is as open to
//! the control channel as to the pointer. One gesture must make one command.

use std::sync::{Mutex, OnceLock};

use egui::RichText;
use plumcraft_graph::{Graph, NodeId, PortRef, ports};
use plumcraft_proto::catalogue::{NodeDef, SchemaReply};
use plumcraft_proto::data::DataRef;
use plumcraft_session::Session;
use plumcraft_store::Blob;
use serde_json::{Value, json};

use crate::canvas::Action;
use crate::theme::Tokens;

/// What an editor reads, each frame.
pub struct Context<'a> {
    pub node: &'a NodeId,
    /// The node's title.
    pub title: &'a str,
    /// The port it edits (`editor.port`).
    pub port: &'a str,
    /// That port's value: its widget's, else its default.
    pub value: &'a Value,
    /// What the last run sent this kind (`NodeReport.editor`). Hostile input, like any message.
    pub data: Option<&'a Value>,
    pub graph: &'a Graph,
    pub catalog: &'a SchemaReply,
    /// The workflow's file, when it has one.
    pub path: Option<&'a std::path::Path>,
    /// A blob's bytes, through the engine link.
    pub resolve: &'a dyn Fn(&DataRef) -> Option<Blob>,
    pub tokens: &'a Tokens,
}

/// One open editor. It lives as long as its window, and must follow [`Context::value`]: an undo
/// changes the value under it.
pub trait ValueEditor {
    /// Draw inside the window. Returns the commands its controls ask for.
    fn show(&mut self, ui: &mut egui::Ui, cx: &Context<'_>) -> Vec<Action>;
    /// Its state, for `ui.inspect`.
    fn inspect(&self) -> Value {
        Value::Null
    }
    /// Let go of its textures: a texture means something only to the context that made it, and
    /// the offscreen renderer ([`crate::offscreen`]) draws in a second one, around which this is
    /// called. An editor that keeps none needn't care; one that does remakes them when next drawn.
    fn forget_textures(&mut self) {}
}

/// A kind of editor.
pub struct EditorKind {
    /// `editor.kind`.
    pub kind: &'static str,
    /// The window's title, before the node's.
    pub title: &'static str,
    /// What the button says (`Edit Thing…`).
    pub label: &'static str,
    /// The button's hover text.
    pub help: &'static str,
    /// A short word on a value, for where there is no widget for it.
    pub summary: fn(&Value) -> String,
    /// A fresh one.
    pub make: fn() -> Box<dyn ValueEditor>,
}

static KINDS: OnceLock<Vec<EditorKind>> = OnceLock::new();
/// What an application adds, until the registry is built.
static ADDED: Mutex<Vec<EditorKind>> = Mutex::new(Vec::new());

/// Add an application's editors. Before anything reads the registry; a kind already taken is left
/// out, as a command's id is.
pub fn register(kinds: Vec<EditorKind>) -> Result<(), String> {
    if KINDS.get().is_some() {
        return Err("value editors are registered at startup".into());
    }
    ADDED.lock().map_err(|_| "the editor registry is poisoned".to_owned())?.extend(kinds);
    Ok(())
}

/// Every kind as the core's menu model takes them: `(editor.kind, label, help)`.
pub fn facts() -> Vec<(String, String, String)> {
    scene_editors().into_iter().map(|e| (e.kind, e.label, e.help)).collect()
}

/// Every kind as the core's scene takes them (N2 slice 2): its words and its summary.
///
/// Then the core's own editors (B173: `mask`, `curves`; `scene::core_editors`) for a kind the
/// application registered none of, as every shell lists them: the row's words and the node menu's
/// item, the editor itself the core's value window here, where this editor draws none of its own.
pub fn scene_editors() -> Vec<plumcraft_editor_core::scene::Editor> {
    let mut out: Vec<plumcraft_editor_core::scene::Editor> = registry()
        .iter()
        .map(|k| plumcraft_editor_core::scene::Editor { kind: k.kind.to_owned(), label: k.label.to_owned(), help: k.help.to_owned(), summary: Some(k.summary) })
        .collect();
    for e in plumcraft_editor_core::scene::core_editors() {
        if !out.iter().any(|x| x.kind == e.kind) {
            out.push(e);
        }
    }
    out
}

/// Every kind this application has. PlumCraft itself has none.
pub fn registry() -> &'static [EditorKind] {
    KINDS.get_or_init(|| {
        let added = ADDED.lock().map(|mut a| std::mem::take(&mut *a)).unwrap_or_default();
        let mut kinds: Vec<EditorKind> = Vec::with_capacity(added.len());
        for k in added {
            if kinds.iter().any(|x| x.kind == k.kind) {
                plumcraft_editor_core::log::say(plumcraft_editor_core::log::Level::Warn, "editor", &format!("value editor {}: the kind is taken", k.kind));
            } else {
                kinds.push(k);
            }
        }
        kinds
    })
}

/// The kind `kind`, when this application has it.
pub fn find(kind: &str) -> Option<&'static EditorKind> {
    registry().iter().find(|k| k.kind == kind)
}

/// The editor a node offers: the one it names, when this application draws that kind.
pub fn offered(def: &NodeDef) -> Option<&'static EditorKind> {
    find(&def.editor.as_ref()?.kind)
}

/// The editor for input `port` of a node, when the node names one for that port.
pub fn for_port(def: &NodeDef, port: &str) -> Option<&'static EditorKind> {
    offered(def).filter(|_| def.editor.as_ref().is_some_and(|e| e.port == port))
}

struct Open {
    node: NodeId,
    kind: &'static EditorKind,
    editor: Box<dyn ValueEditor>,
}

/// The editors open.
#[derive(Default)]
pub struct ValueEditors {
    open: Vec<Open>,
    /// Editors of the core's own kinds (B173: `mask` and `curves`, which the native shells draw)
    /// this application draws none of: the core's value window, the value as JSON to edit
    /// and apply, by node, with the text being edited.
    fallback: Vec<(NodeId, String)>,
}

impl ValueEditors {
    pub fn is_empty(&self) -> bool {
        self.open.is_empty() && self.fallback.is_empty()
    }

    /// Follow `views.editors`, and draw each window. Returns the commands to run, and anything
    /// to say of an editor that couldn't open.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        session: &Session,
        resolve: &dyn Fn(&DataRef) -> Option<Blob>,
        tokens: &Tokens,
    ) -> (Vec<Action>, Option<String>) {
        let mut actions = Vec::new();
        let mut note = None;
        let wanted = &session.views.editors;
        self.open.retain(|o| wanted.contains(&o.node));
        self.fallback.retain(|(n, _)| wanted.contains(n));
        for (i, node) in wanted.iter().enumerate() {
            let def = session.graph().node(node).and_then(|n| session.catalog().node(&n.kind));
            let Some(kind) = def.and_then(offered) else {
                // one of the core's own editors (B173: mask, curves) this application draws none
                // of: the core's value window, as the native shells draw a kind they have no
                // editor of their own for. Another kind nobody registered stays closed, said once.
                let core_kind =
                    def.and_then(|d| d.editor.as_ref()).is_some_and(|e| plumcraft_editor_core::scene::core_editors().iter().any(|c| c.kind == e.kind));
                if core_kind && let Ok(model) = plumcraft_editor_core::reports::value_editor(session, &node.0) {
                    if !fallback_window(ctx, &mut self.fallback, node, &model, i, &mut actions) {
                        actions.push(("node.editor", json!({"node": node, "open": false})));
                    }
                    continue;
                }
                // the node went, or an engine with other nodes came
                if let Some(named) = def.and_then(|d| d.editor.as_ref()) {
                    note = Some(format!("this application has no {} editor", named.kind));
                }
                actions.push(("node.editor", json!({"node": node, "open": false})));
                continue;
            };
            if !self.open.iter().any(|o| o.node == *node && o.kind.kind == kind.kind) {
                self.open.retain(|o| o.node != *node);
                self.open.push(Open { node: node.clone(), kind, editor: (kind.make)() });
            }
            let Some(open) = self.open.iter_mut().find(|o| o.node == *node) else { continue };
            if !window(ctx, open, i, session, resolve, tokens, &mut actions) {
                actions.push(("node.editor", json!({"node": node, "open": false})));
            }
        }
        (actions, note)
    }

    /// Every open editor lets go of its textures ([`ValueEditor::forget_textures`]).
    pub fn forget_textures(&mut self) {
        self.open.iter_mut().for_each(|o| o.editor.forget_textures());
    }

    /// For `ui.inspect`.
    pub fn inspect(&self) -> Value {
        self.open
            .iter()
            .map(|o| json!({"node": o.node, "kind": o.kind.kind, "state": o.editor.inspect()}))
            .chain(self.fallback.iter().map(|(n, _)| json!({"node": n, "kind": "value", "state": null})))
            .collect()
    }
}

/// The core's value window for `node` (`reports::value_editor`'s model): its words, the value as
/// JSON in a field, Apply (`node.set`, the text read as JSON) and Close. Returns whether it stays
/// open.
fn fallback_window(ctx: &egui::Context, texts: &mut Vec<(NodeId, String)>, node: &NodeId, model: &Value, i: usize, actions: &mut Vec<Action>) -> bool {
    let edit = &model["sections"][0]["rows"][0]["edits"][0];
    let current = edit["value"].as_str().unwrap_or_default().to_owned();
    let k = match texts.iter().position(|(n, _)| n == node) {
        Some(k) => k,
        None => {
            texts.push((node.clone(), current.clone()));
            texts.len() - 1
        }
    };
    let mut open = true;
    let title = model["title"].as_str().unwrap_or("Value").to_owned();
    egui::Window::new(&title)
        .id(egui::Id::new(("plumcraft-value-window", &node.0)))
        .open(&mut open)
        .default_pos(egui::pos2(80.0 + 24.0 * i as f32, 120.0))
        .show(ctx, |ui| {
            ui.label(RichText::new(model["intro"].as_str().unwrap_or_default()).small());
            if model["wired"] == true {
                ui.label("A wire feeds this value: Apply sets the value the wire overrides.");
            }
            let Some((_, text)) = texts.get_mut(k) else { return };
            ui.add(egui::TextEdit::multiline(text).code_editor().desired_rows(8).desired_width(f32::INFINITY));
            ui.horizontal(|ui| {
                let parsed = serde_json::from_str::<Value>(text);
                if ui.add_enabled(parsed.is_ok() && *text != current, egui::Button::new("Apply")).clicked()
                    && let Ok(v) = parsed
                {
                    let mut p = edit["params"].clone();
                    p["value"] = v;
                    actions.push(("node.set", p));
                }
                if ui.button("Revert").clicked() {
                    *text = current.clone();
                }
            });
        });
    open
}

/// One editor's window. Returns whether it stays open.
fn window(
    ctx: &egui::Context,
    open: &mut Open,
    index: usize,
    session: &Session,
    resolve: &dyn Fn(&DataRef) -> Option<Blob>,
    tokens: &Tokens,
    actions: &mut Vec<Action>,
) -> bool {
    let (graph, catalog) = (session.graph(), session.catalog());
    let node = open.node.clone();
    let Some(n) = graph.node(&node) else { return false };
    let Some(def) = catalog.node(&n.kind) else { return false };
    let port = def.editor.as_ref().map_or("", |e| e.port.as_str());
    let value = def.input(port).map_or(Value::Null, |p| ports::current_value(def, n, port, p));
    let title = plumcraft_session::node_title(graph, catalog, &node);
    let data = session.run.node(&node.0).and_then(|r| r.editor.get(open.kind.kind));
    let wired = graph.wire_into(&PortRef::new(node.clone(), port)).is_some();
    let mut still = true;
    let step = 28.0 * index as f32;
    egui::Window::new(RichText::new(format!("{} — {title}", open.kind.title)).strong())
        .id(egui::Id::new(("plumcraft-value-editor", &node)))
        .open(&mut still)
        .resizable(false)
        .collapsible(true)
        .default_pos(ctx.content_rect().left_top() + egui::vec2(120.0 + step, 90.0 + step))
        .frame(egui::Frame::window(&ctx.global_style()).fill(tokens.panel))
        .show(ctx, |ui| {
            if wired {
                ui.label(RichText::new(format!("{port} comes from a wire: there is no value here to edit.")).color(tokens.text_dim));
                return;
            }
            let cx = Context { node: &node, title: &title, port, value: &value, data, graph, catalog, path: session.path(), resolve, tokens };
            actions.extend(open.editor.show(ui, &cx));
        });
    still
}
