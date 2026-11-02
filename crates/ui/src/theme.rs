//! Theme tokens. Colours and radii come from here, never from literals in widgets: the canvas,
//! the panels, run states, the engine banner and the Watched panel's charts all read a
//! [`Tokens`].
//!
//! Lifted from the research software's `crates/ui-egui/src/theme.rs`. The research software's table of VI families is
//! generalised: a node's colours come from its family as the engine's catalogue names it
//! ([`Tokens::family_tint`]): the colour of a wire type of the same id when the engine gives one,
//! otherwise a hue derived from the family id, stable across runs and machines.

use crate::craft_ui::theme::WidgetPalette;
use egui::{Color32, CornerRadius, Stroke};
use plumcraft_proto::catalogue::SchemaReply;
use plumcraft_proto::event::NodeState;

/// Nothing drawn (hidden pins, clear header frames, snarl's hidden selection).
pub const CLEAR: Color32 = Color32::TRANSPARENT;

/// A theme: PlumCraft's colours and radii. [`MEDIUM`] is the default (PhotoCraft's default
/// brightness); [`DARK`] is the research software's original dark one. View › Theme switches.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tokens {
    /// The theme's id (`medium`, `dark`).
    pub name: &'static str,
    /// Its menu label.
    pub label: &'static str,
    pub canvas: Color32,
    pub panel: Color32,
    pub text: Color32,
    /// Secondary text: docs, captions. Still readable at body size (over 4.5:1 on the panel).
    pub text_dim: Color32,
    /// Tertiary text: type tags and ids, never for anything that must be read.
    pub text_faint: Color32,
    /// Hairlines between sections and rows.
    pub separator: Color32,
    /// Text fields, dropdowns and other inputs, with their border.
    pub field: Color32,
    pub field_border: Color32,
    /// A section header's band in a panel.
    pub section: Color32,
    pub accent: Color32,
    pub error: Color32,
    /// Something to look at that isn't an error (an engine connecting, a run pending).
    pub warning: Color32,
    /// Background of the engine banner.
    pub banner: Color32,
    pub widget: WidgetPalette,
    pub radius: u8,
    /// Pin colour for a wire type the engine gave no colour.
    pub pin_default: Color32,
    /// Fill of a widget-only input's pin (it can't be wired, so it isn't drawn).
    pub pin_hidden: Color32,
    /// Node borders and badges by run state.
    pub state_queued: Color32,
    pub state_running: Color32,
    pub state_done: Color32,
    /// Cached results: as done, dimmer.
    pub state_cached: Color32,
    pub state_error: Color32,
    pub state_skipped: Color32,
    pub state_cancelled: Color32,
    /// A result an edit has made out of date.
    pub stale: Color32,
    /// A node revealed or watched.
    pub highlight: Color32,
    /// Behind a thumbnail that can't be shown.
    pub placeholder: Color32,
    /// The tint over a stale or failed thumbnail.
    pub faded: Color32,
    /// The Watched panel's plot and table bars.
    pub chart: Color32,
    /// Audio waveforms (node thumbnails and the Watched panel), drawn on `waveform_bg`.
    pub waveform: Color32,
    pub waveform_bg: Color32,
    /// The playback position over a waveform.
    pub playhead: Color32,
    /// The spectrogram's colour ramp, quiet to loud (evenly spaced stops).
    pub spectrogram: [Color32; 6],
    /// A node without a family (or of one the theme can't colour).
    pub family_default: FamilyTint,
    /// Groups: a collapsed group's node, and an open group's frame.
    pub group: FamilyTint,
    /// How much of a family's hue goes into a node's header and its body (0..=1).
    pub family_header_mix: f32,
    pub family_body_mix: f32,
    /// The saturation and value of a hue derived from a family id.
    pub family_saturation: f32,
    pub family_value: f32,
}

/// A node's colours: its header band and its body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FamilyTint {
    pub header: Color32,
    pub body: Color32,
}

const fn tint(header: [u8; 3], body: [u8; 3]) -> FamilyTint {
    FamilyTint { header: Color32::from_rgb(header[0], header[1], header[2]), body: Color32::from_rgb(body[0], body[1], body[2]) }
}

pub const DARK: Tokens = Tokens {
    name: "dark",
    label: "Dark",
    canvas: Color32::from_rgb(0x1b, 0x1d, 0x21),
    panel: Color32::from_rgb(0x25, 0x28, 0x2d),
    text: Color32::from_rgb(0xdc, 0xde, 0xe2),
    text_dim: Color32::from_rgb(0xa6, 0xaa, 0xb0),
    text_faint: Color32::from_rgb(0x80, 0x85, 0x8c),
    separator: Color32::from_rgb(0x15, 0x17, 0x1a),
    field: Color32::from_rgb(0x1f, 0x21, 0x25),
    field_border: Color32::from_rgb(0x44, 0x49, 0x52),
    section: Color32::from_rgb(0x2e, 0x31, 0x37),
    accent: Color32::from_rgb(0x4f, 0x8f, 0xd8),
    error: Color32::from_rgb(0xe0, 0x6c, 0x5f),
    warning: Color32::from_rgb(0xd0, 0x9a, 0x3c),
    banner: Color32::from_rgb(0x6b, 0x2e, 0x28),
    widget: WidgetPalette {
        noninteractive: Color32::from_rgb(0x25, 0x28, 0x2d),
        inactive: Color32::from_rgb(0x32, 0x36, 0x3c),
        hovered: Color32::from_rgb(0x3d, 0x42, 0x4a),
        active: Color32::from_rgb(0x48, 0x4e, 0x57),
        open: Color32::from_rgb(0x3d, 0x42, 0x4a),
    },
    radius: 4,
    pin_default: Color32::from_rgb(0x9a, 0x9a, 0x9a),
    pin_hidden: CLEAR,
    state_queued: Color32::from_rgb(0x6a, 0x70, 0x7a),
    state_running: Color32::from_rgb(0x4f, 0x8f, 0xd8),
    state_done: Color32::from_rgb(0x5f, 0xb0, 0x6e),
    state_cached: Color32::from_rgb(0x3f, 0x6e, 0x49),
    state_error: Color32::from_rgb(0xe0, 0x6c, 0x5f),
    state_skipped: Color32::from_rgb(0x55, 0x59, 0x60),
    state_cancelled: Color32::from_rgb(0x55, 0x59, 0x60),
    stale: Color32::from_rgb(0xd0, 0x9a, 0x3c),
    highlight: Color32::from_rgb(0xe8, 0xd4, 0x4d),
    placeholder: Color32::from_rgb(0x2c, 0x2f, 0x35),
    faded: Color32::from_rgba_premultiplied(110, 110, 110, 110),
    chart: Color32::from_rgb(0x4f, 0x8f, 0xd8),
    waveform: Color32::from_rgb(0x3a, 0xb8, 0xa6),
    waveform_bg: Color32::from_rgb(0x14, 0x16, 0x19),
    playhead: Color32::from_rgb(0xe8, 0xd4, 0x4d),
    spectrogram: [
        Color32::from_rgb(0x00, 0x00, 0x04),
        Color32::from_rgb(0x3b, 0x0f, 0x70),
        Color32::from_rgb(0x8c, 0x29, 0x81),
        Color32::from_rgb(0xde, 0x49, 0x68),
        Color32::from_rgb(0xfe, 0x9f, 0x6d),
        Color32::from_rgb(0xfc, 0xfd, 0xbf),
    ],
    family_default: tint([0x36, 0x3a, 0x41], [0x25, 0x28, 0x2d]),
    group: tint([0x6a, 0x36, 0x5a], [0x2b, 0x24, 0x2a]),
    family_header_mix: 0.55,
    family_body_mix: 0.10,
    family_saturation: 0.55,
    family_value: 0.62,
};

/// PhotoCraft's default brightness (Photoshop's second level: #535353 panels, #282828 canvas),
/// with the same text greys. The default.
pub const MEDIUM: Tokens = Tokens {
    name: "medium",
    label: "Medium",
    canvas: Color32::from_rgb(40, 40, 40),
    panel: Color32::from_rgb(83, 83, 83),
    text: Color32::from_rgb(238, 238, 238),
    text_dim: Color32::from_rgb(212, 212, 212),
    text_faint: Color32::from_rgb(170, 170, 170),
    separator: Color32::from_rgb(62, 62, 62),
    field: Color32::from_rgb(69, 69, 69),
    field_border: Color32::from_rgb(104, 104, 104),
    section: Color32::from_rgb(74, 74, 74),
    accent: Color32::from_rgb(90, 160, 245),
    error: Color32::from_rgb(240, 110, 110),
    warning: Color32::from_rgb(0xe0, 0xa8, 0x48),
    banner: Color32::from_rgb(0x8a, 0x3a, 0x33),
    widget: WidgetPalette {
        noninteractive: Color32::from_rgb(83, 83, 83),
        inactive: Color32::from_rgb(69, 69, 69),
        hovered: Color32::from_rgb(98, 98, 98),
        active: Color32::from_rgb(112, 112, 112),
        open: Color32::from_rgb(98, 98, 98),
    },
    radius: 4,
    pin_default: Color32::from_rgb(170, 170, 170),
    pin_hidden: CLEAR,
    state_queued: Color32::from_rgb(0x8a, 0x90, 0x9a),
    state_running: Color32::from_rgb(90, 160, 245),
    state_done: Color32::from_rgb(0x6f, 0xc4, 0x7e),
    state_cached: Color32::from_rgb(0x4f, 0x8a, 0x5b),
    state_error: Color32::from_rgb(240, 110, 110),
    state_skipped: Color32::from_rgb(0x70, 0x74, 0x7a),
    state_cancelled: Color32::from_rgb(0x70, 0x74, 0x7a),
    stale: Color32::from_rgb(0xe0, 0xa8, 0x48),
    highlight: Color32::from_rgb(0xf0, 0xdc, 0x58),
    placeholder: Color32::from_rgb(60, 60, 60),
    faded: Color32::from_rgba_premultiplied(110, 110, 110, 110),
    chart: Color32::from_rgb(90, 160, 245),
    waveform: Color32::from_rgb(0x4c, 0xd0, 0xbc),
    waveform_bg: Color32::from_rgb(30, 30, 30),
    playhead: Color32::from_rgb(0xf0, 0xdc, 0x58),
    spectrogram: [
        Color32::from_rgb(0x00, 0x00, 0x04),
        Color32::from_rgb(0x3b, 0x0f, 0x70),
        Color32::from_rgb(0x8c, 0x29, 0x81),
        Color32::from_rgb(0xde, 0x49, 0x68),
        Color32::from_rgb(0xfe, 0x9f, 0x6d),
        Color32::from_rgb(0xfc, 0xfd, 0xbf),
    ],
    family_default: tint([0x44, 0x48, 0x50], [0x53, 0x53, 0x53]),
    group: tint([0x84, 0x46, 0x70], [0x5e, 0x52, 0x5a]),
    family_header_mix: 0.6,
    family_body_mix: 0.10,
    family_saturation: 0.5,
    family_value: 0.62,
};

/// The default theme.
pub const DEFAULT: Tokens = MEDIUM;

/// Every theme, in menu order.
pub const ALL: &[Tokens] = &[MEDIUM, DARK];

/// The theme named `name`.
pub fn by_name(name: &str) -> Option<Tokens> {
    ALL.iter().copied().find(|t| t.name == name)
}

/// `a` moved `t` of the way to `b` (each channel; alpha from `a`).
fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
    let ch = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgba_unmultiplied(ch(a.r(), b.r()), ch(a.g(), b.g()), ch(a.b(), b.b()), a.a())
}

/// A stable hue (degrees / 360) for an id: FNV-1a, so it is the same on every machine and
/// every run (std's hasher promises neither).
pub fn hue_of(id: &str) -> f32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in id.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    (h % 360) as f32 / 360.0
}

impl Tokens {
    /// The colours of a node of `family`: the colour of the catalogue's wire type with the
    /// family's id when the engine gives one, else a hue from the id; mixed into
    /// [`Tokens::family_default`] so text stays readable. No family: the default.
    pub fn family_tint(&self, catalog: &SchemaReply, family: &str) -> FamilyTint {
        if family.is_empty() {
            return self.family_default;
        }
        let engine = catalog.wire_types.iter().find(|w| w.id.eq_ignore_ascii_case(family)).and_then(|w| w.color.as_deref()).and_then(parse_hex);
        let hue = engine.unwrap_or_else(|| egui::ecolor::HsvaGamma { h: hue_of(family), s: self.family_saturation, v: self.family_value, a: 1.0 }.into());
        FamilyTint { header: mix(self.family_default.header, hue, self.family_header_mix), body: mix(self.family_default.body, hue, self.family_body_mix) }
    }

    /// The spectrogram ramp at `t` (0 quiet ..= 1 loud), between its stops.
    pub fn ramp(&self, t: f32) -> Color32 {
        let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
        let n = self.spectrogram.len() - 1;
        let x = t * n as f32;
        let i = (x.floor() as usize).min(n.saturating_sub(1));
        match (self.spectrogram.get(i), self.spectrogram.get(i + 1)) {
            (Some(a), Some(b)) => mix(*a, *b, x - i as f32),
            _ => self.waveform_bg,
        }
    }

    /// The colour of a run state (a node's border and badge); `None` for none.
    pub fn state_color(&self, state: NodeState) -> Option<Color32> {
        match state {
            NodeState::Queued => Some(self.state_queued),
            NodeState::Started | NodeState::Progress | NodeState::Preview => Some(self.state_running),
            NodeState::Done => Some(self.state_done),
            NodeState::Cached => Some(self.state_cached),
            NodeState::Error => Some(self.state_error),
            NodeState::Skipped => Some(self.state_skipped),
            NodeState::Cancelled => Some(self.state_cancelled),
            NodeState::Unknown => None,
        }
    }

    /// Install these tokens as the context's visuals.
    pub fn apply(&self, ctx: &egui::Context) {
        let mut v = egui::Visuals::dark();
        v.panel_fill = self.panel;
        v.window_fill = self.panel;
        v.window_stroke = Stroke::new(1.0, self.separator);
        v.extreme_bg_color = self.field;
        v.faint_bg_color = self.section;
        v.code_bg_color = self.field;
        v.override_text_color = Some(self.text);
        v.selection.bg_fill = self.accent;
        v.selection.stroke = Stroke::new(1.0, self.text);
        v.hyperlink_color = self.accent;
        v.error_fg_color = self.error;
        v.warn_fg_color = self.warning;
        v.slider_trailing_fill = true;
        self.widget.apply_to(&mut v.widgets, CornerRadius::same(self.radius), Stroke::new(1.0, self.text));
        // Borders: hairlines between rows, a visible edge on every input.
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, self.separator);
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, self.text_dim);
        for w in [&mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.open] {
            w.bg_stroke = Stroke::new(1.0, self.field_border);
        }
        v.widgets.active.bg_stroke = Stroke::new(1.0, self.accent);
        // PlumCraft draws its own palette whatever the system's mode (eframe would otherwise
        // swap in egui's light visuals).
        ctx.set_theme(egui::ThemePreference::Dark);
        ctx.set_visuals_of(egui::Theme::Dark, v);
        ctx.style_mut_of(egui::Theme::Dark, |s| {
            // Small text is for tags, not for anything to read: 10.5 pt, as PhotoCraft, not
            // egui's 9.
            s.text_styles.insert(egui::TextStyle::Small, egui::FontId::proportional(10.5));
            s.text_styles.insert(egui::TextStyle::Heading, egui::FontId::proportional(15.0));
            s.spacing.slider_width = 150.0;
            s.spacing.tooltip_width = 320.0;
        });
    }
}

/// Parse an engine colour (`#rrggbb`).
pub fn parse_hex(s: &str) -> Option<Color32> {
    let hex = s.strip_prefix('#')?;
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    Some(Color32::from_rgb(byte(0)?, byte(2)?, byte(4)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use plumcraft_proto::catalogue::WireType;

    #[test]
    fn families_have_their_own_stable_colours() {
        let c = SchemaReply::default();
        for t in ALL {
            assert_ne!(t.family_tint(&c, "source"), t.family_tint(&c, "output"));
            assert_eq!(t.family_tint(&c, ""), t.family_default);
            assert_eq!(t.family_tint(&c, "text"), t.family_tint(&c, "text"), "the same id, the same colour");
        }
        // FNV-1a pinned: the hue never changes between runs, machines or Rust versions.
        assert_eq!(hue_of(""), (0x811c_9dc5u32 % 360) as f32 / 360.0);
        assert!((0.0..1.0).contains(&hue_of("text")));
    }

    #[test]
    fn an_engine_colour_wins() {
        let mut c = SchemaReply::default();
        c.wire_types.push(WireType { id: "TEXT".into(), color: Some("#c9a227".into()), ..Default::default() });
        let t = DARK;
        let from_engine = t.family_tint(&c, "text");
        assert_ne!(from_engine, t.family_tint(&SchemaReply::default(), "text"));
        assert_eq!(from_engine.header, mix(t.family_default.header, Color32::from_rgb(0xc9, 0xa2, 0x27), t.family_header_mix));
    }

    #[test]
    fn themes_are_named_and_readable() {
        assert_eq!(by_name("medium").map(|t| t.label), Some("Medium"));
        assert_eq!(by_name("dark").map(|t| t.label), Some("Dark"));
        assert_eq!(by_name("neon"), None);
        assert_eq!(DEFAULT.name, "medium");
        let c = SchemaReply::default();
        // body and secondary text keep WCAG's 4.5:1 on the panel in every theme, and text on
        // any family's body and header
        for t in ALL {
            for (what, col) in [("text", t.text), ("text_dim", t.text_dim)] {
                let r = contrast(col, t.panel);
                assert!(r >= 4.5, "{}: {what} on the panel is {r:.1}:1", t.name);
            }
            for fam in ["", "text", "source", "output", "audio", "image", "a family with a long name", "zz"] {
                let f = t.family_tint(&c, fam);
                assert!(contrast(t.text, f.body) >= 4.5, "{}: text on a {fam:?} body is {:.1}:1", t.name, contrast(t.text, f.body));
                assert!(contrast(t.text, f.header) >= 3.0, "{}: text on a {fam:?} header is {:.1}:1", t.name, contrast(t.text, f.header));
            }
        }
    }

    #[test]
    fn waveforms_and_spectrograms_are_readable() {
        for t in ALL {
            // a waveform and its playhead stand out from the waveform's ground, and the ground
            // from the panel it sits on is a visible box
            let r = contrast(t.waveform, t.waveform_bg);
            assert!(r >= 4.5, "{}: waveform on its ground is {r:.1}:1", t.name);
            let r = contrast(t.playhead, t.waveform_bg);
            assert!(r >= 4.5, "{}: playhead on the ground is {r:.1}:1", t.name);
            let r = contrast(t.playhead, t.waveform);
            assert!(r >= 1.3, "{}: playhead over the waveform is {r:.1}:1", t.name);
            // the ramp goes from dark to light, each stop lighter than the last
            let ramp = t.spectrogram;
            assert!(contrast(ramp[0], ramp[5]) >= 10.0, "{}: the spectrogram ramp is flat", t.name);
            for w in ramp.windows(2) {
                assert!(lum(w[1]) > lum(w[0]), "{}: {:?} then {:?}", t.name, w[0], w[1]);
            }
            assert_eq!(t.ramp(0.0), ramp[0]);
            assert_eq!(t.ramp(1.0), ramp[5]);
            assert_eq!(t.ramp(f32::NAN), ramp[0]);
            assert_eq!(t.ramp(0.2), ramp[1]);
        }
    }

    #[test]
    fn every_state_but_unknown_has_a_colour() {
        for s in [NodeState::Queued, NodeState::Started, NodeState::Done, NodeState::Cached, NodeState::Error, NodeState::Skipped, NodeState::Cancelled] {
            assert!(DEFAULT.state_color(s).is_some(), "{s:?}");
        }
        assert_eq!(DEFAULT.state_color(NodeState::Unknown), None);
    }

    /// WCAG 2 relative luminance.
    fn lum(c: Color32) -> f64 {
        let ch = |v: u8| {
            let v = v as f64 / 255.0;
            if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
    }

    /// WCAG 2 contrast ratio.
    fn contrast(a: Color32, b: Color32) -> f64 {
        let (l1, l2) = (lum(a).max(lum(b)), lum(a).min(lum(b)));
        (l1 + 0.05) / (l2 + 0.05)
    }

    #[test]
    fn hex_colours() {
        assert_eq!(parse_hex("#4f8fd8"), Some(Color32::from_rgb(0x4f, 0x8f, 0xd8)));
        for bad in ["", "#", "4f8fd8", "#4f8fd", "#4f8fdz", "#ééé", "#4f8fd8ff"] {
            assert_eq!(parse_hex(bad), None, "{bad}");
        }
    }
}
