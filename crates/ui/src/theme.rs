//! Theme tokens. Colours, radii and type sizes come from here, never from literals in widgets:
//! the canvas, the panels, run states, the engine banner and the Watched panel's charts all read
//! a [`Tokens`].
//!
//! Three themes ship (docs/research/22-themes.md): [`PAPER`], a warm light one and the default,
//! [`PEWTER`], a mid grey, and [`DUSK`], a dark one. The earlier `medium` and `dark` are read as
//! Pewter and Dusk ([`by_name`]), so an old preferences file still opens.
//!
//! A node's colours come from its family as the engine's catalogue names it
//! ([`Tokens::family_tint`]): one of twelve desaturated **fruit** colours, set in OKLCH at the
//! theme's lightness and chroma. The family id hashes into the twelve, and within one catalogue
//! a clash steps to the next free fruit; a wire type of the family's id with a colour keeps that
//! colour's hue at the theme's lightness and chroma; a family the user pinned takes its pin.
//! Olive is the quiet one; plum is kept for groups.
//!
//! Lifted from the research software's `crates/ui-egui/src/theme.rs`, and redone for research 22.

use std::collections::{BTreeMap, BTreeSet};

use crate::craft_ui::theme::WidgetPalette;
use egui::{Color32, CornerRadius, FontFamily, FontId, Stroke};
use plumcraft_proto::catalogue::SchemaReply;
use plumcraft_proto::event::NodeState;

/// Nothing drawn (hidden pins, clear header frames, snarl's hidden selection).
pub const CLEAR: Color32 = Color32::TRANSPARENT;

/// The body text size every other size is derived from, in points.
pub const BASE_TEXT: f32 = 14.0;
/// The text sizes the Settings window offers.
pub const TEXT_SIZES: std::ops::RangeInclusive<f32> = 11.0..=18.0;
/// The UI zooms it offers (egui's `zoom_factor`).
pub const ZOOMS: std::ops::RangeInclusive<f32> = 0.8..=1.6;

/// The family fruits, in hue order: a family's id hashes into these.
pub const FRUITS: [&str; 12] = ["cherry", "apricot", "quince", "lemon", "pear", "greengage", "melon", "juniper", "blueberry", "damson", "grape", "fig"];
/// The quiet fruit, for a family the user wants to recede.
pub const OLIVE: &str = "olive";
/// The groups' fruit.
pub const PLUM: &str = "plum";
/// The OKLCH hue of each family fruit, and its lightness tier (+1 lighter, -1 deeper):
/// neighbours alternate, so even the closest hues differ in lightness.
const FRUIT_HUES: [(f32, f32); 12] = [
    (22.0, 1.0),
    (58.0, -1.0),
    (84.0, 1.0),
    (104.0, -1.0),
    (124.0, 1.0),
    (148.0, -1.0),
    (176.0, 1.0),
    (208.0, -1.0),
    (248.0, 1.0),
    (276.0, -1.0),
    (302.0, 1.0),
    (356.0, -1.0),
];
/// The step between fruits when two families of one catalogue hash to the same one (coprime
/// with 12, so every fruit is reached).
const PROBE: usize = 5;

/// A theme: PlumCraft's colours, radii and text size. [`PAPER`] is the default. Settings ›
/// Appearance (or `view.theme`) switches, and the user's overrides are applied on top
/// ([`crate::appearance::Appearance::resolve`]).
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

/// The text styles PlumCraft draws in, beyond egui's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Text {
    /// A node's title (SemiBold).
    Title,
    /// A node's port labels.
    Port,
    /// Captions and preview text on the canvas.
    Caption,
    /// Small text drawn by hand: axis ticks, tags.
    Small,
    /// A net label's tag (monospace).
    Tag,
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

const fn tint(header: u32, body: u32) -> FamilyTint {
    FamilyTint { header: rgb(header), body: rgb(body) }
}

const fn fruits(hex: [u32; 12]) -> [Color32; 12] {
    let mut out = [CLEAR; 12];
    let mut i = 0;
    while i < 12 {
        out[i] = rgb(hex[i]);
        i += 1;
    }
    out
}

const MAGMA: [Color32; 6] = [rgb(0x000004), rgb(0x3b0f70), rgb(0x8c2981), rgb(0xde4968), rgb(0xfe9f6d), rgb(0xfcfdbf)];

/// Warm light, like the light flow tools (GDE, Node-RED, KNIME): ink on off-white, pastel fruit
/// headers with dark titles. The default.
pub const PAPER: Tokens = Tokens {
    name: "paper",
    label: "Paper",
    dark: false,
    canvas: rgb(0xe9e5dc),
    panel: rgb(0xf5f3ee),
    text: rgb(0x22201c),
    text_dim: rgb(0x57524a),
    text_faint: rgb(0x7f796f),
    separator: rgb(0xd3cdc2),
    field: rgb(0xffffff),
    field_border: rgb(0x8a8377),
    section: rgb(0xebe7df),
    accent: rgb(0x2c66ad),
    error: rgb(0xb3342a),
    warning: rgb(0x8f6200),
    banner: rgb(0xf4d8d1),
    widget: WidgetPalette { noninteractive: rgb(0xf5f3ee), inactive: rgb(0xe8e4dc), hovered: rgb(0xddd8ce), active: rgb(0xd0cabe), open: rgb(0xddd8ce) },
    radius: 5,
    pin_default: rgb(0x7f796f),
    pin_hidden: CLEAR,
    state_queued: rgb(0x7d838d),
    state_running: rgb(0x2c66ad),
    state_done: rgb(0x3a8a4c),
    state_cached: rgb(0x6f9c78),
    state_error: rgb(0xc0392b),
    state_skipped: rgb(0xa29d94),
    state_cancelled: rgb(0xa29d94),
    stale: rgb(0xb07d12),
    highlight: rgb(0xd4a300),
    placeholder: rgb(0xe2ddd3),
    // rgba(245, 243, 238, 0.55), premultiplied
    faded: Color32::from_rgba_premultiplied(135, 134, 131, 140),
    chart: rgb(0x2c66ad),
    waveform: rgb(0x1d7468),
    waveform_bg: rgb(0xfbfaf7),
    playhead: rgb(0x2a2722),
    spectrogram: MAGMA,
    family_default: tint(0xddd8cf, 0xfbfaf6),
    group: tint(0xedb8e7, 0xfaf3f4),
    family_body_mix: 0.10,
    header_text: rgb(0x22201c),
    wire: rgb(0x7d786f),
    grid: rgb(0xddd8ce),
    fruits: fruits([0xfec8c5, 0xe6b692, 0xefd4a0, 0xc8c48c, 0xcde0ab, 0xa1cfa6, 0xa3e8d6, 0x86cfda, 0xb9dcfe, 0xb3bef1, 0xe0cdff, 0xe9aec2]),
    olive: rgb(0xb0b29a),
    fruit_l: 0.845,
    fruit_dl: 0.035,
    fruit_c: 0.074,
    text_size: BASE_TEXT,
    pins: BTreeMap::new(),
    semibold: false,
};

/// Mid grey, for image work on a neutral ground: the docks recede, headers carry white titles.
/// Replaces the earlier Medium.
pub const PEWTER: Tokens = Tokens {
    name: "pewter",
    label: "Pewter",
    dark: true,
    canvas: rgb(0x4b4d52),
    panel: rgb(0x43454a),
    text: rgb(0xf5f4f1),
    text_dim: rgb(0xd6d4cf),
    text_faint: rgb(0xbdbbb6),
    separator: rgb(0x36383c),
    field: rgb(0x3a3c40),
    field_border: rgb(0x9a9ca1),
    section: rgb(0x3e4044),
    accent: rgb(0x94c2f7),
    error: rgb(0xfbc2ba),
    warning: rgb(0xebb85e),
    banner: rgb(0x7a3a33),
    widget: WidgetPalette { noninteractive: rgb(0x43454a), inactive: rgb(0x55575c), hovered: rgb(0x66686d), active: rgb(0x6a6d72), open: rgb(0x66686d) },
    radius: 5,
    pin_default: rgb(0xb4b2ad),
    pin_hidden: CLEAR,
    state_queued: rgb(0xa1a6ae),
    state_running: rgb(0x94c2f7),
    state_done: rgb(0x82d290),
    state_cached: rgb(0x5f9a6b),
    state_error: rgb(0xfbc2ba),
    state_skipped: rgb(0x85888d),
    state_cancelled: rgb(0x85888d),
    stale: rgb(0xebb85e),
    highlight: rgb(0xfff3b0),
    placeholder: rgb(0x45474c),
    faded: Color32::from_rgba_premultiplied(110, 110, 110, 110),
    chart: rgb(0x94c2f7),
    waveform: rgb(0x62d6c3),
    waveform_bg: rgb(0x2f3134),
    playhead: rgb(0xfff3b0),
    spectrogram: MAGMA,
    family_default: tint(0x64676c, 0x57595e),
    group: tint(0x7d5279, 0x5b5861),
    family_body_mix: 0.10,
    header_text: rgb(0xffffff),
    wire: rgb(0xb4b2ad),
    grid: rgb(0x56585d),
    fruits: fruits([0x905b59, 0x785131, 0x7f683a, 0x615d2b, 0x637345, 0x3e6644, 0x3a796a, 0x1e666f, 0x4a6f93, 0x505782, 0x73628e, 0x7a4a5c]),
    olive: rgb(0x696b56),
    fruit_l: 0.500,
    fruit_dl: 0.030,
    fruit_c: 0.070,
    text_size: BASE_TEXT,
    pins: BTreeMap::new(),
    semibold: false,
};

/// Dark, the research software's dark theme with brighter text, visible borders and deeper
/// headers. Replaces the earlier Dark.
pub const DUSK: Tokens = Tokens {
    name: "dusk",
    label: "Dusk",
    dark: true,
    canvas: rgb(0x202226),
    panel: rgb(0x2a2c31),
    text: rgb(0xeceae6),
    text_dim: rgb(0xbab7b1),
    text_faint: rgb(0x8d8b86),
    separator: rgb(0x18191c),
    field: rgb(0x232529),
    field_border: rgb(0x7a7e87),
    section: rgb(0x31343a),
    accent: rgb(0x6c9fe0),
    error: rgb(0xec7d70),
    warning: rgb(0xddae55),
    banner: rgb(0x6b2e28),
    widget: WidgetPalette { noninteractive: rgb(0x2a2c31), inactive: rgb(0x363940), hovered: rgb(0x41454c), active: rgb(0x4c5058), open: rgb(0x41454c) },
    radius: 5,
    pin_default: rgb(0xa3a19c),
    pin_hidden: CLEAR,
    state_queued: rgb(0x7a808a),
    state_running: rgb(0x6c9fe0),
    state_done: rgb(0x68ba77),
    state_cached: rgb(0x467a51),
    state_error: rgb(0xec7d70),
    state_skipped: rgb(0x5c6067),
    state_cancelled: rgb(0x5c6067),
    stale: rgb(0xddae55),
    highlight: rgb(0xecd65a),
    placeholder: rgb(0x2f3237),
    faded: Color32::from_rgba_premultiplied(110, 110, 110, 110),
    chart: rgb(0x6c9fe0),
    waveform: rgb(0x45c2ae),
    waveform_bg: rgb(0x17181b),
    playhead: rgb(0xecd65a),
    spectrogram: MAGMA,
    family_default: tint(0x3d4047, 0x2e3136),
    group: tint(0x664162, 0x34333a),
    family_body_mix: 0.10,
    header_text: rgb(0xf4f2ee),
    wire: rgb(0x8d8b86),
    grid: rgb(0x282a2f),
    fruits: fruits([0x784a49, 0x603e24, 0x69562e, 0x4d491e, 0x515f37, 0x2f5034, 0x2d6457, 0x115059, 0x3c5c7a, 0x3e4468, 0x5f5076, 0x623948]),
    olive: rgb(0x555745),
    fruit_l: 0.430,
    fruit_dl: 0.032,
    fruit_c: 0.062,
    text_size: BASE_TEXT,
    pins: BTreeMap::new(),
    semibold: false,
};

/// The default theme.
pub const DEFAULT: Tokens = PAPER;

/// Every theme, in the Settings window's order.
pub const ALL: [Tokens; 3] = [PAPER, PEWTER, DUSK];

/// Names a theme was once called, read as the theme that replaced it.
const ALIASES: &[(&str, &str)] = &[("medium", "pewter"), ("dark", "dusk")];

/// The theme named `name` (or once called `name`: `medium` is Pewter, `dark` is Dusk).
pub fn by_name(name: &str) -> Option<Tokens> {
    let name = ALIASES.iter().find(|(old, _)| *old == name).map_or(name, |(_, new)| new);
    ALL.iter().find(|t| t.name == name).cloned()
}

/// `a` moved `t` of the way to `b` (each channel; alpha from `a`).
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
    let ch = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgba_unmultiplied(ch(a.r(), b.r()), ch(a.g(), b.g()), ch(a.b(), b.b()), a.a())
}

/// FNV-1a of an id: the same on every machine and every run (std's hasher promises neither).
pub fn fnv1a(id: &str) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    for b in id.bytes() {
        h ^= u32::from(b);
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// The fruit slot of each of a catalogue's families (and of `extra`): each takes its hash slot,
/// in id order, and a clash steps by [`PROBE`] to the next free fruit. Stable for a given set of
/// families whatever the catalogue's order; past twelve families, fruits are shared.
pub fn fruit_slots<'a>(catalog: &'a SchemaReply, extra: &'a str) -> BTreeMap<&'a str, usize> {
    let mut families: BTreeSet<&str> = catalog.nodes.iter().map(|n| n.family.as_str()).filter(|f| !f.is_empty()).collect();
    if !extra.is_empty() {
        families.insert(extra);
    }
    let mut taken = [false; 12];
    let mut out = BTreeMap::new();
    for (n, f) in families.into_iter().enumerate() {
        if n % 12 == 0 {
            taken = [false; 12];
        }
        let mut i = fnv1a(f) as usize % 12;
        for _ in 0..12 {
            if !taken.get(i).copied().unwrap_or(true) {
                break;
            }
            i = (i + PROBE) % 12;
        }
        if let Some(t) = taken.get_mut(i) {
            *t = true;
        }
        out.insert(f, i);
    }
    out
}

/// The fruit `name`'s index in [`FRUITS`].
pub fn fruit_index(name: &str) -> Option<usize> {
    FRUITS.iter().position(|f| *f == name)
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
    /// The header colour of fruit `name` (the twelve, olive, or plum).
    pub fn fruit(&self, name: &str) -> Option<Color32> {
        match name {
            OLIVE => Some(self.olive),
            PLUM => Some(self.group.header),
            _ => fruit_index(name).and_then(|i| self.fruits.get(i).copied()),
        }
    }

    /// Set fruit `name`'s header colour (plum is the groups' header, and their body follows).
    pub fn set_fruit(&mut self, name: &str, c: Color32) -> bool {
        match name {
            OLIVE => self.olive = c,
            PLUM => self.group = FamilyTint { header: c, body: mix(self.family_default.body, c, self.family_body_mix) },
            _ => match fruit_index(name).and_then(|i| self.fruits.get_mut(i)) {
                Some(f) => *f = c,
                None => return false,
            },
        }
        true
    }

    /// The fruit `family` takes in `catalog` (by hash and probe), ignoring pins and engine
    /// colours. `None` for no family.
    pub fn family_fruit(&self, catalog: &SchemaReply, family: &str) -> Option<&'static str> {
        if family.is_empty() {
            return None;
        }
        fruit_slots(catalog, family).get(family).and_then(|i| FRUITS.get(*i)).copied()
    }

    /// The colours of a node of `family`: the user's pin, else the hue of the catalogue's wire
    /// type with the family's id (when the engine colours it) at the theme's fruit lightness and
    /// chroma, else its fruit. No family: the default.
    pub fn family_tint(&self, catalog: &SchemaReply, family: &str) -> FamilyTint {
        if family.is_empty() {
            return self.family_default;
        }
        let header = self.pins.get(family).copied().unwrap_or_else(|| {
            let engine = catalog.wire_types.iter().find(|w| w.id.eq_ignore_ascii_case(family)).and_then(|w| w.color.as_deref()).and_then(parse_hex);
            match engine {
                Some(c) => {
                    let (_, chroma, hue) = oklch(c);
                    // a grey from the engine stays a grey
                    let chroma = if chroma < 0.02 { 0.0 } else { self.fruit_c };
                    from_oklch(self.fruit_l, chroma, hue)
                }
                None => self.family_fruit(catalog, family).and_then(|f| self.fruit(f)).unwrap_or(self.family_default.header),
            }
        });
        FamilyTint { header, body: mix(self.family_default.body, header, self.family_body_mix) }
    }

    /// The fruit set recomputed from the theme's `fruit_l`, `fruit_dl` and `fruit_c` (the
    /// hard-coded tables are these, rounded): what the palette sliders give.
    pub fn computed_fruits(&self) -> [Color32; 12] {
        let mut out = [CLEAR; 12];
        for (o, (h, tier)) in out.iter_mut().zip(FRUIT_HUES) {
            *o = from_oklch(self.fruit_l + tier * self.fruit_dl, self.fruit_c, h);
        }
        out
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

    /// A size scaled from the 14 pt base to this theme's text size.
    fn size(&self, at_base: f32) -> f32 {
        let s = if self.text_size.is_finite() { self.text_size.clamp(*TEXT_SIZES.start(), *TEXT_SIZES.end()) } else { BASE_TEXT };
        at_base * s / BASE_TEXT
    }

    /// The SemiBold family when it is installed, else the text face.
    fn bold_family(&self) -> FontFamily {
        if self.semibold { FontFamily::Name(crate::fonts::SEMIBOLD.into()) } else { FontFamily::Proportional }
    }

    /// The font of one of PlumCraft's own text styles.
    pub fn font(&self, text: Text) -> FontId {
        match text {
            Text::Title => FontId::new(self.size(14.0), self.bold_family()),
            Text::Port => FontId::proportional(self.size(13.0)),
            Text::Caption => FontId::proportional(self.size(12.5)),
            Text::Small => FontId::proportional(self.size(12.0)),
            Text::Tag => FontId::monospace(self.size(12.0)),
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
        let mut out = Vec::new();
        let mut add = |fg: &'static str, bg: &'static str, need: f64| {
            if let (Some(f), Some(b)) = (self.token(fg), self.token(bg)) {
                out.push(Check { fg, bg, ratio: contrast(f, b), need });
            }
        };
        for bg in ["panel", "section", "field", "canvas", "widget_inactive", "widget_hovered", "widget_active", "banner", "default_body"] {
            add("text", bg, 4.5);
        }
        for bg in ["panel", "section", "field", "canvas", "default_body"] {
            add("text_dim", bg, 4.5);
        }
        add("text_faint", "panel", 3.0);
        add("text_faint", "default_body", 3.0);
        add("accent", "panel", 4.5);
        add("error", "panel", 4.5);
        add("error", "default_body", 4.5);
        add("warning", "panel", 3.0);
        add("header_text", "default_header", 4.5);
        add("field_border", "field", 3.0);
        add("field_border", "panel", 3.0);
        for fg in ["accent", "state_done", "state_error", "state_running", "state_queued", "wire"] {
            add(fg, "canvas", 3.0);
        }
        add("highlight", "canvas", 1.8);
        add("pin_default", "default_body", 3.0);
        add("waveform", "waveform_bg", 4.5);
        add("playhead", "waveform_bg", 4.5);
        add("playhead", "waveform", 1.3);
        add("chart", "panel", 3.0);
        out
    }

    /// The checks with `token` on either side.
    pub fn checks_of(&self, token: &str) -> Vec<Check> {
        self.checks().into_iter().filter(|c| c.fg == token || c.bg == token).collect()
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

/// The tokens a user can change, by group, as the Settings window lists them.
pub const TOKENS: &[(&str, &[&str])] = &[
    ("Surfaces", &["canvas", "panel", "section", "field", "separator", "banner", "placeholder"]),
    ("Text", &["text", "text_dim", "text_faint", "header_text", "accent", "error", "warning"]),
    ("Controls", &["field_border", "widget_inactive", "widget_hovered", "widget_active"]),
    ("Run states", &["state_queued", "state_running", "state_done", "state_cached", "state_error", "state_skipped", "state_cancelled", "stale", "highlight"]),
    ("Media", &["chart", "waveform", "waveform_bg", "playhead"]),
    ("Canvas", &["wire", "grid", "pin_default", "default_header", "default_body"]),
];

/// One contrast pair a theme must pass.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Check {
    pub fg: &'static str,
    pub bg: &'static str,
    pub ratio: f64,
    pub need: f64,
}

impl Check {
    pub fn passes(&self) -> bool {
        self.ratio >= self.need
    }
}

/// Parse a colour (`#rrggbb`).
pub fn parse_hex(s: &str) -> Option<Color32> {
    let hex = s.strip_prefix('#')?;
    if hex.len() != 6 || !hex.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    Some(Color32::from_rgb(byte(0)?, byte(2)?, byte(4)?))
}

/// A colour as `#rrggbb`.
pub fn hex(c: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

fn lin(v: u8) -> f64 {
    let v = f64::from(v) / 255.0;
    if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
}

fn delin(v: f64) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let s = if v <= 0.003_130_8 { 12.92 * v } else { 1.055 * v.powf(1.0 / 2.4) - 0.055 };
    (255.0 * s).round().clamp(0.0, 255.0) as u8
}

/// WCAG 2 relative luminance.
pub fn luminance(c: Color32) -> f64 {
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

/// WCAG 2 contrast ratio (1 ..= 21).
pub fn contrast(a: Color32, b: Color32) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// OKLab (Björn Ottosson, 2020) of an sRGB colour.
pub fn oklab(c: Color32) -> [f64; 3] {
    let (r, g, b) = (lin(c.r()), lin(c.g()), lin(c.b()));
    let l = 0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b;
    let m = 0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b;
    let s = 0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b;
    let (l, m, s) = (l.cbrt(), m.cbrt(), s.cbrt());
    [
        0.210_454_255_3 * l + 0.793_617_785_0 * m - 0.004_072_046_8 * s,
        1.977_998_495_1 * l - 2.428_592_205_0 * m + 0.450_593_709_9 * s,
        0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766_0 * s,
    ]
}

fn oklab_to_linear(l: f64, a: f64, b: f64) -> [f64; 3] {
    let l_ = (l + 0.396_337_777_4 * a + 0.215_803_757_3 * b).powi(3);
    let m_ = (l - 0.105_561_345_8 * a - 0.063_854_172_8 * b).powi(3);
    let s_ = (l - 0.089_484_177_5 * a - 1.291_485_548_0 * b).powi(3);
    [
        4.076_741_662_1 * l_ - 3.307_711_591_3 * m_ + 0.230_969_929_2 * s_,
        -1.268_438_004_6 * l_ + 2.609_757_401_1 * m_ - 0.341_319_396_5 * s_,
        -0.004_196_086_3 * l_ - 0.703_418_614_7 * m_ + 1.707_614_701_0 * s_,
    ]
}

/// OKLCH (lightness, chroma, hue in degrees) of an sRGB colour.
pub fn oklch(c: Color32) -> (f32, f32, f32) {
    let [l, a, b] = oklab(c);
    (l as f32, a.hypot(b) as f32, b.atan2(a).to_degrees().rem_euclid(360.0) as f32)
}

/// The sRGB colour at OKLCH (`l`, `c`, `h` degrees), its chroma reduced until it is in gamut.
pub fn from_oklch(l: f32, c: f32, h: f32) -> Color32 {
    let (l, h) = (f64::from(if l.is_finite() { l.clamp(0.0, 1.0) } else { 0.5 }), f64::from(if h.is_finite() { h } else { 0.0 }).to_radians());
    let mut c = f64::from(if c.is_finite() { c.clamp(0.0, 0.4) } else { 0.0 });
    loop {
        let rgb = oklab_to_linear(l, c * h.cos(), c * h.sin());
        if rgb.iter().all(|v| (-1e-4..=1.0 + 1e-4).contains(v)) || c <= 0.0 {
            return Color32::from_rgb(delin(rgb[0]), delin(rgb[1]), delin(rgb[2]));
        }
        c = (c - 0.002).max(0.0);
    }
}

/// The OKLab distance between two colours, ×100 (about 2 is just noticeable, 5 clearly apart).
pub fn delta_e(a: Color32, b: Color32) -> f64 {
    let (x, y) = (oklab(a), oklab(b));
    100.0 * x.iter().zip(y).map(|(p, q)| (p - q).powi(2)).sum::<f64>().sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use plumcraft_proto::catalogue::{NodeDef, WireType};

    fn catalog(families: &[&str]) -> SchemaReply {
        let mut c = SchemaReply::default();
        for (i, f) in families.iter().enumerate() {
            c.nodes.push(NodeDef { id: format!("n{i}"), family: (*f).into(), ..Default::default() });
        }
        c
    }

    #[test]
    fn families_have_their_own_stable_colours() {
        let c = SchemaReply::default();
        for t in &ALL {
            assert_ne!(t.family_tint(&c, "source"), t.family_tint(&c, "output"));
            assert_eq!(t.family_tint(&c, ""), t.family_default);
            assert_eq!(t.family_tint(&c, "text"), t.family_tint(&c, "text"), "the same id, the same colour");
        }
        // FNV-1a pinned: the fruit never changes between runs, machines or Rust versions.
        assert_eq!(fnv1a(""), 0x811c_9dc5);
        assert_eq!(fnv1a("a"), 0xe40c_292c);
    }

    /// The proposal's table: eight families, `filter` and `table` hash to fig, and the probe
    /// moves `table` (later in id order) to cherry. The order of the catalogue doesn't matter.
    #[test]
    fn families_take_their_fruit_by_hash_and_probe() {
        let fams = ["source", "output", "text", "audio", "image", "filter", "math", "table"];
        let want = [
            ("source", "blueberry"),
            ("output", "pear"),
            ("text", "grape"),
            ("audio", "damson"),
            ("image", "quince"),
            ("filter", "fig"),
            ("math", "juniper"),
            ("table", "cherry"),
        ];
        let c = catalog(&fams);
        let mut rev = fams;
        rev.reverse();
        let r = catalog(&rev);
        for (f, fruit) in want {
            assert_eq!(PAPER.family_fruit(&c, f), Some(fruit), "{f}");
            assert_eq!(PAPER.family_fruit(&r, f), Some(fruit), "{f}, the catalogue reversed");
        }
        // alone, table hashes to fig
        assert_eq!(PAPER.family_fruit(&catalog(&["table"]), "table"), Some("fig"));
        // in one catalogue no two families share a fruit, up to twelve
        let many: Vec<String> = (0..12).map(|i| format!("family{i}")).collect();
        let refs: Vec<&str> = many.iter().map(String::as_str).collect();
        let c = catalog(&refs);
        let got: BTreeSet<_> = refs.iter().map(|f| PAPER.family_fruit(&c, f)).collect();
        assert_eq!(got.len(), 12);
        // the tint is the fruit's header, the body a tenth of it
        let c = catalog(&fams);
        for t in &ALL {
            let tint = t.family_tint(&c, "source");
            assert_eq!(Some(tint.header), t.fruit("blueberry"));
            assert_eq!(tint.body, mix(t.family_default.body, tint.header, t.family_body_mix));
        }
    }

    #[test]
    fn an_engine_colour_keeps_its_hue_in_each_themes_register() {
        let mut c = SchemaReply::default();
        c.wire_types.push(WireType { id: "TEXT".into(), color: Some("#c9a227".into()), ..Default::default() });
        for t in &ALL {
            let from_engine = t.family_tint(&c, "text");
            assert_ne!(from_engine, t.family_tint(&SchemaReply::default(), "text"));
            let (l, ch, h) = oklch(from_engine.header);
            let (_, _, want) = oklch(Color32::from_rgb(0xc9, 0xa2, 0x27));
            assert!((l - t.fruit_l).abs() < 0.01, "{}: lightness {l}", t.name);
            assert!((ch - t.fruit_c).abs() < 0.01, "{}: chroma {ch}", t.name);
            assert!((h - want).abs() < 4.0, "{}: hue {h} not {want}", t.name);
        }
    }

    #[test]
    fn a_pin_beats_the_engine_and_the_hash() {
        let mut c = SchemaReply::default();
        c.wire_types.push(WireType { id: "TEXT".into(), color: Some("#c9a227".into()), ..Default::default() });
        let mut t = PAPER;
        t.pins.insert("text".into(), t.olive);
        assert_eq!(t.family_tint(&c, "text").header, t.olive);
    }

    #[test]
    fn themes_are_named_and_old_names_still_open() {
        assert_eq!(by_name("paper").map(|t| t.label), Some("Paper"));
        assert_eq!(by_name("pewter").map(|t| t.label), Some("Pewter"));
        assert_eq!(by_name("dusk").map(|t| t.label), Some("Dusk"));
        assert_eq!(by_name("medium").map(|t| t.name), Some("pewter"));
        assert_eq!(by_name("dark").map(|t| t.name), Some("dusk"));
        assert_eq!(by_name("neon"), None);
        assert_eq!(by_name(""), None);
        assert_eq!(DEFAULT.name, "paper");
        assert_eq!(ALL.iter().map(|t| t.dark).collect::<Vec<_>>(), [false, true, true], "Paper is light, Pewter and Dusk dark");
    }

    #[test]
    fn every_theme_passes_its_contrast_checks() {
        for t in &ALL {
            let checks = t.checks();
            assert!(checks.len() >= 35, "{}: {} checks", t.name, checks.len());
            for c in checks {
                assert!(c.passes(), "{}: {} on {} is {:.2}:1, needs {}", t.name, c.fg, c.bg, c.ratio, c.need);
            }
        }
    }

    /// Text on every fruit header, and on every fruit body, in every theme; and on a header an
    /// engine's colour made, whatever its hue.
    #[test]
    fn text_is_readable_on_every_fruit() {
        for t in &ALL {
            let mut headers: Vec<(String, Color32)> = FRUITS.iter().map(|f| ((*f).to_owned(), t.fruit(f).unwrap_or(CLEAR))).collect();
            headers.push((OLIVE.into(), t.olive));
            headers.push((PLUM.into(), t.group.header));
            for hue in (0..360).step_by(15) {
                headers.push((format!("engine hue {hue}"), from_oklch(t.fruit_l, t.fruit_c, hue as f32)));
            }
            for (name, h) in headers {
                let body = mix(t.family_default.body, h, t.family_body_mix);
                for (what, fg, bg, need) in [
                    ("header_text", t.header_text, h, 4.5),
                    ("text_dim", t.text_dim, h, 3.0),
                    ("text", t.text, body, 4.5),
                    ("text_faint", t.text_faint, body, 3.0),
                ] {
                    let r = contrast(fg, bg);
                    assert!(r >= need, "{}: {what} on {name} is {r:.2}:1", t.name);
                }
            }
        }
    }

    /// The twelve family fruits stay apart: the closest pair is at least 4 in OKLab ×100 (the
    /// proposal's swatches give 4.3 to 5.1), and plum, the groups', from every family fruit.
    #[test]
    fn the_fruits_can_be_told_apart() {
        for t in &ALL {
            let mut closest = f64::MAX;
            for (i, a) in t.fruits.iter().enumerate() {
                for b in t.fruits.iter().skip(i + 1) {
                    closest = closest.min(delta_e(*a, *b));
                }
                assert!(delta_e(*a, t.group.header) >= 4.0, "{}: plum and {} are {:.1} apart", t.name, FRUITS[i], delta_e(*a, t.group.header));
            }
            assert!(closest >= 4.0, "{}: the closest fruits are {closest:.1} apart", t.name);
        }
    }

    /// The hard-coded fruit tables are the OKLCH formula, rounded (the palette sliders and an
    /// engine's colour use the formula).
    #[test]
    fn the_fruit_tables_are_the_formula() {
        for t in &ALL {
            for (i, (got, want)) in t.computed_fruits().iter().zip(t.fruits).enumerate() {
                let d = [got.r().abs_diff(want.r()), got.g().abs_diff(want.g()), got.b().abs_diff(want.b())];
                assert!(d.iter().all(|d| *d <= 2), "{}: {} is {} by the formula, {} in the table", t.name, FRUITS[i], hex(*got), hex(want));
            }
        }
    }

    #[test]
    fn waveforms_and_spectrograms_are_readable() {
        for t in &ALL {
            // the ramp goes from dark to light, each stop lighter than the last
            let ramp = t.spectrogram;
            assert!(contrast(ramp[0], ramp[5]) >= 10.0, "{}: the spectrogram ramp is flat", t.name);
            for w in ramp.windows(2) {
                assert!(luminance(w[1]) > luminance(w[0]), "{}: {:?} then {:?}", t.name, w[0], w[1]);
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

    #[test]
    fn every_listed_token_can_be_read_and_set() {
        let mut t = PAPER;
        for (_, names) in TOKENS {
            for n in *names {
                assert!(t.token(n).is_some(), "{n}");
                *t.token_mut(n).unwrap() = Color32::from_rgb(1, 2, 3);
                assert_eq!(t.token(n), Some(Color32::from_rgb(1, 2, 3)), "{n}");
            }
        }
        assert_eq!(t.token("nope"), None);
        assert!(t.set_fruit("pear", Color32::BLACK));
        assert_eq!(t.fruit("pear"), Some(Color32::BLACK));
        assert!(!t.set_fruit("banana", Color32::BLACK));
    }

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
    }

    #[test]
    fn hex_colours() {
        assert_eq!(parse_hex("#4f8fd8"), Some(Color32::from_rgb(0x4f, 0x8f, 0xd8)));
        assert_eq!(hex(Color32::from_rgb(0x4f, 0x8f, 0xd8)), "#4f8fd8");
        for bad in ["", "#", "4f8fd8", "#4f8fd", "#4f8fdz", "#ééé", "#4f8fd8ff"] {
            assert_eq!(parse_hex(bad), None, "{bad}");
        }
    }

    #[test]
    fn oklch_round_trips_and_never_leaves_gamut() {
        for c in [Color32::from_rgb(0xc9, 0xa2, 0x27), Color32::WHITE, Color32::BLACK, Color32::from_rgb(20, 200, 90)] {
            let (l, ch, h) = oklch(c);
            let back = from_oklch(l, ch, h);
            assert!(delta_e(c, back) < 0.5, "{c:?} -> {back:?}");
        }
        for bad in [f32::NAN, f32::INFINITY, -1.0, 9.0] {
            let _ = from_oklch(bad, bad, bad);
        }
    }
}
