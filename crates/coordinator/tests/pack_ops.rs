//! The coordinator routes a pack operation (spec 15) to the engine `params.engine` names, else
//! to the first engine that advertises it; one nobody offers is `unknown_op`.

use std::sync::Arc;
use std::time::Duration;

use plumcraft_coordinator::Coordinator;
use plumcraft_engine::host::Host;
use plumcraft_engine::{Engine, PackOp};
use plumcraft_host::{Connection, InProc};
use plumcraft_proto::error::ErrorCode;
use plumcraft_proto::session::EngineInfo;
use serde_json::{Value, json};

fn engine_with(id: &'static str, ops: &[&'static str]) -> Arc<dyn Connection> {
    let ops: Vec<&'static str> = ops.to_vec();
    let h = Host::start(
        Box::new(move || {
            let mut e = Engine::new(EngineInfo { id: id.into(), name: id.into(), version: "1".into(), ..Default::default() }).with_store(None);
            for op in &ops {
                let f: PackOp = Arc::new(move |p: Value| Ok(json!({"from": id, "params": p})));
                e = e.with_pack_op(op, f);
            }
            Ok(e)
        }),
        None,
    );
    h.wait_ready(Duration::from_secs(5));
    Arc::new(InProc(h))
}

#[test]
fn pack_ops_go_to_the_engine_that_offers_them_or_the_one_named() {
    let c = Coordinator::new().with_engine("a", engine_with("a", &["x.p.one"])).with_engine("b", engine_with("b", &["x.p.one", "x.p.two"]));
    assert_eq!(c.call("x.p.two", json!({})).unwrap()["from"], "b");
    assert_eq!(c.call("x.p.one", json!({})).unwrap()["from"], "a", "the first that offers it");
    assert_eq!(c.call("x.p.one", json!({"engine": "b"})).unwrap()["from"], "b", "the one named");
    assert_eq!(c.call("x.p.three", json!({})).unwrap_err().code, ErrorCode::UnknownOp);
}
