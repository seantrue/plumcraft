//! `view.derive` (FR6, research note 30, spec 10.5): a derived view of a value this engine can
//! read, made by the SDK, not by a node: `{ref: DataRef, view: {kind, params}, max_bytes?}` →
//! `{table, meta}` (a TABLE and what else it says) or `{array: {dtype, shape}, b64}`. The value is
//! read from the engine's store by its handle (or its inline bytes), checked against the handle,
//! and reduced by `plumcraft-views`, the reductions the Python SDK's `views.py` matches by the
//! shared vectors. Nothing a client sends opens a path.

use base64::Engine as _;
use plumcraft_proto::data::{DataRef, PortValue};
use plumcraft_proto::error::{ErrorCode, ProtoError};
use plumcraft_store::Store;
use serde_json::{Value, json};

use crate::Data;

/// The most bytes an answer carries (its JSON), unless the request asks for less.
pub const MAX_ANSWER: u64 = 16 << 20;

fn bad(m: impl Into<String>) -> ProtoError {
    ProtoError::new(ErrorCode::BadParams, m.into())
}

/// The op (module docs).
pub fn derive(store: Option<&Store>, params: &Value) -> Result<Value, ProtoError> {
    let o = params.as_object().ok_or_else(|| bad("view.derive takes {ref, view, max_bytes?}"))?;
    if let Some(k) = o.keys().find(|k| !matches!(k.as_str(), "ref" | "view" | "max_bytes")) {
        return Err(bad(format!("view.derive takes ref, view and max_bytes, not {:?}", k.chars().take(32).collect::<String>())));
    }
    let r: DataRef = serde_json::from_value(o.get("ref").cloned().unwrap_or(Value::Null)).map_err(|e| bad(format!("ref: {e}")))?;
    let view = plumcraft_views::parse(o.get("view").unwrap_or(&Value::Null)).map_err(bad)?;
    let max = o.get("max_bytes").and_then(Value::as_u64).unwrap_or(MAX_ANSWER).min(MAX_ANSWER);
    let a = match crate::bind::to_data(store, &[], "view", &PortValue::Blob { data: r }) {
        Ok(Data::Array(a)) => a,
        Ok(_) => return Err(bad("a view is of an array")),
        Err(e) => return Err(ProtoError::new(ErrorCode::NotFound, e.message)),
    };
    let made = plumcraft_views::derive(&view, &plumcraft_views::Array { dtype: &a.dtype, shape: &a.shape, bytes: &a.bytes }).map_err(bad)?;
    let out = match made {
        plumcraft_views::Derived::Table { table, meta } => json!({"table": table, "meta": meta}),
        plumcraft_views::Derived::Array { dtype, shape, bytes } => {
            json!({"array": {"dtype": dtype, "shape": shape}, "b64": base64::engine::general_purpose::STANDARD.encode(bytes)})
        }
    };
    let len = serde_json::to_vec(&out).map_or(u64::MAX, |v| v.len() as u64);
    if len > max {
        return Err(ProtoError::new(ErrorCode::TooLarge, format!("the view is {len} bytes, more than {max}")));
    }
    Ok(out)
}
