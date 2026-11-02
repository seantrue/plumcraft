//! The value on a wire (docs/research/16-viewers.md, section 2.4): hovering a wire shows what
//! flows along it, as its viewer's preview. Every display the editor sees (the Watched panel's,
//! pop-outs', peeks') is kept by port, newest last; a wire whose value isn't known yet asks for
//! it after [`PEEK_AFTER`] with `view.peek` (one run, mostly memo hits), which doesn't change the
//! Watched panel. Until it comes the tooltip shows the source node's state and thumbnail. A
//! click on the wire watches it, a double click pops it out (`crate::canvas`).

use std::collections::HashMap;
use std::time::{Duration, Instant};

use plumcraft_proto::display::{Display, Rendition};
use serde_json::json;

use crate::EditorApp;
use crate::viewers::Pane;

/// How long the pointer rests on a wire before its value shows.
pub const SHOW_AFTER: Duration = Duration::from_millis(250);
/// How long before a value not yet known is asked for.
pub const PEEK_AFTER: Duration = Duration::from_millis(500);
/// The most ports' displays kept.
pub const MAX_CACHED: usize = 512;

/// What the wire previews keep.
#[derive(Default)]
pub struct WireView {
    /// The newest display seen for each port (node, port).
    cache: HashMap<(String, String), Display>,
    /// The wire hovered and since when.
    hovered: Option<((String, String), Instant)>,
    /// The port a peek was asked for (once per hover).
    peeked: Option<(String, String)>,
    pane: Pane,
    /// The port the pane shows.
    shown: Option<(String, String)>,
}

impl WireView {
    /// The display kept for a port.
    pub fn cached(&self, node: &str, port: &str) -> Option<&Display> {
        self.cache.get(&(node.to_owned(), port.to_owned()))
    }

    /// Forget everything (a new graph).
    pub fn clear(&mut self) {
        *self = WireView::default();
    }

    /// The port the last preview showed and its viewer's state, for `ui.inspect`.
    pub fn inspect(&self) -> serde_json::Value {
        json!({"port": self.shown.as_ref().map(|(n, p)| format!("{n}.{p}")), "pane": self.pane.inspect()})
    }
}

impl EditorApp {
    /// Keep every display the run state has, by port.
    pub fn cache_displays(&mut self) {
        if self.session.run.nodes.is_empty() && self.session.run.watch.is_empty() {
            self.wires.cache.clear();
        }
        // A peek answered: kept here, so later runs needn't ask for it again.
        if !self.session.run.running && self.session.run.entry(plumcraft_session::run::WatchRole::Peek).is_some_and(|(_, d)| d.is_some()) {
            let peeked =
                self.session.run.entry(plumcraft_session::run::WatchRole::Peek).and_then(|(w, d)| d.map(|d| ((w.node.clone(), w.port.clone()), d.clone())));
            if let Some((k, d)) = peeked {
                self.wires.cache.insert(k, d);
            }
            self.session.run.set_watch(plumcraft_session::run::WatchRole::Peek, None);
        }
        for (w, d) in self.session.run.watch.iter().zip(&self.session.run.watched) {
            if let Some(d) = d.as_ref().filter(|d| d.error.is_none()) {
                if self.wires.cache.len() >= MAX_CACHED && !self.wires.cache.contains_key(&(w.node.clone(), w.port.clone())) {
                    self.wires.cache.clear();
                }
                self.wires.cache.insert((w.node.clone(), w.port.clone()), d.clone());
            }
        }
    }

    /// The hovered wire's value, in a tooltip at the pointer.
    pub fn show_wire_preview(&mut self, ctx: &egui::Context) {
        let Some((from, to, at)) = self.canvas.hover_wire.clone() else {
            self.wires.hovered = None;
            self.wires.peeked = None;
            return;
        };
        let key = (from.node.0.clone(), from.port.clone());
        let since = match &self.wires.hovered {
            Some((k, t)) if *k == key => *t,
            _ => {
                self.wires.hovered = Some((key.clone(), Instant::now()));
                self.wires.peeked = None;
                ctx.request_repaint_after(SHOW_AFTER);
                return;
            }
        };
        let rested = since.elapsed();
        if rested < SHOW_AFTER {
            ctx.request_repaint_after(SHOW_AFTER - rested);
            return;
        }
        let cached = self.wires.cache.get(&key).cloned();
        if cached.is_none() && rested >= PEEK_AFTER && self.wires.peeked.as_ref() != Some(&key) && self.session.connected {
            self.wires.peeked = Some(key.clone());
            self.run("view.peek", &json!({"node": key.0, "port": key.1}));
        } else if cached.is_none() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        let resolve = self.resolver();
        let full = self.full_reader();
        if let Some(d) = &cached {
            if self.wires.shown.as_ref() != Some(&key) {
                self.wires.pane = Pane::default();
                self.wires.shown = Some(key.clone());
            }
            self.wires.pane.sync(ctx, Some(d), Vec::new(), &mut self.thumbs, &resolve, &full, Default::default());
        }
        // The source node's state and thumbnail, while its value isn't known.
        let node_run = self.session.run.node(&from.node.0).cloned();
        let thumb = node_run.as_ref().and_then(|n| n.thumb.as_ref()).and_then(|r| match r {
            Rendition::Rgba8 { data, .. } => self.thumbs.get(ctx, data, &resolve).ok(),
            _ => None,
        });
        let first_output = plumcraft_session::default_output(&self.session, &from.node).is_some_and(|p| p == from.port);
        let ty = self
            .session
            .catalog()
            .node(&self.session.graph().node(&from.node).map(|n| n.kind.clone()).unwrap_or_default())
            .and_then(|d| d.output(&from.port))
            .map(|p| p.ty.clone())
            .unwrap_or_default();
        let tokens = self.tokens;
        let asking = rested >= PEEK_AFTER && self.session.connected;
        let pane = &mut self.wires.pane;
        egui::Area::new(egui::Id::new("plumcraft-wire-preview")).order(egui::Order::Tooltip).fixed_pos(at + egui::vec2(16.0, 16.0)).interactable(false).show(
            ctx,
            |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_max_width(300.0);
                    ui.label(egui::RichText::new(format!("{}.{} -> {}.{}", from.node, from.port, to.node, to.port)).small().strong().color(tokens.text));
                    ui.label(egui::RichText::new(&ty).small().color(tokens.text_faint));
                    match &cached {
                        Some(_) => {
                            if node_run.as_ref().is_some_and(|n| n.is_stale()) {
                                ui.label(egui::RichText::new("stale: from before the last edit").small().color(tokens.stale));
                            }
                            pane.preview_ui(ui, egui::vec2(280.0, 180.0), &tokens);
                        }
                        None => {
                            if let (Some(t), true) = (&thumb, first_output) {
                                crate::viewers::fitted_image(ui, t, egui::vec2(280.0, 180.0));
                            }
                            let state = node_run.as_ref().and_then(|n| n.state).map_or("not run yet".to_owned(), |s| format!("{s:?}").to_lowercase());
                            let waiting = if asking { "asking for the value…" } else { "" };
                            ui.label(egui::RichText::new(format!("{state} {waiting}")).small().color(tokens.text_dim));
                        }
                    }
                    ui.label(egui::RichText::new("click: watch · double-click: pop out").small().color(tokens.text_faint));
                });
            },
        );
    }
}
