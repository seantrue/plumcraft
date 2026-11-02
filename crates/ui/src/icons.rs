//! Icons: the Lucide SVGs in `icon_data`, recoloured to white, rasterized by egui_extras' SVG
//! loader at their on-screen size and tinted per use (as photocraft draws its icons).
//!
//! Lifted from the research software's `crates/ui-egui/src/icons.rs`, under PlumCraft's own `bytes://` prefix.

use std::sync::OnceLock;

use crate::craft_ui::icons::{SvgIconSet, SvgStyle};
use egui::{Color32, Response, Vec2};

use crate::icon_data::ICONS;

/// The icon shown for a name the set doesn't have (an engine may name any Lucide icon).
pub const FALLBACK: &str = "box";

fn white_icons() -> &'static SvgIconSet {
    static SET: OnceLock<SvgIconSet> = OnceLock::new();
    SET.get_or_init(|| SvgIconSet::new(ICONS, SvgStyle { current_color: "#ffffff", stroke_width: Some("1.75") }, "bytes://plumcraft/icons/", FALLBACK))
}

/// Is `name` one of the embedded icons?
pub fn exists(name: &str) -> bool {
    white_icons().contains(name)
}

/// Install the SVG loader (once per context; later calls do nothing).
pub fn ensure_loaders(ctx: &egui::Context) {
    egui_extras::install_image_loaders(ctx);
}

/// An icon, tinted, `size` points square (an unknown name shows [`FALLBACK`]).
pub fn image(name: &str, size: f32, tint: Color32) -> egui::Image<'static> {
    let name = if exists(name) { name } else { FALLBACK };
    egui::Image::new(white_icons().source(name)).fit_to_exact_size(Vec2::splat(size)).tint(tint)
}

/// An icon laid out in `ui`, with `tip` on hover.
pub fn icon(ui: &mut egui::Ui, name: &str, size: f32, tint: Color32, tip: &str) -> Response {
    let r = ui.add(image(name, size, tint).sense(egui::Sense::hover()));
    if tip.is_empty() { r } else { r.on_hover_text(tip) }
}

/// A frameless icon button. `label` is what it's called (its tooltip, and the name
/// accessibility tools and tests find it by).
pub fn button(ui: &mut egui::Ui, name: &str, size: f32, tint: Color32, label: &str) -> Response {
    let r = ui.add(egui::Button::image(image(name, size, tint)).frame(false));
    labeled(&r, label);
    r.on_hover_text(label)
}

/// An icon button that opens a dropdown menu of `add_contents`.
pub fn menu(ui: &mut egui::Ui, name: &str, size: f32, tint: Color32, label: &str, add_contents: impl FnOnce(&mut egui::Ui)) -> Response {
    let (r, _) = egui::containers::menu::MenuButton::from_button(egui::Button::image(image(name, size, tint)).frame(false)).ui(ui, add_contents);
    labeled(&r, label);
    r.on_hover_text(label)
}

fn labeled(r: &Response, label: &str) {
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, r.enabled(), label));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_is_an_svg_and_the_fallback_is_there() {
        assert!(exists(FALLBACK));
        for name in ["chevron-down", "chevron-right", "ellipsis-vertical"] {
            assert!(exists(name), "the canvas uses {name}");
        }
        for (name, bytes) in ICONS {
            let text = std::str::from_utf8(bytes).unwrap();
            assert!(text.contains("<svg") && text.contains("currentColor"), "{name}");
        }
        assert!(!exists("no-such-icon"));
    }
}
