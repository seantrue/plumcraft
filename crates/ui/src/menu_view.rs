//! Drawing the core's menu model ([`plumcraft_editor_core::menu`]) with egui: the menu bar and
//! every context menu are built by the core as data and drawn here, as they were drawn before
//! (N1 slice 8). What an item does is the caller's: its command, run as the menu item always
//! ran it, or (for an item with no command yet) what the shell does itself by the item's id.

use egui::RichText;
use plumcraft_editor_core::menu::{Item, Look, Mod, Shortcut};

use crate::theme::Tokens;

pub use plumcraft_editor_core::menu::{Context, Facts, Parts};

/// A model shortcut as egui's: `primary` is egui's COMMAND (⌘ on macOS, Ctrl elsewhere). `None`
/// for a key egui doesn't know.
pub fn to_egui(sc: &Shortcut) -> Option<egui::KeyboardShortcut> {
    let mut mods = egui::Modifiers::NONE;
    for m in &sc.mods {
        mods |= match m {
            Mod::Primary => egui::Modifiers::COMMAND,
            Mod::Ctrl => egui::Modifiers::CTRL,
            Mod::Shift => egui::Modifiers::SHIFT,
            Mod::Alt => egui::Modifiers::ALT,
        };
    }
    Some(egui::KeyboardShortcut::new(mods, egui::Key::from_name(&sc.key)?))
}

/// The text a menu item the shell does itself shows for its keys, as the Edit menu has always
/// written it (`⌘X` on macOS, `Ctrl+X` elsewhere; `⌫`; `F`; `Tab`).
fn fixed_text(sc: &Shortcut) -> String {
    let primary = if cfg!(target_os = "macos") { "⌘" } else { "Ctrl+" };
    let mods: String = sc.mods.iter().map(|m| if *m == Mod::Primary { primary } else { "" }).collect();
    let key = match sc.key.as_str() {
        "Backspace" => "⌫",
        k => k,
    };
    format!("{mods}{key}")
}

/// An item the shell did itself before N2 slice 1, or one of the canvas's commands: the Edit
/// menu's canvas items, Fit, Search…, and what is still the shell's (no command).
fn canvas_item(item: &Item) -> bool {
    match item.command.as_deref() {
        None => true,
        Some(c) => plumcraft_editor_core::canvas_cmds::find(c).is_some() || ["edit.cut", "edit.copy", "edit.paste", "frame.add"].contains(&c),
    }
}

/// How to draw: the theme (for dim lines), whether this is a menu of the menu bar (where the
/// shell's own items show their keys), and whether a click closes the menu.
#[derive(Clone, Copy)]
pub struct Style<'a> {
    pub tokens: &'a Tokens,
    pub bar: bool,
    pub close: bool,
}

/// Draw `items`; the item clicked, if any.
pub fn draw(ui: &mut egui::Ui, items: &[Item], style: Style<'_>) -> Option<Item> {
    let mut clicked = None;
    for item in items {
        if let Some(c) = draw_one(ui, item, style) {
            clicked = Some(c);
        }
    }
    clicked
}

fn label(item: &Item) -> RichText {
    let mut t = RichText::new(&item.label);
    if item.strong {
        t = t.strong();
    }
    t
}

fn draw_one(ui: &mut egui::Ui, item: &Item, style: Style<'_>) -> Option<Item> {
    if item.separator {
        ui.separator();
        return None;
    }
    if item.submenu {
        let mut picked = None;
        let inner = Style { bar: false, ..style };
        let mut open = |ui: &mut egui::Ui| {
            ui.menu_button(label(item), |ui| picked = draw(ui, &item.items, inner));
        };
        // the Edit menu's Align and Distribute have always sat in an enabled scope of their own
        if !item.enabled || item.id == "edit.align" || item.id == "edit.distribute" {
            ui.add_enabled_ui(item.enabled, open);
        } else {
            open(ui);
        }
        return picked;
    }
    let r = match item.look {
        Look::Note => {
            ui.weak(&item.label);
            return None;
        }
        Look::Text => {
            ui.label(&item.label);
            return None;
        }
        Look::Dim => {
            ui.label(RichText::new(&item.label).color(style.tokens.text_dim));
            return None;
        }
        Look::Row => {
            let mut picked = None;
            ui.horizontal(|ui| {
                ui.label(&item.label);
                picked = draw(ui, &item.items, style);
            });
            return picked;
        }
        Look::Section => {
            let mut picked = None;
            scrolling(ui, "engine-choices", |ui| picked = draw(ui, &item.items, style));
            return picked;
        }
        Look::Radio => ui.add_enabled(item.enabled, egui::RadioButton::new(item.checked == Some(true), label(item))),
        Look::Choice => ui.add_enabled(item.enabled, egui::Button::selectable(item.checked == Some(true), label(item))),
        Look::Selected => {
            let text = match item.color.as_deref() {
                Some(hex) => RichText::new(&item.label).color(crate::theme::parse_hex(hex).unwrap_or(egui::Color32::GRAY)),
                None => RichText::new(&item.label),
            };
            ui.add_enabled(item.enabled, egui::Button::new(text).selected(item.checked == Some(true)))
        }
        Look::Check | Look::Plain => {
            let text = match (item.look, item.checked) {
                (Look::Check, Some(true)) => format!("✓ {}", item.label),
                (Look::Check, _) => format!("    {}", item.label),
                _ => item.label.clone(),
            };
            let mut button = egui::Button::new(text);
            match (&item.command, &item.params, &item.shortcut) {
                // the canvas's items in a menu of the bar show their keys as they always have, or
                // none (they were the shell's own until N2 slice 1)
                (_, _, sc) if style.bar && canvas_item(item) => button = button.shortcut_text(sc.as_ref().map(fixed_text).unwrap_or_default()),
                // a registry command's keys, as the platform writes them
                (Some(_), None, Some(sc)) => {
                    if let Some(sc) = to_egui(sc) {
                        button = button.shortcut_text(ui.ctx().format_shortcut(&sc));
                    }
                }
                _ => {}
            }
            ui.add_enabled(item.enabled, button)
        }
    };
    let r = match (&item.hover, item.enabled) {
        (Some(h), _) => r.on_hover_text(h),
        _ => r,
    };
    let r = match (&item.reason, item.enabled) {
        (Some(why), false) => r.on_disabled_hover_text(why),
        _ => r,
    };
    if r.clicked() {
        if style.close {
            ui.close();
        }
        return Some(item.clone());
    }
    None
}

/// A menu list that scrolls once it would run off the bottom of the window.
fn scrolling(ui: &mut egui::Ui, salt: &str, add: impl FnOnce(&mut egui::Ui)) {
    let room = (ui.ctx().content_rect().bottom() - ui.cursor().top() - 24.0).max(120.0);
    // a menu offers less than the window, so the list asks for what is left below it
    egui::ScrollArea::vertical().id_salt(salt).min_scrolled_height(room).max_height(room).auto_shrink([true, true]).show(ui, add);
}

/// A clicked item's command as the registry's own id (a [`crate::canvas::Action`]'s), with its
/// params (`{}` without).
pub fn action(item: &Item) -> Option<crate::canvas::Action> {
    let command = item.command.as_deref()?;
    let id = match plumcraft_session::find(command) {
        Some(spec) => spec.id,
        None => plumcraft_editor_core::canvas_cmds::find(command)?.id,
    };
    Some((id, item.params.clone().unwrap_or_else(|| serde_json::json!({}))))
}

/// Open the window an item names that the shell does itself (a text field can't live in a
/// menu): renaming a node, a group or a frame, a group's promoted parameter.
pub fn open_window(ctx: &egui::Context, item: &Item) {
    let p = item.params.clone().unwrap_or_default();
    let text = |k: &str| p.get(k).and_then(serde_json::Value::as_str).unwrap_or_default().to_owned();
    match item.id.as_str() {
        "rename.node" => {
            crate::rename_view::request(ctx, crate::rename_view::RenameTarget::Node(text("node").as_str().into()), &text("title"), &text("placeholder"))
        }
        "rename.frame" => crate::rename_view::request(
            ctx,
            crate::rename_view::RenameTarget::Frame(plumcraft_graph::FrameId(text("frame"))),
            &text("title"),
            crate::frame_view::UNTITLED,
        ),
        "rename.group" => crate::groups_view::request_rename(ctx, &text("group").as_str().into(), &text("name")),
        "group.editPromoted" => crate::groups_view::open_parameter(ctx, &text("group").as_str().into(), &text("label")),
        _ => {}
    }
}
