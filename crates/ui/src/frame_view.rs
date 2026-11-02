//! Frames on the canvas ([`plumcraft_graph::frames`]): titled rectangles for arranging nodes.
//!
//! - **Drawn under the nodes:** egui-snarl paints its background in graph coordinates before
//!   the nodes, and [`paint_frames`] adds the frames there.
//! - **The tab** (an actions menu, an icon and the title) sits on the frame's top edge, outside
//!   the frame, so nothing inside (a group filling it, a node dragged to its top) can cover it.
//!   It is an overlay:
//!   - its ☰ opens the frame's menu: retitle, recolour or remove the frame;
//!   - drag the title to move the frame and what it holds (`frame.move`);
//!   - right-click the title for the same menu, or double-click it to retitle.
//! - **The corner handle** (bottom right) resizes the frame (`frame.resize`).
//! - **Edit › Frame Selection** frames the selected nodes (`frame.add`).
//!
//! Lifted from the research software's `crates/ui-egui/src/frame_view.rs`. A drag here keeps what the frame
//! held when it began, so a frame dragged over a node doesn't collect it (the research software's asks again
//! at every step); retitling uses [`crate::rename_view`].

use egui::{Color32, Pos2, Rect, Vec2};
use plumcraft_graph::{Frame, FrameId, Graph};
use serde_json::{Value, json};

use crate::canvas::{Action, Canvas};
use crate::rename_view::{self, RenameTarget};
use crate::theme::{Tokens, parse_hex};

/// The icon before a frame's title.
pub const FRAME_ICON: &str = "frame";

/// What a frame with no title is called.
const UNTITLED: &str = "Frame";

/// Colours a frame can take from its menu (the theme's accent when none is chosen).
pub const SWATCHES: [(&str, &str); 6] =
    [("Blue", "#3d6fb6"), ("Green", "#3f8f5a"), ("Amber", "#b5852e"), ("Red", "#b54a4a"), ("Violet", "#7a58b0"), ("Grey", "#6b7280")];

fn tint(frame: &Frame, tokens: &Tokens) -> Color32 {
    frame.color.as_deref().and_then(parse_hex).unwrap_or(tokens.accent)
}

fn rect_of(frame: &Frame) -> Rect {
    Rect::from_min_size(Pos2::new(frame.pos[0], frame.pos[1]), Vec2::new(frame.size[0], frame.size[1]))
}

/// The frames' rects in graph coordinates (Fit takes them in).
pub fn frame_rects(graph: &Graph) -> impl Iterator<Item = Rect> + '_ {
    graph.frames.values().map(rect_of)
}

/// Paint the frames with snarl's background painter (graph coordinates, under the nodes).
pub fn paint_frames(painter: &egui::Painter, graph: &Graph, tokens: &Tokens) {
    let radius = f32::from(tokens.radius) * 2.0;
    for frame in graph.frames.values() {
        let r = rect_of(frame);
        let c = tint(frame, tokens);
        painter.rect_filled(r, radius, c.gamma_multiply(0.08));
        painter.rect_stroke(r, radius, egui::Stroke::new(1.0, c.gamma_multiply(0.6)), egui::StrokeKind::Inside);
    }
}

impl Canvas {
    /// Is a screen position on a frame's tab or handle (last drawn)? They sit over the canvas,
    /// so a click there isn't a click on what is under them.
    pub(crate) fn over_frame_tab(&self, screen: Pos2) -> bool {
        self.frame_tabs.iter().any(|r| r.contains(screen))
    }

    /// Each frame's tab and resize handle. Returns the commands their gestures make.
    pub(crate) fn frame_overlay(&mut self, ctx: &egui::Context, graph: &Graph, tokens: &Tokens) -> Vec<Action> {
        let mut actions = Vec::new();
        let t = self.to_global;
        self.frame_tabs.clear();
        if t.scaling <= 0.0 {
            return actions;
        }
        for (id, frame) in &graph.frames {
            let screen = Rect::from_min_max(t * rect_of(frame).min, t * rect_of(frame).max);
            if !screen.intersects(self.rect) {
                continue;
            }
            let title = if frame.title.is_empty() { UNTITLED } else { frame.title.as_str() };
            // the tab: its bottom on the frame's top edge, at most as wide as the frame
            let header = egui::Area::new(egui::Id::new(("plumcraft-frame-header", id.0.as_str())))
                .order(egui::Order::Middle)
                .constrain(false)
                .pivot(egui::Align2::LEFT_BOTTOM)
                .fixed_pos(screen.left_top() + Vec2::new(0.0, 1.0));
            let shown = header.show(ctx, |ui| {
                ui.set_clip_rect(self.rect);
                let size = 14.0 * t.scaling.clamp(0.6, 1.6);
                let text = egui::RichText::new(title).color(tokens.text).size(size).strong();
                let round = (f32::from(tokens.radius) * 2.0) as u8;
                let tab = egui::Frame::new()
                    .fill(tint(frame, tokens).gamma_multiply(0.75))
                    .corner_radius(egui::CornerRadius { nw: round, ne: round, sw: 0, se: 0 })
                    .inner_margin(egui::Margin::symmetric(6, 3));
                let r = tab
                    .show(ui, |ui| {
                        // one line, cut with "…" past the frame's width (but a narrow frame still
                        // shows a readable title; an Area shrinks to its content, so a wrapping
                        // label broke the title one word per line)
                        ui.set_max_width((screen.width() - 12.0).max(180.0));
                        ui.horizontal(|ui| {
                            ui.spacing_mut().button_padding = Vec2::splat(2.0);
                            crate::icons::menu(ui, "menu", size + 1.0, tokens.text, "Frame actions", |ui| frame_menu(ui, id, frame, &mut actions));
                            crate::icons::icon(ui, FRAME_ICON, size + 1.0, tokens.text, "");
                            ui.add(egui::Label::new(text).truncate().selectable(false).sense(egui::Sense::click_and_drag()))
                        })
                        .inner
                    })
                    .inner;
                let label = format!("frame {title}");
                r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, &label));
                if r.drag_started() {
                    self.frame_drag = Some((id.clone(), graph.frame_held(id)));
                }
                if r.dragged() {
                    let d = r.drag_delta() / t.scaling;
                    if d != Vec2::ZERO {
                        let mut p = json!({"frame": id, "pos": [frame.pos[0] + d.x, frame.pos[1] + d.y]});
                        if let Some((_, held)) = self.frame_drag.as_ref().filter(|(f, _)| f == id) {
                            p["nodes"] = json!(held.nodes);
                            p["groups"] = json!(held.groups);
                        }
                        actions.push(("frame.move", p));
                    }
                }
                if r.drag_stopped() {
                    self.frame_drag = None;
                }
                if r.double_clicked() {
                    rename_view::request(ui.ctx(), RenameTarget::Frame(id.clone()), &frame.title, UNTITLED);
                }
                r.on_hover_cursor(egui::CursorIcon::Grab)
                    .on_hover_text(format!("{title}\n\nDrag to move the frame and its nodes; double-click to rename; right-click for its menu"))
                    .context_menu(|ui| frame_menu(ui, id, frame, &mut actions));
            });
            self.frame_tabs.push(shown.response.rect);
            let handle = egui::Area::new(egui::Id::new(("plumcraft-frame-resize", id.0.as_str())))
                .order(egui::Order::Middle)
                .constrain(false)
                .fixed_pos(screen.max - Vec2::new(16.0, 16.0));
            let shown = handle.show(ctx, |ui| {
                ui.set_clip_rect(self.rect);
                let (r, painter) = ui.allocate_painter(Vec2::splat(14.0), egui::Sense::drag());
                let c = tint(frame, tokens);
                let corner = r.rect.right_bottom();
                for k in [4.0, 8.0, 12.0] {
                    painter.line_segment([corner - Vec2::new(k, 0.0), corner - Vec2::new(0.0, k)], egui::Stroke::new(1.2, c.gamma_multiply(0.9)));
                }
                let label = format!("Resize frame {title}");
                r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, &label));
                let r = r.on_hover_cursor(egui::CursorIcon::ResizeNwSe).on_hover_text("Drag to resize the frame");
                if r.dragged() {
                    let d = r.drag_delta() / t.scaling;
                    if d != Vec2::ZERO {
                        actions.push(("frame.resize", json!({"frame": id, "size": [frame.size[0] + d.x, frame.size[1] + d.y]})));
                    }
                }
            });
            self.frame_tabs.push(shown.response.rect);
        }
        actions
    }
}

/// A frame's menu: its title, a colour, remove.
fn frame_menu(ui: &mut egui::Ui, id: &FrameId, frame: &Frame, actions: &mut Vec<Action>) {
    ui.weak(format!("frame {id}"));
    // (a window: a text field can't live in a menu, crate::rename_view)
    if ui.button("Rename…").clicked() {
        rename_view::request(ui.ctx(), RenameTarget::Frame(id.clone()), &frame.title, UNTITLED);
        ui.close();
    }
    ui.menu_button("Colour", |ui| {
        for (name, hex) in SWATCHES {
            let c = parse_hex(hex).unwrap_or(Color32::GRAY);
            let picked = frame.color.as_deref() == Some(hex);
            if ui.add(egui::Button::new(egui::RichText::new(format!("■ {name}")).color(c)).selected(picked)).clicked() {
                actions.push(("frame.set", json!({"frame": id, "color": hex})));
                ui.close();
            }
        }
        if ui.add(egui::Button::new("Theme accent").selected(frame.color.is_none())).clicked() {
            actions.push(("frame.set", json!({"frame": id, "color": null})));
            ui.close();
        }
    });
    if ui.button("Remove Frame").on_hover_text("Its nodes stay").clicked() {
        actions.push(("frame.remove", json!({"frame": id})));
        ui.close();
    }
}

/// `ui.inspect`'s `frames`: each frame, with what it holds now.
pub fn inspect(graph: &Graph) -> Value {
    let list: Vec<Value> = graph
        .frames
        .iter()
        .map(|(id, f)| {
            let held = graph.frame_held(id);
            json!({"id": id, "title": f.title, "pos": f.pos, "size": f.size, "color": f.color, "nodes": held.nodes, "groups": held.groups})
        })
        .collect();
    Value::Array(list)
}
