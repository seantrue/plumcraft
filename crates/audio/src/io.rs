//! Files: `audio.load` (any format SoundCraft's audio-io reads) and `audio.save` (WAV); and WAV
//! values, `audio.from_wav` and its inverse `audio.to_wav`, so an app's bounce (SoundCraft's
//! `WAV`) can cross into this engine's nodes; and arrays, `audio.to_array` and its inverse
//! `audio.from_array`, so the samples reach the array engine's nodes. All four are converters
//! (`docs/research/20-convert.md`).

use std::sync::Arc;

use plumcraft_engine::{Array, Blob, BlobSource, Cx, Data, Inputs, Node, Outputs};
use plumcraft_proto::Handle;
use plumcraft_proto::catalogue::{MemoScope, MemoSpec, PathFilter, PathHint, Port};
use plumcraft_proto::data::{BlobMeta, Form};
use plumcraft_proto::error::NodeError;
use serde_json::json;
use soundcraft_audio_io::{AudioBuffer, BitDepth, EncodeOptions, FileFormat};

use crate::util::{self, config, op_error};
use crate::{ARRAY, AUDIO, INT, MAX_CHANNELS, MAX_FILE, MAX_RATE, MIN_RATE, STRING, WAV, audio_in, boolean, choice, def, int, one, op, output};

/// Extensions offered in the open dialog (audio-io reads more by content).
const READS: &[&str] = &["wav", "wave", "bwf", "rf64", "aif", "aiff", "aifc", "flac", "mp3", "ogg", "oga", "m4a", "mp4", "aac", "caf", "mka", "mkv"];
const DEPTHS: &[&str] = &["float32", "int24", "int16", "int32"];

pub(crate) fn nodes() -> Vec<Box<dyn Node>> {
    let path = Port {
        name: "path".into(),
        ty: STRING.into(),
        label: Some("File".into()),
        default: json!(""),
        path: Some(PathHint {
            mode: "open".into(),
            filters: vec![PathFilter { name: "Audio".into(), extensions: READS.iter().map(|s| (*s).to_owned()).collect() }],
        }),
        ..Default::default()
    };
    let mut save = def(
        "audio.save",
        "Save WAV",
        &["Output"],
        "Writes the audio as a WAV file on the engine's machine and returns its path. Refuses to replace a file unless `overwrite` is on.",
        vec![
            audio_in("audio"),
            // A plain STRING, not a PathHint: the executor and the HTTP host treat every PathHint
            // port as a source to read and upload, which a file being written is not.
            Port {
                name: "path".into(),
                ty: STRING.into(),
                label: Some("File (.wav)".into()),
                default: json!(""),
                // A save path: never read, keyed by content or uploaded (PathHint::is_source).
                path: Some(PathHint { mode: "save".into(), ..Default::default() }),
                ..Default::default()
            },
            choice("bits", "Sample format", DEPTHS, "float32"),
            boolean("overwrite", "Overwrite", false),
        ],
        vec![output("path", STRING)],
    );
    save.flags.output_node = true;
    // Writing a file is a side effect: never served from the memo.
    save.memo = MemoSpec { scope: MemoScope::None, ..Default::default() };
    vec![
        op(
            def(
                "audio.load",
                "Load audio",
                &["Sources"],
                "Decodes an audio file: WAV/BWF/RF64, AIFF, FLAC, MP3, Ogg Vorbis, AAC/ALAC, CAF, Matroska. Keyed by the file's content.",
                vec![path],
                vec![output("audio", AUDIO)],
            ),
            load,
        ),
        op(save, save_wav),
        op(
            converts(
                def(
                    "audio.from_wav",
                    "WAV to Audio",
                    &["Sources"],
                    "Decodes a WAV value, such as SoundCraft's bounce, to audio. The inverse of Audio to WAV.",
                    vec![Port { name: "wav".into(), ty: WAV.into(), socket: Some(plumcraft_proto::catalogue::Socket::Socket), ..Default::default() }],
                    vec![output("audio", AUDIO)],
                ),
                WAV,
                AUDIO,
            ),
            from_wav,
        ),
        op(
            // lossless at its default, float32
            converts(
                def(
                    "audio.to_wav",
                    "Audio to WAV",
                    &["Output"],
                    "Encodes audio as a WAV value. The inverse of WAV to Audio.",
                    vec![audio_in("audio"), choice("bits", "Bits", DEPTHS, "float32")],
                    vec![output("wav", WAV)],
                ),
                AUDIO,
                WAV,
            ),
            to_wav,
        ),
        op(
            // not lossless: the sample rate leaves by its own output; a series, not a picture
            marked(
                def(
                    "audio.to_array",
                    "Audio to Array",
                    &["Output"],
                    "The samples as a (frames, channels) float32 array, its columns named (mono; left, right; ch1, ch2, …), and the sample rate beside it. The inverse of Array to Audio, given that rate.",
                    vec![audio_in("audio")],
                    vec![output("array", ARRAY), output("sample_rate", INT)],
                ),
                plumcraft_proto::convert::Converts::new(AUDIO, ARRAY, false).to_shape(plumcraft_proto::convert::SERIES),
            ),
            to_array,
        ),
        op(
            marked(
                def(
                    "audio.from_array",
                    "Array to Audio",
                    &["Sources"],
                    "An array as audio: (frames, channels), or 1-D for mono, of any number type, each value a sample (-1 to 1 is full scale). The inverse of Audio to Array.",
                    vec![
                        Port { name: "array".into(), ty: ARRAY.into(), socket: Some(plumcraft_proto::catalogue::Socket::Socket), ..Default::default() },
                        Port {
                            socket: Some(plumcraft_proto::catalogue::Socket::Both),
                            tooltip: Some("Wire Audio to Array's sample_rate to give the audio back as it was".into()),
                            ..int("sample_rate", "Sample rate", 48_000, i64::from(MIN_RATE), i64::from(MAX_RATE))
                        },
                    ],
                    vec![output("audio", AUDIO)],
                ),
                plumcraft_proto::convert::Converts::new(ARRAY, AUDIO, true).from_shape(plumcraft_proto::convert::SERIES),
            ),
            from_array,
        ),
    ]
}

/// Names for `n` channels.
fn channel_names(n: usize) -> Vec<String> {
    match n {
        1 => vec!["mono".into()],
        2 => vec!["left".into(), "right".into()],
        _ => (1..=n).map(|c| format!("ch{c}")).collect(),
    }
}

fn to_array(i: &Inputs, _cx: &mut Cx) -> Result<Outputs, NodeError> {
    let a = util::audio(i, "audio")?;
    let ch = usize::from(a.channels);
    let array = Array::from_f32(vec![a.frames(), ch as u64], &a.samples).with_space(None, channel_names(ch));
    Ok(Outputs::from([("array".to_owned(), Data::Array(array)), ("sample_rate".to_owned(), Data::Json(json!(a.sample_rate)))]))
}

fn from_array(i: &Inputs, _cx: &mut Cx) -> Result<Outputs, NodeError> {
    let a = match i.get("array") {
        Some(Data::Array(a)) => a,
        Some(_) => return Err(config("array: expected an array")),
        None => return Err(config("array: wire an array")),
    };
    let rate = util::ranged(i, "sample_rate", f64::from(MIN_RATE), f64::from(MAX_RATE))?.round() as u32;
    let channels = match a.shape.as_slice() {
        [_] => 1,
        [_, c] => usize::try_from(*c).unwrap_or(usize::MAX),
        s => return Err(config(format!("array: {} dimensions; audio is (frames, channels), or 1-D for mono", s.len()))),
    };
    if channels == 0 || channels > MAX_CHANNELS {
        return Err(config(format!("array: {channels} channels (1..={MAX_CHANNELS})")));
    }
    let values = plumcraft_engine::arrays::values(a).ok_or_else(|| config(format!("array: its {} values can't be read as samples", a.dtype)))?;
    if values.iter().any(|v| !v.is_finite()) {
        return Err(config("array: a value isn't a finite number"));
    }
    let ch: Vec<Vec<f32>> = (0..channels).map(|c| values.iter().skip(c).step_by(channels).copied().collect()).collect();
    Ok(one("audio", util::emit(rate, &ch)?))
}

/// Marks `d` a lossless converter (`docs/research/20-convert.md`).
fn converts(d: plumcraft_proto::catalogue::NodeDef, from: &str, to: &str) -> plumcraft_proto::catalogue::NodeDef {
    converts_as(d, from, to, true)
}

/// Marks `d` a converter with `mark` (an array end's shape, say).
fn marked(mut d: plumcraft_proto::catalogue::NodeDef, mark: plumcraft_proto::convert::Converts) -> plumcraft_proto::catalogue::NodeDef {
    d.converts = vec![mark];
    d
}

/// Marks `d` a converter from `from` to `to`.
fn converts_as(mut d: plumcraft_proto::catalogue::NodeDef, from: &str, to: &str, lossless: bool) -> plumcraft_proto::catalogue::NodeDef {
    d.converts = vec![plumcraft_proto::convert::Converts::new(from, to, lossless)];
    d
}

fn depth(bits: usize) -> BitDepth {
    match bits {
        1 => BitDepth::Int24,
        2 => BitDepth::Int16,
        3 => BitDepth::Int32,
        _ => BitDepth::Float32,
    }
}

fn from_wav(i: &Inputs, cx: &mut Cx) -> Result<Outputs, NodeError> {
    let blob = i.get("wav").and_then(Data::blob).ok_or_else(|| config("wav: wire a WAV value"))?;
    decode(blob.read(MAX_FILE)?, Some("wav".to_owned()), "wav", cx)
}

fn to_wav(i: &Inputs, cx: &mut Cx) -> Result<Outputs, NodeError> {
    let a = util::audio(i, "audio")?;
    let bit_depth = depth(util::pick(i, "bits", DEPTHS)?);
    util::tick(cx, 0.0, "encoding")?;
    let buf = AudioBuffer { sample_rate: a.sample_rate, channels: a.to_channels() };
    let bytes = soundcraft_audio_io::encode(&buf, &EncodeOptions { format: FileFormat::Wav, bit_depth, dither: false, bwf: None })
        .map_err(|e| op_error(format!("wav: {e}")))?;
    let blob = Blob {
        handle: Handle::md5_raw(&bytes),
        meta: BlobMeta { media: "audio/wav".into(), size: bytes.len() as u64, form: Form::default(), aliases: Vec::new() },
        source: BlobSource::Bytes(Arc::from(bytes)),
        publish: true,
        thumb: None,
    };
    Ok(one("wav", Data::Blob(Box::new(blob))))
}

fn load(i: &Inputs, cx: &mut Cx) -> Result<Outputs, NodeError> {
    let path = util::path(i, "path")?;
    let meta = std::fs::metadata(&path).map_err(|e| config(format!("{path}: {e}")))?;
    if !meta.is_file() {
        return Err(config(format!("{path}: not a file")));
    }
    if meta.len() > MAX_FILE {
        return Err(config(format!("{path}: larger than {} MiB", MAX_FILE >> 20)));
    }
    util::tick(cx, 0.0, "reading")?;
    let bytes = std::fs::read(&path).map_err(|e| op_error(format!("{path}: {e}")))?;
    let ext = std::path::Path::new(&path).extension().and_then(|e| e.to_str()).map(str::to_owned);
    decode(bytes, ext, &path, cx)
}

/// Decode a file's bytes, named `path` in errors.
fn decode(bytes: Vec<u8>, ext: Option<String>, path: &str, cx: &mut Cx) -> Result<Outputs, NodeError> {
    // Probe first so a file whose header promises more than the limits is refused before decoding.
    let info = soundcraft_audio_io::probe(&bytes, ext.as_deref()).map_err(|e| op_error(format!("{path}: {e}")))?;
    let channels = usize::from(info.channels);
    if channels == 0 || channels > crate::MAX_CHANNELS {
        return Err(op_error(format!("{path}: {channels} channels (1..={})", crate::MAX_CHANNELS)));
    }
    if !(crate::MIN_RATE..=crate::MAX_RATE).contains(&info.sample_rate) {
        return Err(op_error(format!("{path}: sample rate {} Hz is outside {}..={}", info.sample_rate, crate::MIN_RATE, crate::MAX_RATE)));
    }
    if let Ok(frames) = usize::try_from(info.frames)
        && frames > 0
    {
        util::bounds(frames, channels).map_err(|e| op_error(format!("{path}: {e}")))?;
    }
    let decoded = util::off_thread(cx, "decoding", move || soundcraft_audio_io::decode(&bytes, ext.as_deref()))?;
    let (info, buf) = decoded.map_err(|e| op_error(format!("{path}: {e}")))?;
    if !(crate::MIN_RATE..=crate::MAX_RATE).contains(&info.sample_rate) || buf.sample_rate != info.sample_rate {
        return Err(op_error(format!("{path}: sample rate {} Hz is outside {}..={}", buf.sample_rate, crate::MIN_RATE, crate::MAX_RATE)));
    }
    Ok(one("audio", util::emit(buf.sample_rate, &buf.channels).map_err(|e| op_error(format!("{path}: {}", e.message)))?))
}

fn save_wav(i: &Inputs, cx: &mut Cx) -> Result<Outputs, NodeError> {
    let a = util::audio(i, "audio")?;
    let path = util::path(i, "path")?;
    let bits = util::pick(i, "bits", DEPTHS)?;
    let overwrite = util::flag(i, "overwrite")?;
    let p = std::path::Path::new(&path);
    let ext = p.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
    if !matches!(ext.as_deref(), Some("wav" | "wave")) {
        return Err(config(format!("{path}: audio.save writes WAV; name the file .wav")));
    }
    let name = p.file_name().and_then(|n| n.to_str()).ok_or_else(|| config(format!("{path}: no file name")))?;
    let dir = match p.parent() {
        Some(d) if !d.as_os_str().is_empty() => d.to_path_buf(),
        _ => std::path::PathBuf::from("."),
    };
    if !dir.is_dir() {
        return Err(config(format!("{path}: the folder {} doesn't exist", dir.display())));
    }
    match std::fs::symlink_metadata(p) {
        Ok(m) if m.is_dir() => return Err(config(format!("{path} is a folder"))),
        Ok(_) if !overwrite => return Err(config(format!("{path} exists; turn on overwrite to replace it"))),
        _ => {}
    }
    util::tick(cx, 0.0, "encoding")?;
    let bit_depth = depth(bits);
    let buf = AudioBuffer { sample_rate: a.sample_rate, channels: a.to_channels() };
    let bytes = soundcraft_audio_io::encode(&buf, &EncodeOptions { format: FileFormat::Wav, bit_depth, dither: false, bwf: None })
        .map_err(|e| op_error(format!("{path}: {e}")))?;
    util::tick(cx, 0.5, "writing")?;
    // Write beside it, then rename: a reader never sees half a file.
    let tmp = dir.join(format!(".{name}.{}.tmp", std::process::id()));
    std::fs::write(&tmp, &bytes).and_then(|()| std::fs::rename(&tmp, p)).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        op_error(format!("{path}: {e}"))
    })?;
    Ok(one("path", Data::Json(json!(path))))
}
