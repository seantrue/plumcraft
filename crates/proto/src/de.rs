//! Lenient deserialisation helpers (spec section 3, rules 5 and 8): absent and `null` mean the
//! default, and a malformed optional part of a message is dropped instead of failing the whole
//! message.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

/// Treat an explicit JSON `null` as the field's default (Python writes unset fields as `null`).
pub fn null_as_default<'de, D, T>(de: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(de)?.unwrap_or_default())
}

/// An optional part that is dropped (`None`) when it doesn't parse: a bad handle, an unknown
/// rendition kind. Any JSON at all is accepted here; only the inner type decides.
pub fn lenient_opt<'de, D, T>(de: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let v = Value::deserialize(de)?;
    Ok(serde_json::from_value(v).ok())
}

/// A field that falls back to its default when it doesn't parse.
pub fn lenient<'de, D, T>(de: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned + Default,
{
    let v = Value::deserialize(de)?;
    Ok(serde_json::from_value(v).unwrap_or_default())
}

/// A list whose malformed or unknown items are skipped. A non-list (or `null`) is empty.
pub fn lenient_vec<'de, D, T>(de: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let v = Value::deserialize(de)?;
    Ok(match v {
        Value::Array(items) => items.into_iter().filter_map(|i| serde_json::from_value(i).ok()).collect(),
        _ => Vec::new(),
    })
}

/// A string-keyed map whose malformed values are skipped. A non-object (or `null`) is empty.
pub fn lenient_map<'de, D, T>(de: D) -> Result<std::collections::BTreeMap<String, T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let v = Value::deserialize(de)?;
    Ok(match v {
        Value::Object(items) => items.into_iter().filter_map(|(k, i)| serde_json::from_value(i).ok().map(|t| (k, t))).collect(),
        _ => Default::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Default, Deserialize)]
    #[serde(default)]
    struct T {
        #[serde(deserialize_with = "null_as_default")]
        n: u32,
        #[serde(deserialize_with = "lenient_opt")]
        o: Option<u8>,
        #[serde(deserialize_with = "lenient")]
        l: u8,
        #[serde(deserialize_with = "lenient_vec")]
        v: Vec<u8>,
        #[serde(deserialize_with = "lenient_map")]
        m: std::collections::BTreeMap<String, u8>,
    }

    #[test]
    fn bad_parts_are_dropped_not_fatal() {
        let t: T = serde_json::from_str(r#"{"n": null, "o": 300, "l": "x", "v": [1, "two", 3, -1], "m": {"a": 1, "b": []}}"#).unwrap();
        assert_eq!(t.n, 0);
        assert_eq!(t.o, None);
        assert_eq!(t.l, 0);
        assert_eq!(t.v, [1, 3]);
        assert_eq!(t.m.len(), 1);
        let t: T = serde_json::from_str(r#"{"v": {"not": "a list"}, "m": [1]}"#).unwrap();
        assert!(t.v.is_empty() && t.m.is_empty());
    }
}
