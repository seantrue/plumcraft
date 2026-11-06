//! Pack operations (spec 15, `x.<pack>.<op>`) in the SDK: an engine registers a handler, `hello`
//! advertises it, the host answers it on the caller's thread, an unknown one is `unknown_op`, a
//! panicking one is an error and the engine carries on, and a malformed name is never registered.

use std::sync::Arc;
use std::time::Duration;

use plumcraft_engine::host::Host;
use plumcraft_engine::{Engine, PackOp, is_pack_op};
use plumcraft_proto::error::{ErrorCode, ProtoError};
use plumcraft_proto::session::{EngineInfo, EngineState};
use serde_json::{Value, json};

fn engine() -> Engine {
    let echo: PackOp = Arc::new(|p: Value| Ok(json!({"echo": p})));
    let boom: PackOp = Arc::new(|_| -> Result<Value, ProtoError> { panic!("boom") });
    Engine::new(EngineInfo { id: "t".into(), name: "T".into(), version: "1".into(), ..Default::default() })
        .with_store(None)
        .with_pack_op("x.t.echo", echo.clone())
        .with_pack_op("x.t.boom", boom)
        .with_pack_op("not.a.pack.op", echo.clone())
        .with_pack_op("x.t.too.long", echo)
}

fn host() -> Host {
    let h = Host::start(Box::new(|| Ok(engine())), None);
    assert!(matches!(h.wait_ready(Duration::from_secs(5)), EngineState::Connected { .. }));
    h
}

#[test]
fn names_are_x_pack_op() {
    assert!(is_pack_op("x.vi.export") && is_pack_op("x.pipe.call"));
    for bad in ["x.vi", "vi.export", "x..export", "x.a.b.c", ""] {
        assert!(!is_pack_op(bad), "{bad}");
    }
}

#[test]
fn a_pack_op_is_advertised_and_answered() {
    let h = host();
    let caps = engine().caps().clone();
    assert!(caps.has("x.t.echo") && caps.has("x.t.boom"));
    assert!(!caps.has("not.a.pack.op") && !caps.has("x.t.too.long"), "malformed names aren't registered");
    assert_eq!(h.dispatch("x.t.echo", json!({"a": 1})).unwrap(), json!({"echo": {"a": 1}}));
    assert_eq!(h.dispatch("x.t.nope", json!({})).unwrap_err().code, ErrorCode::UnknownOp);
    assert_eq!(h.dispatch("x.t.too.long", json!({})).unwrap_err().code, ErrorCode::UnknownOp);
}

#[test]
fn a_panicking_pack_op_is_an_error_and_the_engine_carries_on() {
    let h = host();
    let e = h.dispatch("x.t.boom", json!({})).unwrap_err();
    assert_eq!(e.code, ErrorCode::Internal);
    assert!(e.message.contains("x.t.boom"));
    assert_eq!(h.dispatch("x.t.echo", json!(2)).unwrap(), json!({"echo": 2}));
    assert!(matches!(h.state(), EngineState::Connected { .. }));
}
