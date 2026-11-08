//! A command's form (`plumcraft_ui::form_view`): invoked from a menu, a command with a form asks
//! for its params in a window, then runs as any command does. The window says why when the
//! command (or a required field) refuses, and closes when it succeeds, on Cancel and on Esc.

mod common;

use std::sync::{Mutex, Once};

use egui_kittest::kittest::Queryable;
use plumcraft_session::commands::{CommandForm, CommandSpec, FieldKind, FormField, always};
use plumcraft_session::{Session, SessionError};
use serde_json::{Value, json};

/// What `test.greet` was given, each time it ran.
static GOT: Mutex<Vec<Value>> = Mutex::new(Vec::new());

fn greet(_: &mut Session, p: &Value) -> plumcraft_session::Result<Value> {
    if p.get("name").and_then(Value::as_str) == Some("nobody") {
        return Err(SessionError::Params("nobody can't be greeted".into()));
    }
    GOT.lock().unwrap().push(p.clone());
    Ok(json!({"notice": "greeted"}))
}

fn setup() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        plumcraft_session::commands::register(vec![CommandSpec {
            id: "test.greet",
            label: "Greet",
            menu: &["File"],
            shortcut: None,
            params: "{name, token?, note?}",
            enabled: always,
            run: greet,
            journal: false,
        }])
        .unwrap();
        let field = |key, label, kind, required| FormField { key, label, hint: "", kind, required };
        plumcraft_session::commands::register_forms(vec![CommandForm {
            command: "test.greet",
            title: "Greet Someone",
            intro: "Says hello.",
            fields: vec![
                field("name", "Name", FieldKind::Line, true),
                field("token", "Token", FieldKind::Secret, false),
                field("note", "Note", FieldKind::Text, false),
            ],
            submit: "Greet",
            prefill: Some(|_| serde_json::Map::from_iter([("name".to_owned(), json!("Ada"))])),
        }])
        .unwrap();
    });
}

#[test]
fn a_command_with_a_form_asks_then_runs_with_what_was_typed() {
    setup();
    let mut h = common::harness(plumcraft_ui::EditorApp::new(common::session()));
    h.state_mut().invoke("test.greet");
    h.run_steps(3);
    h.get_by_label("Greet Someone");
    h.get_by_label("Says hello.");
    let ran = |name: &str| GOT.lock().unwrap().iter().any(|p| p["name"] == name);
    assert!(!ran("Ada"), "nothing runs until the form is sent");
    assert_eq!(h.state().command_form.as_ref().unwrap().values, ["Ada", "", ""], "prefilled");

    // a required field left empty: the form says so and stays
    h.state_mut().command_form.as_mut().unwrap().values[0].clear();
    h.get_by_label("Greet").click();
    h.run_steps(2);
    h.get_by_label("Name is needed");
    // the command's refusal: said in the window, which stays
    h.state_mut().command_form.as_mut().unwrap().values[0] = "nobody".into();
    h.get_by_label("Greet").click();
    h.run_steps(2);
    let said = h.state().command_form.as_ref().and_then(|f| f.error.clone()).unwrap_or_default();
    assert!(said.contains("nobody can't be greeted"), "{said}");
    assert!(h.query_all_by_label(&said).count() >= 1 && h.state().command_form.is_some(), "said in the window (and the status bar), which stays");
    // sent: runs with the fields as params, empty ones left out, and closes
    let form = h.state_mut().command_form.as_mut().unwrap();
    form.values = vec![" Grace ".into(), "s3cret".into(), String::new()];
    h.get_by_label("Greet").click();
    h.run_steps(2);
    assert!(GOT.lock().unwrap().contains(&json!({"name": "Grace", "token": "s3cret"})), "the fields as params, the empty one left out");
    assert!(!ran("nobody") && !ran("Ada"));
    assert!(h.state().command_form.is_none() && h.query_by_label("Greet Someone").is_none());
}

#[test]
fn cancel_closes_the_form_without_running() {
    setup();
    let mut h = common::harness(plumcraft_ui::EditorApp::new(common::session()));
    h.state_mut().invoke("test.greet");
    h.run_steps(3);
    h.state_mut().command_form.as_mut().unwrap().values[0] = "Cancelled".into();
    h.get_by_label("Cancel").click();
    h.run_steps(2);
    assert!(h.state().command_form.is_none());
    assert!(!GOT.lock().unwrap().iter().any(|p| p["name"] == "Cancelled"));
}

#[test]
fn the_control_channel_runs_the_command_without_the_form() {
    setup();
    let mut h = common::harness(plumcraft_ui::EditorApp::new(common::session()));
    let ctx = h.ctx.clone();
    let r = h.state_mut().control_now(&ctx, "command.run", &json!({"command": "test.greet", "params": {"name": "Linus"}}));
    assert_eq!(r["ok"], true, "{r}");
    assert!(h.state().command_form.is_none());
    assert!(GOT.lock().unwrap().contains(&json!({"name": "Linus"})));
}
