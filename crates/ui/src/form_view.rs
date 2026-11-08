//! A command's form ([`plumcraft_session::commands::CommandForm`]): when a command that has one
//! is invoked from a menu or a shortcut, this window asks for its params, then runs it as any
//! command runs. The window stays open, saying why, when the command refuses; it closes when the
//! command succeeds, on Cancel and on Esc. Enter in a one-line field runs it.
//!
//! The control channel, scripts and the CLI run the command with its params directly; the form
//! is only how a person gives them.

use plumcraft_session::commands::{CommandForm, FieldKind};

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
}

impl FormDialog {
    pub fn new(form: CommandForm, values: Vec<String>) -> Self {
        Self { form, values, error: None, focused: false }
    }
}

impl EditorApp {
    /// Open `id`'s form, if it has one; `false` when it hasn't (the caller runs it as it is).
    pub(crate) fn open_form(&mut self, id: &str) -> bool {
        let Some(form) = plumcraft_session::commands::form(id) else { return false };
        if let Err(why) = plumcraft_session::find(id).map_or(Ok(()), |c| (c.enabled)(&self.session)) {
            self.last_error = Some(why);
            return true;
        }
        let values = form.initial(&self.session);
        self.command_form = Some(FormDialog::new(form, values));
        true
    }

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
                        let edit = match field.kind {
                            FieldKind::Line => egui::TextEdit::singleline(value),
                            FieldKind::Secret => egui::TextEdit::singleline(value).password(true),
                            FieldKind::Text => egui::TextEdit::multiline(value).desired_rows(3),
                        };
                        let r = ui.add(edit.hint_text(field.hint).desired_width(width));
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
        if go {
            let command = dialog.form.command;
            match dialog.form.params(&dialog.values) {
                Err(why) => dialog.error = Some(why),
                Ok(params) => match self.try_run(command, &params) {
                    Ok(_) => self.command_form = None,
                    Err(why) => {
                        if let Some(d) = &mut self.command_form {
                            d.error = Some(why);
                        }
                    }
                },
            }
        } else if cancel || !open {
            self.command_form = None;
        }
    }
}
