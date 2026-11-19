//! Theme tokens in egui's colours. Colours, radii and type sizes come from here, never from
//! literals in widgets: the canvas, the panels, run states, the engine banner and the Watched
//! panel's charts all read a [`Tokens`].
//!
//! The themes, their values and every computation on them (fruits, family tints, contrast
//! checks, OKLCH) are the core's, as data ([`plumcraft_editor_core::theme`], stage N1). This
//! module is the egui shell's side: the same [`Tokens`] with `Color32` fields, as code built on
//! `plumcraft_ui` has always read them, the conversions both ways, and [`Tokens::apply`], which
//! installs them on an egui context. Each method here answers what the core's answers.

use std::collections::BTreeMap;

use crate::craft_ui::theme::WidgetPalette;
use crate::plain::{ToCore, ToEgui};
use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke};
use plumcraft_editor_core::plain::Color;
use plumcraft_editor_core::theme as core;
use plumcraft_proto::catalogue::SchemaReply;
use plumcraft_proto::event::NodeState;

pub use plumcraft_editor_core::theme::{BASE_TEXT, Check, FRUITS, OLIVE, PLUM, TEXT_SIZES, TOKENS, Text, ZOOMS, fnv1a, fruit_index, fruit_slots};

/// Nothing drawn (hidden pins, clear header frames, snarl's hidden selection).
pub const CLEAR: Color32 = Color32::TRANSPARENT;

/// A theme: PlumCraft's colours, radii and text size. [`PAPER`] is the default. Settings ›
/// Appearance (or `view.theme`) switches, and the user's overrides are applied on top
/// ([`crate::appearance::Appearance::resolve`]). The values and the logic are the core's
/// ([`core::Tokens`]); this is them in egui's colours, with what installs them on a context.
#[derive(Debug, Clone, PartialEq)]
pub struct Tokens {
    /// The theme's id (`paper`, `pewter`, `dusk`).
    pub name: &'static str,
    /// Its label.
    pub label: &'static str,
    /// A dark theme: egui starts from its dark visuals (shadows, the defaults this doesn't set).
    pub dark: bool,
    /// The node canvas.
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
    /// A node without a family.
    pub family_default: FamilyTint,
    /// Groups: a collapsed group's node, and an open group's frame (plum).
    pub group: FamilyTint,
    /// How much of a family's header colour goes into its body (0..=1).
    pub family_body_mix: f32,
    /// Text on a node header: its title, icons and status.
    pub header_text: Color32,
    /// A wire whose type has no colour.
    pub wire: Color32,
    /// The canvas grid.
    pub grid: Color32,
    /// The family fruits' header colours, in [`FRUITS`] order.
    pub fruits: [Color32; 12],
    /// The quiet fruit's header colour.
    pub olive: Color32,
    /// The fruits' OKLCH lightness, the tier step and the chroma: an engine's colour keeps its
    /// hue and takes these.
    pub fruit_l: f32,
    pub fruit_dl: f32,
    pub fruit_c: f32,
    /// The body text size in points; the others are derived from it ([`Tokens::font`]).
    pub text_size: f32,
    /// Families the user pinned to a colour (header), by family id.
    pub pins: BTreeMap<String, Color32>,
    /// Inter SemiBold is installed on the context ([`crate::fonts::install`]): titles and
    /// headings use it. Without it they are the text face (egui panics on a family it has no
    /// fonts for, so this is only set once the fonts are known to be there).
    pub semibold: bool,
}

/// A node's colours: its header band and its body.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FamilyTint {
    pub header: Color32,
    pub body: Color32,
}

/// Warm light, like the light flow tools (GDE, Node-RED, KNIME): ink on off-white, pastel fruit
/// headers with dark titles. The default ([`core::PAPER`]).
pub const PAPER: Tokens = Tokens::from_theme(core::PAPER);
/// Mid grey, for image work on a neutral ground ([`core::PEWTER`]).
pub const PEWTER: Tokens = Tokens::from_theme(core::PEWTER);
/// Dark ([`core::DUSK`]).
pub const DUSK: Tokens = Tokens::from_theme(core::DUSK);

/// The theme a new editor starts with.
pub const DEFAULT: Tokens = PAPER;

/// Every theme, in the Settings window's order.
pub const ALL: [Tokens; 3] = [PAPER, PEWTER, DUSK];

/// The theme named `name` (or once called `name`: `medium` is Pewter, `dark` is Dusk).
pub fn by_name(name: &str) -> Option<Tokens> {
    core::by_name(name).map(Tokens::from)
}

const fn c32(c: Color) -> Color32 {
    Color32::from_rgba_premultiplied(c.r, c.g, c.b, c.a)
}

const fn cc(c: Color32) -> Color {
    let [r, g, b, a] = c.to_array();
    Color::from_rgba_premultiplied(r, g, b, a)
}

const fn arr32<const N: usize>(a: [Color; N]) -> [Color32; N] {
    let mut out = [CLEAR; N];
    let mut i = 0;
    while i < N {
        out[i] = c32(a[i]);
        i += 1;
    }
    out
}

const fn arrc<const N: usize>(a: [Color32; N]) -> [Color; N] {
    let mut out = [Color::TRANSPARENT; N];
    let mut i = 0;
    while i < N {
        out[i] = cc(a[i]);
        i += 1;
    }
    out
}

const fn tint32(t: core::FamilyTint) -> FamilyTint {
    FamilyTint { header: c32(t.header), body: c32(t.body) }
}

const fn tintc(t: FamilyTint) -> core::FamilyTint {
    core::FamilyTint { header: cc(t.header), body: cc(t.body) }
}

const fn widgets32(w: core::WidgetPalette) -> WidgetPalette {
    WidgetPalette { noninteractive: c32(w.noninteractive), inactive: c32(w.inactive), hovered: c32(w.hovered), active: c32(w.active), open: c32(w.open) }
}

const fn widgetsc(w: WidgetPalette) -> core::WidgetPalette {
    core::WidgetPalette { noninteractive: cc(w.noninteractive), inactive: cc(w.inactive), hovered: cc(w.hovered), active: cc(w.active), open: cc(w.open) }
}

impl From<core::Tokens> for Tokens {
    fn from(mut t: core::Tokens) -> Tokens {
        let pins = std::mem::take(&mut t.pins).into_iter().map(|(k, v)| (k, v.to_egui())).collect();
        Tokens { pins, ..Tokens::from_theme(t) }
    }
}

impl From<&Tokens> for core::Tokens {
    fn from(t: &Tokens) -> core::Tokens {
        core::Tokens { pins: t.pins.iter().map(|(k, v)| (k.clone(), v.to_core())).collect(), ..t.unpinned() }
    }
}

impl From<FamilyTint> for core::FamilyTint {
    fn from(t: FamilyTint) -> core::FamilyTint {
        tintc(t)
    }
}

impl From<core::FamilyTint> for FamilyTint {
    fn from(t: core::FamilyTint) -> FamilyTint {
        tint32(t)
    }
}

/// The token table: `token` reads one by name, `token_mut` changes it.
macro_rules! token_table {
    ($($name:literal => $($field:ident).+),* $(,)?) => {
        /// Token `name`'s colour ([`TOKENS`]).
        pub fn token(&self, name: &str) -> Option<Color32> {
            Some(match name {
                $($name => self.$($field).+,)*
                _ => return None,
            })
        }

        /// Token `name`, to change.
        pub fn token_mut(&mut self, name: &str) -> Option<&mut Color32> {
            Some(match name {
                $($name => &mut self.$($field).+,)*
                _ => return None,
            })
        }
    };
}

impl Tokens {
    /// `t` in egui's colours, for the constants. Its pins must be empty (a built-in theme's
    /// are): a const fn can't drop a map, so `t` is forgotten, not dropped.
    const fn from_theme(t: core::Tokens) -> Tokens {
        let out = Tokens {
            name: t.name,
            label: t.label,
            dark: t.dark,
            radius: t.radius,
            family_body_mix: t.family_body_mix,
            fruit_l: t.fruit_l,
            fruit_dl: t.fruit_dl,
            fruit_c: t.fruit_c,
            text_size: t.text_size,
            semibold: t.semibold,
            canvas: c32(t.canvas),
            panel: c32(t.panel),
            text: c32(t.text),
            text_dim: c32(t.text_dim),
            text_faint: c32(t.text_faint),
            separator: c32(t.separator),
            field: c32(t.field),
            field_border: c32(t.field_border),
            section: c32(t.section),
            accent: c32(t.accent),
            error: c32(t.error),
            warning: c32(t.warning),
            banner: c32(t.banner),
            pin_default: c32(t.pin_default),
            pin_hidden: c32(t.pin_hidden),
            state_queued: c32(t.state_queued),
            state_running: c32(t.state_running),
            state_done: c32(t.state_done),
            state_cached: c32(t.state_cached),
            state_error: c32(t.state_error),
            state_skipped: c32(t.state_skipped),
            state_cancelled: c32(t.state_cancelled),
            stale: c32(t.stale),
            highlight: c32(t.highlight),
            placeholder: c32(t.placeholder),
            faded: c32(t.faded),
            chart: c32(t.chart),
            waveform: c32(t.waveform),
            waveform_bg: c32(t.waveform_bg),
            playhead: c32(t.playhead),
            header_text: c32(t.header_text),
            wire: c32(t.wire),
            grid: c32(t.grid),
            olive: c32(t.olive),
            spectrogram: arr32(t.spectrogram),
            fruits: arr32(t.fruits),
            family_default: tint32(t.family_default),
            group: tint32(t.group),
            widget: widgets32(t.widget),
            pins: BTreeMap::new(),
        };
        std::mem::forget(t);
        out
    }

    /// These tokens as the core's, without the pins: what every method but
    /// [`Tokens::family_tint`] reads, and cheap (no allocation).
    fn unpinned(&self) -> core::Tokens {
        core::Tokens {
            name: self.name,
            label: self.label,
            dark: self.dark,
            radius: self.radius,
            family_body_mix: self.family_body_mix,
            fruit_l: self.fruit_l,
            fruit_dl: self.fruit_dl,
            fruit_c: self.fruit_c,
            text_size: self.text_size,
            semibold: self.semibold,
            canvas: cc(self.canvas),
            panel: cc(self.panel),
            text: cc(self.text),
            text_dim: cc(self.text_dim),
            text_faint: cc(self.text_faint),
            separator: cc(self.separator),
            field: cc(self.field),
            field_border: cc(self.field_border),
            section: cc(self.section),
            accent: cc(self.accent),
            error: cc(self.error),
            warning: cc(self.warning),
            banner: cc(self.banner),
            pin_default: cc(self.pin_default),
            pin_hidden: cc(self.pin_hidden),
            state_queued: cc(self.state_queued),
            state_running: cc(self.state_running),
            state_done: cc(self.state_done),
            state_cached: cc(self.state_cached),
            state_error: cc(self.state_error),
            state_skipped: cc(self.state_skipped),
            state_cancelled: cc(self.state_cancelled),
            stale: cc(self.stale),
            highlight: cc(self.highlight),
            placeholder: cc(self.placeholder),
            faded: cc(self.faded),
            chart: cc(self.chart),
            waveform: cc(self.waveform),
            waveform_bg: cc(self.waveform_bg),
            playhead: cc(self.playhead),
            header_text: cc(self.header_text),
            wire: cc(self.wire),
            grid: cc(self.grid),
            olive: cc(self.olive),
            spectrogram: arrc(self.spectrogram),
            fruits: arrc(self.fruits),
            family_default: tintc(self.family_default),
            group: tintc(self.group),
            widget: widgetsc(self.widget),
            pins: BTreeMap::new(),
        }
    }

    /// Change these tokens through the core's, keeping the pins.
    fn through_core<R>(&mut self, f: impl FnOnce(&mut core::Tokens) -> R) -> R {
        let mut t = self.unpinned();
        let r = f(&mut t);
        let pins = std::mem::take(&mut self.pins);
        *self = Tokens { pins, ..Tokens::from_theme(t) };
        r
    }

    /// The header colour of fruit `name` (the twelve, olive, or plum).
    pub fn fruit(&self, name: &str) -> Option<Color32> {
        self.unpinned().fruit(name).map(ToEgui::to_egui)
    }

    /// Set fruit `name`'s header colour (plum is the groups' header, and their body follows).
    pub fn set_fruit(&mut self, name: &str, c: Color32) -> bool {
        self.through_core(|t| t.set_fruit(name, c.to_core()))
    }

    /// The fruit `family` takes in `catalog` (by hash and probe), ignoring pins and engine
    /// colours. `None` for no family.
    pub fn family_fruit(&self, catalog: &SchemaReply, family: &str) -> Option<&'static str> {
        self.unpinned().family_fruit(catalog, family)
    }

    /// The colours of a node of `family`: the user's pin, else the hue of the catalogue's wire
    /// type with the family's id (when the engine colours it) at the theme's fruit lightness and
    /// chroma, else its fruit. No family: the default.
    pub fn family_tint(&self, catalog: &SchemaReply, family: &str) -> FamilyTint {
        self.unpinned().family_tint_pinned(self.pins.get(family).map(|c| c.to_core()), catalog, family).into()
    }

    /// The fruit set recomputed from the theme's `fruit_l`, `fruit_dl` and `fruit_c` (the
    /// hard-coded tables are these, rounded): what the palette sliders give.
    pub fn computed_fruits(&self) -> [Color32; 12] {
        arr32(self.unpinned().computed_fruits())
    }

    /// The spectrogram ramp at `t` (0 quiet ..= 1 loud), between its stops.
    pub fn ramp(&self, t: f32) -> Color32 {
        core::ramp(&arrc(self.spectrogram), cc(self.waveform_bg), t).to_egui()
    }

    /// The colour of a run state (a node's border and badge); `None` for none.
    pub fn state_color(&self, state: NodeState) -> Option<Color32> {
        self.unpinned().state_color(state).map(ToEgui::to_egui)
    }

    /// A size scaled from the 14 pt base to this theme's text size.
    fn size(&self, at_base: f32) -> f32 {
        self.unpinned().size(at_base)
    }

    /// The SemiBold family when it is installed, else the text face.
    fn bold_family(&self) -> FontFamily {
        if self.semibold { FontFamily::Name(crate::fonts::SEMIBOLD.into()) } else { FontFamily::Proportional }
    }

    /// The font of one of PlumCraft's own text styles.
    pub fn font(&self, text: Text) -> FontId {
        let size = self.unpinned().text_size_of(text);
        match text {
            Text::Title => FontId::new(size, self.bold_family()),
            Text::Port | Text::Caption | Text::Small => FontId::proportional(size),
            Text::Tag => FontId::monospace(size),
        }
    }

    /// Install these tokens as the context's visuals and text styles. Light themes start from
    /// egui's light visuals, dark ones from its dark ones, and both of egui's themes get them, so
    /// the system's light or dark mode never swaps in another palette.
    pub fn apply(&self, ctx: &egui::Context) {
        let mut v = if self.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        v.panel_fill = self.panel;
        v.window_fill = self.panel;
        v.window_stroke = Stroke::new(1.0, self.separator);
        v.extreme_bg_color = self.field;
        v.faint_bg_color = self.section;
        v.code_bg_color = self.field;
        v.override_text_color = Some(self.text);
        v.weak_text_color = Some(self.text_dim);
        v.selection.bg_fill = if self.dark { self.accent.gamma_multiply(0.45) } else { mix(self.panel, self.accent, 0.3) };
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
        let theme = if self.dark { egui::Theme::Dark } else { egui::Theme::Light };
        ctx.set_theme(if self.dark { egui::ThemePreference::Dark } else { egui::ThemePreference::Light });
        for t in [egui::Theme::Dark, egui::Theme::Light] {
            ctx.set_visuals_of(t, v.clone());
            ctx.style_mut_of(t, |s| {
                s.text_styles.insert(egui::TextStyle::Body, FontId::proportional(self.size(14.0)));
                s.text_styles.insert(egui::TextStyle::Button, FontId::proportional(self.size(14.0)));
                s.text_styles.insert(egui::TextStyle::Small, FontId::proportional(self.size(12.0)));
                s.text_styles.insert(egui::TextStyle::Heading, FontId::new(self.size(17.0), self.bold_family()));
                s.text_styles.insert(egui::TextStyle::Monospace, FontId::monospace(self.size(13.0)));
                s.spacing.slider_width = 150.0;
                s.spacing.tooltip_width = 320.0;
            });
        }
        let _ = theme;
    }

    /// The contrast checks a theme must pass (WCAG 2): text 4.5:1, secondary text, borders,
    /// rings, wires and pins 3:1. The unit tests and the Settings window's badges both use these.
    pub fn checks(&self) -> Vec<Check> {
        self.unpinned().checks()
    }

    /// The checks with `token` on either side.
    pub fn checks_of(&self, token: &str) -> Vec<Check> {
        self.unpinned().checks_of(token)
    }

    token_table! {
    "canvas" => canvas,
    "panel" => panel,
    "section" => section,
    "field" => field,
    "separator" => separator,
    "banner" => banner,
    "placeholder" => placeholder,
    "text" => text,
    "text_dim" => text_dim,
    "text_faint" => text_faint,
    "header_text" => header_text,
    "accent" => accent,
    "error" => error,
    "warning" => warning,
    "field_border" => field_border,
    "widget_inactive" => widget.inactive,
    "widget_hovered" => widget.hovered,
    "widget_active" => widget.active,
    "state_queued" => state_queued,
    "state_running" => state_running,
    "state_done" => state_done,
    "state_cached" => state_cached,
    "state_error" => state_error,
    "state_skipped" => state_skipped,
    "state_cancelled" => state_cancelled,
    "stale" => stale,
    "highlight" => highlight,
    "chart" => chart,
    "waveform" => waveform,
    "waveform_bg" => waveform_bg,
    "playhead" => playhead,
    "wire" => wire,
    "grid" => grid,
    "pin_default" => pin_default,
    "default_header" => family_default.header,
    "default_body" => family_default.body,
    }
}

/// `a` moved `t` of the way to `b` (each channel; alpha from `a`).
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    core::mix(a.to_core(), b.to_core(), t).to_egui()
}

/// Parse a colour (`#rrggbb`).
pub fn parse_hex(s: &str) -> Option<Color32> {
    core::parse_hex(s).map(ToEgui::to_egui)
}

/// A colour as `#rrggbb`.
pub fn hex(c: Color32) -> String {
    core::hex(c.to_core())
}

/// WCAG 2 relative luminance.
pub fn luminance(c: Color32) -> f64 {
    core::luminance(c.to_core())
}

/// WCAG 2 contrast ratio (1 ..= 21).
pub fn contrast(a: Color32, b: Color32) -> f64 {
    core::contrast(a.to_core(), b.to_core())
}

/// OKLab (Björn Ottosson, 2020) of an sRGB colour.
pub fn oklab(c: Color32) -> [f64; 3] {
    core::oklab(c.to_core())
}

/// OKLCH (lightness, chroma, hue in degrees) of an sRGB colour.
pub fn oklch(c: Color32) -> (f32, f32, f32) {
    core::oklch(c.to_core())
}

/// The sRGB colour at OKLCH (`l`, `c`, `h` degrees), its chroma reduced until it is in gamut.
pub fn from_oklch(l: f32, c: f32, h: f32) -> Color32 {
    core::from_oklch(l, c, h).to_egui()
}

/// The OKLab distance between two colours, ×100 (about 2 is just noticeable, 5 clearly apart).
pub fn delta_e(a: Color32, b: Color32) -> f64 {
    core::delta_e(a.to_core(), b.to_core())
}

#[cfg(test)]
mod tests {
    use super::*;
    use plumcraft_proto::catalogue::{NodeDef, WireType};

    #[test]
    fn text_sizes_follow_the_base() {
        let mut t = PAPER;
        assert_eq!(t.font(Text::Port).size, 13.0);
        assert_eq!(t.font(Text::Title).size, 14.0);
        t.text_size = 18.0;
        assert!((t.font(Text::Port).size - 13.0 * 18.0 / 14.0).abs() < 1e-4);
        t.text_size = f32::NAN;
        assert_eq!(t.font(Text::Port).size, 13.0);
        t.text_size = 99.0;
        assert_eq!(t.font(Text::Title).size, 18.0);
        assert_eq!(t.font(Text::Tag).family, FontFamily::Monospace);
        assert_eq!(t.font(Text::Title).family, FontFamily::Proportional, "no SemiBold installed");
    }

    /// The egui tokens are the core's, colour for colour, both ways.
    #[test]
    fn the_themes_are_the_cores_in_egui_colours() {
        for (ours, theirs) in ALL.iter().zip(&core::ALL) {
            assert_eq!(&core::Tokens::from(ours), theirs);
            assert_eq!(&Tokens::from(theirs.clone()), ours);
            assert_eq!(ours.canvas.to_core(), theirs.canvas);
            assert_eq!(ours.widget.hovered.to_core(), theirs.widget.hovered);
        }
        let mut t = PAPER;
        t.pins.insert("x".into(), Color32::from_rgb(1, 2, 3));
        assert_eq!(Tokens::from(core::Tokens::from(&t)), t, "pins survive the round trip");
        assert_eq!(by_name("medium").map(|t| t.name), Some("pewter"));
    }

    /// Every token name reads and writes the same colour as the core's table.
    #[test]
    fn the_token_table_is_the_cores() {
        let mut t = DUSK;
        for (_, names) in TOKENS {
            for n in *names {
                assert_eq!(t.token(n).map(ToCore::to_core), core::DUSK.token(n), "{n}");
                *t.token_mut(n).unwrap() = Color32::from_rgb(4, 5, 6);
                let mut c = core::DUSK;
                *c.token_mut(n).unwrap() = Color::from_rgb(4, 5, 6);
                assert_eq!(core::Tokens::from(&t), c, "{n}");
                t = DUSK;
            }
        }
        assert_eq!(t.token("nope"), None);
    }

    /// Each method answers what the core's does.
    #[test]
    fn methods_answer_as_the_cores() {
        let mut c = SchemaReply::default();
        for (i, f) in ["source", "output", "text", "filter", "table"].iter().enumerate() {
            c.nodes.push(NodeDef { id: format!("n{i}"), family: (*f).into(), ..Default::default() });
        }
        c.wire_types.push(WireType { id: "TEXT".into(), color: Some("#c9a227".into()), ..Default::default() });
        for t in &ALL {
            let mut ours = t.clone();
            ours.pins.insert("table".into(), ours.olive);
            let theirs = core::Tokens::from(&ours);
            for f in ["", "source", "output", "text", "filter", "table", "unknown"] {
                assert_eq!(core::FamilyTint::from(ours.family_tint(&c, f)), theirs.family_tint(&c, f), "{f}");
                assert_eq!(ours.family_fruit(&c, f), theirs.family_fruit(&c, f));
            }
            for x in [-1.0, 0.0, 0.1, 0.2, 0.55, 0.9, 1.0, 2.0, f32::NAN] {
                assert_eq!(ours.ramp(x).to_core(), theirs.ramp(x), "{x}");
            }
            for s in [NodeState::Queued, NodeState::Progress, NodeState::Done, NodeState::Unknown] {
                assert_eq!(ours.state_color(s).map(ToCore::to_core), theirs.state_color(s));
            }
            assert_eq!(arrc(ours.computed_fruits()), theirs.computed_fruits());
            assert_eq!(ours.checks(), theirs.checks());
            for f in ["pear", "olive", "plum", "banana"] {
                assert_eq!(ours.fruit(f).map(ToCore::to_core), theirs.fruit(f), "{f}");
                let (mut a, mut b) = (ours.clone(), theirs.clone());
                assert_eq!(a.set_fruit(f, Color32::from_rgb(9, 9, 9)), b.set_fruit(f, Color::from_rgb(9, 9, 9)));
                assert_eq!(core::Tokens::from(&a), b, "{f}");
            }
        }
        let (a, b) = (Color32::from_rgb(10, 200, 30), Color32::from_rgba_premultiplied(40, 40, 40, 128));
        assert_eq!(mix(a, b, 0.3).to_core(), core::mix(a.to_core(), b.to_core(), 0.3));
        assert_eq!(mix(b, a, 0.7).to_core(), core::mix(b.to_core(), a.to_core(), 0.7));
        assert_eq!(parse_hex("#4f8fd8"), Some(Color32::from_rgb(0x4f, 0x8f, 0xd8)));
        assert_eq!(hex(a), "#0ac81e");
        assert_eq!(from_oklch(0.7, 0.1, 30.0).to_core(), core::from_oklch(0.7, 0.1, 30.0));
        assert_eq!(contrast(a, b), core::contrast(a.to_core(), b.to_core()));
    }
}
