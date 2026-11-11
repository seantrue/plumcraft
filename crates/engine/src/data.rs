//! How a value crosses the interface (spec 4.4, 4.5, 10.2; plan 10.2, D1).
//!
//! - **Small JSON** (at most `limits.max_inline` bytes) travels as `{"kind": "json"}`.
//! - **Large text** becomes an `md5:raw:` blob of its UTF-8 bytes; a large **table** an
//!   `md5:raw:` blob of its JSON. Both live in commonkit as 1-d `|u1` segments.
//! - **Arrays** are always blobs (`md5:nd1:`), so their identity never depends on size: inline
//!   bytes when small, a commonkit segment otherwise.
//! - A blob that needs the store is published only when the session proved it shares the
//!   UI's filesystem (`shm`); otherwise it is `unavailable`: a socket engine fails closed rather
//!   than inventing byte frames (spec 16.3). The HTTP binding (D2) will add `url` locators.

use plumcraft_proto::PortValue;
use plumcraft_proto::catalogue::Media;
use plumcraft_proto::data::{BlobMeta, Column, DataRef, Form, Locator};
use plumcraft_proto::display::Rendition;
use plumcraft_proto::error::NodeError;
use plumcraft_proto::handle::{Algo, Handle};
use plumcraft_store::{Store, inline_ref};
use serde_json::Value;

use crate::{Array, Audio, Blob, BlobSource, Data};

/// What the executor needs to publish values for one run.
#[derive(Debug, Clone, Copy)]
pub struct Plane<'a> {
    pub store: Option<&'a Store>,
    /// The UI proved it reads this engine's files (spec 5.4; always true in-process).
    pub shm: bool,
    /// An HTTP session without the proof: publish to this engine's own store and hand out
    /// `url` locators the UI fetches by handle (spec 13.2 step 5).
    pub urls: bool,
    pub max_inline: usize,
    /// The algorithm of the handles this run makes (spec 4.3; the request's `algo`).
    pub algo: Algo,
}

/// The path a blob is served at on the HTTP binding, relative to the engine's base URL.
pub fn blob_url(handle: &Handle) -> String {
    format!("/v1/blobs/{}/{}/{}", handle.algo().as_str(), handle.recipe().as_str(), handle.digest())
}

pub const TEXT_MEDIA: &str = plumcraft_proto::catalogue::core_types::TEXT_MEDIA;
pub const NPY_MEDIA: &str = "application/x-npy";
pub const JSON_MEDIA: &str = plumcraft_proto::catalogue::core_types::TABLE_MEDIA;

impl Plane<'_> {
    /// A blob's DataRef: inline bytes when they fit, otherwise published to the store.
    fn blob(&self, handle: Handle, meta: BlobMeta, bytes: &[u8], publish: impl FnOnce(&Store) -> Option<(Handle, Vec<Locator>)>) -> Result<DataRef, NodeError> {
        if bytes.len() <= self.max_inline {
            return Ok(inline_ref(handle, meta, bytes));
        }
        let store = self.store.filter(|_| self.shm || self.urls).ok_or_else(|| {
            NodeError::new("unavailable", format!("a {} byte value needs the shared store, and this session's same-host proof didn't pass", bytes.len()))
        })?;
        let (handle, at) = publish(store).ok_or_else(|| NodeError::new("unavailable", "the store declined the value"))?;
        // Remote: the store's files mean nothing to the UI; it fetches by handle instead.
        let at = if self.shm { at } else { vec![Locator::Url { url: blob_url(&handle), expires: None }] };
        Ok(DataRef { handle: Some(handle), meta, at })
    }

    /// A text value as a blob.
    pub fn text_ref(&self, s: &str) -> Result<DataRef, NodeError> {
        let meta = BlobMeta {
            media: TEXT_MEDIA.into(),
            size: s.len() as u64,
            form: Form::Text { lines: s.lines().count() as u64, encoding: "utf-8".into() },
            aliases: Vec::new(),
        };
        let handle = Handle::raw(self.algo, s.as_bytes());
        self.blob(handle.clone(), meta, s.as_bytes(), |st| st.publish_raw_as(&handle, s.as_bytes()))
    }

    /// An array as a blob.
    pub fn array_ref(&self, a: &Array) -> Result<DataRef, NodeError> {
        let handle = Handle::nd1(self.algo, &a.dtype, &a.shape, &a.bytes).map_err(|e| NodeError::new(NodeError::OP, format!("bad array: {e}")))?;
        let meta = BlobMeta {
            media: NPY_MEDIA.into(),
            size: a.bytes.len() as u64,
            form: Form::Array { dtype: a.dtype.clone(), shape: a.shape.clone(), space: a.space.clone(), names: a.names.clone(), table: a.table },
            aliases: Vec::new(),
        };
        self.blob(handle.clone(), meta, &a.bytes, |st| st.publish_array_as(&handle, &a.dtype, &a.shape, &a.bytes))
    }

    /// Audio as a blob: an `<f4` (frames, channels) array with `Form::Audio`.
    pub fn audio_ref(&self, a: &Audio) -> Result<DataRef, NodeError> {
        let shape = [a.frames(), u64::from(a.channels)];
        let bytes = a.bytes();
        let handle = Handle::nd1(self.algo, "<f4", &shape, &bytes).map_err(|e| NodeError::new(NodeError::OP, format!("bad audio: {e}")))?;
        let meta = BlobMeta {
            media: NPY_MEDIA.into(),
            size: bytes.len() as u64,
            form: Form::Audio { sample_rate: a.sample_rate, channels: a.channels, frames: a.frames(), dtype: "<f4".into() },
            aliases: Vec::new(),
        };
        self.blob(handle.clone(), meta, &bytes, |st| st.publish_array_as(&handle, "<f4", &shape, &bytes))
    }

    /// A [`Blob`] value. Bytes in memory cross as text does (inline when small, else the store).
    /// A file is read and published the same way when `publish` is set and it fits the store's
    /// read cap; otherwise it crosses by its `path` locator (after the same-host proof; a remote
    /// session gets the handle and metadata with no locator, a miss it may report, spec 4.6).
    pub fn blob_ref(&self, b: &Blob) -> Result<DataRef, NodeError> {
        let p = match &b.source {
            // A node's own handle: checked once, so a store key always names its bytes (W1).
            BlobSource::Bytes(bytes) if Handle::raw(b.handle.algo(), bytes) != b.handle => {
                return Err(NodeError::new(NodeError::OP, format!("a blob's bytes aren't its handle {}", b.handle)));
            }
            BlobSource::Bytes(bytes) => return self.blob(b.handle.clone(), b.meta.clone(), bytes, |st| st.publish_raw_as(&b.handle, bytes)),
            BlobSource::File(p) => p,
        };
        let fits = std::fs::metadata(p).is_ok_and(|m| m.len() <= plumcraft_store::MAX_READ);
        if !(b.publish && fits) {
            let at = if self.shm { vec![Locator::Path { path: p.display().to_string(), offset: 0 }] } else { Vec::new() };
            return Ok(DataRef { handle: Some(b.handle.clone()), meta: b.meta.clone(), at });
        }
        // Already stored (an earlier report, or a persisted record): touched, not read again.
        let stored = self.store.filter(|_| b.meta.size > self.max_inline as u64 && (self.shm || self.urls)).and_then(|s| s.locate(&b.handle));
        if let Some(at) = stored {
            let at = if self.shm { at } else { vec![Locator::Url { url: blob_url(&b.handle), expires: None }] };
            return Ok(DataRef { handle: Some(b.handle.clone()), meta: b.meta.clone(), at });
        }
        let bytes = std::fs::read(p).map_err(|e| NodeError::new(NodeError::OP, format!("{}: {e}", p.display())))?;
        if Handle::raw(b.handle.algo(), &bytes) != b.handle {
            return Err(NodeError::new(NodeError::OP, format!("{} no longer holds {}", p.display(), b.handle)));
        }
        self.blob(b.handle.clone(), b.meta.clone(), &bytes, |st| st.publish_raw_as(&b.handle, &bytes))
    }

    /// A blob's thumbnail as an `rgba8` rendition.
    pub fn blob_thumb(&self, b: &Blob) -> Option<Rendition> {
        self.rgba8(b.thumb.as_ref()?, "", "")
    }

    /// An (h, w, 4) `|u1` array as an `rgba8` rendition with `role` and `caption`.
    pub fn rgba8(&self, t: &Array, role: &str, caption: &str) -> Option<Rendition> {
        let (h, w) = match t.shape.as_slice() {
            [h, w, 4] if t.dtype == "|u1" => (u32::try_from(*h).ok()?, u32::try_from(*w).ok()?),
            _ => return None,
        };
        let data = self.array_ref(t).ok()?;
        Some(Rendition::Rgba8 { data, w, h, role: role.into(), caption: caption.into(), summary: String::new(), items: 0 })
    }

    /// A table (columnar JSON) as a blob.
    pub fn table_ref(&self, v: &Value) -> Result<DataRef, NodeError> {
        let bytes = serde_json::to_vec(v).map_err(|e| NodeError::new(NodeError::OP, e.to_string()))?;
        let (rows, columns) = table_shape(v);
        let meta = BlobMeta { media: JSON_MEDIA.into(), size: bytes.len() as u64, form: Form::Table { rows, columns }, aliases: Vec::new() };
        {
            let handle = Handle::raw(self.algo, &bytes);
            self.blob(handle.clone(), meta, &bytes, |st| st.publish_raw_as(&handle, &bytes))
        }
    }

    /// A large JSON value as a blob.
    pub fn json_ref(&self, v: &Value) -> Result<DataRef, NodeError> {
        let bytes = serde_json::to_vec(v).map_err(|e| NodeError::new(NodeError::OP, e.to_string()))?;
        let meta = BlobMeta { media: JSON_MEDIA.into(), size: bytes.len() as u64, form: Form::Json, aliases: Vec::new() };
        {
            let handle = Handle::raw(self.algo, &bytes);
            self.blob(handle.clone(), meta, &bytes, |st| st.publish_raw_as(&handle, &bytes))
        }
    }

    /// The value a reply carries for a target's output.
    pub fn port_value(&self, media: Media, d: &Data) -> PortValue {
        let as_blob = |r: Result<DataRef, NodeError>, ty: &str| match r {
            Ok(data) => PortValue::Blob { data },
            Err(e) => PortValue::Opaque { ty: ty.into(), summary: e.message, held: None },
        };
        match d {
            Data::Array(a) => as_blob(self.array_ref(a), "ARRAY"),
            Data::Audio(a) => as_blob(self.audio_ref(a), "AUDIO"),
            Data::Blob(b) => as_blob(self.blob_ref(b), "BLOB"),
            Data::Json(Value::String(s)) if s.len() > self.max_inline => as_blob(self.text_ref(s), "TEXT"),
            Data::Json(v) if media == Media::Table && json_len(v) > self.max_inline => as_blob(self.table_ref(v), "TABLE"),
            Data::Json(v) if media == Media::Tree && json_len(v) > self.max_inline => as_blob(self.json_ref(v), "JSON"),
            Data::Json(v) => PortValue::Json { value: v.clone() },
            Data::List(l) => PortValue::List { items: l.items.iter().map(|i| self.port_value(media, i)).collect() },
        }
    }
}

/// The most bytes a peaks rendition carries inline (spec 10.3).
pub const PEAKS_BYTES: usize = 4096;

/// A waveform overview of `a` (spec 10.3 `peaks`): (1, channels, bins, 2) `<f4` min/max, inline,
/// with as many bins as fit [`PEAKS_BYTES`] (and no more than there are frames).
pub fn peaks(a: &Audio) -> Option<Rendition> {
    let ch = usize::from(a.channels);
    let frames = usize::try_from(a.frames()).ok()?;
    if ch == 0 || frames == 0 {
        return None;
    }
    let bins = (PEAKS_BYTES / (ch * 8)).min(frames).max(1);
    let mut out: Vec<f32> = Vec::with_capacity(ch * bins * 2);
    for c in 0..ch {
        for b in 0..bins {
            let (lo, hi) = (b * frames / bins, ((b + 1) * frames / bins).max(b * frames / bins + 1));
            let (mut mn, mut mx) = (f32::INFINITY, f32::NEG_INFINITY);
            for f in lo..hi.min(frames) {
                let s = a.samples.get(f * ch + c).copied().unwrap_or(0.0);
                mn = mn.min(s);
                mx = mx.max(s);
            }
            out.extend([mn, mx]);
        }
    }
    let shape = [1, ch as u64, bins as u64, 2];
    let bytes: Vec<u8> = out.iter().flat_map(|v| v.to_le_bytes()).collect();
    let handle = Handle::md5_nd1("<f4", &shape, &bytes).ok()?;
    let meta = BlobMeta {
        media: NPY_MEDIA.into(),
        size: bytes.len() as u64,
        form: Form::Array { dtype: "<f4".into(), shape: shape.to_vec(), space: None, names: Vec::new(), table: false },
        aliases: Vec::new(),
    };
    Some(Rendition::Peaks { data: inline_ref(handle, meta, &bytes), bins: bins as u32, channels: a.channels })
}

/// A table's row count and columns, from `{"columns": [{name, type, values}]}`.
pub fn table_shape(v: &Value) -> (u64, Vec<Column>) {
    let cols = v.get("columns").and_then(Value::as_array).cloned().unwrap_or_default();
    let rows = cols.first().and_then(|c| c.get("values")).and_then(Value::as_array).map_or(0, |r| r.len() as u64);
    let columns = cols
        .iter()
        .map(|c| Column {
            name: c.get("name").and_then(Value::as_str).unwrap_or_default().into(),
            ty: c.get("type").and_then(Value::as_str).unwrap_or_default().into(),
        })
        .collect();
    (rows, columns)
}

pub fn json_len(v: &Value) -> usize {
    serde_json::to_vec(v).map_or(0, |b| b.len())
}
