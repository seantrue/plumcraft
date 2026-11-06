//! The status bar's messages: the run's errors, the application's segment, a save's outcome,
//! the last thing that went wrong. Each is a chip whose background says how serious it is
//! (accent for information, the warning colour, the error colour, each mixed into the panel),
//! in the theme's ordinary text colour. The messages share what width the bar has left: one that
//! doesn't fit is cut short or left out, the most serious shown first, and a "more…" button says
//! so. Clicking any message, or
//! that button, opens the Messages window (`view.messages`), which shows every one in full.

use egui::{Color32, Sense, TextWrapMode, WidgetText};
use plumcraft_session::commands::Tone;

use crate::app_hooks::tone_color;
use crate::theme::{Tokens, mix};

/// One message in the status bar.
#[derive(Debug, Clone, PartialEq)]
pub struct Msg {
    pub tone: Tone,
    pub text: String,
    /// More detail, shown on hover and in the Messages window.
    pub detail: Option<String>,
}

impl Msg {
    pub fn new(tone: Tone, text: impl Into<String>) -> Msg {
        Msg { tone, text: text.into(), detail: None }
    }

    pub fn detail(mut self, d: impl Into<String>) -> Msg {
        let d = d.into();
        self.detail = (!d.is_empty()).then_some(d);
        self
    }
}

/// The label of the button that opens the Messages window when something isn't shown in full
/// ("+N more…" when N messages are left out altogether).
pub const MORE: &str = "more…";
/// The Messages window's title.
pub const TITLE: &str = "Messages";

/// How far a tone's colour is mixed into the panel for a chip's background.
const TINT: f32 = 0.28;
/// The width kept for the "more…" button.
const MORE_W: f32 = 64.0;
/// Below this, a message isn't started: it counts as not shown.
const MIN_W: f32 = 48.0;

/// A chip's background.
pub fn background(tone: Tone, tokens: &Tokens) -> Color32 {
    mix(tokens.panel, tone_color(tone, tokens), TINT)
}

fn chip(ui: &mut egui::Ui, m: &Msg, tokens: &Tokens, galley: std::sync::Arc<egui::Galley>) -> egui::Response {
    egui::Frame::new()
        .fill(background(m.tone, tokens))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(6, 1))
        .show(ui, |ui| ui.add(egui::Label::new(galley).sense(Sense::click())))
        .inner
}

/// Draw `msgs` in what width `ui` has left, one line. Returns true when the Messages window
/// should open (a message, or the "more…" button, was clicked).
pub fn line(ui: &mut egui::Ui, msgs: &[Msg], tokens: &Tokens) -> bool {
    let mut open = false;
    let (mut cut, mut left_out) = (0usize, 0usize);
    for (i, m) in by_severity(msgs).into_iter().enumerate() {
        let room = ui.available_width() - MORE_W - ui.spacing().item_spacing.x - 12.0;
        if room < MIN_W {
            left_out = msgs.len() - i;
            cut += left_out;
            break;
        }
        let text = WidgetText::from(egui::RichText::new(&m.text).color(tokens.text));
        let galley = text.into_galley(ui, Some(TextWrapMode::Truncate), room, egui::TextStyle::Body);
        if galley.elided {
            cut += 1;
        }
        let r = chip(ui, m, tokens, galley);
        let hover = match &m.detail {
            Some(d) => format!("{}\n\n{d}\n\nClick to see every message in full", m.text),
            None => format!("{}\n\nClick to see every message in full", m.text),
        };
        if r.on_hover_text(hover).clicked() {
            open = true;
        }
    }
    if cut > 0 {
        let tip = if cut == 1 {
            "1 message isn't shown in full: click to read it".to_owned()
        } else {
            format!("{cut} messages aren't shown in full: click to read them")
        };
        let label = if left_out > 0 { format!("+{left_out} {MORE}") } else { MORE.to_owned() };
        if ui.small_button(label).on_hover_text(tip).clicked() {
            open = true;
        }
    }
    open
}

/// The most serious first (errors, warnings, information), each kind in its own order.
pub fn by_severity(msgs: &[Msg]) -> Vec<&Msg> {
    let rank = |t: Tone| match t {
        Tone::Error => 0,
        Tone::Warn => 1,
        Tone::Info => 2,
    };
    let mut v: Vec<&Msg> = msgs.iter().collect();
    v.sort_by_key(|m| rank(m.tone));
    v
}

/// The Messages window: every message in full, wrapped and selectable. Closes itself when
/// there is nothing to show.
pub fn window(ctx: &egui::Context, open: &mut bool, msgs: &[Msg], tokens: &Tokens) {
    if msgs.is_empty() {
        *open = false;
    }
    if !*open {
        return;
    }
    egui::Window::new(TITLE)
        .open(open)
        .collapsible(false)
        .resizable(true)
        .default_width(520.0)
        .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-12.0, -36.0))
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                for m in by_severity(msgs) {
                    egui::Frame::new().fill(background(m.tone, tokens)).corner_radius(4.0).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.add(egui::Label::new(egui::RichText::new(&m.text).color(tokens.text)).wrap().selectable(true));
                        if let Some(d) = &m.detail {
                            ui.add(egui::Label::new(egui::RichText::new(d).color(tokens.text_dim)).wrap().selectable(true));
                        }
                    });
                    ui.add_space(4.0);
                }
            });
        });
}
