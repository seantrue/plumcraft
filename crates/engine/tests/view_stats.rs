//! FR6: the per-channel display stats (spec 10.2 `stats`) both SDKs compute, held to the shared
//! vectors' `sdk_stats` (`crates/proto/fixtures/views.json`), which `python/tests/test_views.py`
//! runs for the Python SDK: the two agree on every count and within 1e-9 on every number.

use base64::Engine as _;
use plumcraft_engine::{Array, arrays};
use serde_json::Value;

#[test]
fn the_display_stats_agree_with_the_shared_vectors() {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../proto/fixtures/views.json");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
    let tol = v["tolerance"].as_f64().unwrap();
    let close = |a: f64, b: f64| (a - b).abs() <= tol * a.abs().max(b.abs()).max(1e-300);
    for c in v["sdk_stats"].as_array().unwrap() {
        let i = &c["input"];
        let shape: Vec<u64> = i["shape"].as_array().unwrap().iter().map(|d| d.as_u64().unwrap()).collect();
        let bytes = base64::engine::general_purpose::STANDARD.decode(i["b64"].as_str().unwrap()).unwrap();
        let a = Array::new(i["dtype"].as_str().unwrap(), shape, bytes);
        let s = arrays::stats(&a).unwrap();
        let want = c["channels"].as_array().unwrap();
        assert_eq!(s.channels.len(), want.len(), "{}", c["name"]);
        for (g, w) in s.channels.iter().zip(want) {
            assert!(close(g.min.unwrap(), w["min"].as_f64().unwrap()) && close(g.max.unwrap(), w["max"].as_f64().unwrap()), "{}", c["name"]);
            assert_eq!(g.nan, w["nan"].as_u64().unwrap(), "{}", c["name"]);
            let h = g.histogram.as_ref().unwrap();
            assert_eq!(h.counts, w["counts"].as_array().unwrap().iter().map(|n| n.as_u64().unwrap()).collect::<Vec<_>>(), "{}", c["name"]);
            assert!(close(h.hi.unwrap(), w["hi"].as_f64().unwrap()), "{}", c["name"]);
        }
    }
}
