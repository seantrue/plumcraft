//! Renaming in a small window. Right-click menus close on any click inside them (egui's context
//! menus, and egui-snarl's node menus, which are made from them), so a text field can't live in
//! a menu: clicking into it closed the menu. A menu's **Rename…** asks for this window with
//! [`request`]; the app takes the request and shows the window, where the name is typed, Enter
//! (or Rename) applies it as one command, and Esc (or Cancel) leaves it.
//!
//! Lifted from the research software's `crates/ui-egui/src/rename_view.rs`, for what is titled: nodes and
//! frames (the research software's frames have a window of their own; a group has
//! [`crate::groups_view::rename_window`], from before this one came across).

use plumcraft_graph::{FrameId, Graph, MAX_FRAME_TITLE, MAX_TITLE_CHARS, NodeId};
use serde_json::{Value, json};

/// Why a name isn't sent: nothing changed.
const UNCHANGED: &str = "Unchanged";

/// What is renamed.
#[derive(Debug, Clone, PartialEq)]
pub enum RenameTarget {
    /// A node's title: blank goes back to its type's name.
    Node(NodeId),
    /// A frame's title.
    Frame(FrameId),
}

/// The open rename window.
#[derive(Debug, Clone)]
pub struct RenameDialog {
    pub target: RenameTarget,
    /// The current title (a node's own title: blank when titled by its type).
    pub current: String,
    /// Shown greyed in an empty field: the node's type name.
    pub hint: String,
    pub text: String,
    focused: bool,
}

fn request_id() -> egui::Id {
    egui::Id::new("plumcraft-rename-request")
}

/// Ask for the rename window (from a menu; the app opens it at the end of the frame).
pub fn request(ctx: &egui::Context, target: RenameTarget, current: &str, hint: &str) {
    let dialog = RenameDialog { target, current: current.to_owned(), hint: hint.to_owned(), text: current.to_owned(), focused: false };
    ctx.data_mut(|d| d.insert_temp(request_id(), dialog));
}

/// The window asked for since the last call, if any.
pub fn take_request(ctx: &egui::Context) -> Option<RenameDialog> {
    ctx.data_mut(|d| {
        let asked = d.get_temp::<RenameDialog>(request_id());
        d.remove::<RenameDialog>(request_id());
        asked
    })
}

impl RenameDialog {
    /// Does the graph still have what is being renamed?
    pub fn target_exists(&self, graph: &Graph) -> bool {
        match &self.target {
            RenameTarget::Node(n) => graph.node(n).is_some(),
            RenameTarget::Frame(f) => graph.frame(f).is_some(),
        }
    }

    /// Why the typed name can't be applied, if it can't.
    pub fn refusal(&self) -> Option<&'static str> {
        let text = self.text.trim();
        let max = match &self.target {
            RenameTarget::Node(_) => MAX_TITLE_CHARS,
            RenameTarget::Frame(_) => MAX_FRAME_TITLE,
        };
        if text.chars().count() > max {
            Some("Too long")
        } else if text.chars().any(char::is_control) {
            Some("No control characters")
        } else if text == self.current.trim() {
            Some(UNCHANGED)
        } else {
            None
        }
    }

    fn command(&self) -> (&'static str, Value) {
        let text = self.text.trim();
        match &self.target {
            RenameTarget::Node(n) => ("node.title", json!({"node": n, "title": text})),
            RenameTarget::Frame(f) => ("frame.set", json!({"frame": f, "title": text})),
        }
    }

    /// Draw the window. Returns the command to run (when applied) and whether it stays open.
    pub fn show(&mut self, ctx: &egui::Context) -> (Option<(&'static str, Value)>, bool) {
        let name = if self.current.is_empty() { &self.hint } else { &self.current };
        let title = match &self.target {
            RenameTarget::Node(_) => format!("Rename {name}"),
            RenameTarget::Frame(_) => format!("Rename frame {name}"),
        };
        let mut open = true;
        let mut command = None;
        let mut close = false;
        egui::Window::new(title)
            .id(egui::Id::new("plumcraft-rename"))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Title");
                    let r = ui.add(egui::TextEdit::singleline(&mut self.text).hint_text(&self.hint).desired_width(220.0));
                    if !self.focused {
                        r.request_focus();
                        self.focused = true;
                    }
                    if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        match self.refusal() {
                            None => command = Some(self.command()),
                            // Enter on what is already there: done
                            Some(UNCHANGED) => close = true,
                            Some(_) => r.request_focus(),
                        }
                    }
                });
                if matches!(self.target, RenameTarget::Node(_)) {
                    ui.weak("Leave it blank to title the node by its type.");
                }
                ui.horizontal(|ui| {
                    let refusal = self.refusal();
                    let rename = ui.add_enabled(refusal.is_none(), egui::Button::new("Rename"));
                    let rename = match refusal {
                        Some(why) => rename.on_disabled_hover_text(why),
                        None => rename,
                    };
                    if rename.clicked() {
                        command = Some(self.command());
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            });
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        }
        let stays = open && !close && command.is_none();
        (command, stays)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dialog(target: RenameTarget, current: &str, text: &str) -> RenameDialog {
        RenameDialog { target, current: current.into(), hint: String::new(), text: text.into(), focused: false }
    }

    #[test]
    fn what_cant_be_applied_says_why() {
        let node = RenameTarget::Node(NodeId::from("n1"));
        let frame = RenameTarget::Frame(FrameId::from("f1"));
        assert_eq!(dialog(node.clone(), "", "shout").refusal(), None);
        assert_eq!(dialog(node.clone(), "shout", "  shout ").refusal(), Some(UNCHANGED), "only spaces changed");
        assert_eq!(dialog(node.clone(), "shout", "").refusal(), None, "blank is a title by its type");
        assert_eq!(dialog(node.clone(), "", &"x".repeat(201)).refusal(), Some("Too long"));
        assert_eq!(dialog(node, "", "a\tb").refusal(), Some("No control characters"));
        assert_eq!(dialog(frame, "", &"x".repeat(201)).refusal(), Some("Too long"));
    }

    #[test]
    fn a_rename_is_the_targets_command() {
        assert_eq!(dialog(RenameTarget::Node(NodeId::from("n1")), "", " shout ").command(), ("node.title", json!({"node": "n1", "title": "shout"})));
        assert_eq!(dialog(RenameTarget::Frame(FrameId::from("f1")), "", " in ").command(), ("frame.set", json!({"frame": "f1", "title": "in"})));
    }
}
