//! A command's form ([`plumcraft_session::commands::CommandForm`]): when a command that has one
//! is invoked from a menu or a shortcut, the core raises a `form` ask
//! ([`plumcraft_editor_core::invoke`]) and this window presents it; its answer runs the command
//! as any command runs. The window stays open, saying why, when the command refuses; it closes when the
//! command succeeds, on Cancel and on Esc. Enter in a one-line field runs it.
//!
//! The control channel, scripts and the CLI run the command with its params directly; the form
//! is only how a person gives them.

use plumcraft_session::commands::{CommandForm, FieldKind, PickChoice};

use crate::EditorApp;

/// The open form: which command, and what has been typed.
#[derive(Debug, Clone)]
pub struct FormDialog {
    pub form: CommandForm,
    /// One per field, in order.
    pub values: Vec<String>,
    /// Why the last try didn't run (the form's own check, or the command's refusal).
    pub error: Option<String>,
    focused: bool,
    /// The `form` ask it presents.
    pub ask: Option<plumcraft_editor_core::asks::AskId>,
}

impl FormDialog {
    pub fn new(form: CommandForm, values: Vec<String>) -> Self {
        Self { form, values, error: None, focused: false, ask: None }
    }
}

impl EditorApp {
    /// The open form's window, and running its command.
    pub(crate) fn form_window(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.command_form else { return };
        let (mut go, mut cancel) = (false, false);
        let mut open = true;
        let width = 340.0;
        egui::Window::new(dialog.form.title)
            .id(egui::Id::new(("plumcraft-form", dialog.form.command)))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .frame(egui::Frame::window(&ctx.global_style()).fill(self.tokens.panel))
            .show(ctx, |ui| {
                if !dialog.form.intro.is_empty() {
                    ui.label(egui::RichText::new(dialog.form.intro).color(self.tokens.text_dim));
                }
                egui::Grid::new(("plumcraft-form-grid", dialog.form.command)).num_columns(2).show(ui, |ui| {
                    for (i, (field, value)) in dialog.form.fields.iter().zip(dialog.values.iter_mut()).enumerate() {
                        let label = if field.required { format!("{} *", field.label) } else { field.label.to_owned() };
                        ui.label(label);
                        let r = match field.kind {
                            FieldKind::Line => ui.add(egui::TextEdit::singleline(value).hint_text(field.hint).desired_width(width)),
                            FieldKind::Secret => ui.add(egui::TextEdit::singleline(value).password(true).hint_text(field.hint).desired_width(width)),
                            FieldKind::Text => ui.add(egui::TextEdit::multiline(value).desired_rows(3).hint_text(field.hint).desired_width(width)),
                            FieldKind::Pick { choices, other } => pick_field(ui, (dialog.form.command, field.key), choices, other, value, field.hint, width),
                        };
                        if i == 0 && !dialog.focused {
                            r.request_focus();
                            dialog.focused = true;
                        }
                        if field.kind != FieldKind::Text && r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            go = true;
                        }
                        ui.end_row();
                    }
                });
                if let Some(e) = &dialog.error {
                    ui.label(egui::RichText::new(e).color(self.tokens.error));
                }
                ui.horizontal(|ui| {
                    go |= ui.button(dialog.form.submit).clicked();
                    cancel = ui.button("Cancel").clicked();
                });
                cancel |= ui.input(|i| i.key_pressed(egui::Key::Escape));
            });
        // the answer goes to the core, which runs the command; refused, the ask stays open with
        // why (crate::asks_view keeps the window and shows it)
        let ask = dialog.ask;
        if go {
            let values: serde_json::Map<String, serde_json::Value> =
                dialog.form.fields.iter().zip(&dialog.values).map(|(f, v)| (f.key.to_owned(), serde_json::Value::from(v.as_str()))).collect();
            match ask {
                Some(id) => {
                    if let Err(e) = self.answer_ask(id, &serde_json::json!({"values": values})) {
                        self.command_form = None;
                        self.doc.last_error = Some(e);
                    }
                }
                None => self.command_form = None,
            }
        } else if cancel || !open {
            self.command_form = None;
            if let Some(id) = ask {
                let _ = self.cancel_ask(id);
            }
        }
    }
}

/// A pick-list field (decision 5, N4 person gate; contract 11, `kind: "pick"`): a combo box of
/// the core's choices and, with `other`, a line of text below it for any other value. `value` is
/// the param: a choice's `value`, or what was typed. The response is the text line's (or the
/// combo box's when there is none), so Enter in it runs the form.
pub fn pick_field(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    choices: &[PickChoice],
    other: bool,
    value: &mut String,
    hint: &str,
    width: f32,
) -> egui::Response {
    let picked = choices.iter().find(|c| c.value == value.trim());
    let shown = match picked {
        Some(c) => c.label.to_owned(),
        None if other && !value.trim().is_empty() => "Other".to_owned(),
        None => "Choose…".to_owned(),
    };
    ui.vertical(|ui| {
        let combo = egui::ComboBox::from_id_salt(id).selected_text(shown).width(width).show_ui(ui, |ui| {
            for c in choices {
                let r = ui.selectable_label(picked.is_some_and(|p| p.value == c.value), c.label);
                let r = if c.hint.is_empty() { r } else { r.on_hover_text(c.hint) };
                if r.clicked() {
                    *value = c.value.to_owned();
                }
            }
        });
        if other { ui.add(egui::TextEdit::singleline(value).hint_text(hint).desired_width(width)) } else { combo.response }
    })
    .inner
}
