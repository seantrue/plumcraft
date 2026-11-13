//! The status bar's two-line cells (plumcraft #86, layout B; the maintainer: "D17 B", after
//! "Status window is very over crowded. Consider starting to stack some cells vertically or use
//! sub/super scripts"). The engine, document and run cells each draw a main line and, under it, a
//! smaller and dimmer line with its detail, so the row is about half as wide as one line of
//! everything and the messages keep room.

use egui::{FontId, RichText, TextStyle};
use plumcraft_session::commands::Tone;

use crate::theme::Tokens;

/// The widest the engine's status line is drawn, in points; a longer one is cut with an ellipsis
/// and shown whole on hover.
pub const ENGINE_STATUS_WIDTH: f32 = 240.0;

/// The detail line's text size, against the body's.
pub const SUB_SCALE: f32 = 0.8;

/// One cell: `main` on its line, and `sub`, when there is one, under it in the smaller size.
/// What `main` returns is returned.
pub fn status_cell<R>(ui: &mut egui::Ui, main: impl FnOnce(&mut egui::Ui) -> R, sub: Option<impl FnOnce(&mut egui::Ui)>) -> R {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        let out = ui.horizontal(main).inner;
        if let Some(sub) = sub {
            ui.horizontal(|ui| {
                sub_style(ui);
                sub(ui);
            });
        }
        out
    })
    .inner
}

/// Text in `ui` from here on is the detail line's size.
pub fn sub_style(ui: &mut egui::Ui) {
    let body = ui.style().text_styles.get(&TextStyle::Body).map_or(12.5, |f| f.size);
    ui.style_mut().override_font_id = Some(FontId::proportional(body * SUB_SCALE));
    ui.spacing_mut().interact_size.y = body * SUB_SCALE + 2.0;
}

/// The colour of the engine's status line: dim for `Info`, as the other detail lines, and the
/// theme's warning and error colours.
pub fn tone_text_color(tone: Tone, tokens: &Tokens) -> egui::Color32 {
    match tone {
        Tone::Info => tokens.text_dim,
        Tone::Warn => tokens.warning,
        Tone::Error => tokens.error,
    }
}

/// The engine's status line: in its tone's colour, cut at [`ENGINE_STATUS_WIDTH`], whole on hover.
pub fn engine_status_line(ui: &mut egui::Ui, text: &str, tone: Tone, tokens: &Tokens) {
    ui.scope(|ui| {
        ui.set_max_width(ENGINE_STATUS_WIDTH);
        let label = egui::Label::new(RichText::new(text).color(tone_text_color(tone, tokens))).truncate().show_tooltip_when_elided(false);
        ui.add(label).on_hover_text(text);
    });
}
