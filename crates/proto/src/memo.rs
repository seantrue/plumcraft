//! Memo keys (spec 14): the typed canonical form (14.1) and the plan key (14.2, 14.3).
//!
//! The canonical form tags every number, so an INT `1` and a FLOAT `1.0` never share a key, and
//! writes floats as their IEEE-754 bit pattern, so every language agrees on them. Rust and Python
//! are held to the same bytes by `fixtures/memo_canon.json`.
//!
//! One known gap: `serde_json` reads an integer beyond the 64-bit range as a float, so such a
//! value keys as a float here and as an integer in Python. The vectors mark that case.

use std::fmt::Write as _;

use md5::{Digest, Md5};
use serde_json::{Map, Value};

/// The tag that starts every memo key's canonical list.
pub const MEMO_V1: &str = "plumcraft-memo-v1";

/// How a parameter is coerced before it is encoded (spec 14.1 rule 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Numeric {
    Int,
    Float,
}

/// Coerce a resolved value to its port's numeric kind: an integral float on an INT port becomes
/// an integer, an integer on a FLOAT port becomes a float. Anything else is left as it is.
pub fn coerce(v: &Value, kind: Option<Numeric>) -> Value {
    match (kind, v) {
        (Some(Numeric::Float), Value::Number(n)) if !n.is_f64() => n.as_f64().map_or_else(|| v.clone(), float),
        (Some(Numeric::Int), Value::Number(n)) => match n.as_f64() {
            Some(f) if n.is_f64() && f.fract() == 0.0 && f.is_finite() && f.abs() < 9.007_199_254_740_992e15 => Value::from(f as i64),
            _ => v.clone(),
        },
        _ => v.clone(),
    }
}

fn float(f: f64) -> Value {
    serde_json::Number::from_f64(f).map_or(Value::Null, Value::Number)
}

/// A float that JSON can't carry (NaN, ±Inf) in the canonical form, for values built in code.
pub fn canon_f64(f: f64) -> String {
    format!("{{\"f\":\"0x{:016x}\"}}", f.to_bits())
}

/// The typed canonical form of a JSON value (spec 14.1).
pub fn canon(v: &Value) -> String {
    let mut out = String::new();
    write_canon(&mut out, v);
    out
}

fn write_canon(out: &mut String, v: &Value) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                let _ = write!(out, "{{\"i\":\"{i}\"}}");
            } else if let Some(u) = n.as_u64() {
                let _ = write!(out, "{{\"i\":\"{u}\"}}");
            } else {
                out.push_str(&canon_f64(n.as_f64().unwrap_or(f64::NAN)));
            }
        }
        Value::String(s) => out.push_str(&serde_json::to_string(s).unwrap_or_default()),
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canon(out, x);
            }
            out.push(']');
        }
        Value::Object(m) => write_object(out, m),
    }
}

fn write_object(out: &mut String, m: &Map<String, Value>) {
    // Keys sorted by their UTF-8 bytes (Rust's `str` order).
    let mut keys: Vec<&String> = m.keys().collect();
    keys.sort();
    out.push('{');
    for (i, k) in keys.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&serde_json::to_string(k).unwrap_or_default());
        out.push(':');
        if let Some(x) = m.get(*k) {
            write_canon(out, x);
        }
    }
    out.push('}');
}

/// A node's plan key (spec 14.2, 14.3): MD5 of the canonical form of
/// `["plumcraft-memo-v1", kind, version, salt, device_class, params, inputs]`, written
/// `memo:md5:<hex>`. `params` are the resolved non-wired values, already coerced; `inputs` maps
/// each wired port to its identity, the upstream plan key and port (`<key>#<port>`).
pub fn plan_key(kind: &str, version: u32, salt: &str, device_class: Option<&str>, params: &Map<String, Value>, inputs: &Map<String, Value>) -> String {
    let list = Value::Array(vec![
        Value::from(MEMO_V1),
        Value::from(kind),
        Value::from(version),
        Value::from(salt),
        device_class.map_or(Value::Null, Value::from),
        Value::Object(params.clone()),
        Value::Object(inputs.clone()),
    ]);
    let digest = Md5::digest(canon(&list).as_bytes());
    let mut hex = String::with_capacity(32);
    for b in digest {
        let _ = write!(hex, "{b:02x}");
    }
    format!("memo:md5:{hex}")
}

/// The identity of a wired input: its upstream node's plan key and port.
pub fn input_identity(upstream_key: &str, port: &str, slice: Option<&str>) -> Value {
    match slice {
        Some(s) => Value::from(format!("{upstream_key}#{port}[{s}]")),
        None => Value::from(format!("{upstream_key}#{port}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The shared vectors, also checked by `python/tests/test_memo.py`.
    #[test]
    fn shared_vectors() {
        let v: Value = serde_json::from_str(include_str!("../fixtures/memo_canon.json")).unwrap();
        let mut checked = 0;
        for case in v["canon"].as_array().unwrap() {
            if case["python_only"].as_bool() == Some(true) {
                continue;
            }
            let kind = match case["port"].as_str() {
                Some("INT") => Some(Numeric::Int),
                Some("FLOAT") => Some(Numeric::Float),
                _ => None,
            };
            let got = match case["float_bits"].as_str() {
                Some(bits) => canon_f64(f64::from_bits(u64::from_str_radix(bits.trim_start_matches("0x"), 16).unwrap())),
                None => canon(&coerce(&case["in"], kind)),
            };
            assert_eq!(got, case["canon"].as_str().unwrap(), "{}", case["note"]);
            checked += 1;
        }
        for case in v["keys"].as_array().unwrap() {
            let key = plan_key(
                case["kind"].as_str().unwrap(),
                u32::try_from(case["version"].as_u64().unwrap()).unwrap(),
                case["salt"].as_str().unwrap(),
                case["device_class"].as_str(),
                case["params"].as_object().unwrap(),
                case["inputs"].as_object().unwrap(),
            );
            assert_eq!(key, case["key"].as_str().unwrap(), "{}", case["note"]);
            checked += 1;
        }
        assert!(checked > 10);
    }

    #[test]
    fn one_and_one_point_oh_differ() {
        assert_ne!(canon(&json!(1)), canon(&json!(1.0)));
        assert_eq!(canon(&coerce(&json!(1), Some(Numeric::Float))), canon(&json!(1.0)));
        assert_eq!(canon(&coerce(&json!(2.0), Some(Numeric::Int))), canon(&json!(2)));
        assert_eq!(coerce(&json!(2.5), Some(Numeric::Int)), json!(2.5), "not integral: left alone");
    }
}
