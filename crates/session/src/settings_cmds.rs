//! The Settings window (`app.settings`, File › Settings…) and the appearance commands its
//! controls dispatch (docs/research/22-themes.md section 6). The look is the window's, not the
//! session's: each command checks its parameters' shape and leaves the request in
//! [`crate::view_cmds::Views::ui_asks`], which the editor carries out at once and answers for (it
//! knows the themes' tokens and fruits). Headless, a request does nothing. None is journaled.

use serde_json::{Value, json};

use crate::commands::{CommandSpec, always, opt_str_param, str_param};
use crate::{Result, Session, SessionError};

/// The Settings window's tabs.
pub const TABS: &[&str] = &["appearance", "engines", "cache"];
/// The text sizes the editor takes, in points (the editor's `theme::TEXT_SIZES`).
pub const TEXT_SIZES: std::ops::RangeInclusive<f64> = 11.0..=18.0;
/// The UI zooms it takes (`theme::ZOOMS`).
pub const ZOOMS: std::ops::RangeInclusive<f64> = 0.8..=1.6;
/// The most requests kept for the window (a headless session has no window to take them).
const MAX_ASKS: usize = 64;
/// The longest name taken (a token, fruit, family or theme).
const MAX_NAME: usize = 128;

fn ask(s: &mut Session, id: &str, p: &Value) -> Value {
    if s.views.ui_asks.len() >= MAX_ASKS {
        s.views.ui_asks.remove(0);
    }
    s.views.ui_asks.push((id.to_owned(), p.clone()));
    json!({"asked": id})
}

fn name<'a>(p: &'a Value, key: &str) -> Result<&'a str> {
    let v = str_param(p, key)?;
    if v.is_empty() || v.chars().count() > MAX_NAME {
        return Err(SessionError::Params(format!("\"{key}\" must be 1 to {MAX_NAME} characters")));
    }
    Ok(v)
}

/// A colour (`#rrggbb`) or null (reset).
fn color(p: &Value, key: &str) -> Result<()> {
    match p.get(key) {
        None => Err(SessionError::Params(format!("\"{key}\" is required: \"#rrggbb\", or null to reset"))),
        Some(Value::Null) => Ok(()),
        Some(Value::String(c)) if is_hex(c) => Ok(()),
        Some(_) => Err(SessionError::Params(format!("\"{key}\" must be \"#rrggbb\" or null"))),
    }
}

fn is_hex(c: &str) -> bool {
    c.len() == 7 && c.starts_with('#') && c.bytes().skip(1).all(|b| b.is_ascii_hexdigit())
}

fn number_in(p: &Value, key: &str, range: &std::ops::RangeInclusive<f64>) -> Result<()> {
    match p.get(key) {
        Some(Value::Null) => Ok(()),
        Some(v) if v.as_f64().is_some_and(|x| range.contains(&x)) => Ok(()),
        _ => Err(SessionError::Params(format!("\"{key}\" must be a number from {} to {}, or null for the default", range.start(), range.end()))),
    }
}

fn theme_param(p: &Value) -> Result<()> {
    if let Some(t) = opt_str_param(p, "theme")?
        && (t.is_empty() || t.chars().count() > MAX_NAME)
    {
        return Err(SessionError::Params("\"theme\" must be a theme's name".into()));
    }
    Ok(())
}

/// `app.settings {tab?, open?}`: open (or close) the Settings window, on a tab.
fn settings(s: &mut Session, p: &Value) -> Result<Value> {
    let tab = match opt_str_param(p, "tab")? {
        Some(t) => Some(*TABS.iter().find(|x| **x == t).ok_or_else(|| SessionError::Params(format!("\"tab\" must be one of {}", TABS.join(", "))))?),
        None => None,
    };
    let open = match p.get("open") {
        None | Some(Value::Null) => true,
        Some(v) => v.as_bool().ok_or_else(|| SessionError::Params("\"open\" must be true or false".into()))?,
    };
    s.views.settings = if open { Some(tab.map_or_else(|| s.views.settings.clone().unwrap_or_else(|| TABS[0].to_owned()), str::to_owned)) } else { None };
    Ok(json!({"open": open, "tab": s.views.settings}))
}

fn text_size(s: &mut Session, p: &Value) -> Result<Value> {
    number_in(p, "size", &TEXT_SIZES)?;
    Ok(ask(s, "view.textSize", p))
}

fn zoom(s: &mut Session, p: &Value) -> Result<Value> {
    number_in(p, "zoom", &ZOOMS)?;
    Ok(ask(s, "view.zoom", p))
}

fn set_token(s: &mut Session, p: &Value) -> Result<Value> {
    name(p, "token")?;
    color(p, "color")?;
    theme_param(p)?;
    Ok(ask(s, "theme.setToken", p))
}

fn set_fruit(s: &mut Session, p: &Value) -> Result<Value> {
    name(p, "fruit")?;
    color(p, "color")?;
    theme_param(p)?;
    Ok(ask(s, "theme.setFruit", p))
}

fn pin_family(s: &mut Session, p: &Value) -> Result<Value> {
    name(p, "family")?;
    match p.get("to") {
        Some(Value::Null) => {}
        Some(Value::String(t)) if !t.is_empty() && t.chars().count() <= MAX_NAME => {}
        _ => return Err(SessionError::Params("\"to\" must be a fruit's name, \"#rrggbb\", or null to unpin".into())),
    }
    Ok(ask(s, "theme.pinFamily", p))
}

fn reset(s: &mut Session, p: &Value) -> Result<Value> {
    theme_param(p)?;
    Ok(ask(s, "theme.reset", p))
}

fn import(s: &mut Session, p: &Value) -> Result<Value> {
    str_param(p, "path")?;
    Ok(ask(s, "theme.import", p))
}

fn export(s: &mut Session, p: &Value) -> Result<Value> {
    str_param(p, "path")?;
    theme_param(p)?;
    Ok(ask(s, "theme.export", p))
}

/// A command of the window's, in no menu (its controls are in the Settings window).
fn ui(id: &'static str, label: &'static str, params: &'static str, run: crate::commands::Run) -> CommandSpec {
    CommandSpec { id, label, menu: &[], shortcut: None, params, enabled: always, run, journal: false }
}

pub(crate) fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "app.settings",
            label: "Settings…",
            menu: &["File"],
            shortcut: Some("Cmd+Comma"),
            params: "{tab?: appearance | engines | cache, open?: bool (default true)} -> {open, tab, theme, themes: [{name, label}], appearance: {textSize, zoom, overrides, fruits, families}, autoEngines, cacheReport}: the Settings window: Appearance (theme, text size, zoom, colours, fruits, theme files), Engines (Pick Engines Automatically), Cache (the Cache Report)",
            enabled: always,
            run: settings,
            journal: false,
        },
        ui("view.textSize", "Text Size", "{size: 11 to 18 points | null (14)} -> {textSize}: the body text size; every other size follows it", text_size),
        ui("view.zoom", "UI Zoom", "{zoom: 0.8 to 1.6 | null (1)} -> {zoom}: scale the whole window", zoom),
        ui(
            "theme.setToken",
            "Set Theme Colour",
            "{token, color: \"#rrggbb\" | null (the theme's own), theme?: name (default: the current)} -> {theme, token, color, checks: [{fg, bg, ratio, need}]}: change one of a theme's colours; checks are its contrast pairs",
            set_token,
        ),
        ui(
            "theme.setFruit",
            "Set Fruit Colour",
            "{fruit: cherry … fig | olive | plum, color: \"#rrggbb\" | null, theme?} -> {theme, fruit, color}: change a node-header fruit",
            set_fruit,
        ),
        ui(
            "theme.pinFamily",
            "Pin Family Colour",
            "{family, to: a fruit's name | \"#rrggbb\" | null (its own)} -> {family, to, fruit}: give a node family a fixed header colour, in every theme",
            pin_family,
        ),
        ui("theme.reset", "Reset Theme Colours", "{theme?} -> {theme}: a theme's colours and fruits back to the built-in ones", reset),
        ui(
            "theme.import",
            "Import Theme…",
            "{path: a .plumtheme.json} -> {theme, skipped: [what was ignored]}: read a theme file's changes over its base theme, and switch to it",
            import,
        ),
        ui(
            "theme.export",
            "Export Theme…",
            "{path, theme?} -> {path, theme}: write the theme's changes from its built-in colours as a .plumtheme.json",
            export,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_opens_on_a_tab_and_closes() {
        let mut s = Session::default();
        assert_eq!(s.execute("app.settings", &json!({})).unwrap(), json!({"open": true, "tab": "appearance"}));
        assert_eq!(s.execute("app.settings", &json!({"tab": "cache"})).unwrap()["tab"], "cache");
        assert_eq!(s.execute("app.settings", &json!({})).unwrap()["tab"], "cache", "it stays on its tab");
        assert_eq!(s.execute("app.settings", &json!({"open": false})).unwrap(), json!({"open": false, "tab": null}));
        for bad in [json!({"tab": "colours"}), json!({"tab": 3}), json!({"open": "yes"})] {
            assert!(s.execute("app.settings", &bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn appearance_requests_are_checked_and_left_for_the_window() {
        let mut s = Session::default();
        for (id, good) in [
            ("view.textSize", json!({"size": 16})),
            ("view.textSize", json!({"size": null})),
            ("view.zoom", json!({"zoom": 1.2})),
            ("theme.setToken", json!({"token": "accent", "color": "#123abc"})),
            ("theme.setToken", json!({"token": "accent", "color": null, "theme": "dusk"})),
            ("theme.setFruit", json!({"fruit": "pear", "color": "#000000"})),
            ("theme.pinFamily", json!({"family": "x.audio", "to": "olive"})),
            ("theme.pinFamily", json!({"family": "x.audio", "to": null})),
            ("theme.reset", json!({})),
            ("theme.import", json!({"path": "a.plumtheme.json"})),
            ("theme.export", json!({"path": "a.plumtheme.json"})),
        ] {
            assert_eq!(s.execute(id, &good).unwrap(), json!({"asked": id}), "{id} {good}");
        }
        assert_eq!(s.views.ui_asks.len(), 11);
        for (id, bad) in [
            ("view.textSize", json!({"size": 40})),
            ("view.textSize", json!({"size": "big"})),
            ("view.textSize", json!({})),
            ("view.zoom", json!({"zoom": 0})),
            ("theme.setToken", json!({"token": "accent"})),
            ("theme.setToken", json!({"token": "accent", "color": "red"})),
            ("theme.setToken", json!({"token": "", "color": null})),
            ("theme.setToken", json!({"token": "x".repeat(500), "color": null})),
            ("theme.setFruit", json!({"fruit": "pear", "color": "#00000g"})),
            ("theme.pinFamily", json!({"family": "x", "to": 3})),
            ("theme.pinFamily", json!({"family": "x"})),
            ("theme.reset", json!({"theme": 3})),
            ("theme.import", json!({})),
            ("theme.export", json!({"path": 1})),
        ] {
            assert!(s.execute(id, &bad).is_err(), "{id} {bad}");
        }
        for _ in 0..200 {
            s.execute("theme.reset", &json!({})).unwrap();
        }
        assert_eq!(s.views.ui_asks.len(), MAX_ASKS, "a headless session keeps no more than the window would take");
    }
}
