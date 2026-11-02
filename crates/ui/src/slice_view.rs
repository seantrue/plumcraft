//! List slices on wires: the label drawn on a sliced wire (`[:-1]`), and the small editor that
//! sets one, opened from the wire's menu ("Slice…") or by double-clicking the label.
//!
//! The expression is checked as it is typed ([`plumcraft_graph::parse_slice`], the engine's
//! rules); the type check against the ports happens in `wire.slice`, whose refusal shows in the
//! status bar. Setting or clearing a slice is an undo step. Lifted from the research software
//! (`crates/ui-egui/src/slice_view.rs`, 2026-10-31, `8480c03`).

use egui::{Pos2, Vec2};
use plumcraft_graph::{PortRef, Slice, parse_slice};
use serde_json::json;

use crate::canvas::Action;
use crate::theme::Tokens;

/// A slice being edited.
#[derive(Debug, Clone, PartialEq)]
pub struct SliceEdit {
    pub from: PortRef,
    pub to: PortRef,
    pub text: String,
    /// Where the editor opens (screen).
    pub screen_pos: Pos2,
    /// Ask for keyboard focus on the first frame.
    pub focus: bool,
    /// Don't close on the click that opened it.
    pub fresh: bool,
}

impl SliceEdit {
    pub fn new(from: PortRef, to: PortRef, current: Option<&str>, screen_pos: Pos2) -> Self {
        SliceEdit { from, to, text: current.unwrap_or_default().to_owned(), screen_pos, focus: true, fresh: true }
    }
}

/// The label on a sliced wire.
pub fn label_text(slice: &str) -> String {
    format!("[{slice}]")
}

/// `2nd`, `3rd`, `11th`, `21st`.
fn ordinal(n: i64) -> String {
    let n = n.unsigned_abs();
    let suffix = match (n % 10, n % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{n}{suffix}")
}

/// What a slice picks, in words (the label's tooltip and the editor's hint).
pub fn describe(s: &Slice) -> String {
    match *s {
        Slice::Index(0) => "the first item (one item, not a list)".into(),
        Slice::Index(-1) => "the last item (one item, not a list)".into(),
        Slice::Index(i) if i < 0 => format!("item {i}, counting back from the end (one item, not a list)"),
        Slice::Index(i) => format!("item {i} (one item, not a list)"),
        Slice::Range { start, stop, step } => {
            let from = match start {
                None => "from the start".to_owned(),
                Some(-1) => "from the last item".to_owned(),
                Some(a) if a < 0 => format!("from the {}-last item", ordinal(a)),
                Some(a) => format!("from item {a}"),
            };
            let to = match stop {
                None => "to the end".to_owned(),
                Some(-1) => "up to (not including) the last".to_owned(),
                Some(b) if b < 0 => format!("up to (not including) the {}-last", ordinal(b)),
                Some(b) => format!("up to (not including) item {b}"),
            };
            let every = match step {
                None | Some(1) => String::new(),
                Some(-1) => ", reversed".to_owned(),
                Some(k) if k < 0 => format!(", every {}, reversed", ordinal(k)),
                Some(k) => format!(", every {}", ordinal(k)),
            };
            format!("a list: {from} {to}{every}")
        }
    }
}

/// One label to draw: where (screen), and the wire it belongs to.
#[derive(Debug, Clone)]
pub struct WireLabel {
    pub at: Pos2,
    pub from: PortRef,
    pub to: PortRef,
    pub slice: String,
}

/// Draw the labels of sliced wires inside `clip`. A double-click on one opens its editor.
pub fn draw_labels(ctx: &egui::Context, clip: egui::Rect, labels: &[WireLabel], tokens: &Tokens) -> Option<SliceEdit> {
    let mut open = None;
    for (i, l) in labels.iter().enumerate() {
        if !clip.contains(l.at) {
            continue;
        }
        let text = label_text(&l.slice);
        let hover = match parse_slice(&l.slice) {
            Ok(s) => format!("Slice {text}: {}.\nDouble-click to edit; right-click the wire for more.", describe(&s)),
            Err(e) => format!("{e}\nDouble-click to fix it."),
        };
        let bad = parse_slice(&l.slice).is_err();
        let area = egui::Area::new(egui::Id::new(("plumcraft-slice-label", i, &l.to.node.0, &l.to.port)))
            .order(egui::Order::Middle)
            .fixed_pos(l.at)
            .pivot(egui::Align2::CENTER_CENTER)
            .constrain_to(clip)
            .show(ctx, |ui| {
                let fill = if bad { tokens.error } else { tokens.panel };
                egui::Frame::new()
                    .fill(fill)
                    .stroke(egui::Stroke::new(1.0, tokens.accent))
                    .corner_radius(4.0)
                    .inner_margin(egui::Margin::symmetric(4, 1))
                    .show(ui, |ui| ui.add(egui::Label::new(egui::RichText::new(&text).monospace().size(11.0).color(tokens.text)).sense(egui::Sense::click())))
                    .inner
            });
        let r = area.inner.on_hover_text(hover);
        if r.double_clicked() {
            open = Some(SliceEdit::new(l.from.clone(), l.to.clone(), Some(&l.slice), r.rect.left_bottom() + Vec2::new(0.0, 4.0)));
        }
    }
    open
}

/// What the editor did this frame.
#[derive(Debug, Clone, PartialEq)]
pub enum EditorOutcome {
    Open,
    Closed,
    Apply(Action),
}

/// Draw the slice editor. Enter (or Set) applies a valid expression, an empty one clears the
/// slice; Esc, Cancel or a click outside closes it.
pub fn editor(ctx: &egui::Context, edit: &mut SliceEdit, tokens: &Tokens) -> EditorOutcome {
    let parsed = if edit.text.trim().is_empty() { None } else { Some(parse_slice(&edit.text)) };
    let mut outcome = EditorOutcome::Open;
    let area = egui::Area::new(egui::Id::new("plumcraft-slice-editor")).order(egui::Order::Foreground).fixed_pos(edit.screen_pos).show(ctx, |ui| {
        egui::Frame::menu(ui.style()).show(ui, |ui| {
            ui.set_max_width(300.0);
            ui.weak(format!("Slice {} → {}", edit.from, edit.to));
            let r =
                ui.add(egui::TextEdit::singleline(&mut edit.text).hint_text("1:-1:2, :-1, ::2 or -1").desired_width(180.0).font(egui::TextStyle::Monospace));
            if edit.focus {
                r.request_focus();
                edit.focus = false;
            }
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            match &parsed {
                None => ui.weak("Empty: no slice (the whole list)."),
                Some(Ok(s)) => ui.label(egui::RichText::new(describe(s)).small()),
                Some(Err(e)) => ui.colored_label(tokens.error, e),
            };
            let valid = !matches!(parsed, Some(Err(_)));
            let mut set = false;
            ui.horizontal(|ui| {
                set = ui.add_enabled(valid, egui::Button::new("Set")).clicked();
                if ui.button("Cancel").clicked() {
                    outcome = EditorOutcome::Closed;
                }
                if !edit.text.is_empty() && ui.button("Clear").on_hover_text("Remove the slice").clicked() {
                    outcome = EditorOutcome::Apply(("wire.slice", json!({"to": edit.to, "slice": null})));
                }
            });
            if (set || enter) && valid {
                let slice = match &parsed {
                    Some(Ok(s)) => json!(s.to_string()),
                    _ => json!(null),
                };
                outcome = EditorOutcome::Apply(("wire.slice", json!({"to": edit.to, "slice": slice})));
            }
        });
    });
    let outside = ctx.input(|i| i.pointer.any_click()) && !area.response.hovered() && !edit.fresh;
    let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    edit.fresh = false;
    if outcome == EditorOutcome::Open && (outside || escape) {
        outcome = EditorOutcome::Closed;
    }
    outcome
}

impl crate::canvas::Canvas {
    /// Slice labels on the wires that have one, and the slice editor when open. Returns commands.
    pub(crate) fn slice_overlay(
        &mut self,
        ctx: &egui::Context,
        style: &egui::Style,
        graph: &plumcraft_graph::Graph,
        catalog: &plumcraft_proto::catalogue::SchemaReply,
        tokens: &Tokens,
    ) -> Vec<Action> {
        let labels: Vec<WireLabel> = self
            .wire_midpoints(style, catalog)
            .into_iter()
            .filter_map(|(at, from, to)| {
                let slice = graph.wire_into(&to).filter(|w| w.from == from)?.slice.clone()?;
                Some(WireLabel { at, from, to, slice })
            })
            .collect();
        if let Some(open) = draw_labels(ctx, self.rect, &labels, tokens) {
            self.slice_edit = Some(open);
        }
        let mut actions = Vec::new();
        if let Some(edit) = &mut self.slice_edit {
            match editor(ctx, edit, tokens) {
                EditorOutcome::Open => {}
                EditorOutcome::Closed => self.slice_edit = None,
                EditorOutcome::Apply(a) => {
                    actions.push(a);
                    self.slice_edit = None;
                }
            }
        }
        actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slices_are_described() {
        let d = |s: &str| describe(&parse_slice(s).unwrap());
        assert_eq!(d("-1"), "the last item (one item, not a list)");
        assert_eq!(d("2"), "item 2 (one item, not a list)");
        assert_eq!(d(":-1"), "a list: from the start up to (not including) the last");
        assert_eq!(d("1:-1:2"), "a list: from item 1 up to (not including) the last, every 2nd");
        assert_eq!(d("-3:"), "a list: from the 3rd-last item to the end");
        assert_eq!(d("::-1"), "a list: from the start to the end, reversed");
        assert_eq!(label_text(":-1"), "[:-1]");
    }
}
