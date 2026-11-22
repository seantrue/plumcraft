//! A `form` ask: a command's form ([`plumcraft_session::commands::CommandForm`]), when a command
//! that has one is invoked from a menu or a shortcut, or a form the core builds as it asks (Save
//! Result…'s format options). The core raises the ask ([`plumcraft_editor_core::invoke`]) with
//! the form as its params (`{command, title, intro, fields, submit, values, error?}`) and this
//! window presents those params; its answer goes back to the core, which runs the command as any
//! command runs. The window stays open, saying why, when the command refuses; it closes when the
//! command succeeds, on Cancel and on Esc. Enter in a one-line field runs it.
//!
//! The control channel, scripts and the CLI run the command with its params directly; the form
//! is only how a person gives them.

use plumcraft_session::commands::PickChoice;
use serde_json::Value;

use crate::EditorApp;

/// How a field is edited, as the ask names it (`kind`).
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Line,
    Secret,
    Text,
    /// A pick list: `(value, label, hint)` choices, and whether other text is taken.
    Pick {
        choices: Vec<(String, String, String)>,
        other: bool,
    },
}

/// One field of the form, from the ask's params.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub key: String,
    pub label: String,
    pub hint: String,
    pub kind: Kind,
    pub required: bool,
}

/// The open form: its ask's params, and what has been typed.
#[derive(Debug, Clone)]
pub struct FormDialog {
    pub command: String,
    pub title: String,
    pub intro: String,
    pub submit: String,
    pub fields: Vec<Field>,
    /// One per field, in order.
    pub values: Vec<String>,
    /// Why the last try didn't run (the form's own check, or the command's refusal).
    pub error: Option<String>,
    focused: bool,
    /// The `form` ask it presents.
    pub ask: Option<plumcraft_editor_core::asks::AskId>,
}

impl FormDialog {
    /// The form a `form` ask's params describe (hostile-safe: what doesn't read is left out).
    pub fn from_params(p: &Value) -> Self {
        let text = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_owned();
        let fields: Vec<Field> = p
            .get("fields")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter(|f| f.get("key").and_then(Value::as_str).is_some_and(|k| !k.is_empty()))
            .map(|f| Field {
                key: text(f, "key"),
                label: text(f, "label"),
                hint: text(f, "hint"),
                kind: match f.get("kind").and_then(Value::as_str) {
                    Some("secret") => Kind::Secret,
                    Some("text") => Kind::Text,
                    Some("pick") => Kind::Pick {
                        choices: f
                            .get("choices")
                            .and_then(Value::as_array)
                            .into_iter()
                            .flatten()
                            .map(|c| (text(c, "value"), text(c, "label"), text(c, "hint")))
                            .collect(),
                        other: f.get("other").and_then(Value::as_bool).unwrap_or(false),
                    },
                    _ => Kind::Line,
                },
                required: f.get("required").and_then(Value::as_bool).unwrap_or(false),
            })
            .collect();
        let values = fields.iter().map(|f| p.get("values").and_then(|v| v.get(&f.key)).and_then(Value::as_str).unwrap_or_default().to_owned()).collect();
        let submit = Some(text(p, "submit")).filter(|s| !s.is_empty()).unwrap_or_else(|| "OK".into());
        Self {
            command: text(p, "command"),
            title: text(p, "title"),
            intro: text(p, "intro"),
            submit,
            fields,
            values,
            error: p.get("error").and_then(Value::as_str).map(str::to_owned),
            focused: false,
            ask: None,
        }
    }
}

impl EditorApp {
    /// The open form's window, and running its command.
    pub(crate) fn form_window(&mut self, ctx: &egui::Context) {
        let Some(dialog) = &mut self.command_form else { return };
        let (mut go, mut cancel) = (false, false);
        let mut open = true;
        let width = 340.0;
        egui::Window::new(dialog.title.clone())
            .id(egui::Id::new(("plumcraft-form", dialog.command.clone())))
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .frame(egui::Frame::window(&ctx.global_style()).fill(self.tokens.panel))
            .show(ctx, |ui| {
                if !dialog.intro.is_empty() {
                    ui.label(egui::RichText::new(&dialog.intro).color(self.tokens.text_dim));
                }
                egui::Grid::new(("plumcraft-form-grid", dialog.command.clone())).num_columns(2).show(ui, |ui| {
                    for (i, (field, value)) in dialog.fields.iter().zip(dialog.values.iter_mut()).enumerate() {
                        let label = if field.required { format!("{} *", field.label) } else { field.label.clone() };
                        ui.label(label);
                        let r = match &field.kind {
                            Kind::Line => ui.add(egui::TextEdit::singleline(value).hint_text(&field.hint).desired_width(width)),
                            Kind::Secret => ui.add(egui::TextEdit::singleline(value).password(true).hint_text(&field.hint).desired_width(width)),
                            Kind::Text => ui.add(egui::TextEdit::multiline(value).desired_rows(3).hint_text(&field.hint).desired_width(width)),
                            Kind::Pick { choices, other } => {
                                let choices: Vec<(&str, &str, &str)> = choices.iter().map(|(v, l, h)| (v.as_str(), l.as_str(), h.as_str())).collect();
                                pick(ui, (dialog.command.as_str(), field.key.as_str()), &choices, *other, value, &field.hint, width)
                            }
                        };
                        if i == 0 && !dialog.focused {
                            r.request_focus();
                            dialog.focused = true;
                        }
                        if field.kind != Kind::Text && r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            go = true;
                        }
                        ui.end_row();
                    }
                });
                if let Some(e) = &dialog.error {
                    ui.label(egui::RichText::new(e).color(self.tokens.error));
                }
                ui.horizontal(|ui| {
                    go |= ui.button(&dialog.submit).clicked();
                    cancel = ui.button("Cancel").clicked();
                });
                cancel |= ui.input(|i| i.key_pressed(egui::Key::Escape));
            });
        // the answer goes to the core, which runs the command; refused, the ask stays open with
        // why (crate::asks_view keeps the window and shows it)
        let ask = dialog.ask;
        if go {
            let values: serde_json::Map<String, serde_json::Value> =
                dialog.fields.iter().zip(&dialog.values).map(|(f, v)| (f.key.clone(), serde_json::Value::from(v.as_str()))).collect();
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
    let choices: Vec<(&str, &str, &str)> = choices.iter().map(|c| (c.value, c.label, c.hint)).collect();
    pick(ui, id, &choices, other, value, hint, width)
}

/// [`pick_field`] over `(value, label, hint)` choices.
fn pick(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    choices: &[(&str, &str, &str)],
    other: bool,
    value: &mut String,
    hint: &str,
    width: f32,
) -> egui::Response {
    let picked = choices.iter().find(|c| c.0 == value.trim());
    let shown = match picked {
        Some(c) => c.1.to_owned(),
        None if other && !value.trim().is_empty() => "Other".to_owned(),
        None => "Choose…".to_owned(),
    };
    ui.vertical(|ui| {
        let combo = egui::ComboBox::from_id_salt(id).selected_text(shown).width(width).show_ui(ui, |ui| {
            for c in choices {
                let r = ui.selectable_label(picked.is_some_and(|p| p.0 == c.0), c.1);
                let r = if c.2.is_empty() { r } else { r.on_hover_text(c.2) };
                if r.clicked() {
                    *value = c.0.to_owned();
                }
            }
        });
        if other { ui.add(egui::TextEdit::singleline(value).hint_text(hint).desired_width(width)) } else { combo.response }
    })
    .inner
}
