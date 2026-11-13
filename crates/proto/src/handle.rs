//! Content handles (spec section 4.1–4.3): `algo ":" recipe ":" digest`, for example
//! `md5:nd1:9e107d9d372bb6826bd81d3542a419d6`.
//!
//! - `nd1` names an array by today's `publish.digest` (dtype string, Python's shape repr, C-order
//!   bytes), so every array the research software publishes keeps its key. Big-endian arrays hash as their
//!   little-endian twin (new in protocol 2).
//! - `raw` names bytes as stored.
//!
//! The algorithm and recipe sets are closed and the hex is exact lowercase: anything else is a
//! [`HandleError`], and a reference holding it is treated as absent.
//!
//! Every handle this process makes is in one algorithm, [`Algo::current`]: commonkit's switch
//! (the tree's `HASH` in `.policy.json`, then `COMMONKIT_HASH`, then `b3`), resolved once per
//! process. No code path chooses (#91, #92, #93). Digests come from `commonkit::digest`.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Algo {
    Md5,
    /// BLAKE3: required for uploads into stores shared by several clients (spec 4.3; B10: MD5
    /// was the slowest step of a large array's crossing).
    B3,
}

impl Algo {
    /// This process's algorithm: commonkit's `policy::current()`, resolved once (the tree's
    /// `HASH`, then `COMMONKIT_HASH`, then `b3`). Every handle the process makes uses it.
    pub fn current() -> Algo {
        match commonkit::digest::current() {
            commonkit::digest::Algo::Md5 => Algo::Md5,
            _ => Algo::B3,
        }
    }

    /// Where [`Algo::current`] came from: `policy file <path>`, `environment COMMONKIT_HASH` or
    /// `default` (the status bar's `hash md5 · policy file`).
    pub fn source() -> String {
        commonkit::policy::source().to_string()
    }

    /// `md5` or `b3`.
    pub fn from_name(s: &str) -> Option<Algo> {
        match s {
            "md5" => Some(Algo::Md5),
            "b3" => Some(Algo::B3),
            _ => None,
        }
    }

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

    /// `<current>:raw:` of bytes as stored ([`Algo::current`]).
    pub fn raw(bytes: &[u8]) -> Self {
        Handle { algo: Algo::current(), recipe: Recipe::Raw, digest: commonkit::digest::raw(bytes) }
    }

    /// `<current>:nd1:` of an array given as numpy would describe it: `dtype` is `dtype.str`
    /// (`<f4`, `|u1`, `>c8`, `<U3`, `<M8[ns]`), `shape` its dimensions and `bytes` its C-order
    /// element bytes. Byte order is normalised to little-endian first. The recipe is the same
    /// for both algorithms (spec 4.2), and is commonkit's `nd1` (contract §3.8).
    pub fn nd1(dtype: &str, shape: &[u64], bytes: &[u8]) -> Result<Self, HandleError> {
        let digest = commonkit::digest::nd1(dtype, shape, bytes).map_err(|e| match e {
            commonkit::digest::Nd1Error::Length { got, expected } => HandleError::Length { got, expected: expected.unwrap_or(usize::MAX) },
            _ => HandleError::Dtype(truncate(dtype)),
        })?;
        Ok(Handle { algo: Algo::current(), recipe: Recipe::Nd1, digest })
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
            assert!(!matches!(Handle::nd1(ok, &[0], &[]), Err(HandleError::Dtype(_))), "{ok}");
        }
        for bad in ["", "<", "|O8", "|V16", "<f", "<fx", "<f4[ns]", "?f4", "<q8", "<f99999999"] {
            assert!(matches!(Handle::nd1(bad, &[0], &[]), Err(HandleError::Dtype(_))), "{bad}");
        }
        // Big-endian hashes as its little-endian twin.
        assert_eq!(Handle::nd1(">f4", &[1], &[0x3f, 0x80, 0, 0]).unwrap(), Handle::nd1("<f4", &[1], &[0, 0, 0x80, 0x3f]).unwrap());
    }

    #[test]
    fn wrong_length_is_an_error() {
        assert!(matches!(Handle::nd1("<f4", &[2, 3], &[0; 23]), Err(HandleError::Length { got: 23, expected: 24 })));
        assert!(Handle::nd1("<f4", &[u64::MAX, 3], &[]).is_err());
    }

    /// The shared vectors (`fixtures/handles.json`): md5 from research_engine's own
    /// `publish.digest` and `hashlib.md5` (claude-debug/make_handle_vectors.py), b3 from the
    /// `blake3` package (claude-debug/make_b3_handle_vectors.py). Each process checks its own
    /// algorithm's column; the suite runs once per algorithm.
    #[test]
    fn shared_vectors() {
        let text = include_str!("../fixtures/handles.json");
        let v: serde_json::Value = serde_json::from_str(text).unwrap();
        let vectors = v["vectors"].as_array().unwrap();
        assert!(vectors.len() >= 10);
        for t in vectors {
            let name = t["name"].as_str().unwrap();
            let bytes = base64::engine::general_purpose::STANDARD.decode(t["bytes_b64"].as_str().unwrap()).unwrap();
            let field = if Algo::current() == Algo::Md5 { "handle" } else { "b3" };
            let expect: Handle = t[field].as_str().unwrap_or_else(|| panic!("{name}: no {field}")).parse().unwrap();
            let got = match t["recipe"].as_str().unwrap() {
                "nd1" => {
                    let shape: Vec<u64> = t["shape"].as_array().unwrap().iter().map(|s| s.as_u64().unwrap()).collect();
                    Handle::nd1(t["dtype"].as_str().unwrap(), &shape, &bytes).unwrap()
                }
                _ => Handle::raw(&bytes),
            };
            assert_eq!(got, expect, "{name} ({field})");
        }
    }

    /// Every handle is in the process's algorithm, and its digest has that length.
    #[test]
    fn handles_follow_the_switch() {
        let a = Algo::current();
        let h = Handle::raw(b"x");
        assert_eq!((h.algo(), h.digest().len()), (a, a.hex_len()));
        let h = Handle::nd1("<f4", &[1], &[0; 4]).unwrap();
        assert_eq!((h.algo(), h.digest().len()), (a, a.hex_len()));
    }

    /// The suite runs isolated, in the algorithm it was asked for (the hash facade, §3.1).
    #[test]
    fn hash_is_what_was_asked() {
        commonkit::testing::assert_isolated();
        if let Ok(asked) = std::env::var("COMMONKIT_HASH") {
            assert_eq!(Algo::from_name(asked.trim()), Some(Algo::current()), "COMMONKIT_HASH={asked}");
        }
    }
}
