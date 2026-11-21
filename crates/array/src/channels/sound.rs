//! Arrays as sound and back (N4 slice 5): Array to Audio reads a series (frames, or frames ×
//! channels) as samples at a rate; Audio to Array gives a sound's samples as a (frames, channels)
//! float32 array. Each is the other's inverse, so a sound crosses into the array nodes and back
//! unchanged (the AUDIO core type, `docs/interface.md` section 3).

use std::sync::Arc;

use plumcraft_engine::{Array, Audio, Cx, Data, Inputs, Node, Outputs};
use plumcraft_proto::catalogue::{Port, Socket, core_types};
use plumcraft_proto::error::NodeError;

use crate::op::{Op, array_in, array_out, b, bool_in, config, int_in, n};

/// The most columns of a 2-D array read as channels (more is a picture, heard row by row).
pub const MAX_CHANNELS: u64 = 8;
/// The most samples Array to Audio makes (about 25 minutes of stereo at 48 kHz).
pub const MAX_SAMPLES: u64 = 1 << 27;

pub fn nodes() -> Vec<Box<dyn Node>> {
    vec![
        Box::new(Op {
            id: "array.to_audio",
            display: "Array to Audio",
            group: "Sound",
            description: "An array as a sound: a 1-D series is one channel; a 2-D array is frames by channels when it has at most 8 columns, and is read row by row as one channel otherwise (a picture heard as a signal). Normalize centres the samples and scales the loudest to full scale; without it they are kept as they are, clipped to -1 to 1 when played.",
            inputs: vec![
                array_in("array", "The samples: (frames,) or (frames, channels)"),
                int_in("sample_rate", 48_000, 1.0, 384_000.0, "Samples a second"),
                bool_in("normalize", true, "Centre the samples and scale the loudest to full scale"),
            ],
            outputs: vec![Port { name: "audio".into(), ty: core_types::AUDIO.into(), tooltip: Some("The sound".into()), ..Default::default() }],
            run: to_audio,
        }),
        Box::new(Op {
            id: "array.from_audio",
            display: "Audio to Array",
            group: "Sound",
            description: "A sound's samples as a float32 array, frames by channels: Array to Audio's inverse.",
            inputs: vec![Port {
                name: "audio".into(),
                ty: core_types::AUDIO.into(),
                socket: Some(Socket::Socket),
                tooltip: Some("A sound".into()),
                ..Default::default()
            }],
            outputs: vec![array_out("array", "(frames, channels) float32")],
            run: from_audio,
        }),
    ]
}

fn to_audio(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    audio_of(i)
}

fn audio_of(i: &Inputs) -> Result<Outputs, NodeError> {
    let a = match i.get("array") {
        Some(Data::Array(a)) => a,
        _ => return Err(config("array: connect an array")),
    };
    let rate = u32::try_from(n(i, "sample_rate", 48_000).clamp(1, 384_000)).unwrap_or(48_000);
    let total: u64 = a.shape.iter().try_fold(1u64, |x, d| x.checked_mul(*d)).ok_or_else(|| config("array: too large"))?;
    if total == 0 {
        return Err(config("array: it has no samples"));
    }
    if total > MAX_SAMPLES {
        return Err(config(format!("array: more than {MAX_SAMPLES} samples")));
    }
    let channels = match a.shape.as_slice() {
        [_, c] if (1..=MAX_CHANNELS).contains(c) => *c,
        _ => 1,
    };
    let mut v = plumcraft_engine::arrays::values(a).ok_or_else(|| config(format!("array: can't read {} as numbers", a.dtype)))?;
    for x in &mut v {
        if !x.is_finite() {
            *x = 0.0;
        }
    }
    if b(i, "normalize", true) {
        let ch = channels as usize;
        for c in 0..ch {
            let frames = v.len() / ch;
            let mean = (0..frames).map(|f| f64::from(v[f * ch + c])).sum::<f64>() / frames.max(1) as f64;
            for f in 0..frames {
                v[f * ch + c] -= mean as f32;
            }
        }
        let peak = v.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        if peak > 0.0 {
            for x in &mut v {
                *x /= peak;
            }
        }
    }
    let audio = Audio { sample_rate: rate, channels: u16::try_from(channels).unwrap_or(1), samples: Arc::from(v) };
    Ok(Outputs::from([("audio".to_owned(), Data::Audio(audio))]))
}

fn from_audio(i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
    samples_of(i)
}

fn samples_of(i: &Inputs) -> Result<Outputs, NodeError> {
    let a = match i.get("audio") {
        Some(Data::Audio(a)) => a,
        _ => return Err(config("audio: connect a sound")),
    };
    let ch = u64::from(a.channels.max(1));
    let bytes: Vec<u8> = a.samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    let frames = a.samples.len() as u64 / ch;
    Ok(Outputs::from([("array".to_owned(), Data::Array(Array::new("<f4", vec![frames, ch], bytes)))]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sound_crosses_to_an_array_and_back_unchanged() {
        let s = Audio { sample_rate: 8000, channels: 2, samples: Arc::from(vec![0.5f32, -0.5, 0.25, 0.0]) };
        let mut i = Inputs::new();
        i.insert("audio".into(), Data::Audio(s.clone()));
        let out = samples_of(&i).unwrap();
        let Some(Data::Array(a)) = out.get("array") else { panic!("an array") };
        assert_eq!(a.shape, vec![2, 2]);
        let mut j = Inputs::new();
        j.insert("array".into(), Data::Array(a.clone()));
        j.insert("sample_rate".into(), Data::Json(serde_json::json!(8000)));
        j.insert("normalize".into(), Data::Json(serde_json::json!(false)));
        let back = audio_of(&j).unwrap();
        let Some(Data::Audio(b)) = back.get("audio") else { panic!("a sound") };
        assert_eq!((b.sample_rate, b.channels, &*b.samples), (8000, 2, &*s.samples));
    }

    #[test]
    fn junk_is_refused() {
        let mut i = Inputs::new();
        assert!(audio_of(&i).is_err());
        i.insert("array".into(), Data::Array(Array::new("<f4", vec![3], vec![0u8; 5])));
        assert!(audio_of(&i).is_err(), "torn bytes");
        i.insert("array".into(), Data::Array(Array::new("<f4", vec![0], Vec::<u8>::new())));
        assert!(audio_of(&i).is_err(), "empty");
    }
}
