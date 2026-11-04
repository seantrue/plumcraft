//! Each audio node on short signals, in-process: shape, rate, and the effect it should have.

mod common;

use common::*;
use plumcraft_proto::event::NodeState;
use plumcraft_proto::run::RunReply;
use serde_json::{Value, json};
use soundcraft_audio_io::{AudioBuffer, BitDepth, EncodeOptions, FileFormat};

fn tone(seconds: f64, rate: u32, channels: u32, freq: f64, level: f64) -> (&'static str, &'static str, Value) {
    ("t", "audio.tone", json!({"seconds": seconds, "sample_rate": rate, "channels": channels, "frequency": freq, "level_db": level}))
}

/// Run a chain and return its reply, targeting every node.
fn run_chain(h: &Conn, steps: &[(&str, &str, Value)]) -> RunReply {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let id = format!("n{}", N.fetch_add(1, std::sync::atomic::Ordering::SeqCst));
    let targets: Vec<&str> = steps.iter().map(|s| s.0).collect();
    go(h, &id, chain(steps), &targets)
}

fn ok(reply: &RunReply) {
    assert!(reply.ok, "{:?}", reply.nodes.iter().map(|(k, n)| (k, n.state, n.error.clone())).collect::<Vec<_>>());
}

fn error_kind(reply: &RunReply, node: &str) -> String {
    let n = &reply.nodes[node];
    assert_eq!(n.state, NodeState::Error, "{node}: {:?}", n);
    n.error.as_ref().unwrap().kind.clone()
}

fn db(g: f32) -> f64 {
    20.0 * f64::from(g).log10()
}

/// Write a WAV (float32) from channels.
fn wav(path: &std::path::Path, rate: u32, channels: Vec<Vec<f32>>) {
    let bytes = soundcraft_audio_io::encode(
        &AudioBuffer { sample_rate: rate, channels },
        &EncodeOptions { format: FileFormat::Wav, bit_depth: BitDepth::Float32, dither: false, bwf: None },
    )
    .unwrap();
    std::fs::write(path, bytes).unwrap();
}

/// Silence 0.5 s, a 1 kHz tone 0.5 s, silence 0.5 s, the tone 0.5 s, at 48 kHz mono.
fn tone_silence_tone() -> std::path::PathBuf {
    let rate = 48_000usize;
    let mut x = vec![0.0f32; rate * 2];
    for (k, v) in x.iter_mut().enumerate() {
        let in_tone = (rate / 2..rate).contains(&k) || (rate * 3 / 2..rate * 2).contains(&k);
        if in_tone {
            *v = 0.5 * (k as f32 * 1000.0 * std::f32::consts::TAU / rate as f32).sin();
        }
    }
    let p = scratch("tst").join("tone-silence-tone.wav");
    wav(&p, rate as u32, vec![x]);
    p
}

#[test]
fn the_catalogue_is_the_audio_family() {
    let h = inproc();
    let schema = call(&h, "schema", json!({}));
    let nodes = schema["nodes"].as_array().unwrap();
    let ids: Vec<&str> = nodes.iter().map(|n| n["id"].as_str().unwrap()).collect();
    for want in [
        "audio.load",
        "audio.save",
        "audio.tone",
        "audio.noise",
        "audio.gain",
        "audio.normalize",
        "audio.fade",
        "audio.reverse",
        "audio.invert",
        "audio.remove_dc",
        "audio.trim",
        "audio.mix",
        "audio.resample",
        "audio.time_stretch",
        "audio.pitch_shift",
        "audio.biquad",
        "audio.peak",
        "audio.loudness",
        "audio.spectrum",
        "audio.spectrogram",
        "audio.from_wav",
        "audio.to_wav",
        "audio.to_array",
        "audio.from_flac",
        "audio.to_flac",
        "audio.from_aiff",
        "audio.to_aiff",
        "audio.from_array",
        "audio.transients",
        "audio.silence",
        "audio.fx.compressor",
        "audio.fx.plate_reverb",
    ] {
        assert!(ids.contains(&want), "{want} missing");
    }
    // its own nodes; the SDK's Vary nodes are "Building blocks"
    assert!(nodes.iter().filter(|n| n["category"][0] != "Building blocks").all(|n| n["family"] == "audio" && n["category"][0] == "Audio"));
    let (fx, skipped) = plumcraft_audio::effects();
    assert_eq!(ids.iter().filter(|i| i.starts_with("audio.fx.")).count(), fx.len());
    for gone in ["signal_generator", "subtractive_synth", "drum_synth", "sampler", "time_shift"] {
        assert!(skipped.iter().any(|(id, _)| *id == gone), "{gone} should be left out");
        assert!(!ids.contains(&format!("audio.fx.{gone}").as_str()));
    }
    assert!(fx.len() >= 20, "{fx:?}");
    // Every effect's parameters are widgets beside the one audio socket.
    for n in nodes.iter().filter(|n| n["id"].as_str().unwrap().starts_with("audio.fx.")) {
        let inputs = n["inputs"].as_array().unwrap();
        assert_eq!(inputs.iter().filter(|p| p["name"] == "audio").count(), 1, "{}", n["id"]);
        for p in inputs.iter().filter(|p| p["name"] != "audio") {
            assert!(["FLOAT", "BOOL", "ENUM"].contains(&p["type"].as_str().unwrap()), "{}: {p}", n["id"]);
            assert!(!p["default"].is_null(), "{}: {p}", n["id"]);
        }
    }
    let save = nodes.iter().find(|n| n["id"] == "audio.save").unwrap();
    assert_eq!(save["flags"]["output_node"], true);
    assert_eq!(save["memo"]["scope"], "none");
    let wt: Vec<&str> = schema["wire_types"].as_array().unwrap().iter().map(|w| w["id"].as_str().unwrap()).collect();
    for t in ["AUDIO", "FLOAT", "INT", "BOOL", "STRING", "ENUM", "TABLE", "ARRAY"] {
        assert!(wt.contains(&t), "{t}");
    }
}

#[test]
fn tone_has_its_shape_rate_and_level() {
    let h = inproc();
    let r = run_chain(&h, &[tone(0.25, 32_000, 2, 1000.0, -6.0)]);
    ok(&r);
    let s = sound(&h, &r, "t");
    assert_eq!((s.rate, s.channels.len(), s.frames()), (32_000, 2, 8000));
    assert!((db(s.peak()) + 6.0).abs() < 0.05, "{}", db(s.peak()));
    assert_eq!(s.channels[0], s.channels[1], "the same on every channel");
}

#[test]
fn noise_is_seeded() {
    let h = inproc();
    let n = |seed: u64| {
        let r = run_chain(&h, &[("t", "audio.noise", json!({"seed": seed, "seconds": 0.1, "sample_rate": 8000, "channels": 2, "level_db": -12.0}))]);
        ok(&r);
        sound(&h, &r, "t")
    };
    let (a, b, c) = (n(7), n(7), n(8));
    assert_eq!(a, b, "the same seed, the same noise");
    assert_ne!(a.channels, c.channels);
    assert_ne!(a.channels[0], a.channels[1], "each channel its own stream");
    assert!(db(a.peak()) <= -12.0 + 1e-6 && db(a.peak()) > -13.0);
}

#[test]
fn gain_plus_6_db_doubles_the_peak() {
    let h = inproc();
    let r = run_chain(&h, &[tone(0.1, 8000, 1, 440.0, -12.0), ("g", "audio.gain", json!({"db": 6.0206}))]);
    ok(&r);
    let (a, b) = (sound(&h, &r, "t"), sound(&h, &r, "g"));
    assert!((b.peak() / a.peak() - 2.0).abs() < 1e-4);
}

#[test]
fn normalize_hits_its_target() {
    let h = inproc();
    for target in [-1.0, -12.0, 0.0] {
        let r = run_chain(&h, &[tone(0.1, 8000, 2, 440.0, -30.0), ("n", "audio.normalize", json!({"target_db": target})), ("p", "audio.peak", json!({}))]);
        ok(&r);
        assert!((float_out(&r, "p", "db") - target).abs() < 0.01, "{target}: {}", float_out(&r, "p", "db"));
    }
}

#[test]
fn reverse_invert_and_trim() {
    let h = inproc();
    let r = run_chain(&h, &[tone(0.1, 8000, 1, 300.0, -3.0), ("r", "audio.reverse", json!({}))]);
    ok(&r);
    let (a, b) = (sound(&h, &r, "t"), sound(&h, &r, "r"));
    let mut rev = a.channels[0].clone();
    rev.reverse();
    assert_eq!(b.channels[0], rev);
    let r = run_chain(&h, &[tone(0.1, 8000, 1, 300.0, -3.0), ("i", "audio.invert", json!({}))]);
    let (a, b) = (sound(&h, &r, "t"), sound(&h, &r, "i"));
    assert!(a.channels[0].iter().zip(&b.channels[0]).all(|(x, y)| *x == -*y));
    let r = run_chain(&h, &[tone(1.0, 8000, 2, 300.0, -3.0), ("x", "audio.trim", json!({"start": 0.25, "end": 0.75}))]);
    ok(&r);
    let (a, b) = (sound(&h, &r, "t"), sound(&h, &r, "x"));
    assert_eq!(b.frames(), 4000);
    assert_eq!(b.channels[1], a.channels[1][2000..6000]);
    let r = run_chain(&h, &[tone(1.0, 8000, 1, 300.0, -3.0), ("x", "audio.trim", json!({"start": 0.5, "end": 0.0}))]);
    assert_eq!(sound(&h, &r, "x").frames(), 4000, "end 0 keeps to the end");
    let r = run_chain(&h, &[tone(1.0, 8000, 1, 300.0, -3.0), ("x", "audio.trim", json!({"start": 2.0}))]);
    assert_eq!(error_kind(&r, "x"), "config", "trim past the end leaves nothing");
}

#[test]
fn fade_starts_and_ends_silent() {
    let h = inproc();
    let r = run_chain(&h, &[tone(0.5, 8000, 1, 1000.0, 0.0), ("f", "audio.fade", json!({"fade_in": 0.1, "fade_out": 0.1, "shape": "linear"}))]);
    ok(&r);
    let s = sound(&h, &r, "f");
    let c = &s.channels[0];
    assert_eq!(c[0], 0.0);
    assert!(c[..80].iter().all(|v| v.abs() < 0.11), "the first 10 ms is under 10% of full scale");
    assert!(c[c.len() - 8..].iter().all(|v| v.abs() < 0.01));
    assert!(c[2000..2100].iter().any(|v| v.abs() > 0.9), "the middle is untouched");
}

#[test]
fn remove_dc_takes_out_the_mean() {
    let h = inproc();
    let p = scratch("dc").join("dc.wav");
    wav(&p, 8000, vec![(0..8000).map(|k| 0.25 + 0.5 * (k as f32 * 0.3).sin()).collect()]);
    let r = run_chain(&h, &[("l", "audio.load", json!({"path": p.display().to_string()})), ("d", "audio.remove_dc", json!({}))]);
    ok(&r);
    let s = sound(&h, &r, "d");
    let mean: f64 = s.channels[0].iter().map(|v| f64::from(*v)).sum::<f64>() / s.frames() as f64;
    assert!(mean.abs() < 1e-4, "{mean}");
}

#[test]
fn mix_adds_with_gains_and_checks_rates() {
    let h = inproc();
    let g = |rate_b: u32, ch_b: u32| {
        graph(
            json!({
                "a": {"kind": "audio.tone", "values": {"seconds": 0.1, "sample_rate": 8000, "channels": 2, "frequency": 200.0, "level_db": -6.0}},
                "b": {"kind": "audio.tone", "values": {"seconds": 0.2, "sample_rate": rate_b, "channels": ch_b, "frequency": 200.0, "level_db": -6.0}},
                "m": {"kind": "audio.mix", "values": {"gain_a": 0.0, "gain_b": -144.0}}
            }),
            &[("a", "audio", "m", "a"), ("b", "audio", "m", "b")],
        )
    };
    let r = go(&h, "mix1", g(8000, 1), &["a", "m"]);
    ok(&r);
    let (a, m) = (sound(&h, &r, "a"), sound(&h, &r, "m"));
    assert_eq!((m.channels.len(), m.frames()), (2, 1600), "mono b spreads to both channels; the longer length");
    assert_eq!(m.channels[0][..800], a.channels[0][..], "b at -144 dB is silent");
    assert!(m.channels[0][800..].iter().all(|v| *v == 0.0));
    let r = go(&h, "mix2", g(16_000, 2), &["m"]);
    assert_eq!(error_kind(&r, "m"), "config", "mismatched rates");
    let r = go(&h, "mix3", g(8000, 3), &["m"]);
    assert_eq!(error_kind(&r, "m"), "config", "2 and 3 channels");
}

#[test]
fn resample_stretch_and_pitch_change_lengths_as_they_should() {
    let h = inproc();
    let r = run_chain(&h, &[tone(0.5, 48_000, 2, 440.0, -6.0), ("r", "audio.resample", json!({"sample_rate": 24_000}))]);
    ok(&r);
    let s = sound(&h, &r, "r");
    assert_eq!((s.rate, s.frames()), (24_000, 12_000));
    let r = run_chain(&h, &[tone(0.5, 16_000, 1, 440.0, -6.0), ("s", "audio.time_stretch", json!({"ratio": 2.0}))]);
    ok(&r);
    assert_eq!(sound(&h, &r, "s").frames(), 16_000);
    // +12 semitones: the same length, about twice the zero crossings.
    let r = run_chain(&h, &[tone(0.5, 16_000, 1, 200.0, -6.0), ("p", "audio.pitch_shift", json!({"semitones": 12.0}))]);
    ok(&r);
    let (a, b) = (sound(&h, &r, "t"), sound(&h, &r, "p"));
    assert_eq!(a.frames(), b.frames());
    let crossings = |c: &[f32]| c.windows(2).filter(|w| (w[0] < 0.0) != (w[1] < 0.0)).count() as f64;
    let ratio = crossings(&b.channels[0][800..7200]) / crossings(&a.channels[0][800..7200]);
    assert!((ratio - 2.0).abs() < 0.15, "{ratio}");
}

#[test]
fn biquad_low_pass_cuts_highs_and_passes_lows() {
    let h = inproc();
    let peak_after = |freq: f64| {
        let r = run_chain(
            &h,
            &[
                tone(0.5, 48_000, 1, freq, 0.0),
                ("f", "audio.biquad", json!({"type": "low_pass", "frequency": 500.0})),
                ("x", "audio.trim", json!({"start": 0.1})),
                ("p", "audio.peak", json!({})),
            ],
        );
        ok(&r);
        float_out(&r, "p", "db")
    };
    // 12 dB/octave, four octaves above the corner, once the onset has rung out.
    assert!(peak_after(8000.0) < -40.0, "{}", peak_after(8000.0));
    assert!(peak_after(100.0) > -0.5);
    let r = run_chain(&h, &[tone(0.1, 8000, 1, 100.0, 0.0), ("f", "audio.biquad", json!({"frequency": 5000.0}))]);
    assert_eq!(error_kind(&r, "f"), "config", "above Nyquist");
}

#[test]
fn every_effect_renders_as_soundcraft_does_and_deterministically() {
    let h = inproc();
    let (fx, _) = plumcraft_audio::effects();
    let input = tone(0.05, 8000, 2, 330.0, -6.0);
    for id in fx {
        let kind = format!("audio.fx.{id}");
        let steps = [input.clone(), ("e", kind.as_str(), json!({}))];
        let (r1, r2) = (run_chain(&h, &steps), run_chain(&h, &steps));
        ok(&r1);
        let (a, e) = (sound(&h, &r1, "t"), sound(&h, &r1, "e"));
        assert_eq!(e.rate, 8000, "{id}");
        assert_eq!(e.channels.len(), 2, "{id}");
        assert!(e.frames() >= a.frames(), "{id}: never shorter than the input");
        assert_eq!(blob(&r1, "e", "audio").handle, blob(&r2, "e", "audio").handle, "{id}: deterministic");
        // The node's block render is SoundCraft's apply_plugin_with_tail, sample for sample.
        let mut p = soundcraft_dsp::create(id).unwrap();
        let theirs = soundcraft_dsp::offline::apply_plugin_with_tail(&a.channels, p.as_mut(), 8000.0);
        assert_eq!(e.channels, theirs, "{id}");
        assert_eq!(plumcraft_audio::render_effect(id, &a.channels, 8000.0).unwrap(), theirs, "{id}");
    }
}

#[test]
fn effect_parameters_are_widgets_that_change_the_sound() {
    let h = inproc();
    let quiet = run_chain(&h, &[tone(0.2, 8000, 1, 330.0, -6.0), ("e", "audio.fx.gain", json!({}))]);
    ok(&quiet);
    let schema = call(&h, "schema", json!({}));
    let gain = schema["nodes"].as_array().unwrap().iter().find(|n| n["id"] == "audio.fx.gain").unwrap().clone();
    let port = gain["inputs"].as_array().unwrap().iter().find(|p| p["type"] == "FLOAT").unwrap()["name"].as_str().unwrap().to_owned();
    let loud = run_chain(&h, &[tone(0.2, 8000, 1, 330.0, -6.0), ("e", "audio.fx.gain", json!({ port.clone(): 6.0 }))]);
    ok(&loud);
    assert!(sound(&h, &loud, "e").peak() > sound(&h, &quiet, "e").peak() * 1.5, "{port}");
    let bad = run_chain(&h, &[tone(0.2, 8000, 1, 330.0, -6.0), ("e", "audio.fx.gain", json!({ port: 1e9 }))]);
    assert_eq!(error_kind(&bad, "e"), "config", "out of range is an error, not a clamp");
}

#[test]
fn peak_and_loudness_of_known_tones() {
    let h = inproc();
    // A 1 kHz sine peaking at -20 dBFS reads about -23 LUFS (a full-scale sine is -3.01 LUFS).
    let r = run_chain(&h, &[tone(3.0, 48_000, 1, 1000.0, -20.0), ("p", "audio.peak", json!({}))]);
    ok(&r);
    assert!((float_out(&r, "p", "db") + 20.0).abs() < 0.01);
    let r = run_chain(&h, &[tone(3.0, 48_000, 1, 1000.0, -20.0), ("l", "audio.loudness", json!({}))]);
    ok(&r);
    let lufs = float_out(&r, "l", "lufs");
    assert!((lufs + 23.01).abs() < 0.3, "{lufs}");
    // Too short to gate: the silence floor, still a number.
    let r = run_chain(&h, &[tone(0.1, 48_000, 1, 1000.0, -20.0), ("l", "audio.loudness", json!({}))]);
    assert!(float_out(&r, "l", "lufs") <= -70.0);
}

/// A WAV value and back: the same samples at float32, the same rate and channels.
#[test]
fn audio_to_wav_and_back_keeps_every_sample() {
    let h = inproc();
    let nodes = json!({"t": {"kind": "audio.tone", "values": {"seconds": 0.2, "sample_rate": 44_100, "channels": 2, "frequency": 440.0, "level": -6.0}},
                       "w": {"kind": "audio.to_wav", "values": {"bits": "float32"}}, "a": {"kind": "audio.from_wav"}});
    let r = go(&h, "wavback", graph(nodes, &[("t", "audio", "w", "audio"), ("w", "wav", "a", "wav")]), &["t", "w", "a"]);
    ok(&r);
    let wav = h.resolve(&blob(&r, "w", "wav")).unwrap();
    assert_eq!(&wav.bytes[..4], b"RIFF");
    let (was, back) = (h.resolve(&blob(&r, "t", "audio")).unwrap(), h.resolve(&blob(&r, "a", "audio")).unwrap());
    assert_eq!((back.shape.as_slice(), &back.bytes), (was.shape.as_slice(), &was.bytes));
}

/// Audio as an array and back (docs/research/20-convert.md): (frames, channels) float32, the rate
/// beside it, and every sample the same when the rate is wired back.
#[test]
fn audio_to_array_and_back_keeps_every_sample() {
    let h = inproc();
    let nodes = json!({"t": {"kind": "audio.tone", "values": {"seconds": 0.2, "sample_rate": 44_100, "channels": 2, "frequency": 440.0, "level_db": -6.0}},
                       "x": {"kind": "audio.to_array"}, "a": {"kind": "audio.from_array"}});
    let wires = [("t", "audio", "x", "audio"), ("x", "array", "a", "array"), ("x", "sample_rate", "a", "sample_rate")];
    let r = go(&h, "arrayback", graph(nodes, &wires), &["t", "x", "a"]);
    ok(&r);
    let arr = h.resolve(&blob(&r, "x", "array")).unwrap();
    assert_eq!((arr.dtype.as_str(), arr.shape.as_slice()), ("<f4", &[8820u64, 2][..]));
    assert_eq!(float_out(&r, "x", "sample_rate"), 44_100.0);
    let (was, back) = (h.resolve(&blob(&r, "t", "audio")).unwrap(), h.resolve(&blob(&r, "a", "audio")).unwrap());
    assert_eq!((back.shape.as_slice(), &back.bytes), (was.shape.as_slice(), &was.bytes));
    // both are converters, so a Convert finds them
    let schema: plumcraft_proto::catalogue::SchemaReply = serde_json::from_value(call(&h, "schema", json!({}))).unwrap();
    let path = schema.convert_paths("WAV", "ARRAY");
    assert_eq!(path.first().map(|p| p.iter().map(|c| c.kind).collect::<Vec<_>>()), Some(vec!["audio.from_wav", "audio.to_array"]));
}

#[test]
fn array_to_audio_takes_mono_and_refuses_what_isnt_samples() {
    let h = inproc();
    let through = |id: &str, kind: &str, values: Value, out: &str| {
        let nodes = json!({"t": {"kind": "audio.tone", "values": {"seconds": 0.1, "sample_rate": 48_000, "channels": 1, "frequency": 1000.0, "level_db": -20.0}},
                           "s": {"kind": kind, "values": values}, "a": {"kind": "audio.from_array"}});
        go(&h, id, graph(nodes, &[("t", "audio", "s", "audio"), ("s", out, "a", "array")]), &["t", "s", "a"])
    };
    // a 1-D spectrum is mono audio
    let r = through("mono", "audio.spectrum", json!({"fft_size": 1024}), "spectrum");
    ok(&r);
    assert_eq!(h.resolve(&blob(&r, "a", "audio")).unwrap().shape, [513u64, 1]);
    // a (16, 32) picture would be 32 channels
    let r = through("wide", "audio.spectrogram", json!({"width": 32, "height": 16}), "spectrogram");
    assert_eq!(error_kind(&r, "a"), "config", "too many channels");
    let r = go(&h, "noarray", graph(json!({"a": {"kind": "audio.from_array"}}), &[]), &["a"]);
    assert_eq!(error_kind(&r, "a"), "config", "no wire, no value");
}

/// FLAC and AIFF values and back (docs/research/20-convert.md, 6.2): AIFF at float32 keeps
/// every sample; FLAC at 24 bits keeps the shape, the rate and the sound to within a 24-bit step.
#[test]
fn flac_and_aiff_values_come_back_as_audio() {
    let h = inproc();
    for (enc, dec, port, bits) in [("audio.to_aiff", "audio.from_aiff", "aiff", "float32"), ("audio.to_flac", "audio.from_flac", "flac", "int24")] {
        let nodes = json!({"t": {"kind": "audio.tone", "values": {"seconds": 0.2, "sample_rate": 44_100, "channels": 2, "frequency": 440.0, "level_db": -6.0}},
                           "e": {"kind": enc, "values": {"bits": bits}}, "d": {"kind": dec}});
        let r = go(&h, port, graph(nodes, &[("t", "audio", "e", "audio"), ("e", port, "d", port)]), &["t", "e", "d"]);
        ok(&r);
        let (was, back) = (sound(&h, &r, "t"), sound(&h, &r, "d"));
        assert_eq!((back.rate, back.channels.len(), back.frames()), (was.rate, was.channels.len(), was.frames()), "{port}");
        let worst = was.channels.iter().flatten().zip(back.channels.iter().flatten()).map(|(a, b)| (a - b).abs()).fold(0.0f32, f32::max);
        let step = if port == "flac" { 1.0 / 8_388_608.0 } else { 0.0 };
        assert!(worst <= step, "{port}: off by {worst}");
    }
}

#[test]
fn from_wav_refuses_what_isnt_audio() {
    let h = inproc();
    let r = go(&h, "nowav", graph(json!({"a": {"kind": "audio.from_wav"}}), &[]), &["a"]);
    assert_eq!(error_kind(&r, "a"), "config", "no wire, no value");
}

/// A tone is a line at its pitch: a row across the picture, on the log scale, high pitches at the
/// top, and level 0 to 1.
#[test]
fn a_spectrogram_draws_a_tone_at_its_pitch() {
    let h = inproc();
    let values = json!({"fft_size": 4096, "width": 32, "height": 64, "floor": -80.0, "low": 100.0});
    let r = run_chain(&h, &[tone(1.0, 48_000, 2, 1000.0, -6.0), ("s", "audio.spectrogram", values)]);
    ok(&r);
    let arr = h.resolve(&blob(&r, "s", "spectrogram")).unwrap();
    assert_eq!((arr.dtype.as_str(), arr.shape.as_slice()), ("<f4", &[64u64, 32][..]));
    let v: Vec<f32> = arr.bytes.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect();
    assert!(v.iter().all(|x| (0.0..=1.0).contains(x)));
    let column: Vec<f32> = (0..64).map(|y| v[y * 32 + 16]).collect();
    let (row, _) = column.iter().enumerate().fold((0, f32::MIN), |m, (k, x)| if *x > m.1 { (k, *x) } else { m });
    let want = (1.0 - (10.0f64).ln() / (240.0f64).ln()) * 63.0;
    assert!((row as f64 - want).abs() <= 2.0, "row {row}, wanted about {want}");
    let low = run_chain(&h, &[tone(0.1, 8000, 1, 400.0, -6.0), ("s", "audio.spectrogram", json!({"fft_size": 256, "width": 16, "height": 16, "low": 5000.0}))]);
    assert_eq!(error_kind(&low, "s"), "config", "above the highest pitch here");
}

#[test]
fn spectrum_peaks_at_the_tone() {
    let h = inproc();
    let r = run_chain(&h, &[tone(1.0, 48_000, 2, 1500.0, -6.0), ("s", "audio.spectrum", json!({"fft_size": 4096}))]);
    ok(&r);
    let b = blob(&r, "s", "spectrum");
    let arr = h.resolve(&b).unwrap();
    assert_eq!((arr.dtype.as_str(), arr.shape.as_slice()), ("<f4", &[2049u64][..]));
    let v: Vec<f32> = arr.bytes.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect();
    let (k, top) = v.iter().enumerate().fold((0, f32::MIN), |m, (k, x)| if *x > m.1 { (k, *x) } else { m });
    assert!((k as f64 - 1500.0 * 4096.0 / 48_000.0).abs() <= 1.0, "bin {k}");
    assert!((f64::from(top) + 6.0).abs() < 1.5, "{top}");
    let r = run_chain(&h, &[tone(0.1, 8000, 1, 100.0, -6.0), ("s", "audio.spectrum", json!({"fft_size": 1000}))]);
    assert_eq!(error_kind(&r, "s"), "config");
}

#[test]
fn transients_and_silence_in_tone_silence_tone() {
    let h = inproc();
    let p = tone_silence_tone();
    let g = graph(
        json!({
            "l": {"kind": "audio.load", "values": {"path": p.display().to_string()}},
            "t": {"kind": "audio.transients", "values": {}},
            "s": {"kind": "audio.silence", "values": {"threshold_db": -40.0, "min_silence": 0.1}}
        }),
        &[("l", "audio", "t", "audio"), ("l", "audio", "s", "audio")],
    );
    let r = go(&h, "ts", g, &["t", "s"]);
    ok(&r);
    let onsets = json_out(&r, "t", "onsets");
    let times: Vec<f64> = onsets["columns"][0]["values"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    assert_eq!(onsets["columns"][0]["name"], "time");
    assert!(times.iter().any(|t| (t - 0.5).abs() < 0.02), "{times:?}");
    assert!(times.iter().any(|t| (t - 1.5).abs() < 0.02), "{times:?}");
    let regions = json_out(&r, "s", "regions");
    let col = |k: usize| regions["columns"][k]["values"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect::<Vec<_>>();
    let (starts, ends) = (col(0), col(1));
    assert_eq!(starts.len(), 2, "{regions}");
    assert!((starts[0] - 0.5).abs() < 0.001 && (ends[0] - 1.0).abs() < 0.001, "{regions}");
    assert!((starts[1] - 1.5).abs() < 0.001 && (ends[1] - 2.0).abs() < 0.001, "{regions}");
}

#[test]
fn save_and_load_round_trip_and_refuse_what_they_should() {
    let h = inproc();
    let dir = scratch("io");
    let f = dir.join("tone.wav");
    let path = f.display().to_string();
    let save = |p: &str, extra: Value| {
        let mut v = json!({"path": p});
        v.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
        run_chain(&h, &[tone(0.2, 22_050, 2, 440.0, -3.0), ("w", "audio.save", v)])
    };
    let r = save(&path, json!({}));
    ok(&r);
    assert_eq!(json_out(&r, "w", "path"), json!(path));
    let original = sound(&h, &r, "t");
    let r = run_chain(&h, &[("l", "audio.load", json!({"path": path}))]);
    ok(&r);
    assert_eq!(sound(&h, &r, "l"), original, "float32 WAV round-trips exactly");
    assert_eq!(error_kind(&save(&path, json!({})), "w"), "config", "no overwrite unless asked");
    ok(&save(&path, json!({"overwrite": true, "bits": "int16"})));
    let r = run_chain(&h, &[("l", "audio.load", json!({"path": path}))]);
    let s16 = sound(&h, &r, "l");
    assert!(s16.channels.iter().flatten().zip(original.channels.iter().flatten()).all(|(a, b)| (a - b).abs() < 1e-4), "16-bit within a quantum");
    assert_eq!(error_kind(&save(&dir.join("x.mp3").display().to_string(), json!({})), "w"), "config");
    assert_eq!(error_kind(&save(&dir.join("nope/x.wav").display().to_string(), json!({})), "w"), "config");
    assert_eq!(error_kind(&save(&dir.display().to_string(), json!({})), "w"), "config");
    assert_eq!(error_kind(&save("", json!({})), "w"), "config");
    let missing = run_chain(&h, &[("l", "audio.load", json!({"path": dir.join("missing.wav").display().to_string()}))]);
    assert_eq!(error_kind(&missing, "l"), "config");
    let junk = dir.join("junk.wav");
    std::fs::write(&junk, b"RIFF\x10\0\0\0WAVEfmt nonsense that is not audio").unwrap();
    let r = run_chain(&h, &[("l", "audio.load", json!({"path": junk.display().to_string()}))]);
    assert_eq!(error_kind(&r, "l"), "op");
}
