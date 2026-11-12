//! JSON Schema for protocol 2 (feature `schema`), so engines in any language can check their
//! messages. One document: every type under `$defs`, and the top-level messages listed by name.
//!
//! The schema describes what writers MUST send. Readers in this crate are more lenient than it
//! (spec section 3, rule 5): unknown fields, kinds and states are ignored, not refused.
//!
//! `schema/protocol2.json` is the checked-in copy; `cargo run -p plumcraft-proto --example
//! export_schema --features schema` rewrites it, and a test fails when it is stale.

use schemars::generate::SchemaSettings;
use serde_json::{Map, Value, json};

use crate::catalogue::SchemaReply;
use crate::data::{DataRef, PortValue};
use crate::display::{Display, WatchRequest};
use crate::error::ProtoError;
use crate::event::{Envelope, LiveUpdate, LogRecord, Resync, StatusRecord};
use crate::run::{PeekReply, PeekRequest, PlanReply, RunAck, RunReply, RunRequest, RunSnapshot, SaveReply, SaveRequest};
use crate::session::{EngineState, Hello, HelloReply, SessionConfirm};

/// The protocol 2 schema document.
pub fn protocol2() -> Value {
    let mut generator = SchemaSettings::draft2020_12().into_generator();
    let mut messages = Map::new();
    let mut add = |name: &str, schema: schemars::Schema| {
        messages.insert(name.to_owned(), schema.to_value());
    };
    add("Hello", generator.subschema_for::<Hello>());
    add("HelloReply", generator.subschema_for::<HelloReply>());
    add("SessionConfirm", generator.subschema_for::<SessionConfirm>());
    add("SchemaReply", generator.subschema_for::<SchemaReply>());
    add("RunRequest", generator.subschema_for::<RunRequest>());
    add("RunAck", generator.subschema_for::<RunAck>());
    add("RunReply", generator.subschema_for::<RunReply>());
    add("RunSnapshot", generator.subschema_for::<RunSnapshot>());
    add("PeekRequest", generator.subschema_for::<PeekRequest>());
    add("PeekReply", generator.subschema_for::<PeekReply>());
    add("PlanReply", generator.subschema_for::<PlanReply>());
    add("SaveRequest", generator.subschema_for::<SaveRequest>());
    add("SaveReply", generator.subschema_for::<SaveReply>());
    add("Envelope", generator.subschema_for::<Envelope>());
    add("StatusRecord", generator.subschema_for::<StatusRecord>());
    add("LiveUpdate", generator.subschema_for::<LiveUpdate>());
    add("LogRecord", generator.subschema_for::<LogRecord>());
    add("Resync", generator.subschema_for::<Resync>());
    add("EngineState", generator.subschema_for::<EngineState>());
    add("Display", generator.subschema_for::<Display>());
    add("WatchRequest", generator.subschema_for::<WatchRequest>());
    add("DataRef", generator.subschema_for::<DataRef>());
    add("PortValue", generator.subschema_for::<PortValue>());
    add("ProtoError", generator.subschema_for::<ProtoError>());
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "$id": "https://github.com/seantrue/plumcraft/schema/protocol2.json",
        "title": "PlumCraft protocol 2",
        "description": "The UI <-> engine interface (docs/interface.md). An envelope's body is one of: EngineState (engine.state), StatusRecord (node.status), LiveUpdate (node.live), RunReply (run.reply), LogRecord (log), Resync (resync), {schema_digest} (schema.changed).",
        "messages": messages,
        "$defs": generator.take_definitions(true),
    })
}

/// Make sure a type is in the document (for tests).
#[cfg(test)]
fn has_def<T: schemars::JsonSchema>(doc: &Value) -> bool {
    doc["$defs"].get(T::schema_name().as_ref()).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checked_in_schema_is_current() {
        let doc = protocol2();
        let text = serde_json::to_string_pretty(&doc).unwrap() + "\n";
        let on_disk = include_str!("../schema/protocol2.json");
        assert!(text == on_disk, "schema/protocol2.json is stale: cargo run -p plumcraft-proto --example export_schema --features schema");
    }

    /// plumcraft #86 (review finding 6): `texts` is an open map of strings, so a new key such as
    /// `engine_status` needs no schema change, and the checked-in schema names none of them.
    #[test]
    fn texts_keys_are_not_in_the_schema() {
        let on_disk: Value = serde_json::from_str(include_str!("../schema/protocol2.json")).unwrap();
        let texts = &on_disk["$defs"]["SchemaReply"]["properties"]["texts"];
        assert_eq!(texts, &json!({"additionalProperties": {"type": "string"}, "type": "object"}), "{texts}");
        let text = include_str!("../schema/protocol2.json");
        for key in [crate::catalogue::texts::ENGINE_STATUS, crate::catalogue::texts::ENGINE_STATUS_TONE, crate::catalogue::texts::ENGINE_NOTICE] {
            assert!(!text.contains(&format!("\"{key}\"")), "{key} is in the schema");
        }
    }

    #[test]
    fn document_shape() {
        let doc = protocol2();
        assert_eq!(doc["messages"].as_object().unwrap().len(), 24);
        assert!(has_def::<crate::Handle>(&doc));
        assert!(has_def::<StatusRecord>(&doc));
        assert_eq!(doc["$defs"]["Handle"]["type"], "string");
    }
}
