//! What an application of the editor shows through its hooks
//! ([`plumcraft_session::commands::Hooks`]): a line on a node and a value for the Watched panel
//! (`node_overlays`), labels on wires (`wire_marks`) and a status bar segment (`status`).
//!
//! The hooks are read once a frame ([`AppShown::read`]); while any of them has something to show
//! the editor repaints a few times a second ([`REPAINT`]), so values that change outside a run
//! keep up. The editor never knows what the application means by them.

use std::collections::HashMap;
use std::time::Duration;

use egui::{Pos2, Vec2};
use plumcraft_graph::{Graph, NodeId};
use plumcraft_proto::PortValue;
use plumcraft_proto::catalogue::{Media, SchemaReply};
use plumcraft_proto::display::{Display, Rendition, WatchRequest};
use plumcraft_session::commands::{self, Tone, WireMark};

use crate::theme::Tokens;

/// How often the editor repaints while the application shows anything.
pub const REPAINT: Duration = Duration::from_millis(100);

/// The most characters of an overlay's text drawn on a node (the rest on hover).
pub const NODE_CHARS: usize = 160;

/// What the application's hooks gave this frame.
#[derive(Debug, Clone, Default)]
pub struct AppShown {
    /// Each node's overlay line, by node id.
    pub texts: HashMap<String, String>,
    /// Each node's overlay value, by node id.
    pub values: HashMap<String, PortValue>,
    /// The status bar segment.
    pub status: Option<String>,
    /// Anything shown (the editor repaints while there is).
    pub any: bool,
}

impl AppShown {
    /// Read the node overlays and the status segment (wire marks are read as the canvas draws,
    /// since they need its wires' places).
    pub fn read() -> Self {
        let mut shown = AppShown { status: commands::app_status(), ..Default::default() };
        for (NodeId(id), o) in commands::node_overlays() {
            if let Some(t) = o.text.filter(|t| !t.is_empty()) {
                shown.texts.insert(id.clone(), t);
            }
            if let Some(v) = o.value {
                shown.values.insert(id, v);
            }
        }
        shown.any = shown.status.is_some() || !shown.texts.is_empty() || !shown.values.is_empty();
        shown
    }

    /// The Watched panel's display of the application's value for the watched node, if it has
    /// one: it takes the place of the run's.
    pub fn display(&self, w: &WatchRequest, graph: &Graph, catalog: &SchemaReply) -> Option<Display> {
        let v = self.values.get(&w.node)?;
        let ty = graph.node(&NodeId(w.node.clone())).and_then(|n| catalog.node(&n.kind)).and_then(|d| d.output(&w.port)).map(|p| p.ty.clone());
        let media = ty.as_deref().and_then(|t| catalog.wire_type(t)).map(|t| t.media);
        Some(display_of(v, w, ty.unwrap_or_default(), media))
    }
}

/// A Watched panel display of an application's value. `media` is the watched port's wire type's
/// (none: worked out from the value).
pub fn display_of(v: &PortValue, w: &WatchRequest, ty: String, media: Option<Media>) -> Display {
    let mut d = Display { ty, node: w.node.clone(), port: w.port.clone(), items: 1, ..Default::default() };
    let item = match v {
        PortValue::List { items } => {
            d.items = items.len() as u64;
            let i = if w.index < 0 { items.len() as i64 + w.index } else { w.index };
            d.index = i.clamp(0, (items.len() as i64 - 1).max(0));
            match items.get(d.index as usize) {
                Some(item) => item,
                None => {
                    d.media = media.unwrap_or(Media::Bytes);
                    d.summary = "an empty list, from the application".into();
                    return d;
                }
            }
        }
        other => other,
    };
    let of = if d.items > 1 { format!(" (item {} of {})", d.index + 1, d.items) } else { String::new() };
    match item {
        PortValue::Json { value } => {
            d.media = match (media, value) {
                (Some(Media::Table), _) => Media::Table,
                (Some(Media::Text), serde_json::Value::String(_)) => Media::Text,
                _ => Media::Scalar,
            };
            if let (Media::Text, serde_json::Value::String(text)) = (d.media, value) {
                d.renditions.push(Rendition::Excerpt {
                    text: text.clone(),
                    offset: 0,
                    total_chars: text.chars().count() as u64,
                    lines: text.lines().count() as u64,
                });
            } else {
                d.value = Some(value.clone());
            }
            d.summary = format!("from the application{of}");
        }
        PortValue::Blob { data } => {
            d.media = media.unwrap_or(Media::Bytes);
            d.summary = format!("{} bytes, from the application{of}", data.meta.size);
            d.full = Some(data.clone());
        }
        PortValue::Opaque { summary, .. } => {
            d.media = media.unwrap_or(Media::Bytes);
            d.summary = format!("{summary} (from the application{of})");
        }
        PortValue::List { items } => {
            d.media = media.unwrap_or(Media::Bytes);
            d.summary = format!("a list of {}, from the application{of}", items.len());
        }
    }
    d
}

/// One wire mark to draw: where (screen) and what.
#[derive(Debug, Clone)]
pub struct Placed {
    pub at: Pos2,
    /// The input the wire goes into (the label's id).
    pub to: String,
    pub mark: WireMark,
}

/// The colour of a mark's outline.
pub fn tone_color(tone: Tone, tokens: &Tokens) -> egui::Color32 {
    match tone {
        Tone::Info => tokens.accent,
        Tone::Warn => tokens.warning,
        Tone::Error => tokens.error,
    }
}

/// Draw the application's wire marks inside `clip`.
pub fn draw_marks(ctx: &egui::Context, clip: egui::Rect, marks: &[Placed], tokens: &Tokens) {
    for (i, m) in marks.iter().enumerate() {
        if !clip.contains(m.at) || m.mark.label.is_empty() {
            continue;
        }
        let area = egui::Area::new(egui::Id::new(("plumcraft-app-wire-mark", i, &m.to)))
            .order(egui::Order::Middle)
            .fixed_pos(m.at)
            .pivot(egui::Align2::CENTER_CENTER)
            .constrain_to(clip)
            .interactable(m.mark.tooltip.is_some())
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(tokens.panel)
                    .stroke(egui::Stroke::new(1.5, tone_color(m.mark.tone, tokens)))
                    .corner_radius(4.0)
                    .inner_margin(egui::Margin::symmetric(4, 1))
                    .show(ui, |ui| ui.add(egui::Label::new(egui::RichText::new(&m.mark.label).size(11.0).color(tokens.text)).extend().selectable(false)))
                    .inner
            });
        if let Some(tip) = &m.mark.tooltip {
            area.inner.on_hover_text(tip);
        }
    }
}

impl crate::canvas::Canvas {
    /// The application's marks on the wires drawn this frame, at each wire's middle (below a
    /// slice's label, when the wire has one).
    pub(crate) fn mark_overlay(&self, ctx: &egui::Context, style: &egui::Style, graph: &Graph, catalog: &SchemaReply, tokens: &Tokens) -> bool {
        let marks = commands::wire_marks(graph);
        if marks.is_empty() {
            return false;
        }
        let by_input: HashMap<(String, String), WireMark> = marks.into_iter().map(|(to, m)| ((to.node.0, to.port), m)).collect();
        let placed: Vec<Placed> = self
            .wire_midpoints(style, catalog)
            .into_iter()
            .filter_map(|(at, from, to)| {
                let mark = by_input.get(&(to.node.0.clone(), to.port.clone()))?.clone();
                let sliced = graph.wire_into(&to).filter(|w| w.from == from).is_some_and(|w| w.slice.is_some());
                let at = if sliced { at + Vec2::new(0.0, 18.0) } else { at };
                Some(Placed { at, to: format!("{}.{}", to.node, to.port), mark })
            })
            .collect();
        draw_marks(ctx, self.rect, &placed, tokens);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn watch(index: i64) -> WatchRequest {
        WatchRequest { node: "n".into(), port: "out".into(), index, ..Default::default() }
    }

    #[test]
    fn application_values_become_displays() {
        let d = display_of(&PortValue::Json { value: json!(42) }, &watch(0), "INT".into(), None);
        assert_eq!((d.media, d.value.clone(), d.items), (Media::Scalar, Some(json!(42)), 1));
        let d = display_of(&PortValue::Json { value: json!("hello") }, &watch(0), "TEXT".into(), Some(Media::Text));
        assert!(matches!(&d.renditions[..], [Rendition::Excerpt { text, .. }] if text == "hello"));
        let list = PortValue::List { items: vec![PortValue::Json { value: json!(1) }, PortValue::Json { value: json!(2) }] };
        let d = display_of(&list, &watch(-1), String::new(), None);
        assert_eq!((d.items, d.index, d.value), (2, 1, Some(json!(2))));
        assert!(d.summary.contains("item 2 of 2"));
        // hostile: an empty list, an index far out
        let d = display_of(&PortValue::List { items: vec![] }, &watch(99), String::new(), None);
        assert_eq!(d.items, 0);
        let d = display_of(&list, &watch(i64::MIN + 1), String::new(), None);
        assert_eq!(d.index, 0);
    }
}
