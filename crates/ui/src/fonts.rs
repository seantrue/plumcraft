//! Fonts: egui's own, with Inter (OFL, `assets/fonts/OFL-Inter.txt`) behind them for the
//! symbols they lack. Engines' names and docs use `→`, `±`, `π`, `›` and the like, which Ubuntu
//! Light (egui's text face) doesn't have and egui draws as a box. Inter is a fallback, not the
//! text face, so text keeps its look and only those glyphs change.
//!
//! Lifted from the research software's `crates/ui-egui/src/fonts.rs`.

use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};

const INTER: &str = "Inter";

/// egui's font definitions with Inter after the text face in each family.
pub fn definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(INTER.to_owned(), Arc::new(FontData::from_static(include_bytes!("../assets/fonts/Inter-Regular.ttf"))));
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        if let Some(stack) = fonts.families.get_mut(&family) {
            // after the family's own face (Ubuntu Light, or Hack), before egui's emoji fonts
            let at = stack.len().min(1);
            stack.insert(at, INTER.to_owned());
        }
    }
    fonts
}

/// Install [`definitions`] (once per context, at setup).
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(definitions());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(v: &serde_json::Value, out: &mut String) {
        match v {
            serde_json::Value::String(s) => out.push_str(s),
            serde_json::Value::Array(a) => a.iter().for_each(|v| strings(v, out)),
            serde_json::Value::Object(o) => o.values().for_each(|v| strings(v, out)),
            _ => {}
        }
    }

    /// Every character the text engine's catalogue uses can be drawn, and the symbols the
    /// editor itself draws (the research software's maintainer saw "Lab → RGB" drawn as "Lab ☐ RGB").
    #[test]
    fn every_character_the_engine_sends_has_a_glyph() {
        let ctx = egui::Context::default();
        install(&ctx);
        ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
        let mut text = String::from("→ › … × ± π ✓ ■ ▶ —");
        strings(&serde_json::to_value(plumcraft_text::engine().schema()).unwrap(), &mut text);
        let font = egui::FontId::proportional(14.0);
        let missing: std::collections::BTreeSet<char> = ctx.fonts_mut(|f| text.chars().filter(|c| !c.is_control() && !f.has_glyph(&font, *c)).collect());
        assert!(missing.is_empty(), "no glyph for {missing:?}");
    }

    /// Without Inter, the arrow had no glyph: the test above guards something real.
    #[test]
    fn egui_alone_lacks_the_arrow() {
        let ctx = egui::Context::default();
        ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
        assert!(!ctx.fonts_mut(|f| f.has_glyph(&egui::FontId::proportional(14.0), '→')));
    }
}
