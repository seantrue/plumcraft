//! Settings as data (N4 slice 4; `docs/shell-interface.md` 14.4): the preferences, the theme and
//! the system's appearance a core keeps for every document of a process ([`Look`]), the
//! settings a shell renders natively ([`model`], `settings.get`), and the settings commands
//! carried out in the core ([`Look::apply`]) for a shell that has no look of its own (the C ABI's
//! apps). The egui editor keeps its own look and window, and answers `settings.get` from the same
//! [`model`].
//!
//! Every setting is changed through a command a script and the control channel can also run
//! (golden rule 4): an item names its `command`, the `param` its value goes in and any fixed
//! `params`. Nothing lives only in a shell's own defaults.
//!
//! **The theme** is the one the preferences name, or `system` (the default for a native shell):
//! Paper in light mode and Dusk in dark, following `lifecycle.appearance {dark}` (contract 14.2).
//! The system's accessibility settings change the tokens where they can: **Increase Contrast**
//! moves the dim text, borders, wires and grid towards the text colour ([`more_contrast`]);
//! **Reduce Transparency** makes every token opaque over the canvas. Reduce Motion is the shell's
//! (the run's pulse is drawn by it); the core reports it.

use std::sync::{Arc, Mutex, MutexGuard};

use serde_json::{Value, json};

use crate::appearance::{Appearance, MAX_THEME_BYTES};
use crate::prefs;
use crate::theme::{self, FRUITS, OLIVE, PLUM, TOKENS, Tokens};

/// The theme name that follows the system.
pub const SYSTEM: &str = "system";
/// Animation speeds View › Animation Speed offers, in ms (0 is real time).
pub const SPEEDS: [u32; 5] = [0, 250, 500, 1000, 2000];

/// What the system says of its appearance (`lifecycle.appearance`, `display.*`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct System {
    pub dark: bool,
    pub more_contrast: bool,
    pub reduce_motion: bool,
    pub reduce_transparency: bool,
}

impl System {
    pub fn json(&self) -> Value {
        json!({"dark": self.dark, "contrast": if self.more_contrast { "more" } else { "normal" },
               "reduceMotion": self.reduce_motion, "reduceTransparency": self.reduce_transparency})
    }

    /// `lifecycle.appearance {dark?, contrast?: "more" | "normal", reduceMotion?, reduceTransparency?}`:
    /// what it says, over what was; an error for a field of the wrong shape or an unknown one.
    pub fn with(mut self, p: &Value) -> Result<System, String> {
        crate::control::check_fields(p, &["dark", "contrast", "reduceMotion", "reduceTransparency"])?;
        let flag = |key: &str, now: bool| match p.get(key) {
            None | Some(Value::Null) => Ok(now),
            Some(Value::Bool(b)) => Ok(*b),
            Some(_) => Err(format!("{key} must be true or false")),
        };
        self.dark = flag("dark", self.dark)?;
        self.reduce_motion = flag("reduceMotion", self.reduce_motion)?;
        self.reduce_transparency = flag("reduceTransparency", self.reduce_transparency)?;
        self.more_contrast = match p.get("contrast") {
            None | Some(Value::Null) => self.more_contrast,
            Some(Value::String(c)) if c == "more" => true,
            Some(Value::String(c)) if c == "normal" => false,
            Some(_) => return Err("contrast must be \"more\" or \"normal\"".into()),
        };
        Ok(self)
    }
}

/// The look every document of a core shares: the preferences (kept in their file), the system's
/// appearance, and a generation that rises with every change, which each document compares
/// with what it last showed.
#[derive(Debug, Default)]
pub struct Look {
    pub store: prefs::Store,
    pub system: System,
    /// Offer the UI zoom (`view.zoom`): a desktop that scales its own window (egui). The Apple
    /// shells don't: the system scales the display, and the canvas has its own zoom.
    pub ui_zoom: bool,
    generation: u64,
}

/// A look shared by a core's documents.
pub type SharedLook = Arc<Mutex<Look>>;

/// Lock a shared look, taking a poisoned lock as it is.
pub fn lock(l: &SharedLook) -> MutexGuard<'_, Look> {
    l.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A theme's name as kept: `system` or a built-in theme's (old names read as theirs).
fn chosen_name(name: &str) -> Option<&'static str> {
    if name == SYSTEM { Some(SYSTEM) } else { theme::by_name(name).map(|t| t.name) }
}

impl Look {
    pub fn new(store: prefs::Store, system: System) -> Look {
        Look { store, system, ui_zoom: false, generation: 1 }
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }

    fn bump(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    /// The theme chosen: the preferences', else following the system.
    pub fn chosen(&self) -> &'static str {
        self.store.prefs().theme.as_deref().and_then(chosen_name).unwrap_or(SYSTEM)
    }

    /// The built-in theme shown: the chosen one, or for `system` Paper in light and Dusk in dark.
    pub fn shown(&self) -> &'static str {
        match self.chosen() {
            SYSTEM if self.system.dark => theme::DUSK.name,
            SYSTEM => theme::PAPER.name,
            name => name,
        }
    }

    /// The tokens everything is drawn in: the theme shown with the person's changes, then what
    /// the system's accessibility settings ask.
    pub fn tokens(&self) -> Tokens {
        let mut t = self.store.prefs().appearance.resolve(self.shown());
        if self.system.more_contrast {
            more_contrast(&mut t);
        }
        if self.system.reduce_transparency {
            opaque(&mut t);
        }
        t
    }

    /// The system's appearance changed: true when it changes what is shown.
    pub fn set_system(&mut self, system: System) -> bool {
        if system == self.system {
            return false;
        }
        self.system = system;
        self.bump();
        true
    }

    /// The run's look and engine picking as kept (`view.runColors`, `view.animationSpeed`,
    /// `engine.auto`), for a document to take: `(run colours on, animation ms, pick engines)`.
    pub fn run_prefs(&self) -> (Option<bool>, Option<u32>, Option<bool>) {
        let p = self.store.prefs();
        (p.run_colors, p.anim_ms, p.auto_engines)
    }

    fn change_appearance(&mut self, f: impl FnOnce(&mut Appearance)) {
        let mut a = self.store.prefs().appearance.clone();
        f(&mut a);
        if a != self.store.prefs().appearance {
            self.store.set_appearance(a);
            self.bump();
        }
    }

    /// Keep the one run setting a command changed in a document (`view.runColors`,
    /// `view.animationSpeed` or `engine.auto`), so every document and the next launch have it.
    /// Only that one: the document's other two may be stale (it has not followed the look since
    /// another document changed them), and writing them back would lose that change (B41).
    pub fn keep_run_pref(&mut self, kept: RunPref) {
        let p = self.store.prefs();
        let (colors, anim, auto) = (p.run_colors, p.anim_ms, p.auto_engines);
        match kept {
            RunPref::Colors(c) if colors != Some(c) => self.store.set_run_colors(c),
            RunPref::Anim(ms) if anim != ms => self.store.set_anim_ms(ms),
            RunPref::Auto(a) if auto != Some(a) => self.store.set_auto_engines(a),
            _ => return,
        }
        self.bump();
    }

    /// One settings command, its shape already checked by the session (`view.theme`,
    /// `view.textSize`, `view.zoom`, `theme.setToken`, `theme.setFruit`, `theme.pinFamily`,
    /// `theme.reset`, `theme.import`, `theme.export`): carried out on the preferences, and its
    /// reply, as the egui editor's Settings window gives it.
    pub fn apply(&mut self, id: &str, p: &Value, catalog: &plumcraft_proto::catalogue::SchemaReply) -> Result<Value, String> {
        let current = self.shown();
        let theme_of = |p: &Value| -> Result<&'static str, String> {
            match p.get("theme").and_then(Value::as_str) {
                None => Ok(current),
                Some(n) => theme::by_name(n).map(|t| t.name).ok_or_else(|| format!("no theme called {:?}", n.chars().take(64).collect::<String>())),
            }
        };
        let color = |p: &Value| p.get("color").and_then(Value::as_str).map(str::to_owned);
        match id {
            "view.theme" => {
                let name = p.get("name").and_then(Value::as_str).unwrap_or_default();
                let Some(kept) = chosen_name(name) else {
                    return Err(format!("no theme called {:?}; there are system, {}", name.chars().take(64).collect::<String>(), theme_names().join(", ")));
                };
                if self.store.prefs().theme.as_deref() != Some(kept) {
                    self.store.set_theme(kept);
                    self.bump();
                }
                Ok(json!({"theme": kept, "shown": self.shown()}))
            }
            "view.textSize" => {
                let size = p.get("size").and_then(Value::as_f64).map(|s| s as f32);
                self.change_appearance(|a| a.text_size = size);
                Ok(json!({"textSize": size.unwrap_or(theme::BASE_TEXT)}))
            }
            "view.zoom" => {
                let zoom = p.get("zoom").and_then(Value::as_f64).map(|z| z as f32);
                self.change_appearance(|a| a.zoom = zoom);
                Ok(json!({"zoom": zoom.unwrap_or(1.0)}))
            }
            "theme.setToken" => {
                let theme = theme_of(p)?;
                let token = p.get("token").and_then(Value::as_str).unwrap_or_default();
                if theme::PAPER.token(token).is_none() {
                    let names: Vec<&str> = TOKENS.iter().flat_map(|(_, t)| t.iter().copied()).collect();
                    return Err(format!("no token called {:?}; there are {}", token.chars().take(64).collect::<String>(), names.join(", ")));
                }
                let c = color(p);
                self.change_appearance(|a| a.set_token(theme, token, c.as_deref()));
                let t = self.store.prefs().appearance.resolve(theme);
                let checks: Vec<Value> = t
                    .checks_of(token)
                    .into_iter()
                    .map(|c| json!({"fg": c.fg, "bg": c.bg, "ratio": (c.ratio * 100.0).round() / 100.0, "need": c.need}))
                    .collect();
                Ok(json!({"theme": theme, "token": token, "color": t.token(token).map(theme::hex), "checks": checks}))
            }
            "theme.setFruit" => {
                let theme = theme_of(p)?;
                let fruit = p.get("fruit").and_then(Value::as_str).unwrap_or_default();
                if theme::PAPER.fruit(fruit).is_none() {
                    return Err(format!(
                        "no fruit called {:?}; there are {}, {OLIVE} and {PLUM}",
                        fruit.chars().take(64).collect::<String>(),
                        FRUITS.join(", ")
                    ));
                }
                let c = color(p);
                self.change_appearance(|a| a.set_fruit(theme, fruit, c.as_deref()));
                let t = self.store.prefs().appearance.resolve(theme);
                Ok(json!({"theme": theme, "fruit": fruit, "color": t.fruit(fruit).map(theme::hex)}))
            }
            "theme.pinFamily" => {
                let family = p.get("family").and_then(Value::as_str).unwrap_or_default();
                let to = p.get("to").and_then(Value::as_str);
                let t = self.tokens();
                if let Some(to) = to
                    && t.fruit(to).is_none()
                    && theme::parse_hex(to).is_none()
                {
                    return Err(format!(
                        "{:?} is neither a fruit ({}, {OLIVE}, {PLUM}) nor a #rrggbb",
                        to.chars().take(64).collect::<String>(),
                        FRUITS.join(", ")
                    ));
                }
                self.change_appearance(|a| a.pin(family, to));
                let header = self.tokens().family_tint(catalog, family).header;
                Ok(json!({"family": family, "to": to, "fruit": theme::hex(header)}))
            }
            "theme.reset" => {
                let theme = theme_of(p)?;
                self.change_appearance(|a| a.reset(theme));
                Ok(json!({"theme": theme}))
            }
            "theme.import" => {
                use std::io::Read;
                let path = p.get("path").and_then(Value::as_str).unwrap_or_default();
                let file = std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
                let mut text = String::new();
                file.take(MAX_THEME_BYTES as u64 + 1).read_to_string(&mut text).map_err(|e| format!("{path}: {e}"))?;
                let mut a = self.store.prefs().appearance.clone();
                let (base, skipped) = a.import(&text)?;
                self.store.set_appearance(a);
                self.store.set_theme(base);
                self.bump();
                Ok(json!({"theme": base, "skipped": skipped}))
            }
            "theme.export" => {
                let theme = theme_of(p)?;
                let path = p.get("path").and_then(Value::as_str).unwrap_or_default();
                let text = serde_json::to_string_pretty(&self.store.prefs().appearance.export(theme)).map_err(|e| e.to_string())?;
                plumcraft_session::file_cmds::write_atomic(std::path::Path::new(path), &text).map_err(|e| format!("{path}: {e}"))?;
                Ok(json!({"path": path, "theme": theme}))
            }
            other => Err(format!("{other} is not one of the settings' commands")),
        }
    }
}

fn theme_names() -> Vec<&'static str> {
    theme::ALL.iter().map(|t| t.name).collect()
}

/// Increase Contrast: dim text, borders, wires, pins and the grid moved towards the text colour,
/// so every pair the contrast checks name is at least as far apart as it was.
pub fn more_contrast(t: &mut Tokens) {
    let text = t.text;
    for (name, by) in [("text_dim", 0.5), ("text_faint", 0.5), ("separator", 0.35), ("field_border", 0.4), ("wire", 0.35), ("grid", 0.2), ("pin_default", 0.35)]
    {
        if let Some(c) = t.token_mut(name) {
            *c = theme::mix(*c, text, by);
        }
    }
}

/// Reduce Transparency: every token with some transparency made opaque, as it shows over the
/// canvas.
pub fn opaque(t: &mut Tokens) {
    let canvas = t.canvas;
    let names: Vec<&str> = TOKENS.iter().flat_map(|(_, n)| n.iter().copied()).collect();
    for name in names {
        if let Some(c) = t.token_mut(name)
            && c.a < 255
        {
            let alpha = f32::from(c.a) / 255.0;
            let mut solid = *c;
            solid.a = 255;
            *c = theme::mix(canvas, solid, alpha);
            c.a = 255;
        }
    }
}

/// One run setting a document changed, for [`Look::keep_run_pref`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunPref {
    /// View › Colour Nodes While Running.
    Colors(bool),
    /// View › Animation Speed, in ms (`None`: the default).
    Anim(Option<u32>),
    /// Pick Engines Automatically.
    Auto(bool),
}

/// What a shell tells the model of itself.
#[derive(Debug, Clone, Copy, Default)]
pub struct Facts {
    /// Offer the UI zoom ([`Look::ui_zoom`]).
    pub ui_zoom: bool,
    /// Offer the text size: a shell whose canvas draws with it (the egui editor). The Apple apps
    /// draw faces at the platform's sizes and follow the system's text size, so they don't offer
    /// one that would change nothing there (N4 slice 6); `view.textSize` is kept for scripts.
    pub text_size: bool,
    /// Pick Engines Automatically is on in this document.
    pub auto_engines: bool,
    /// View › Colour Nodes While Running is on.
    pub run_colors: bool,
    /// View › Animation Speed, in ms.
    pub anim_ms: u32,
    /// The Cache Report is open.
    pub cache_report: bool,
}

/// One item: `{id, label, kind, value, command, param?, params?, …}`.
fn item(id: &str, label: &str, kind: &str, value: Value, command: &str, param: Option<&str>) -> Value {
    let mut v = json!({"id": id, "label": label, "kind": kind, "value": value, "command": command});
    if let Some(p) = param {
        v["param"] = json!(p);
    }
    v
}

/// The worst contrast pair a token is in, as the egui window's badge says it.
fn contrast_of(t: &Tokens, token: &str) -> Option<Value> {
    let checks = t.checks_of(token);
    let worst = checks.iter().min_by(|a, b| (a.ratio / a.need).total_cmp(&(b.ratio / b.need)))?;
    let other = if worst.fg == token { worst.bg } else { worst.fg };
    let level = if !worst.passes() {
        "fail"
    } else if worst.ratio < 4.5 {
        "low"
    } else {
        "ok"
    };
    Some(json!({"ratio": (worst.ratio * 10.0).round() / 10.0, "with": other, "need": worst.need, "level": level,
                "text": format!("{:.1}:1 with {other}", worst.ratio)}))
}

fn label_of(name: &str) -> String {
    let mut s = name.replace('_', " ");
    if let Some(f) = s.get(0..1) {
        let up = f.to_uppercase();
        s.replace_range(0..1, &up);
    }
    s
}

/// The settings as a shell renders them (`settings.get`): `{chosen, shown, system, gen,
/// sections: [{id, label, note?, items: [item]}]}`. An item is `{id, label, kind, value, command,
/// param?, params?, …}`: kind `choice` (`options: [{value, label}]`), `number` (`min`, `max`,
/// `step`, `unit`, `default`), `toggle`, `color` (`"#rrggbb"`, `default`, `changed`,
/// `contrast`), `action` (a button: `invoke: true` runs it as `ui.invoke` does, asking what it
/// needs), `note` (text alone); `group` names a heading within the section; `reset` an item's
/// own reset (`{command, params}`). A shell sets an item with `command.run {command, params:
/// params + {param: value}}`.
pub fn model(
    prefs: &prefs::Prefs,
    chosen: &str,
    shown_tokens: &Tokens,
    system: &System,
    generation: u64,
    catalog: &plumcraft_proto::catalogue::SchemaReply,
    f: &Facts,
) -> Value {
    let a = &prefs.appearance;
    let t = shown_tokens;
    let mut options = vec![json!({"value": SYSTEM, "label": "Match the System"})];
    options.extend(theme::ALL.iter().map(|th| json!({"value": th.name, "label": th.label})));
    let mut theme_item = item("theme", "Theme", "choice", json!(chosen), "view.theme", Some("name"));
    theme_item["options"] = json!(options);
    theme_item["note"] = json!(format!("Match the System shows Paper in light mode and Dusk in dark; now {}", t.label));
    let mut text = item("textSize", "Text size", "number", json!(a.text_size.unwrap_or(theme::BASE_TEXT)), "view.textSize", Some("size"));
    text["min"] = json!(theme::TEXT_SIZES.start());
    text["max"] = json!(theme::TEXT_SIZES.end());
    text["step"] = json!(0.5);
    text["unit"] = json!("pt");
    text["default"] = json!(theme::BASE_TEXT);
    let mut appearance = vec![theme_item];
    if f.text_size {
        appearance.push(text);
    }
    if f.ui_zoom {
        let mut z = item("zoom", "Zoom", "number", json!(a.zoom.unwrap_or(1.0)), "view.zoom", Some("zoom"));
        z["min"] = json!(theme::ZOOMS.start());
        z["max"] = json!(theme::ZOOMS.end());
        z["step"] = json!(0.05);
        z["unit"] = json!("×");
        z["default"] = json!(1.0);
        appearance.push(z);
    }
    let mut acc = vec![];
    for (id, label, on) in [
        ("contrast", "Increase Contrast", system.more_contrast),
        ("transparency", "Reduce Transparency", system.reduce_transparency),
        ("motion", "Reduce Motion", system.reduce_motion),
    ] {
        let mut n = item(id, label, "note", json!(if on { "On" } else { "Off" }), "", None);
        n["group"] = json!("From the system's accessibility settings");
        if let Some(o) = n.as_object_mut() {
            o.remove("command");
        }
        acc.push(n);
    }
    appearance.extend(acc);
    // a theme's colours, by the Settings window's groups
    let base = theme::by_name(t.name).unwrap_or(theme::DEFAULT);
    let overridden = a.overrides.get(t.name).cloned().unwrap_or_default();
    let mut colours = Vec::new();
    for (group, names) in TOKENS {
        for name in *names {
            let Some(c) = t.token(name) else { continue };
            let mut it = item(&format!("token.{name}"), &label_of(name), "color", json!(theme::hex(c)), "theme.setToken", Some("color"));
            it["params"] = json!({"token": name, "theme": t.name});
            it["group"] = json!(group);
            it["default"] = json!(base.token(name).map(theme::hex));
            it["changed"] = json!(overridden.contains_key(*name));
            it["reset"] = json!({"command": "theme.setToken", "params": {"token": name, "theme": t.name, "color": null}});
            if let Some(c) = contrast_of(t, name) {
                it["contrast"] = c;
            }
            colours.push(it);
        }
    }
    let changed_fruits = a.fruits.get(t.name).cloned().unwrap_or_default();
    let mut families = Vec::new();
    for name in FRUITS.iter().chain([OLIVE, PLUM].iter()) {
        let Some(c) = t.fruit(name) else { continue };
        let mut it = item(&format!("fruit.{name}"), &label_of(name), "color", json!(theme::hex(c)), "theme.setFruit", Some("color"));
        it["params"] = json!({"fruit": name, "theme": t.name});
        it["group"] = json!("Fruits");
        it["default"] = json!(base.fruit(name).map(theme::hex));
        it["changed"] = json!(changed_fruits.contains_key(*name));
        it["reset"] = json!({"command": "theme.setFruit", "params": {"fruit": name, "theme": t.name, "color": null}});
        families.push(it);
    }
    let fams: std::collections::BTreeSet<&str> = catalog.nodes.iter().map(|n| n.family.as_str()).filter(|f| !f.is_empty()).collect();
    for fam in fams {
        let own = t.family_fruit(catalog, fam).unwrap_or("default");
        let mut opts = vec![json!({"value": null, "label": format!("Its own ({own})")})];
        opts.extend(FRUITS.iter().chain([OLIVE].iter()).map(|f| json!({"value": f, "label": label_of(f)})));
        let mut it = item(&format!("family.{fam}"), fam, "choice", json!(a.families.get(fam)), "theme.pinFamily", Some("to"));
        it["params"] = json!({"family": fam});
        it["options"] = json!(opts);
        it["group"] = json!("This catalogue's families");
        it["swatch"] = json!(theme::hex(t.family_tint(catalog, fam).header));
        families.push(it);
    }
    let any_changed = !overridden.is_empty() || !changed_fruits.is_empty();
    let mut files = vec![
        {
            let mut i = item("import", "Import Theme…", "action", Value::Null, "theme.import", None);
            i["invoke"] = json!(true);
            i
        },
        {
            let mut i = item("export", "Export Theme…", "action", Value::Null, "theme.export", None);
            i["invoke"] = json!(true);
            i
        },
    ];
    let mut reset = item("reset", &format!("Reset {}", t.label), "action", Value::Null, "theme.reset", None);
    reset["params"] = json!({"theme": t.name});
    reset["enabled"] = json!(any_changed);
    files.push(reset);
    let mut speed = item("animationSpeed", "Animation speed", "choice", json!(f.anim_ms), "view.animationSpeed", Some("ms"));
    speed["options"] =
        json!(SPEEDS.iter().map(|ms| json!({"value": ms, "label": if *ms == 0 { "Real Time".to_owned() } else { format!("{ms} ms") }})).collect::<Vec<_>>());
    let running = vec![item("runColors", "Colour Nodes While Running", "toggle", json!(f.run_colors), "view.runColors", Some("on")), speed];
    let mut auto = item("autoEngines", "Pick Engines Automatically", "toggle", json!(f.auto_engines), "engine.auto", Some("on"));
    auto["note"] = json!("Start the engines a workflow needs when it is opened or inserted, each hosted the fastest way it can be");
    let mut cache =
        item("cacheReport", if f.cache_report { "Close Cache Report" } else { "Open Cache Report" }, "action", Value::Null, "run.cacheReport", None);
    cache["params"] = json!({"open": !f.cache_report});
    json!({
        "chosen": chosen,
        "shown": t.name,
        "system": system.json(),
        "gen": generation,
        "sections": [
            {"id": "appearance", "label": "Appearance", "items": appearance},
            {"id": "colours", "label": format!("Colours of {}", t.label), "note": "Changes are kept per theme", "items": colours},
            {"id": "families", "label": "Node Families", "note": "Each family takes one of twelve fruits; olive is the quiet one, plum is the groups'", "items": families},
            {"id": "files", "label": "Theme Files", "note": "A .plumtheme.json holds a theme's changes from its built-in colours", "items": files},
            {"id": "running", "label": "Running", "items": running},
            {"id": "engines", "label": "Engines", "items": [auto]},
            {"id": "cache", "label": "Cache", "note": "What the caches did: the latest run, this session, each engine and each node", "items": [cache]},
        ],
    })
}

/// The commands whose effect is the look's (carried out by [`Look::apply`]).
pub fn is_look_command(id: &str) -> bool {
    matches!(
        id,
        "view.theme"
            | "view.textSize"
            | "view.zoom"
            | "theme.setToken"
            | "theme.setFruit"
            | "theme.pinFamily"
            | "theme.reset"
            | "theme.import"
            | "theme.export"
    )
}

/// After a command ran in a document with a shared look (the C ABI's): carry out what it left for
/// the look (the session leaves settings commands in `views.ui_asks` and `view.theme` in
/// `views.theme_asked`), keep the run look and engine picking, and give the reply.
pub fn after_command(d: &mut crate::document::Document, look: &SharedLook, id: &str, v: Value) -> Result<Value, String> {
    let asks = std::mem::take(&mut d.session.views.ui_asks);
    let theme = d.session.views.theme_asked.take();
    let mut out = v;
    let mut l = lock(look);
    if let Some(name) = theme {
        out = l.apply("view.theme", &json!({"name": name}), d.session.catalog())?;
    }
    for (ask, p) in asks {
        out = l.apply(&ask, &p, d.session.catalog())?;
    }
    let kept = match id {
        "view.runColors" => Some(RunPref::Colors(!d.session.views.run_colors_off)),
        "view.animationSpeed" => Some(RunPref::Anim(d.session.views.anim_ms)),
        "engine.auto" => Some(RunPref::Auto(d.session.engines.auto)),
        _ => None,
    };
    if let Some(kept) = kept {
        // only what the command changed is kept; the document then takes the rest from the look
        // (it may not have followed since another document changed them)
        l.keep_run_pref(kept);
        drop(l);
        follow(d);
        l = lock(look);
    }
    if id == "app.settings"
        && let (Some(o), Value::Object(more)) = (out.as_object_mut(), model_of(d, &l))
    {
        o.extend(more);
    }
    Ok(out)
}

/// A document's settings model, from the shared look.
pub fn model_of(d: &crate::document::Document, l: &Look) -> Value {
    model(l.store.prefs(), l.chosen(), &l.tokens(), &l.system, l.generation(), d.session.catalog(), &facts_of(d, l))
}

fn facts_of(d: &crate::document::Document, l: &Look) -> Facts {
    let views = &d.session.views;
    Facts {
        ui_zoom: l.ui_zoom,
        // the C ABI's shells draw faces at the platform's sizes (N4 slice 6)
        text_size: false,
        auto_engines: d.session.engines.auto,
        run_colors: !views.run_colors_off,
        anim_ms: views.anim_ms(),
        cache_report: views.cache_report,
    }
}

/// A document takes the look's run look and engine picking when the look changed since it last
/// looked: true when it did.
pub fn follow(d: &mut crate::document::Document) -> bool {
    let Some(look) = d.look.clone() else { return false };
    let l = lock(&look);
    if d.look_seen == l.generation() {
        return false;
    }
    d.look_seen = l.generation();
    let (colors, anim, auto) = l.run_prefs();
    let views = &mut d.session.views;
    if let Some(c) = colors {
        views.run_colors_off = !c;
    }
    views.anim_ms = anim;
    if let Some(a) = auto {
        d.session.engines.auto = a;
    }
    true
}

/// `settings.get {fromDisk?}`: the model; with `fromDisk: true`, of the preferences as their file
/// holds them now (read again, nothing changed), so a script sees what a reopen would.
pub fn get(d: &crate::document::Document, p: &Value) -> Result<Value, String> {
    crate::control::check_fields(p, &["fromDisk"])?;
    let Some(look) = &d.look else { return Err(crate::control::not_here("settings.get")) };
    let l = lock(look);
    match p.get("fromDisk") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => Ok(model_of(d, &l)),
        Some(Value::Bool(true)) => {
            let Some(path) = l.store.path() else {
                let mut v = model_of(d, &l);
                v["persisted"] = json!(false);
                return Ok(v);
            };
            let read = Look { store: prefs::Store::at(path.to_path_buf()), system: l.system, ui_zoom: l.ui_zoom, generation: l.generation() };
            // the run settings as the file keeps them, not as this document has them now
            let mut facts = facts_of(d, &read);
            let (colors, anim, auto) = read.run_prefs();
            facts.run_colors = colors.unwrap_or(true);
            facts.anim_ms = anim.unwrap_or(plumcraft_session::view_cmds::DEFAULT_ANIM_MS);
            facts.auto_engines = auto.unwrap_or(facts.auto_engines);
            let p = read.store.prefs();
            let mut v = model(p, read.chosen(), &read.tokens(), &read.system, read.generation(), d.session.catalog(), &facts);
            v["persisted"] = json!(true);
            Ok(v)
        }
        Some(_) => Err("fromDisk must be true or false".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn look_at(name: &str) -> (Look, std::path::PathBuf) {
        let path = plumcraft_scratch::dir("settings").join(format!("{name}.json"));
        let _ = std::fs::remove_file(&path);
        (Look::new(prefs::Store::at(path.clone()), System::default()), path)
    }

    fn cat() -> plumcraft_proto::catalogue::SchemaReply {
        plumcraft_proto::catalogue::SchemaReply::default()
    }

    #[test]
    fn the_theme_follows_the_system_until_one_is_chosen_and_is_kept() {
        let (mut l, path) = look_at("follow");
        assert_eq!((l.chosen(), l.shown()), (SYSTEM, "paper"));
        assert!(l.set_system(System { dark: true, ..System::default() }));
        assert_eq!(l.shown(), "dusk");
        assert!(!l.set_system(l.system), "the same appearance changes nothing");
        let g = l.generation();
        assert_eq!(l.apply("view.theme", &json!({"name": "pewter"}), &cat()).unwrap()["shown"], json!("pewter"));
        assert!(l.generation() > g);
        assert_eq!(l.shown(), "pewter", "a theme chosen stays whatever the system");
        assert!(l.apply("view.theme", &json!({"name": "chartreuse"}), &cat()).is_err());
        // kept: a new look on the same file has it
        let again = Look::new(prefs::Store::at(path), System::default());
        assert_eq!(again.chosen(), "pewter");
    }

    #[test]
    fn colour_changes_are_kept_per_theme_and_reset() {
        let (mut l, path) = look_at("colours");
        let r = l.apply("theme.setToken", &json!({"token": "accent", "color": "#123456"}), &cat()).unwrap();
        assert_eq!(r["color"], json!("#123456"));
        assert_eq!(theme::hex(l.tokens().accent), "#123456");
        assert!(l.apply("theme.setToken", &json!({"token": "nope", "color": "#123456"}), &cat()).is_err());
        l.apply("theme.setFruit", &json!({"fruit": "fig", "color": "#00ff00"}), &cat()).unwrap();
        let m = model(l.store.prefs(), l.chosen(), &l.tokens(), &l.system, l.generation(), &cat(), &Facts::default());
        let colours = &m["sections"][1]["items"];
        let accent = colours.as_array().unwrap().iter().find(|i| i["id"] == "token.accent").unwrap();
        assert_eq!((accent["value"].as_str(), accent["changed"].as_bool()), (Some("#123456"), Some(true)));
        assert!(accent["contrast"]["text"].as_str().is_some());
        let again = Look::new(prefs::Store::at(path.clone()), System::default());
        assert_eq!(theme::hex(again.tokens().accent), "#123456", "kept in the file");
        l.apply("theme.reset", &json!({}), &cat()).unwrap();
        assert_ne!(theme::hex(l.tokens().accent), "#123456");
        let export = plumcraft_scratch::dir("settings").join("t.plumtheme.json");
        l.apply("theme.setToken", &json!({"token": "wire", "color": "#abcdef"}), &cat()).unwrap();
        l.apply("theme.export", &json!({"path": export}), &cat()).unwrap();
        l.apply("theme.reset", &json!({}), &cat()).unwrap();
        assert_eq!(l.apply("theme.import", &json!({"path": export}), &cat()).unwrap()["theme"], json!("paper"));
        assert_eq!(theme::hex(l.tokens().wire), "#abcdef");
    }

    /// B41: a document that had not yet followed the look ran a run setting (here Pick Engines
    /// Automatically, as every document does once its first engine is there) and wrote its stale
    /// copy of all three back, losing the Animation Speed another document had just set.
    #[test]
    fn a_run_setting_changed_in_one_document_survives_another_that_has_not_followed_yet() {
        use crate::document::{self, Document};
        let (l, path) = look_at("b41");
        let look: SharedLook = Arc::new(Mutex::new(l));
        let open = || {
            let mut d = Document::new(plumcraft_session::Session::new(plumcraft_text::engine().schema()));
            d.look = Some(Arc::clone(&look));
            follow(&mut d);
            d
        };
        let (mut a, mut b) = (open(), open());
        document::execute(&mut a, "view.animationSpeed", &json!({"ms": 1000})).unwrap();
        // b runs before it follows (the core's tick had not reached it)
        document::execute(&mut b, "engine.auto", &json!({"on": true})).unwrap();
        assert_eq!(lock(&look).run_prefs().1, Some(1000), "b wrote its stale animation speed back");
        assert_eq!(b.session.views.anim_ms(), 1000, "b takes the rest from the look as it keeps its own");
        follow(&mut a);
        assert_eq!(a.session.views.anim_ms(), 1000);
        // and the file, which a reopen reads, has it, as settings.get {fromDisk} says
        assert_eq!(Look::new(prefs::Store::at(path), System::default()).run_prefs().1, Some(1000));
        let disk = get(&b, &json!({"fromDisk": true})).unwrap();
        let running = disk["sections"].as_array().unwrap().iter().find(|s| s["id"] == "running").unwrap().clone();
        assert_eq!(running["items"][1]["value"], 1000, "{running}");
    }

    #[test]
    fn increased_contrast_never_lowers_a_pair_and_reduced_transparency_is_opaque() {
        for base in theme::ALL {
            let before = base.clone();
            let mut more = base.clone();
            more_contrast(&mut more);
            for (b, m) in before.checks().iter().zip(more.checks().iter()) {
                assert!(m.ratio + 1e-6 >= b.ratio, "{} on {} in {}: {} -> {}", b.fg, b.bg, base.name, b.ratio, m.ratio);
            }
            let mut o = base.clone();
            opaque(&mut o);
            for (_, names) in TOKENS {
                for n in *names {
                    assert_eq!(o.token(n).map(|c| c.a), Some(255), "{n} in {}", base.name);
                }
            }
        }
    }

    #[test]
    fn the_model_names_a_command_for_every_setting() {
        let (l, _) = look_at("model");
        let m = model(
            l.store.prefs(),
            l.chosen(),
            &l.tokens(),
            &l.system,
            l.generation(),
            &cat(),
            &Facts { anim_ms: 500, run_colors: true, text_size: true, ..Facts::default() },
        );
        let mut kinds = std::collections::BTreeSet::new();
        for s in m["sections"].as_array().unwrap() {
            for it in s["items"].as_array().unwrap() {
                let kind = it["kind"].as_str().unwrap();
                kinds.insert(kind.to_owned());
                if kind != "note" {
                    let c = it["command"].as_str().unwrap();
                    assert!(plumcraft_session::find(c).is_some() || is_look_command(c), "{c}");
                }
            }
        }
        assert_eq!(kinds.into_iter().collect::<Vec<_>>(), ["action", "choice", "color", "note", "number", "toggle"]);
        assert!(m["sections"][0]["items"].as_array().unwrap().iter().all(|i| i["id"] != "zoom"), "no UI zoom on a shell that doesn't scale");
        let apple = model(l.store.prefs(), l.chosen(), &l.tokens(), &l.system, l.generation(), &cat(), &Facts::default());
        assert!(apple["sections"][0]["items"].as_array().unwrap().iter().all(|i| i["id"] != "textSize"), "no text size where the canvas doesn't draw with it");
        let sys = System::default().with(&json!({"dark": true, "contrast": "more"})).unwrap();
        assert!(sys.dark && sys.more_contrast);
        for bad in [json!({"dark": "yes"}), json!({"contrast": "max"}), json!({"other": true})] {
            assert!(System::default().with(&bad).is_err(), "{bad}");
        }
    }
}
