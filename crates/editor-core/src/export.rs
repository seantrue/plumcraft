//! A pop-out's value taken out of the editor (N4 slice 5): copied, written to a file to share or
//! drag out, or saved through the engine. The commands are the core's (`value.copy`,
//! `value.export`, `value.share`, `value.saveAs`); invoked for a person they raise the existing
//! asks (`clipboard.set`, `share`, `file.save` through `run.save`), and run by a script they reply
//! with what they made.
//!
//! What a file holds: the value's own bytes where they are a file already (a picture's PNG, an
//! SVG, a text, JSON), a table as CSV, a sound as a 32-bit float WAV, an array as `.npy`; anything
//! else as its bytes with `.bin`. Values are read through the media service, waiting for it (a
//! person's explicit act, as `doc.open` waits), at most [`MAX_EXPORT_BYTES`].

use std::path::PathBuf;
use std::sync::Arc;

use plumcraft_media::table::Table;
use plumcraft_media::{MediaService, Want};
use plumcraft_proto::catalogue::Media;
use plumcraft_proto::data::Form;
use plumcraft_proto::display::Display;
use plumcraft_session::Session;
use plumcraft_session::run::WatchRole;
use serde_json::{Value, json};

/// The most bytes a value taken out may have.
pub const MAX_EXPORT_BYTES: usize = 1 << 30;
/// The most rows copied as text.
pub const MAX_COPY_ROWS: usize = 100_000;
/// The most characters copied.
pub const MAX_COPY_CHARS: usize = 64 << 20;

fn popout_value(session: &Session, params: &Value) -> Result<(u64, Display), String> {
    let id = params.get("popout").and_then(Value::as_u64).ok_or("popout must be a pop-out's id")?;
    let (_, d) = session.run.entry(WatchRole::Popout(id)).ok_or_else(|| format!("no pop-out {id}"))?;
    let d = d.cloned().ok_or("no value yet: run the graph")?;
    if let Some(e) = &d.error {
        return Err(format!("this value failed: {}", e.message));
    }
    Ok((id, d))
}

fn bytes(d: &Display, media: &MediaService) -> Result<Arc<plumcraft_store::Blob>, String> {
    let full = d.full.as_ref().ok_or("this value has no bytes to take out")?;
    let b = media.blocking(full, Want::Bytes).bytes().ready().ok_or("the value's bytes couldn't be read: run again")?;
    if b.bytes.len() > MAX_EXPORT_BYTES {
        return Err(format!("{} MiB is more than a value taken out may be", b.bytes.len() >> 20));
    }
    Ok(b)
}

fn json_value(d: &Display, media: &MediaService) -> Result<Value, String> {
    if let Some(v) = &d.value {
        return Ok(v.clone());
    }
    let b = bytes(d, media)?;
    serde_json::from_slice(&b.bytes).map_err(|e| format!("not JSON: {e}"))
}

fn csv_field(s: &str, sep: char) -> String {
    if s.contains([sep, '"', '\n', '\r']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_owned() }
}

fn table_text(t: &Table, sep: char, max_rows: usize) -> String {
    let mut out = String::new();
    let line = |cells: Vec<String>| cells.iter().map(|c| csv_field(c, sep)).collect::<Vec<_>>().join(&sep.to_string());
    out.push_str(&line(t.columns.iter().map(|c| c.name.clone()).collect()));
    out.push('\n');
    for r in 0..t.rows.min(max_rows) {
        let cells = t
            .columns
            .iter()
            .map(|c| match c.cells.get(r) {
                Some(plumcraft_media::table::Cell::Num(f)) => crate::viewers::number(*f, false),
                Some(c) => match c.json() {
                    Value::Null => String::new(),
                    Value::String(s) => s,
                    v => v.to_string(),
                },
                None => String::new(),
            })
            .collect();
        out.push_str(&line(cells));
        out.push('\n');
    }
    out
}

fn table_of(d: &Display, media: &MediaService) -> Result<Table, String> {
    match &d.value {
        Some(v) => Table::from_value(v),
        None => Table::from_bytes(&bytes(d, media)?.bytes),
    }
}

/// `value.copy {popout, pointer?}`: the value as text for the clipboard, `{text, kind}`: a scalar's
/// text, a text whole, JSON pretty (or the member at `pointer`), a table as tab-separated rows; a
/// picture or sound has no text (`value.export` then).
pub fn copy_text(session: &Session, media: &MediaService, params: &Value) -> Result<Value, String> {
    crate::control::check_fields(params, &["popout", "pointer"])?;
    let (_, d) = popout_value(session, params)?;
    let text = match d.media {
        Media::Scalar => d.value.as_ref().map(crate::widget_text::value_text).unwrap_or_default(),
        Media::Text => match &d.full {
            Some(_) => String::from_utf8_lossy(&bytes(&d, media)?.bytes).into_owned(),
            None => d.value.as_ref().and_then(Value::as_str).unwrap_or_default().to_owned(),
        },
        Media::Table => table_text(&table_of(&d, media)?, '\t', MAX_COPY_ROWS),
        Media::Vector => String::from_utf8_lossy(&bytes(&d, media)?.bytes).into_owned(),
        _ if d.value.is_some() || d.media == Media::Tree => {
            let v = json_value(&d, media)?;
            let at = params.get("pointer").and_then(Value::as_str).unwrap_or("");
            let member = v.pointer(at).ok_or_else(|| format!("no member {at:?}"))?;
            match member {
                Value::String(s) => s.clone(),
                other => serde_json::to_string_pretty(other).unwrap_or_default(),
            }
        }
        _ => return Err("this value has no text to copy: export it instead".into()),
    };
    if text.len() > MAX_COPY_CHARS {
        return Err(format!("{} MiB of text is more than is copied", text.len() >> 20));
    }
    Ok(json!({"text": text, "kind": "text"}))
}

/// A float WAV of a (frames, channels) `<f4` sound.
fn wav(samples: &[u8], channels: u16, rate: u32) -> Vec<u8> {
    let data = u32::try_from(samples.len()).unwrap_or(u32::MAX);
    let mut w = Vec::with_capacity(samples.len() + 44);
    w.extend_from_slice(b"RIFF");
    w.extend_from_slice(&(36u32.saturating_add(data)).to_le_bytes());
    w.extend_from_slice(b"WAVEfmt ");
    w.extend_from_slice(&16u32.to_le_bytes());
    w.extend_from_slice(&3u16.to_le_bytes()); // IEEE float
    w.extend_from_slice(&channels.to_le_bytes());
    w.extend_from_slice(&rate.to_le_bytes());
    w.extend_from_slice(&(rate.saturating_mul(u32::from(channels)) * 4).to_le_bytes());
    w.extend_from_slice(&(channels * 4).to_le_bytes());
    w.extend_from_slice(&32u16.to_le_bytes());
    w.extend_from_slice(b"data");
    w.extend_from_slice(&data.to_le_bytes());
    w.extend_from_slice(samples);
    w
}

/// An `.npy` (version 1) of an array's bytes.
fn npy(dtype: &str, shape: &[u64], data: &[u8]) -> Vec<u8> {
    let dims: Vec<String> = shape.iter().map(u64::to_string).collect();
    let shape = if dims.len() == 1 { format!("({},)", dims[0]) } else { format!("({})", dims.join(", ")) };
    let mut header = format!("{{'descr': '{dtype}', 'fortran_order': False, 'shape': {shape}, }}");
    while (10 + header.len() + 1) % 64 != 0 {
        header.push(' ');
    }
    header.push('\n');
    let mut out = b"\x93NUMPY\x01\x00".to_vec();
    out.extend_from_slice(&u16::try_from(header.len()).unwrap_or(u16::MAX).to_le_bytes());
    out.extend_from_slice(header.as_bytes());
    out.extend_from_slice(data);
    out
}

/// A file name from the node's title and port.
fn file_stem(session: &Session, d: &Display) -> String {
    let title = plumcraft_session::node_title(session.graph(), session.catalog(), &d.node.as_str().into());
    let raw = format!("{title} {}", d.port);
    let s: String = raw.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' { c } else { '_' }).collect();
    let s = s.trim().chars().take(80).collect::<String>();
    if s.is_empty() { "value".into() } else { s }
}

/// What a value's file holds, and its extension.
pub fn file_bytes(d: &Display, media: &MediaService) -> Result<(Vec<u8>, &'static str), String> {
    let mt = d.full.as_ref().map(|f| f.meta.media.split(';').next().unwrap_or_default().trim().to_owned()).unwrap_or_default();
    match d.media {
        Media::Table => Ok((table_text(&table_of(d, media)?, ',', usize::MAX).into_bytes(), "csv")),
        Media::Scalar => Ok((d.value.as_ref().map(crate::widget_text::value_text).unwrap_or_default().into_bytes(), "txt")),
        Media::Tree if d.full.is_none() => Ok((serde_json::to_vec_pretty(&json_value(d, media)?).unwrap_or_default(), "json")),
        Media::Text if d.full.is_none() => Ok((d.value.as_ref().and_then(Value::as_str).unwrap_or_default().as_bytes().to_vec(), "txt")),
        Media::Audio => {
            let b = bytes(d, media)?;
            let (rate, ch) = match d.full.as_ref().map(|f| &f.meta.form) {
                Some(Form::Audio { sample_rate, channels, .. }) => (*sample_rate, *channels),
                _ => (0, 0),
            };
            plumcraft_media::audio::Samples::of(&b, rate)?;
            Ok((wav(&b.bytes, ch.max(1), rate), "wav"))
        }
        Media::Ndarray => {
            let b = bytes(d, media)?;
            Ok((npy(&b.dtype, &b.shape, &b.bytes), "npy"))
        }
        _ => {
            let b = bytes(d, media)?;
            let ext = match mt.as_str() {
                "image/png" => "png",
                "image/jpeg" => "jpg",
                "image/tiff" => "tiff",
                "image/webp" => "webp",
                "image/gif" => "gif",
                "image/svg+xml" => "svg",
                "application/pdf" => "pdf",
                "application/json" => "json",
                m if m.starts_with("text/") => "txt",
                _ if d.media == Media::Vector => "svg",
                _ if d.media == Media::Text => "txt",
                _ if d.media == Media::Tree => "json",
                _ => "bin",
            };
            Ok((b.bytes.to_vec(), ext))
        }
    }
}

/// `value.export {popout}`: the value written to a file of its own under the editor's cache, to
/// share or drag out: `{file, name, bytes}`.
pub fn export(session: &Session, media: &MediaService, params: &Value) -> Result<Value, String> {
    crate::control::check_fields(params, &["popout"])?;
    let (id, d) = popout_value(session, params)?;
    let (data, ext) = file_bytes(&d, media)?;
    let dir: PathBuf = session.docs.root.join("export").join(format!("{}-{id}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let name = format!("{}.{ext}", file_stem(session, &d));
    let path = dir.join(&name);
    let tmp = dir.join(format!(".{name}.part"));
    std::fs::write(&tmp, &data).and_then(|()| std::fs::rename(&tmp, &path)).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(json!({"file": path, "name": name, "bytes": data.len()}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wav_and_an_npy_have_their_headers() {
        let w = wav(&[0u8; 16], 2, 48_000);
        assert_eq!(&w[..4], b"RIFF");
        assert_eq!(w.len(), 44 + 16);
        let n = npy("<f4", &[2, 3], &[0u8; 24]);
        assert_eq!(&n[..6], b"\x93NUMPY");
        assert_eq!((n.len() - 24) % 64, 0);
    }

    #[test]
    fn csv_quotes_what_needs_it() {
        let t = Table::from_value(&json!({"columns": [{"name": "a,b", "values": [1.5, "x\"y"]}]})).unwrap();
        assert_eq!(table_text(&t, ',', 10), "\"a,b\"\n1.5\n\"x\"\"y\"\n");
    }
}
