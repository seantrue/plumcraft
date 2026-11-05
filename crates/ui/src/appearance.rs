//! What the user changed of the look: the text size and UI zoom, colour overrides per theme,
//! fruit colours per theme, and families pinned to a fruit or a colour. Kept in the preferences
//! under `appearance` ([`crate::prefs::Prefs::appearance`]) and applied over a built-in theme by
//! [`Appearance::resolve`]. Settings › Appearance edits it, every change through a command.
//!
//! Theme files (`.plumtheme.json`, docs/research/22-themes.md section 6) carry the differences
//! from a built-in theme. They are input like any other: a bad colour or an unknown token is
//! skipped, a file of another format or version is refused with a message, and the size is
//! capped. Nothing here panics.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::{Value, json};

use crate::theme::{self, Tokens};

/// The largest theme file read.
pub const MAX_THEME_BYTES: usize = 256 * 1024;
/// A theme file's `format`.
pub const FORMAT: &str = "plumcraft-theme";
/// The theme file version this build writes and reads.
pub const VERSION: u64 = 1;
/// The most entries kept in any one map (families, a theme's overrides): a file can't grow
/// without bound.
const MAX_ENTRIES: usize = 512;
/// The longest family id or token name kept.
const MAX_KEY: usize = 128;

/// The user's appearance settings. Every field is optional; what's missing is the default.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    /// Body text size in points ([`theme::TEXT_SIZES`]); none: 14.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_size: Option<f32>,
    /// egui's zoom factor ([`theme::ZOOMS`]); none: 1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zoom: Option<f32>,
    /// Token colours by theme, then token (`{"paper": {"accent": "#2c66ad"}}`).
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub overrides: BTreeMap<String, BTreeMap<String, String>>,
    /// Fruit colours by theme, then fruit.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub fruits: BTreeMap<String, BTreeMap<String, String>>,
    /// Families pinned to a fruit's name or a `#rrggbb`, in every theme.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub families: BTreeMap<String, String>,
    /// Fields this build doesn't know, kept for the file.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// `appearance` in the preferences: junk there is the defaults, and costs nothing else in the
/// file.
pub fn lenient<'de, D: Deserializer<'de>>(d: D) -> Result<Appearance, D::Error> {
    let v = Value::deserialize(d)?;
    Ok(serde_json::from_value::<Appearance>(v).map(Appearance::cleaned).unwrap_or_default())
}

fn clip(s: &str) -> String {
    s.chars().take(MAX_KEY).collect()
}

impl Appearance {
    /// Nothing changed.
    pub fn is_default(&self) -> bool {
        *self == Appearance::default()
    }

    /// Out-of-range numbers dropped and maps capped (what was read from a file).
    fn cleaned(mut self) -> Self {
        self.text_size = self.text_size.filter(|s| theme::TEXT_SIZES.contains(s));
        self.zoom = self.zoom.filter(|z| theme::ZOOMS.contains(z));
        for m in [&mut self.overrides, &mut self.fruits] {
            while m.len() > MAX_ENTRIES {
                m.pop_last();
            }
            for inner in m.values_mut() {
                while inner.len() > MAX_ENTRIES {
                    inner.pop_last();
                }
            }
        }
        while self.families.len() > MAX_ENTRIES {
            self.families.pop_last();
        }
        self
    }

    /// Theme `name` (or the default, when there's none by that name) with these settings over
    /// it. A colour that doesn't parse, or a token or fruit this build doesn't know, is skipped.
    pub fn resolve(&self, name: &str) -> Tokens {
        let mut t = theme::by_name(name).unwrap_or(theme::DEFAULT);
        t.text_size = self.text_size.unwrap_or(theme::BASE_TEXT);
        for (token, c) in self.overrides.get(t.name).into_iter().flatten() {
            if let (Some(slot), Some(c)) = (t.token_mut(token), theme::parse_hex(c)) {
                *slot = c;
            }
        }
        // the groups' body follows a changed body colour
        t.group.body = theme::mix(t.family_default.body, t.group.header, t.family_body_mix);
        for (fruit, c) in self.fruits.get(t.name).into_iter().flatten() {
            if let Some(c) = theme::parse_hex(c) {
                t.set_fruit(fruit, c);
            }
        }
        for (family, pin) in &self.families {
            if let Some(c) = t.fruit(pin).or_else(|| theme::parse_hex(pin)) {
                t.pins.insert(family.clone(), c);
            }
        }
        t
    }

    /// Set (or, with `None`, reset) token `token` of theme `theme`.
    pub fn set_token(&mut self, theme: &str, token: &str, color: Option<&str>) {
        set_in(&mut self.overrides, theme, token, color);
    }

    /// Set (or reset) fruit `fruit`'s colour in theme `theme`.
    pub fn set_fruit(&mut self, theme: &str, fruit: &str, color: Option<&str>) {
        set_in(&mut self.fruits, theme, fruit, color);
    }

    /// Pin `family` to a fruit or colour, or (`None`) let it take its own.
    pub fn pin(&mut self, family: &str, to: Option<&str>) {
        match to {
            Some(to) if self.families.len() < MAX_ENTRIES || self.families.contains_key(family) => {
                self.families.insert(clip(family), to.to_owned());
            }
            Some(_) => {}
            None => {
                self.families.remove(family);
            }
        }
    }

    /// Theme `theme`'s colours back to the built-in ones.
    pub fn reset(&mut self, theme: &str) {
        self.overrides.remove(theme);
        self.fruits.remove(theme);
    }

    /// The theme file for theme `theme`: only what differs from the built-in.
    pub fn export(&self, theme: &str) -> Value {
        let t = theme::by_name(theme).unwrap_or(theme::DEFAULT);
        let mut out = json!({"format": FORMAT, "version": VERSION, "name": t.name, "label": t.label, "base": t.name});
        if let Some(o) = self.overrides.get(t.name).filter(|o| !o.is_empty()) {
            out["tokens"] = json!(o);
        }
        if let Some(f) = self.fruits.get(t.name).filter(|f| !f.is_empty()) {
            out["fruits"] = json!(f);
        }
        out
    }

    /// Read a theme file: its overrides and fruits replace those of its base theme, whose name
    /// is returned with what was skipped. A file of another format or version, or with no base
    /// this build has, is refused.
    pub fn import(&mut self, text: &str) -> Result<(&'static str, Vec<String>), String> {
        if text.len() > MAX_THEME_BYTES {
            return Err(format!("a theme file is at most {} KB", MAX_THEME_BYTES / 1024));
        }
        let v: Value = serde_json::from_str(text).map_err(|e| format!("not a theme file: {e}"))?;
        let o = v.as_object().ok_or("not a theme file: not a JSON object")?;
        if o.get("format").and_then(Value::as_str) != Some(FORMAT) {
            return Err(format!("not a theme file: \"format\" must be {FORMAT:?}"));
        }
        match o.get("version").and_then(Value::as_u64) {
            Some(VERSION) => {}
            other => {
                return Err(format!(
                    "theme file version {} isn't one this build reads (it reads {VERSION})",
                    other.map_or_else(|| "?".into(), |n| n.to_string())
                ));
            }
        }
        let base = o.get("base").and_then(Value::as_str).ok_or("a theme file needs a \"base\" theme")?;
        let t = theme::by_name(base).ok_or_else(|| format!("no built-in theme called {:?}", clip(base)))?;
        let mut skipped = Vec::new();
        let mut take = |key: &str, known: &dyn Fn(&str) -> bool| -> BTreeMap<String, String> {
            let mut out = BTreeMap::new();
            if let Some(m) = o.get(key).and_then(Value::as_object) {
                for (k, v) in m.iter().take(MAX_ENTRIES) {
                    match v.as_str().filter(|s| theme::parse_hex(s).is_some()) {
                        Some(c) if known(k) => {
                            out.insert(clip(k), c.to_owned());
                        }
                        _ => skipped.push(format!("{key}.{}", clip(k))),
                    }
                }
            }
            out
        };
        let tokens = take("tokens", &|k| theme::PAPER.token(k).is_some());
        let fruits = take("fruits", &|k| theme::PAPER.fruit(k).is_some());
        self.overrides.insert(t.name.to_owned(), tokens);
        self.fruits.insert(t.name.to_owned(), fruits);
        self.overrides.retain(|_, m| !m.is_empty());
        self.fruits.retain(|_, m| !m.is_empty());
        Ok((t.name, skipped))
    }

    /// The settings as `app.settings` reports them.
    pub fn summary(&self) -> Value {
        json!({
            "textSize": self.text_size.unwrap_or(theme::BASE_TEXT),
            "zoom": self.zoom.unwrap_or(1.0),
            "overrides": self.overrides,
            "fruits": self.fruits,
            "families": self.families,
        })
    }
}

fn set_in(m: &mut BTreeMap<String, BTreeMap<String, String>>, theme: &str, key: &str, color: Option<&str>) {
    match color {
        Some(c) => {
            if m.len() >= MAX_ENTRIES && !m.contains_key(theme) {
                return;
            }
            let inner = m.entry(clip(theme)).or_default();
            if inner.len() < MAX_ENTRIES || inner.contains_key(key) {
                inner.insert(clip(key), c.to_owned());
            }
        }
        None => {
            if let Some(inner) = m.get_mut(theme) {
                inner.remove(key);
                if inner.is_empty() {
                    m.remove(theme);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::Color32;

    #[test]
    fn overrides_fruits_and_pins_apply_to_their_theme() {
        let mut a = Appearance::default();
        a.set_token("paper", "accent", Some("#112233"));
        a.set_token("paper", "nope", Some("#112233"));
        a.set_token("paper", "text", Some("not a colour"));
        a.set_fruit("paper", "pear", Some("#445566"));
        a.pin("x.audio", Some("olive"));
        a.pin("x.text", Some("#010203"));
        a.pin("x.junk", Some("banana"));
        a.text_size = Some(16.0);
        let t = a.resolve("paper");
        assert_eq!(t.accent, Color32::from_rgb(0x11, 0x22, 0x33));
        assert_eq!(t.text, theme::PAPER.text, "a bad colour is skipped");
        assert_eq!(t.fruit("pear"), Some(Color32::from_rgb(0x44, 0x55, 0x66)));
        assert_eq!(t.pins.get("x.audio"), Some(&theme::PAPER.olive));
        assert_eq!(t.pins.get("x.text"), Some(&Color32::from_rgb(1, 2, 3)));
        assert!(!t.pins.contains_key("x.junk"));
        assert_eq!(t.text_size, 16.0);
        // the other themes keep their own colours, but the pins hold everywhere
        let p = a.resolve("pewter");
        assert_eq!(p.accent, theme::PEWTER.accent);
        assert_eq!(p.pins.get("x.audio"), Some(&theme::PEWTER.olive));
        // an old name resolves as its replacement, an unknown one as the default
        assert_eq!(a.resolve("medium").name, "pewter");
        assert_eq!(a.resolve("neon").name, theme::DEFAULT.name);
        a.reset("paper");
        a.set_token("paper", "accent", None);
        assert_eq!(a.resolve("paper").accent, theme::PAPER.accent);
        a.pin("x.audio", None);
        assert!(!a.families.contains_key("x.audio"));
    }

    #[test]
    fn a_theme_file_round_trips_with_only_the_differences() {
        let mut a = Appearance::default();
        a.set_token("dusk", "canvas", Some("#101010"));
        a.set_fruit("dusk", "fig", Some("#202020"));
        let file = a.export("dusk");
        assert_eq!(file["base"], "dusk");
        assert_eq!(file["tokens"], json!({"canvas": "#101010"}));
        assert!(a.export("paper").get("tokens").is_none(), "nothing changed in Paper");
        let mut b = Appearance::default();
        let (base, skipped) = b.import(&file.to_string()).unwrap();
        assert_eq!(base, "dusk");
        assert!(skipped.is_empty());
        assert_eq!(b.resolve("dusk"), a.resolve("dusk"));
    }

    /// Theme files are hostile input: refused or skipped with a reason, never a panic.
    #[test]
    fn a_hostile_theme_file_is_refused_or_skipped() {
        let mut a = Appearance::default();
        let big = format!("{{\"format\": \"{FORMAT}\", \"x\": \"{}\"}}", "y".repeat(MAX_THEME_BYTES));
        for bad in [
            "",
            "{",
            "[]",
            "null",
            "7",
            r#"{"format": "other", "version": 1, "base": "paper"}"#,
            r#"{"format": "plumcraft-theme", "version": 2, "base": "paper"}"#,
            r#"{"format": "plumcraft-theme", "version": "1", "base": "paper"}"#,
            r#"{"format": "plumcraft-theme", "version": 1}"#,
            r#"{"format": "plumcraft-theme", "version": 1, "base": "neon"}"#,
            r#"{"format": "plumcraft-theme", "version": 1, "base": 3}"#,
            big.as_str(),
        ] {
            assert!(a.import(bad).is_err(), "{}", bad.chars().take(60).collect::<String>());
        }
        assert!(a.is_default(), "a refused file changes nothing");
        let (_, skipped) = a
            .import(r##"{"format": "plumcraft-theme", "version": 1, "base": "paper", "tokens": {"accent": 7, "nope": "#000000", "text": "#zzzzzz", "canvas": "#e6e1d6"}, "fruits": {"banana": "#000000", "pear": "#cfe0a8"}, "fruit_l": "x"}"##)
            .unwrap();
        assert_eq!(skipped.len(), 4, "{skipped:?}");
        let t = a.resolve("paper");
        assert_eq!(t.canvas, Color32::from_rgb(0xe6, 0xe1, 0xd6));
        assert_eq!(t.fruit("pear"), Some(Color32::from_rgb(0xcf, 0xe0, 0xa8)));
        // every truncation and every flipped byte of a good file
        let good = a.export("paper").to_string();
        for n in 0..good.len() {
            if let Some(cut) = good.get(..n) {
                let _ = Appearance::default().import(cut);
            }
        }
        let bytes = good.as_bytes();
        for i in 0..bytes.len() {
            let mut b = bytes.to_vec();
            b[i] ^= 0x5a;
            if let Ok(s) = std::str::from_utf8(&b) {
                let mut x = Appearance::default();
                let _ = x.import(s);
                let _ = x.resolve("paper");
            }
        }
    }

    #[test]
    fn junk_in_the_preferences_is_the_defaults() {
        #[derive(Deserialize)]
        struct P {
            #[serde(deserialize_with = "lenient", default)]
            appearance: Appearance,
        }
        for junk in [
            r#"{"appearance": 7}"#,
            r#"{"appearance": []}"#,
            r#"{"appearance": {"text_size": "big"}}"#,
            r#"{"appearance": {"overrides": {"paper": 3}}}"#,
            r#"{"appearance": {"families": [1, 2]}}"#,
            r#"{"appearance": null}"#,
        ] {
            let p: P = serde_json::from_str(junk).unwrap();
            assert!(p.appearance.is_default(), "{junk}");
        }
        let p: P = serde_json::from_str(r#"{"appearance": {"text_size": 99, "zoom": 1.2}}"#).unwrap();
        assert_eq!(p.appearance.text_size, None, "out of range");
        assert_eq!(p.appearance.zoom, Some(1.2));
    }
}
