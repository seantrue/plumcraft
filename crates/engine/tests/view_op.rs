//! FR6 slice 3: the engine conformance of `view.derive`. Every shared vector
//! (`crates/proto/fixtures/views.json`) asked of the Rust SDK's op, its value an inline reference,
//! answers as the reduction does; `python/tests/test_views.py` asks the Python SDK's the same.

use base64::Engine as _;
use plumcraft_engine::data::Plane;
use serde_json::{Value, json};

fn close(a: &Value, b: &Value, tol: f64, at: &str) {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => assert!(x == y || (x - y).abs() <= tol * x.abs().max(y.abs()).max(1e-300), "{at}: {x} against {y}"),
        _ => assert_eq!(a, b, "{at}"),
    }
}

#[test]
fn every_vector_through_the_op() {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../proto/fixtures/views.json");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
    let tol = v["tolerance"].as_f64().unwrap();
    let plane = Plane { store: None, shm: true, urls: false, max_inline: 1 << 30 };
    let mut asked = 0;
    for c in v["cases"].as_array().unwrap() {
        let i = &c["input"];
        let shape: Vec<u64> = i["shape"].as_array().unwrap().iter().filter_map(Value::as_u64).collect();
        let bytes = base64::engine::general_purpose::STANDARD.decode(i["b64"].as_str().unwrap()).unwrap();
        // a value a reference can carry (a handle needs bytes for its shape)
        let Ok(r) = plane.array_ref(&plumcraft_engine::Array::new(i["dtype"].as_str().unwrap(), shape, bytes)) else { continue };
        asked += 1;
        let got = plumcraft_engine::views::derive(None, &json!({"ref": r, "view": c["view"]}));
        let want = &c["expect"];
        let name = c["name"].as_str().unwrap();
        if want["refused"] == true {
            assert!(got.is_err(), "{name}: refused");
            continue;
        }
        let got = got.unwrap_or_else(|e| panic!("{name}: {e:?}"));
        if let Some(a) = want.get("array") {
            assert_eq!(got["array"]["dtype"], a["dtype"], "{name}");
            assert_eq!(got["array"]["shape"], a["shape"], "{name}");
            assert_eq!(got["b64"], a["b64"], "{name}");
            continue;
        }
        let cols = got["table"]["columns"].as_array().unwrap();
        let names: Vec<&str> = cols.iter().map(|c| c["name"].as_str().unwrap()).collect();
        assert_eq!(json!(names), want["order"], "{name}");
        for col in cols {
            let n = col["name"].as_str().unwrap();
            let (g, w) = (col["values"].as_array().unwrap(), want["columns"][n].as_array().unwrap());
            assert_eq!(g.len(), w.len(), "{name} {n}");
            for (k, (x, y)) in g.iter().zip(w).enumerate() {
                close(x, y, tol, &format!("{name} {n}[{k}]"));
            }
        }
    }
    assert!(asked > 30, "most vectors are asked ({asked})");
}
