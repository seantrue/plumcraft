//! Content handles (spec section 4.1–4.3): `algo ":" recipe ":" digest`, for example
//! `md5:nd1:9e107d9d372bb6826bd81d3542a419d6`.
//!
//! - `nd1` names an array by today's `publish.digest` (dtype string, Python's shape repr, C-order
//!   bytes), so every array the research software publishes keeps its key. Big-endian arrays hash as their
//!   little-endian twin (new in protocol 2).
//! - `raw` names bytes as stored, which is alexandria's CAS digest.
//!
//! The algorithm and recipe sets are closed and the hex is exact lowercase: anything else is a
//! [`HandleError`], and a reference holding it is treated as absent.

use std::fmt;
use std::str::FromStr;

use md5::{Digest, Md5};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Algo {
    Md5,
    /// BLAKE3, required for uploads into stores shared by several clients (spec 4.3). Parsed
    /// here; computed by the stores that accept it.
    B3,
}

impl Algo {
    pub fn as_str(self) -> &'static str {
        match self {
            Algo::Md5 => "md5",
            Algo::B3 => "b3",
        }
    }

    /// Hex digits in a digest of this algorithm.
    pub fn hex_len(self) -> usize {
        match self {
            Algo::Md5 => 32,
            Algo::B3 => 64,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Recipe {
    /// An array: dtype string, shape repr, C-order bytes.
    Nd1,
    /// Bytes as stored.
    Raw,
}

impl Recipe {
    pub fn as_str(self) -> &'static str {
        match self {
            Recipe::Nd1 => "nd1",
            Recipe::Raw => "raw",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HandleError {
    #[error("not a handle: expected algo:recipe:digest")]
    Shape,
    #[error("unknown hash algorithm {0:?}")]
    Algo(String),
    #[error("unknown digest recipe {0:?}")]
    Recipe(String),
    #[error("digest must be {expected} lowercase hex digits")]
    Digest { expected: usize },
    #[error("unsupported dtype {0:?} (object and structured arrays have no content identity)")]
    Dtype(String),
    #[error("array bytes ({got}) don't match dtype and shape ({expected})")]
    Length { got: usize, expected: usize },
}

/// The content identity of an immutable blob. Cheap to clone; compares by value.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Handle {
    algo: Algo,
    recipe: Recipe,
    digest: String,
}

impl Handle {
    pub fn algo(&self) -> Algo {
        self.algo
    }

    pub fn recipe(&self) -> Recipe {
        self.recipe
    }

    /// The lowercase hex digest.
    pub fn digest(&self) -> &str {
        &self.digest
    }

    /// A handle from a digest computed elsewhere (a commonkit key's digest slot).
    pub fn from_digest(algo: Algo, recipe: Recipe, hex: &str) -> Result<Self, HandleError> {
        if hex.len() != algo.hex_len() || !hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
            return Err(HandleError::Digest { expected: algo.hex_len() });
        }
        Ok(Handle { algo, recipe, digest: hex.to_owned() })
    }

    /// `md5:raw:` of bytes as stored (alexandria's item digest).
    pub fn md5_raw(bytes: &[u8]) -> Self {
        Handle { algo: Algo::Md5, recipe: Recipe::Raw, digest: hex(&Md5::digest(bytes)) }
    }

    /// `md5:nd1:` of an array given as numpy would describe it: `dtype` is `dtype.str`
    /// (`<f4`, `|u1`, `>c8`, `<U3`, `<M8[ns]`), `shape` its dimensions and `bytes` its C-order
    /// element bytes. Byte order is normalised to little-endian first.
    pub fn md5_nd1(dtype: &str, shape: &[u64], bytes: &[u8]) -> Result<Self, HandleError> {
        let d = Dtype::parse(dtype)?;
        let count = shape.iter().try_fold(1u64, |n, &s| n.checked_mul(s)).ok_or(HandleError::Length { got: bytes.len(), expected: usize::MAX })?;
        let expected =
            usize::try_from(count).ok().and_then(|c| c.checked_mul(d.itemsize)).ok_or(HandleError::Length { got: bytes.len(), expected: usize::MAX })?;
        if bytes.len() != expected {
            return Err(HandleError::Length { got: bytes.len(), expected });
        }
        let mut h = Md5::new();
        h.update(d.normalised.as_bytes());
        h.update(shape_repr(shape).as_bytes());
        if d.swap_unit > 1 {
            let mut swapped = bytes.to_vec();
            for unit in swapped.chunks_exact_mut(d.swap_unit) {
                unit.reverse();
            }
            h.update(&swapped);
        } else {
            h.update(bytes);
        }
        Ok(Handle { algo: Algo::Md5, recipe: Recipe::Nd1, digest: hex(&h.finalize()) })
    }
}

impl fmt::Display for Handle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.algo.as_str(), self.recipe.as_str(), self.digest)
    }
}

impl FromStr for Handle {
    type Err = HandleError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.splitn(3, ':');
        let (Some(a), Some(r), Some(d)) = (parts.next(), parts.next(), parts.next()) else {
            return Err(HandleError::Shape);
        };
        let algo = match a {
            "md5" => Algo::Md5,
            "b3" => Algo::B3,
            other => return Err(HandleError::Algo(truncate(other))),
        };
        let recipe = match r {
            "nd1" => Recipe::Nd1,
            "raw" => Recipe::Raw,
            other => return Err(HandleError::Recipe(truncate(other))),
        };
        Handle::from_digest(algo, recipe, d)
    }
}

impl Serialize for Handle {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Handle {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let s = String::deserialize(de)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(feature = "schema")]
impl schemars::JsonSchema for Handle {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Handle".into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({
            "type": "string",
            "description": "algo:recipe:digest (spec 4.1)",
            "pattern": "^(md5:(nd1|raw):[0-9a-f]{32}|b3:(nd1|raw):[0-9a-f]{64})$"
        })
    }
}

/// Python's `repr` of a shape tuple: `()`, `(5,)`, `(812, 808, 3)`.
pub fn shape_repr(shape: &[u64]) -> String {
    match shape {
        [] => "()".into(),
        [one] => format!("({one},)"),
        many => format!("({})", many.iter().map(u64::to_string).collect::<Vec<_>>().join(", ")),
    }
}

/// A numpy `dtype.str`, as far as hashing needs it.
struct Dtype {
    /// The string hashed: big-endian and native orders written as `<`.
    normalised: String,
    itemsize: usize,
    /// Bytes per byte-swapped unit when the source is big-endian (1: no swap).
    swap_unit: usize,
}

impl Dtype {
    fn parse(s: &str) -> Result<Self, HandleError> {
        let bad = || HandleError::Dtype(truncate(s));
        let mut chars = s.chars();
        let order = chars.next().ok_or_else(bad)?;
        let kind = chars.next().ok_or_else(bad)?;
        let rest = chars.as_str();
        let (digits, suffix) = rest.find('[').map_or((rest, ""), |i| rest.split_at(i));
        if digits.is_empty() || digits.len() > 6 || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return Err(bad());
        }
        let n: usize = digits.parse().map_err(|_| bad())?;
        if !suffix.is_empty() && !(matches!(kind, 'M' | 'm') && suffix.ends_with(']') && suffix.len() <= 8) {
            return Err(bad());
        }
        // (itemsize, the unit swapped on a big-endian source)
        let (itemsize, unit) = match kind {
            'b' | 'i' | 'u' | 'f' | 'M' | 'm' => (n, n),
            'c' => (n, n / 2),
            'U' => (n.checked_mul(4).ok_or_else(bad)?, 4),
            'S' => (n, 1),
            // object ('O') and structured ('V') arrays: their bytes are pointers or layout
            _ => return Err(bad()),
        };
        if itemsize == 0 && kind != 'S' && kind != 'U' {
            return Err(bad());
        }
        let (order, swap_unit) = match order {
            '<' | '|' => (order, 1),
            '=' => ('<', 1),
            '>' if unit > 1 => ('<', unit),
            '>' => ('|', 1),
            _ => return Err(bad()),
        };
        Ok(Dtype { normalised: format!("{order}{kind}{digits}{suffix}"), itemsize, swap_unit })
    }
}

fn hex(bytes: &[u8]) -> String {
    use fmt::Write;
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Hostile text in an error message is cut short.
fn truncate(s: &str) -> String {
    s.chars().take(32).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    #[test]
    fn parses_and_prints() {
        let h: Handle = "md5:nd1:9e107d9d372bb6826bd81d3542a419d6".parse().unwrap();
        assert_eq!(h.algo(), Algo::Md5);
        assert_eq!(h.recipe(), Recipe::Nd1);
        assert_eq!(h.to_string(), "md5:nd1:9e107d9d372bb6826bd81d3542a419d6");
        let b3: Handle = format!("b3:raw:{}", "ab".repeat(32)).parse().unwrap();
        assert_eq!(b3.algo(), Algo::B3);
        assert_eq!(serde_json::to_string(&h).unwrap(), "\"md5:nd1:9e107d9d372bb6826bd81d3542a419d6\"");
    }

    #[test]
    fn hostile_handles_are_errors() {
        for bad in [
            "",
            "md5",
            "md5:nd1",
            "md5:nd1:",
            "sha1:nd1:9e107d9d372bb6826bd81d3542a419d6",
            "md5:nd2:9e107d9d372bb6826bd81d3542a419d6",
            "md5:nd1:9E107D9D372BB6826BD81D3542A419D6",
            "md5:nd1:9e107d9d372bb6826bd81d3542a419d",
            "md5:nd1:9e107d9d372bb6826bd81d3542a419d6:x",
            "md5:raw:../../../../etc/passwd000000000000",
            "b3:raw:9e107d9d372bb6826bd81d3542a419d6",
            "md5:nd1:9e107d9d372bb6826bd81d3542a419dé",
        ] {
            assert!(bad.parse::<Handle>().is_err(), "{bad}");
        }
        assert!(serde_json::from_str::<Handle>("42").is_err());
    }

    #[test]
    fn shape_repr_matches_python() {
        assert_eq!(shape_repr(&[]), "()");
        assert_eq!(shape_repr(&[5]), "(5,)");
        assert_eq!(shape_repr(&[812, 808, 3]), "(812, 808, 3)");
    }

    #[test]
    fn dtypes() {
        for ok in ["<f4", "|u1", "|b1", ">c8", "<U3", "<M8[ns]", "|S5", ">i2"] {
            assert!(Dtype::parse(ok).is_ok(), "{ok}");
        }
        for bad in ["", "<", "|O8", "|V16", "<f", "<fx", "<f4[ns]", "?f4", "<q8", "<f99999999"] {
            assert!(Dtype::parse(bad).is_err(), "{bad}");
        }
        assert_eq!(Dtype::parse(">f4").unwrap().normalised, "<f4");
        assert_eq!(Dtype::parse(">c8").unwrap().swap_unit, 4);
    }

    #[test]
    fn wrong_length_is_an_error() {
        assert!(matches!(Handle::md5_nd1("<f4", &[2, 3], &[0; 23]), Err(HandleError::Length { got: 23, expected: 24 })));
        assert!(Handle::md5_nd1("<f4", &[u64::MAX, 3], &[]).is_err());
    }

    /// The shared vectors (`fixtures/handles.json`), made by research_engine's own
    /// `publish.digest` and `hashlib.md5` (claude-debug/plumcraft-proto/make_handle_vectors.py).
    #[test]
    fn shared_vectors() {
        let text = include_str!("../fixtures/handles.json");
        let v: serde_json::Value = serde_json::from_str(text).unwrap();
        let vectors = v["vectors"].as_array().unwrap();
        assert!(vectors.len() >= 10);
        for t in vectors {
            let name = t["name"].as_str().unwrap();
            let bytes = base64::engine::general_purpose::STANDARD.decode(t["bytes_b64"].as_str().unwrap()).unwrap();
            let expect: Handle = t["handle"].as_str().unwrap().parse().unwrap();
            let got = match t["recipe"].as_str().unwrap() {
                "nd1" => {
                    let shape: Vec<u64> = t["shape"].as_array().unwrap().iter().map(|s| s.as_u64().unwrap()).collect();
                    Handle::md5_nd1(t["dtype"].as_str().unwrap(), &shape, &bytes).unwrap()
                }
                _ => Handle::md5_raw(&bytes),
            };
            assert_eq!(got, expect, "{name}");
        }
    }
}
