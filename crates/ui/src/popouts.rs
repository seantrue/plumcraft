//! Pop-out windows (docs/research/16-viewers.md): a viewer in a window of its own that follows a
//! port's value. The session holds what is open (`view.popout`, `view.close`; each a watch entry
//! with role `Popout(id)`); this module keeps a [`Pane`] per window, syncs it every frame from
//! the newest display (so every run, and every `preview` record while one runs, updates it),
//! and draws it in its own viewport.
//!
//! **Never modal.** Each pop-out is an egui viewport drawn with the editor's frame: a native
//! window where the backend has several, an in-editor window where it has one (kittests). The
//! editor keeps taking input while any are open. Closing the window runs `view.close`.

use plumcraft_session::run::WatchRole;
use serde_json::json;

use crate::EditorApp;
use crate::viewers::Pane;

/// One open pop-out.
pub struct PopoutWindow {
    pub id: u64,
    pub pane: Pane,
    /// Its title: `node.port`.
    pub title: String,
}

impl EditorApp {
    /// Match the windows to the session's pop-outs, and give each its port's newest display.
    pub fn sync_popouts(&mut self, ctx: &egui::Context) {
        let open: Vec<(u64, Option<String>, _)> = self
            .session
            .views
            .popouts
            .iter_mut()
            .map(|o| (o.id, o.viewer.clone(), plumcraft_session::viewer_cmds::Tools { zoom: o.tools.zoom.take(), ..o.tools }))
            .collect();
        self.popouts.retain(|w| open.iter().any(|(id, ..)| *id == w.id));
        let resolve = self.resolver();
        let full = self.full_reader();
        for (id, viewer, tools) in open {
            let role = WatchRole::Popout(id);
            let (title, display) = match self.session.run.entry(role) {
                Some((w, d)) => (format!("{}.{}", w.node, w.port), d.cloned()),
                None => continue,
            };
            let offered = plumcraft_session::run_cmds::offered_views_for(&self.session, role);
            let w = match self.popouts.iter_mut().position(|w| w.id == id) {
                Some(i) => &mut self.popouts[i],
                None => {
                    self.popouts.push(PopoutWindow { id, pane: Pane::default(), title: title.clone() });
                    let last = self.popouts.len() - 1;
                    &mut self.popouts[last]
                }
            };
            w.title = title;
            w.pane.chosen = viewer;
            if w.pane.sync(ctx, display.as_ref(), offered, &mut self.thumbs, &resolve, &full, tools) {
                self.rerun_pending = true;
            }
        }
    }

    /// Draw every pop-out in its own viewport; run what their controls ask for.
    pub fn show_popouts(&mut self, ctx: &egui::Context) {
        let mut asks = Vec::new();
        let mut closed = Vec::new();
        let tokens = self.tokens.clone();
        for w in &mut self.popouts {
            let (id, title) = (w.id, w.title.clone());
            let vid = egui::ViewportId::from_hash_of(("plumcraft-popout", id));
            let builder =
                egui::ViewportBuilder::default().with_title(format!("{title} — PlumCraft")).with_inner_size([760.0, 560.0]).with_min_inner_size([320.0, 220.0]);
            let pane = &mut w.pane;
            ctx.show_viewport_immediate(vid, builder, |ui, class| {
                let mut draw = |ui: &mut egui::Ui| {
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(&title).monospace().color(tokens.text));
                        if ui.small_button("Close").on_hover_text(format!("view.close {{id: {id}}}")).clicked() {
                            closed.push(id);
                        }
                    });
                    asks.extend(pane.full_ui(ui, WatchRole::Popout(id), &tokens));
                };
                if class == egui::ViewportClass::EmbeddedWindow {
                    // One window only (a kittest, an offscreen picture): egui has already put it
                    // in an in-editor window, which isn't modal either; kept above the canvas's
                    // nodes, which are areas of their own.
                    ui.ctx().move_to_top(ui.layer_id());
                    draw(ui);
                } else {
                    let frame = egui::Frame::new().fill(tokens.panel).inner_margin(egui::Margin::same(8));
                    egui::CentralPanel::default().frame(frame).show(ui, |ui| draw(ui));
                    if ui.ctx().input(|i| i.viewport().close_requested()) {
                        closed.push(id);
                    }
                }
            });
        }
        for id in closed {
            self.run("view.close", &json!({"id": id}));
        }
        self.run_all(asks);
    }

    /// The pop-outs' state, for `ui.inspect`.
    pub fn popouts_inspect(&self) -> serde_json::Value {
        serde_json::Value::Array(self.popouts.iter().map(|w| json!({"id": w.id, "title": w.title, "pane": w.pane.inspect()})).collect())
    }
}
