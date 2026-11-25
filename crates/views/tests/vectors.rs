//! The shared vectors (`crates/proto/fixtures/views.json`, FR6): every case's view of its input,
//! as `plumcraft-views` makes it, against the answer the reference computed. The Python SDK runs
//! the same file (`python/tests/test_views.py`), so the two implementations agree.

use base64::Engine as _;
use plumcraft_views::{Array, Derived, derive, parse};
use serde_json::Value;

fn vectors() -> Value {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../proto/fixtures/views.json");
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn input(c: &Value) -> (String, Vec<u64>, Vec<u8>) {
    let i = &c["input"];
    let shape = i["shape"].as_array().unwrap().iter().map(|d| d.as_u64().unwrap()).collect();
    let bytes = base64::engine::general_purpose::STANDARD.decode(i["b64"].as_str().unwrap()).unwrap();
    (i["dtype"].as_str().unwrap().to_owned(), shape, bytes)
}

fn close(a: &Value, b: &Value, tol: f64, at: &str) {
    match (a, b) {
        (Value::Null, Value::Null) => {}
        (Value::Number(x), Value::Number(y)) if x.is_u64() && y.is_u64() || x.is_i64() && y.is_i64() => assert_eq!(x, y, "{at}"),
        (Value::Number(x), Value::Number(y)) => {
            let (x, y) = (x.as_f64().unwrap(), y.as_f64().unwrap());
            assert!((x - y).abs() <= tol * x.abs().max(y.abs()).max(1e-300) || x == y, "{at}: {x} against {y}");
        }
        _ => assert_eq!(a, b, "{at}"),
    }
}

#[test]
fn every_case_agrees_with_the_reference() {
    let v = vectors();
    let tol = v["tolerance"].as_f64().unwrap();
    let cases = v["cases"].as_array().unwrap();
    assert!(cases.len() >= 30);
    for c in cases {
        let name = c["name"].as_str().unwrap();
        let (dtype, shape, bytes) = input(c);
        let got = parse(&c["view"]).and_then(|view| derive(&view, &Array { dtype: &dtype, shape: &shape, bytes: &bytes }));
        let want = &c["expect"];
        if want["refused"] == true {
            assert!(got.is_err(), "{name}: should be refused, made {got:?}");
            continue;
        }
        match got.unwrap_or_else(|e| panic!("{name}: {e}")) {
            Derived::Table { table, meta } => {
                let cols = want["columns"].as_object().unwrap();
                let ours = table["columns"].as_array().unwrap();
                let names: Vec<&str> = ours.iter().map(|c| c["name"].as_str().unwrap()).collect();
                assert_eq!(names, want["order"].as_array().unwrap().iter().map(|n| n.as_str().unwrap()).collect::<Vec<_>>(), "{name}: the columns, in order");
                for col in ours {
                    let n = col["name"].as_str().unwrap();
                    let (a, b) = (col["values"].as_array().unwrap(), cols[n].as_array().unwrap());
                    assert_eq!(a.len(), b.len(), "{name}: {n}'s length");
                    for (k, (x, y)) in a.iter().zip(b).enumerate() {
                        close(x, y, tol, &format!("{name}: {n}[{k}]"));
                    }
                }
                for (k, w) in want["meta"].as_object().unwrap() {
                    close(&meta[k], w, tol, &format!("{name}: meta {k}"));
                }
            }
            Derived::Array { dtype: d, shape: s, bytes: b } => {
                let w = &want["array"];
                assert_eq!(d, w["dtype"].as_str().unwrap(), "{name}");
                assert_eq!(s, w["shape"].as_array().unwrap().iter().map(|x| x.as_u64().unwrap()).collect::<Vec<_>>(), "{name}");
                assert_eq!(base64::engine::general_purpose::STANDARD.encode(&b), w["b64"].as_str().unwrap(), "{name}: the bytes");
            }
        }
    }
}
