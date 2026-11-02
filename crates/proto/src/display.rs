//! Displays, renditions and watch requests (spec section 10): what the viewer shows for one
//! (node, port, index), for any media kind, without the UI understanding the value.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::catalogue::Media;
use crate::data::DataRef;
use crate::de::{lenient, lenient_opt, lenient_vec, null_as_default};
use crate::error::NodeError;

/// The most array views a display carries (`MAX_PREVIEWS`).
pub const MAX_VIEWS: usize = 4;
/// The most items a sheet shows, and a cell's long side.
pub const SHEET_ITEMS: usize = 64;
pub const SHEET_PX: u32 = 256;

/// A view of a value: an id the wire type lists, and its parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct View {
    #[serde(deserialize_with = "null_as_default")]
    pub id: String,
    #[serde(deserialize_with = "null_as_default")]
    pub params: serde_json::Map<String, Value>,
}

impl Default for View {
    fn default() -> Self {
        View { id: "auto".into(), params: Default::default() }
    }
}

/// A derivative the viewer wants: `{"kind": "rgba8"}`, `{"kind": "peaks", "px": 1600}`,
/// `{"kind": "full"}`. Parameters are kind-specific and kept as JSON.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Want {
    #[serde(default, deserialize_with = "null_as_default")]
    pub kind: String,
    #[serde(flatten)]
    pub params: serde_json::Map<String, Value>,
}

impl Want {
    pub fn kind(kind: &str) -> Self {
        Want { kind: kind.into(), params: Default::default() }
    }
}

/// One value the UI wants to see while (or after) a run (spec 10.4; protocol 1 `inspect`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct WatchRequest {
    #[serde(deserialize_with = "null_as_default")]
    pub node: String,
    #[serde(deserialize_with = "null_as_default")]
    pub port: String,
    #[serde(deserialize_with = "null_as_default")]
    pub index: i64,
    /// Every item of a list, small and labelled.
    #[serde(deserialize_with = "null_as_default", skip_serializing_if = "std::ops::Not::not")]
    pub sheet: bool,
    #[serde(deserialize_with = "lenient")]
    pub view: View,
    /// Empty: the media kind's defaults.
    #[serde(deserialize_with = "lenient_vec", skip_serializing_if = "Vec::is_empty")]
    pub want: Vec<Want>,
    /// The long side of view renditions.
    pub px: u32,
}

impl Default for WatchRequest {
    fn default() -> Self {
        WatchRequest { node: String::new(), port: String::new(), index: 0, sheet: false, view: View::default(), want: Vec::new(), px: 1024 }
    }
}

/// An engine-made derivative the UI draws without understanding the value (spec 10.3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Rendition {
    /// (h, w, 4) `u1`.
    Rgba8 {
        #[serde(rename = "ref")]
        data: DataRef,
        #[serde(default)]
        w: u32,
        #[serde(default)]
        h: u32,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        role: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        caption: String,
        /// A one-line description of the value ("array HSI 64×80 float32").
        #[serde(default, skip_serializing_if = "String::is_empty")]
        summary: String,
        /// List length when the value is a list (0: a single value).
        #[serde(default, skip_serializing_if = "is_zero")]
        items: u64,
    },
    /// (levels, channels, bins, 2) min/max.
    Peaks {
        #[serde(rename = "ref")]
        data: DataRef,
        #[serde(default)]
        bins: u32,
        #[serde(default)]
        channels: u16,
    },
    /// (frames, bins) f32 dB.
    Spectrogram {
        #[serde(rename = "ref")]
        data: DataRef,
        #[serde(default)]
        hop: u32,
        #[serde(default)]
        fft: u32,
        #[serde(default)]
        rate: u32,
    },
    /// Up to 4 KiB of text and where it sits.
    Excerpt {
        #[serde(default)]
        text: String,
        #[serde(default)]
        offset: u64,
        #[serde(default)]
        total_chars: u64,
        #[serde(default)]
        lines: u64,
    },
    Svg {
        #[serde(rename = "ref")]
        data: DataRef,
    },
    MeshLod {
        #[serde(rename = "ref")]
        data: DataRef,
    },
    /// Small inline series.
    Plot {
        #[serde(default)]
        series: Value,
    },
    /// Inline JSON, depth ≤ 32.
    Tree {
        #[serde(default)]
        value: Value,
    },
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

/// One channel's range and histogram.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct ChannelStats {
    #[serde(deserialize_with = "null_as_default")]
    pub name: String,
    pub min: Option<f64>,
    pub max: Option<f64>,
    #[serde(deserialize_with = "null_as_default", skip_serializing_if = "is_zero")]
    pub nan: u64,
    #[serde(deserialize_with = "lenient_opt", skip_serializing_if = "Option::is_none")]
    pub histogram: Option<Histogram>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct Histogram {
    #[serde(deserialize_with = "null_as_default")]
    pub bins: u32,
    pub lo: Option<f64>,
    pub hi: Option<f64>,
    #[serde(deserialize_with = "null_as_default")]
    pub counts: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct Stats {
    #[serde(deserialize_with = "lenient_vec")]
    pub channels: Vec<ChannelStats>,
    /// Stats too large to carry inline, as a JSON blob (spec 10.2).
    #[serde(deserialize_with = "lenient_opt", skip_serializing_if = "Option::is_none")]
    pub blob: Option<DataRef>,
}

/// One item of a list as the sheet shows it.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct SheetCell {
    /// What made it differ from the others ("dim 150 · resample bilinear").
    #[serde(deserialize_with = "null_as_default")]
    pub label: String,
    #[serde(deserialize_with = "null_as_default")]
    pub summary: String,
    #[serde(deserialize_with = "lenient_opt")]
    pub thumb: Option<Rendition>,
}

/// Whether a display is a provisional value from a node still running (spec 10.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct LiveMark {
    pub provisional: bool,
    pub seq: u64,
}

/// What the viewer shows for one watched value.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(default)]
pub struct Display {
    #[serde(deserialize_with = "lenient")]
    pub media: Media,
    /// The wire type.
    #[serde(rename = "type", deserialize_with = "null_as_default")]
    pub ty: String,
    #[serde(deserialize_with = "null_as_default")]
    pub node: String,
    #[serde(deserialize_with = "null_as_default")]
    pub port: String,
    /// Items in the list (1 for a single value) and the one shown.
    #[serde(deserialize_with = "null_as_default")]
    pub items: u64,
    #[serde(deserialize_with = "null_as_default")]
    pub index: i64,
    #[serde(deserialize_with = "null_as_default")]
    pub summary: String,
    /// The value itself, read lazily (probe, compare, playback, 1:1 zoom).
    #[serde(deserialize_with = "lenient_opt")]
    pub full: Option<DataRef>,
    #[serde(deserialize_with = "lenient")]
    pub view: View,
    /// Unknown kinds are skipped.
    #[serde(deserialize_with = "lenient_vec")]
    pub renditions: Vec<Rendition>,
    #[serde(deserialize_with = "lenient")]
    pub stats: Stats,
    /// A scalar.
    pub value: Option<Value>,
    /// What made the item shown: `{style?, params, settings, graph}`.
    pub recipe: Option<Value>,
    #[serde(deserialize_with = "lenient_vec", skip_serializing_if = "Vec::is_empty")]
    pub sheet: Vec<SheetCell>,
    #[serde(deserialize_with = "lenient")]
    pub live: LiveMark,
    #[serde(deserialize_with = "lenient_opt")]
    pub error: Option<NodeError>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_display_parses() {
        let d: Display = serde_json::from_str(
            r#"{"media": "ndarray", "type": "ARRAY", "node": "n7", "port": "value", "items": 7, "index": 2,
                "summary": "array Lab 812×808×3 float32",
                "full": {"handle": "md5:nd1:9e107d9d372bb6826bd81d3542a419d6", "at": [{"kind": "path", "path": "/x.npy"}]},
                "view": {"id": "heat", "params": {}},
                "renditions": [{"kind": "rgba8", "role": "view", "caption": "L", "w": 1024, "h": 1019, "ref": {"at": []}},
                               {"kind": "hologram"}],
                "stats": {"channels": [{"name": "L", "min": 0.0, "max": 100.0, "nan": 0,
                                         "histogram": {"bins": 4, "lo": 0.0, "hi": 100.0, "counts": [1, 2, 3, 4]}}]},
                "live": {"provisional": false, "seq": 0}}"#,
        )
        .unwrap();
        assert_eq!(d.renditions.len(), 1);
        assert_eq!(d.stats.channels[0].histogram.as_ref().unwrap().counts, [1, 2, 3, 4]);
        assert_eq!(d.view.id, "heat");
        let back: Display = serde_json::from_value(serde_json::to_value(&d).unwrap()).unwrap();
        assert_eq!(back, d);
    }

    #[test]
    fn watch_defaults_and_wants() {
        let w: WatchRequest =
            serde_json::from_str(r#"{"node": "n3", "port": "audio", "want": [{"kind": "peaks", "px": 1600, "t0": 0.0}, {"kind": "full"}, 5]}"#).unwrap();
        assert_eq!(w.px, 1024);
        assert_eq!(w.view.id, "auto");
        assert_eq!(w.want.len(), 2);
        assert_eq!(w.want[0].params["px"], 1600);
        let v = serde_json::to_value(&w).unwrap();
        assert_eq!(v["want"][0]["px"], 1600, "parameters flatten back");
    }
}
