//! An application's commands (`commands::register`): they join the registry, run and journal as
//! PlumCraft's do, can't replace one, and only register at startup. A file of its own: the
//! registry is one per process.

use plumcraft_session::commands::{CommandSpec, always, command_specs, register};
use plumcraft_session::{Result, Session};
use serde_json::{Value, json};

fn hello(_: &mut Session, p: &Value) -> Result<Value> {
    Ok(json!({"hello": p["name"]}))
}

fn spec(id: &'static str, label: &'static str) -> CommandSpec {
    CommandSpec { id, label, menu: &["Impress"], shortcut: None, params: "{name} -> {hello}", enabled: always, run: hello, journal: false }
}

#[test]
fn an_application_adds_commands_at_startup() {
    register(vec![spec("pi.hello", "Hello"), spec("graph.tidy", "Not Tidy")]).unwrap();
    let mut s = Session::default();
    assert_eq!(s.execute("pi.hello", &json!({"name": "corgi"})).unwrap(), json!({"hello": "corgi"}));
    assert_eq!(command_specs().iter().find(|c| c.id == "pi.hello").map(|c| c.menu), Some(&["Impress"][..]));
    let tidy: Vec<_> = command_specs().iter().filter(|c| c.id == "graph.tidy").collect();
    assert_eq!((tidy.len(), tidy[0].label), (1, "Tidy"), "PlumCraft's stands");
    assert!(register(vec![spec("pi.late", "Late")]).is_err(), "the registry is built");
}
