//! Data references (spec sections 4.4–4.6): how a message names bytes without carrying them.
//!
//! **The handle is identity; locators are hints.** Two [`DataRef`]s with one handle are the same
//! value whatever their locators say. A locator that no longer resolves is a miss, never an
//! error (spec 4.6).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::de::{lenient, lenient_opt, lenient_vec, null_as_default};
use crate::handle::Handle;

/// A handle plus typed metadata plus the ways to reach the bytes, in the engine's order of
/// preference.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct DataRef {
    /// `None` only for a protocol 1 array published without a commonkit key; the reader may
    /// hash the file if it needs an identity (spec 17).
    #[serde(deserialize_with = "lenient_opt", skip_serializing_if = "Option::is_none")]
    pub handle: Option<Handle>,
    #[serde(deserialize_with = "lenient")]
    pub meta: BlobMeta,
    /// Unknown or malformed locators are skipped.
    #[serde(deserialize_with = "lenient_vec")]
    pub at: Vec<Locator>,
}

impl DataRef {
    /// The first locator of a kind the reader can use, in the engine's order.
    pub fn first(&self, usable: impl Fn(&Locator) -> bool) -> Option<&Locator> {
        self.at.iter().find(|l| usable(l))
    }
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct BlobMeta {
    /// A media type: `application/x-npy`, `text/plain; charset=utf-8`, `image/png`, …
    #[serde(deserialize_with = "null_as_default")]
    pub media: String,
    /// Bytes, checked against what is read (spec 18).
    #[serde(deserialize_with = "null_as_default")]
    pub size: u64,
    #[serde(deserialize_with = "lenient")]
    pub form: Form,
    /// Other handles of the same bytes (an `md5` item's `b3` twin).
    #[serde(deserialize_with = "lenient_vec", skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<Handle>,
}

/// What the bytes hold, enough to choose a viewer before reading them. An unknown kind reads as
/// [`Form::Bytes`].
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Form {
    Array {
        #[serde(default)]
        dtype: String,
        #[serde(default)]
        shape: Vec<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        space: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        names: Vec<String>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        table: bool,
    },
    Text {
        #[serde(default)]
        lines: u64,
        #[serde(default)]
        encoding: String,
    },
    Json,
    /// PCM stored as (frames, channels).
    Audio {
        #[serde(default)]
        sample_rate: u32,
        #[serde(default)]
        channels: u16,
        #[serde(default)]
        frames: u64,
        #[serde(default)]
        dtype: String,
    },
    /// SVG and similar.
    Vector {
        #[serde(default)]
        width: f64,
        #[serde(default)]
        height: f64,
    },
    /// An encoded source picture.
    Image {
        #[serde(default)]
        width: u32,
        #[serde(default)]
        height: u32,
    },
    Mesh {
        #[serde(default)]
        vertices: u64,
        #[serde(default)]
        faces: u64,
    },
    /// An application document (an artcraft file).
    Doc {
        #[serde(default)]
        app: String,
    },
    /// Named typed columns (media `table`), stored as JSON `{"columns": [{name, type, values}]}`.
    Table {
        #[serde(default)]
        rows: u64,
        #[serde(default)]
        columns: Vec<Column>,
    },
    #[default]
    #[serde(other)]
    Bytes,
}

/// A table column: its name and the wire type of its values.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct Column {
    #[serde(deserialize_with = "crate::de::null_as_default")]
    pub name: String,
    #[serde(rename = "type", deserialize_with = "crate::de::null_as_default")]
    pub ty: String,
}

/// One way to reach a blob's bytes. Readers take the first they can use, cheapest first:
/// `mem`, `commonkit`, `path`, their own CAS by handle, `url`, then a derivative request.
/// `commonkit` and `path` are honoured only after the same-host proof, under declared roots
/// (spec 4.4, 5.4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Locator {
    /// A commonkit key, resolved under the reader's own store root.
    Commonkit { key: String },
    /// A file on the same host; `offset` into the payload of a wrapping `.npy`.
    Path {
        path: String,
        #[serde(default, skip_serializing_if = "is_zero")]
        offset: u64,
    },
    /// In-process binding only: a slot in the host's byte table.
    Mem { slot: u64 },
    Url {
        url: String,
        /// Seconds since the epoch.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expires: Option<f64>,
    },
    /// Base64 bytes, at most the session's inline limit.
    Inline { b64: String },
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// A port value in a reply, snapshot or event (spec 4.5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PortValue {
    Blob {
        #[serde(rename = "ref")]
        data: DataRef,
    },
    List {
        #[serde(default, deserialize_with = "lenient_vec")]
        items: Vec<PortValue>,
    },
    /// Numbers, bools, strings, settings, curves.
    Json {
        #[serde(default)]
        value: Value,
    },
    /// A value the engine keeps to itself (a STYLE), with a line describing it.
    Opaque {
        #[serde(default, rename = "type")]
        ty: String,
        #[serde(default)]
        summary: String,
        /// The engine still has it (`values.held`, spec 7.6 rule 11): pass it back to that
        /// engine as an input of the same run.
        #[serde(default, deserialize_with = "crate::de::lenient_opt", skip_serializing_if = "Option::is_none")]
        held: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPEC_EXAMPLE: &str = r#"{"handle": "md5:nd1:9e107d9d372bb6826bd81d3542a419d6",
     "meta": {"media": "application/x-npy", "size": 7873536,
              "form": {"kind": "array", "dtype": "<f4", "shape": [812, 808, 3],
                       "space": "Lab", "names": ["L", "a", "b"], "table": false}},
     "at": [{"kind": "commonkit", "key": "impc:v1:impcarr:9e107d9d372bb6826bd81d3542a419d6:c"},
            {"kind": "path", "path": "/Users/me/Library/Caches/commonkit/store/plumarr/9e/9e107d9d372bb6826bd81d3542a419d6.c.npy"},
            {"kind": "url", "url": "https://engine.example:7311/v1/blobs/md5/nd1/9e107d9d372bb6826bd81d3542a419d6"}]}"#;

    #[test]
    fn spec_example_round_trips() {
        let r: DataRef = serde_json::from_str(SPEC_EXAMPLE).unwrap();
        assert_eq!(r.at.len(), 3);
        assert!(matches!(&r.meta.form, Form::Array { shape, .. } if shape == &[812, 808, 3]));
        let back: DataRef = serde_json::from_value(serde_json::to_value(&r).unwrap()).unwrap();
        assert_eq!(r, back);
    }

    #[test]
    fn unknown_and_malformed_parts_are_dropped() {
        let r: DataRef = serde_json::from_str(
            r#"{"handle": "md5:nd1:NOT-HEX", "meta": {"size": -4, "form": {"kind": "hologram"}},
                "at": [{"kind": "carrier-pigeon"}, {"kind": "path"}, {"kind": "mem", "slot": 3}, 7]}"#,
        )
        .unwrap();
        assert!(r.handle.is_none());
        assert_eq!(r.meta, BlobMeta::default());
        assert_eq!(r.at, [Locator::Mem { slot: 3 }]);
        let f: Form = serde_json::from_str(r#"{"kind": "hologram", "x": 1}"#).unwrap();
        assert_eq!(f, Form::Bytes);
    }

    #[test]
    fn port_values() {
        let v: PortValue = serde_json::from_str(
            r#"{"kind": "list", "items": [{"kind": "json", "value": 3}, {"kind": "??"}, {"kind": "opaque", "type": "STYLE", "summary": "style 'pencil'"}]}"#,
        )
        .unwrap();
        let PortValue::List { items } = v else { panic!("not a list") };
        assert_eq!(items.len(), 2);
        assert!(serde_json::from_str::<PortValue>(r#"{"kind": "blob"}"#).is_err());
    }
}
