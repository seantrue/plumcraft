//! Fonts: Inter (OFL 1.1, `assets/fonts/OFL-Inter.txt`, which covers both of its files) is the
//! text face, 14 pt by default (docs/research/22-themes.md section 5), with egui's Ubuntu Light
//! behind it for anything Inter lacks; Inter SemiBold is a family of its own ([`SEMIBOLD`]) for
//! node titles and headings. Monospace stays egui's Hack, with Inter behind it for symbols.
//! Engines' names and docs use `→`, `±`, `π`, `›` and the like, which Ubuntu Light and Hack don't
//! have and egui would draw as a box.
//!
//! Lifted from the research software's `crates/ui-egui/src/fonts.rs`.

use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};

const INTER: &str = "Inter";
const INTER_SEMIBOLD: &str = "Inter SemiBold";
/// The font family of node titles and headings.
pub const SEMIBOLD: &str = "semibold";

/// egui's font definitions with Inter first in the proportional family, after Hack in the
/// monospace one, and Inter SemiBold (then Inter) in [`SEMIBOLD`].
pub fn definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(INTER.to_owned(), Arc::new(FontData::from_static(include_bytes!("../assets/fonts/Inter-Regular.ttf"))));
    fonts.font_data.insert(INTER_SEMIBOLD.to_owned(), Arc::new(FontData::from_static(include_bytes!("../assets/fonts/Inter-SemiBold.ttf"))));
    let proportional = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    if let Some(stack) = fonts.families.get_mut(&FontFamily::Proportional) {
        stack.insert(0, INTER.to_owned());
    }
    if let Some(stack) = fonts.families.get_mut(&FontFamily::Monospace) {
        // after Hack, before egui's emoji fonts
        let at = stack.len().min(1);
        stack.insert(at, INTER.to_owned());
    }
    let mut semibold = vec![INTER_SEMIBOLD.to_owned(), INTER.to_owned()];
    semibold.extend(proportional);
    fonts.families.insert(FontFamily::Name(SEMIBOLD.into()), semibold);
    fonts
}

/// Whether the context's fonts include [`SEMIBOLD`] (call inside a frame: before the first one
/// a context has no fonts).
pub fn has_semibold(ctx: &egui::Context) -> bool {
    ctx.fonts_mut(|f| f.definitions().families.contains_key(&FontFamily::Name(SEMIBOLD.into())))
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
        // (egui's has_glyph says no for every monospace character, even with egui's own fonts
        // alone, so monospace isn't checked here)
        for font in [egui::FontId::proportional(14.0), egui::FontId::new(14.0, FontFamily::Name(SEMIBOLD.into()))] {
            let missing: std::collections::BTreeSet<char> = ctx.fonts_mut(|f| text.chars().filter(|c| !c.is_control() && !f.has_glyph(&font, *c)).collect());
            assert!(missing.is_empty(), "{font:?}: no glyph for {missing:?}");
        }
    }

    /// Inter is the text face: its first font, and a title's is SemiBold.
    #[test]
    fn inter_is_the_text_face() {
        let d = definitions();
        assert_eq!(d.families.get(&FontFamily::Proportional).and_then(|s| s.first()).map(String::as_str), Some(INTER));
        assert_eq!(d.families.get(&FontFamily::Name(SEMIBOLD.into())).and_then(|s| s.first()).map(String::as_str), Some(INTER_SEMIBOLD));
    }

    /// Without Inter, the arrow had no glyph: the test above guards something real.
    #[test]
    fn egui_alone_lacks_the_arrow() {
        let ctx = egui::Context::default();
        ctx.run_ui(egui::RawInput::default(), |_| {}).textures_delta.clear();
        assert!(!ctx.fonts_mut(|f| f.has_glyph(&egui::FontId::proportional(14.0), '→')));
    }
}
