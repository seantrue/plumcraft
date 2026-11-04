//! Video into and out of arrays (`docs/research/20-convert.md`, 6.4), with ffmpeg, as commonkit's
//! `tests/test_ffmpeg.py` does: a clip is its frames as one (T, H, W, C) array, `rgb24` as `|u1`
//! with 3 channels or `grayf32le` as `<f4` with 1, and the frame rate beside it (as Audio to
//! Array gives the sample rate). One frame, `[t]`, is a picture.
//!
//! - **MP4, MOV and GIF to Array** (FilmCraft's exports) decode every frame once, at the size
//!   it was stored (rotation metadata is not applied). A clip whose frames would pass
//!   [`MAX_FRAMES`] bytes is refused whole, never cut short.
//! - **Array to MP4, MOV and GIF**, their inverses, encode (T, H, W, C) frames (`|u1`, or floats
//!   in 0-1; 1, 3 or 4 channels, the alpha dropped) at a frame rate: H.264 (`yuv420p`, an odd
//!   side padded by a pixel), ProRes 422 HQ, and a GIF with its own palette. None is lossless.
//! - ffmpeg and ffprobe are found on `PATH`, or `$PLUMCRAFT_FFMPEG` and `$PLUMCRAFT_FFPROBE`;
//!   without them a node is a `config` error that says to install ffmpeg. A run is cancelled
//!   between polls of the child, which is then killed.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use plumcraft_engine::{Array, Blob, BlobSource, Cx, Data, Inputs, Node, Outputs};
use plumcraft_proto::Handle;
use plumcraft_proto::catalogue::core_types::ARRAY;
use plumcraft_proto::catalogue::{Port, Socket};
use plumcraft_proto::data::{BlobMeta, Form};
use plumcraft_proto::error::NodeError;
use serde_json::{Value, json};

use super::MAX_FILE;
use crate::op::{Op, config, enum_in, float_in, op_error, out, s};

/// The video types, as the craft packs declare them (bytes).
pub const MP4: &str = "MP4";
pub const MOV: &str = "MOV";
pub const GIF: &str = "GIF";
/// The most bytes of frames a clip decodes to: 512 MiB (about 6 minutes at 160 × 120 RGB, 25 fps;
/// about 3 s at 1920 × 1080).
pub const MAX_FRAMES: u64 = 512 << 20;
/// How often a running ffmpeg is checked for cancel.
const POLL: Duration = Duration::from_millis(25);

pub fn wire_types() -> Vec<&'static str> {
    vec![MP4, MOV, GIF]
}

pub fn nodes() -> Vec<Box<dyn Node>> {
    let pixels = || enum_in("pixels", &["rgb", "gray"], "rgb", "rgb: 8 bits a channel (|u1, 0-255), 3 channels; gray: one float32 channel, 0-1");
    let decode = |id: &'static str, display: &'static str, ty: &'static str, description: &'static str, run| -> Box<dyn Node> {
        Box::new(Op {
            id,
            display,
            group: "Video",
            description,
            inputs: vec![
                Port { name: "video".into(), ty: ty.into(), socket: Some(Socket::Socket), tooltip: Some("The clip".into()), ..Default::default() },
                pixels(),
            ],
            outputs: vec![out("frames", ARRAY, "The frames, (T, H, W, C): [t] is one picture"), out("fps", "FLOAT", "Frames a second")],
            run,
        })
    };
    let encode = |id: &'static str, display: &'static str, ty: &'static str, description: &'static str, run| -> Box<dyn Node> {
        Box::new(Op {
            id,
            display,
            group: "Video",
            description,
            inputs: vec![
                Port {
                    name: "frames".into(),
                    ty: ARRAY.into(),
                    socket: Some(Socket::Socket),
                    tooltip: Some("(T, H, W, C) frames: |u1, or floats in 0-1; 1, 3 or 4 channels".into()),
                    ..Default::default()
                },
                float_in("fps", 25.0, 0.1, 240.0, "Frames a second"),
            ],
            outputs: vec![out("video", ty, "The clip")],
            run,
        })
    };
    vec![
        decode(
            "array.mp4_to_array",
            "MP4 to Array",
            MP4,
            "An MP4 clip (as FilmCraft exports one) as its frames, (T, H, W, 3) |u1, or (T, H, W, 1) float32 gray, and its frame rate, with ffmpeg. A clip over 512 MiB of frames is refused. The inverse of Array to MP4.",
            |i, cx| decode_video(i, cx, MP4),
        ),
        encode("array.array_to_mp4", "Array to MP4", MP4, "(T, H, W, C) frames as an H.264 MP4 at fps, with ffmpeg. The inverse of MP4 to Array.", |i, cx| {
            encode_video(i, cx, MP4)
        }),
        decode(
            "array.mov_to_array",
            "MOV to Array",
            MOV,
            "A MOV clip (as FilmCraft's ProRes export) as its frames, (T, H, W, 3) |u1, or (T, H, W, 1) float32 gray, and its frame rate, with ffmpeg. A clip over 512 MiB of frames is refused. The inverse of Array to MOV.",
            |i, cx| decode_video(i, cx, MOV),
        ),
        encode(
            "array.array_to_mov",
            "Array to MOV",
            MOV,
            "(T, H, W, C) frames as a ProRes 422 HQ MOV at fps, with ffmpeg. The inverse of MOV to Array.",
            |i, cx| encode_video(i, cx, MOV),
        ),
        decode(
            "array.gif_to_array",
            "GIF to Array",
            GIF,
            "An animated GIF (as FilmCraft exports one) as its frames, (T, H, W, 3) |u1, or (T, H, W, 1) float32 gray, and its frame rate, with ffmpeg. The inverse of Array to GIF.",
            |i, cx| decode_video(i, cx, GIF),
        ),
        encode(
            "array.array_to_gif",
            "Array to GIF",
            GIF,
            "(T, H, W, C) frames as an animated GIF at fps, its palette made from the frames, with ffmpeg. The inverse of GIF to Array.",
            |i, cx| encode_video(i, cx, GIF),
        ),
    ]
}

fn media(ty: &str) -> &'static str {
    match ty {
        MP4 => "video/mp4",
        MOV => "video/quicktime",
        _ => "image/gif",
    }
}

fn tool(name: &str, var: &str) -> PathBuf {
    // ffmpeg is usually in /opt/homebrew/bin, which an editor opened from the Finder doesn't have
    std::env::var_os(var).map_or_else(|| plumcraft_proto::programs::find_program(name).unwrap_or_else(|| PathBuf::from(name)), PathBuf::from)
}

/// A folder for one conversion's files, removed when dropped.
struct Work(PathBuf);

impl Work {
    fn new() -> Result<Work, NodeError> {
        static N: AtomicU64 = AtomicU64::new(0);
        let d = std::env::temp_dir().join(format!("plumcraft-video-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir_all(&d).map_err(|e| op_error(format!("a work folder: {e}")))?;
        Ok(Work(d))
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Work {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Run a tool to the end, killing it if the run is cancelled; its stdout on success.
fn run(cmd: &mut Command, cx: &Cx, what: &str) -> Result<Vec<u8>, NodeError> {
    let name = cmd.get_program().to_string_lossy().into_owned();
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            config(format!("{what}: {name} isn't installed (brew install ffmpeg, or set $PLUMCRAFT_FFMPEG)"))
        } else {
            op_error(format!("{what}: starting {name}: {e}"))
        }
    })?;
    // read the pipes on threads, so a chatty tool never blocks on a full one
    let pipe = |r: Option<Box<dyn std::io::Read + Send>>| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(mut r) = r {
                let _ = r.read_to_end(&mut buf);
            }
            buf
        })
    };
    let stdout = pipe(child.stdout.take().map(|p| Box::new(p) as Box<dyn std::io::Read + Send>));
    let stderr = pipe(child.stderr.take().map(|p| Box::new(p) as Box<dyn std::io::Read + Send>));
    let status = loop {
        if cx.cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(NodeError::new(NodeError::CANCELLED, format!("{what}: cancelled")));
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(POLL),
            Err(e) => return Err(op_error(format!("{what}: {name}: {e}"))),
        }
    };
    let out = stdout.join().unwrap_or_default();
    let err = stderr.join().unwrap_or_default();
    if !status.success() {
        let msg = String::from_utf8_lossy(&err);
        let last = msg.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("no message");
        return Err(config(format!("{what}: {name} failed: {last}")));
    }
    Ok(out)
}

/// The clip's file: its own when the blob is one, else written into `work`.
fn clip_path(blob: &Blob, work: &Work, ty: &str) -> Result<PathBuf, NodeError> {
    if let BlobSource::File(p) = &blob.source {
        return Ok(p.clone());
    }
    let path = work.join(&format!("in.{}", ty.to_ascii_lowercase()));
    std::fs::write(&path, blob.read(MAX_FILE)?).map_err(|e| op_error(format!("video: writing it for ffmpeg: {e}")))?;
    Ok(path)
}

/// `30000/1001` as a number; `0/0` (unknown) as `None`.
fn rate(s: &str) -> Option<f64> {
    let (n, d) = s.split_once('/').unwrap_or((s, "1"));
    let (n, d): (f64, f64) = (n.trim().parse().ok()?, d.trim().parse().ok()?);
    (n > 0.0 && d > 0.0).then(|| n / d)
}

/// The first video stream's width, height and frame rate.
fn probe(path: &Path, cx: &Cx) -> Result<(u64, u64, f64), NodeError> {
    let mut cmd = Command::new(tool("ffprobe", "PLUMCRAFT_FFPROBE"));
    cmd.args(["-v", "error", "-select_streams", "v:0", "-show_entries", "stream=width,height,avg_frame_rate,r_frame_rate", "-of", "json"]).arg(path);
    let out = run(&mut cmd, cx, "video")?;
    let v: Value = serde_json::from_slice(&out).map_err(|e| op_error(format!("video: ffprobe's answer: {e}")))?;
    let s = v.get("streams").and_then(|s| s.get(0)).ok_or_else(|| config("video: it has no video stream"))?;
    let dim = |k: &str| s.get(k).and_then(Value::as_u64).filter(|n| *n > 0);
    let (w, h) = dim("width").zip(dim("height")).ok_or_else(|| config("video: its frame size is unknown"))?;
    let fps = ["avg_frame_rate", "r_frame_rate"].iter().find_map(|k| s.get(*k).and_then(Value::as_str).and_then(rate)).unwrap_or(25.0);
    Ok((w, h, fps))
}

fn decode_video(i: &Inputs, cx: &mut Cx, ty: &str) -> Result<Outputs, NodeError> {
    let blob = i.get("video").and_then(Data::blob).ok_or_else(|| config(format!("video: wire a {ty} clip")))?;
    let gray = s(i, "pixels", "rgb") == "gray";
    let work = Work::new()?;
    let input = clip_path(blob, &work, ty)?;
    cx.progress(0.05, Some("probing"));
    let (w, h, fps) = probe(&input, cx)?;
    let (pix_fmt, dtype, channels, size) = if gray { ("grayf32le", "<f4", 1, 4) } else { ("rgb24", "|u1", 3, 1) };
    let frame = w * h * channels * size;
    let raw = work.join("frames.raw");
    cx.progress(0.1, Some("decoding"));
    let mut cmd = Command::new(tool("ffmpeg", "PLUMCRAFT_FFMPEG"));
    cmd.args(["-nostdin", "-v", "error", "-y", "-noautorotate", "-i"])
        .arg(&input)
        .args(["-map", "0:v:0", "-an", "-fps_mode", "passthrough", "-f", "rawvideo", "-pix_fmt", pix_fmt, "-fs"])
        .arg((MAX_FRAMES + 1).to_string())
        .arg(&raw);
    run(&mut cmd, cx, "video")?;
    let len = std::fs::metadata(&raw).map(|m| m.len()).unwrap_or(0);
    if len > MAX_FRAMES {
        return Err(config(format!("video: its frames pass {} MiB; trim it, or make it smaller, first", MAX_FRAMES >> 20)));
    }
    if len == 0 || len % frame != 0 {
        return Err(op_error(format!("video: ffmpeg gave {len} bytes, not whole {w} × {h} frames")));
    }
    let bytes = std::fs::read(&raw).map_err(|e| op_error(format!("video: reading the frames: {e}")))?;
    let frames = Array::new(dtype, vec![len / frame, h, w, channels], bytes);
    Ok(Outputs::from([("frames".to_owned(), Data::Array(frames)), ("fps".to_owned(), Data::Json(json!(fps)))]))
}

/// (T, H, W, C) frames as 8-bit samples of 1 or 3 channels.
fn frames_u8(a: &Array) -> Result<(u64, u64, u64, u64, Vec<u8>), NodeError> {
    let (t, h, w, c) = match a.shape.as_slice() {
        [t, h, w, c] => (*t, *h, *w, *c),
        s => return Err(config(format!("frames: (T, H, W, C) frames, not {}", s.iter().map(u64::to_string).collect::<Vec<_>>().join("×")))),
    };
    if t == 0 || h == 0 || w == 0 || !matches!(c, 1 | 3 | 4) {
        return Err(config(format!("frames: {t}×{h}×{w}×{c}: at least one frame, and 1, 3 or 4 channels")));
    }
    let n = usize::try_from(t * h * w * c).map_err(|_| config("frames: too many"))?;
    let to8 = |v: f64| if v.is_nan() { 0 } else { (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8 };
    let samples: Vec<u8> = match a.dtype.as_str() {
        "|u1" | "<u1" => a.bytes.to_vec(),
        "<f4" => a.bytes.as_chunks::<4>().0.iter().map(|b| to8(f64::from(f32::from_le_bytes(*b)))).collect(),
        "<f8" => a.bytes.as_chunks::<8>().0.iter().map(|b| to8(f64::from_le_bytes(*b))).collect(),
        d => return Err(config(format!("frames: {d}: |u1, or floats in 0-1"))),
    };
    if samples.len() != n {
        return Err(config("frames: its bytes don't fill its shape"));
    }
    if c == 4 {
        let rgb = samples.as_chunks::<4>().0.iter().flat_map(|[r, g, b, _]| [*r, *g, *b]).collect();
        return Ok((t, h, w, 3, rgb));
    }
    Ok((t, h, w, c, samples))
}

fn encode_video(i: &Inputs, cx: &mut Cx, ty: &str) -> Result<Outputs, NodeError> {
    let a = match i.get("frames") {
        Some(Data::Array(a)) => a,
        _ => return Err(config("frames: wire (T, H, W, C) frames")),
    };
    let (_, h, w, c, samples) = frames_u8(a)?;
    let fps = crate::op::f(i, "fps", 25.0).clamp(0.1, 240.0);
    let work = Work::new()?;
    let raw = work.join("frames.raw");
    std::fs::write(&raw, &samples).map_err(|e| op_error(format!("video: writing the frames for ffmpeg: {e}")))?;
    drop(samples);
    let output = work.join(&format!("out.{}", ty.to_ascii_lowercase()));
    cx.progress(0.1, Some("encoding"));
    let mut cmd = Command::new(tool("ffmpeg", "PLUMCRAFT_FFMPEG"));
    cmd.args(["-nostdin", "-v", "error", "-y", "-f", "rawvideo", "-pix_fmt", if c == 1 { "gray" } else { "rgb24" }, "-s"])
        .arg(format!("{w}x{h}"))
        .arg("-r")
        .arg(format!("{fps}"))
        .arg("-i")
        .arg(&raw);
    match ty {
        MP4 => cmd.args(["-vf", "pad=ceil(iw/2)*2:ceil(ih/2)*2", "-c:v", "libx264", "-crf", "18", "-pix_fmt", "yuv420p", "-movflags", "+faststart"]),
        MOV => cmd.args(["-c:v", "prores_ks", "-profile:v", "3", "-pix_fmt", "yuv422p10le"]),
        _ => cmd.args(["-vf", "split[a][b];[a]palettegen[p];[b][p]paletteuse", "-loop", "0"]),
    };
    cmd.arg(&output);
    run(&mut cmd, cx, "video")?;
    let bytes = std::fs::read(&output).map_err(|e| op_error(format!("video: reading ffmpeg's {ty}: {e}")))?;
    let blob = Blob {
        handle: Handle::md5_raw(&bytes),
        meta: BlobMeta { media: media(ty).into(), size: bytes.len() as u64, form: Form::default(), aliases: Vec::new() },
        source: BlobSource::Bytes(Arc::from(bytes)),
        publish: true,
        thumb: None,
    };
    Ok(Outputs::from([("video".to_owned(), Data::Blob(Box::new(blob)))]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates_read_as_ffprobe_writes_them() {
        assert_eq!(rate("25/1"), Some(25.0));
        assert!((rate("30000/1001").unwrap() - 29.97).abs() < 0.01);
        assert_eq!(rate("0/0"), None);
        assert_eq!(rate("12"), Some(12.0));
        assert_eq!(rate("x/1"), None);
    }

    #[test]
    fn frames_are_four_dimensional_and_eight_bit() {
        let rgba = Array::new("|u1", vec![1, 1, 2, 4], vec![1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(frames_u8(&rgba).unwrap(), (1, 1, 2, 3, vec![1, 2, 3, 5, 6, 7]));
        let floats = Array::from_f32(vec![1, 1, 1, 3], &[0.0, 0.5, 2.0]);
        assert_eq!(frames_u8(&floats).unwrap().4, vec![0, 128, 255]);
        assert!(frames_u8(&Array::from_f32(vec![2, 2, 3], &[0.0; 12])).is_err(), "a picture isn't a clip");
        assert!(frames_u8(&Array::new("|u1", vec![1, 1, 1, 2], vec![0, 0])).is_err(), "2 channels");
    }
}
