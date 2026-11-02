//! The panic hunt: every node with hostile parameters (wrong types, NaN as text, huge, negative,
//! empty) and hostile audio (no frames, no channels, ragged samples, non-finite samples, absurd
//! sample rates, too many channels, values that aren't audio) must end `done` or with a
//! `config`/`op` error. A panic would surface as `kind: engine` (the host catches it and rebuilds
//! the engine), so any `engine` error fails the hunt.

mod common;

use std::sync::Arc;

use common::*;
use plumcraft_engine::{Audio, Cx, Data, Inputs, Node, Outputs};
use plumcraft_proto::catalogue::{NodeDef, Port};
use plumcraft_proto::error::NodeError;
use plumcraft_proto::event::NodeState;
use plumcraft_proto::run::RunReply;
use serde_json::{Value, json};

/// Audio no well-behaved node makes, by `kind`.
struct Hostile;

const HOSTILE_KINDS: i64 = 10;

impl Node for Hostile {
    fn def(&self) -> NodeDef {
        NodeDef {
            id: "test.hostile_audio".into(),
            inputs: vec![Port { name: "kind".into(), ty: "INT".into(), default: json!(0), ..Default::default() }],
            outputs: vec![Port { name: "audio".into(), ty: "AUDIO".into(), ..Default::default() }],
            ..Default::default()
        }
    }

    fn run(&self, i: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
        let kind = i.get("kind").and_then(Data::json).and_then(Value::as_i64).unwrap_or(0);
        let a = |rate: u32, channels: u16, samples: Vec<f32>| Audio { sample_rate: rate, channels, samples: Arc::from(samples) };
        let tone: Vec<f32> = (0..800).map(|k| (k as f32 * 0.1).sin()).collect();
        let audio = match kind {
            0 => a(8000, 2, Vec::new()),
            1 => a(8000, 0, tone),
            2 => a(8000, 2, tone[..799].to_vec()),
            3 => a(
                8000,
                1,
                tone.iter()
                    .enumerate()
                    .map(|(k, v)| {
                        if k % 3 == 0 {
                            f32::NAN
                        } else if k % 3 == 1 {
                            f32::INFINITY
                        } else {
                            *v
                        }
                    })
                    .collect(),
            ),
            4 => a(0, 1, tone),
            5 => a(u32::MAX, 1, tone),
            6 => a(8000, 17, vec![0.1; 17 * 10]),
            7 => a(8000, 1, vec![0.0; 800]),
            8 => a(8000, 2, tone.iter().map(|v| v * 1e30).collect()),
            _ => a(8000, 1, vec![f32::MIN_POSITIVE / 4.0; 800]), // subnormal
        };
        Ok(Outputs::from([("audio".to_owned(), Data::Audio(audio))]))
    }
}

fn hunt_engine() -> Conn {
    inproc_with(|| vec![Box::new(Hostile)])
}

fn hostile_values() -> Vec<Value> {
    vec![
        Value::Null,
        json!("not a number"),
        json!("NaN"),
        json!("inf"),
        json!(-1),
        json!(0),
        json!(-0.0),
        json!(1e308),
        json!(-1e308),
        json!(1e15),
        json!(u64::MAX),
        json!(i64::MIN),
        json!(0.5),
        json!([]),
        json!({"a": 1}),
        json!(true),
        json!(""),
        json!("x".repeat(10_000)),
    ]
}

/// Paths `audio.load` should refuse (each an error, never a panic).
fn hostile_load_paths(dir: &std::path::Path) -> Vec<Value> {
    let w = |name: &str, bytes: &[u8]| {
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        json!(p.display().to_string())
    };
    // A real WAV header, then lies: a data chunk far longer than the file, 65535 channels, a
    // zero sample rate.
    let header = |channels: u16, rate: u32, data_len: u32| {
        let mut b = Vec::new();
        b.extend_from_slice(b"RIFF");
        b.extend_from_slice(&36u32.wrapping_add(data_len).to_le_bytes());
        b.extend_from_slice(b"WAVEfmt ");
        b.extend_from_slice(&16u32.to_le_bytes());
        b.extend_from_slice(&3u16.to_le_bytes()); // float
        b.extend_from_slice(&channels.to_le_bytes());
        b.extend_from_slice(&rate.to_le_bytes());
        b.extend_from_slice(&(rate.wrapping_mul(4).wrapping_mul(u32::from(channels))).to_le_bytes());
        b.extend_from_slice(&(4u16.wrapping_mul(channels)).to_le_bytes());
        b.extend_from_slice(&32u16.to_le_bytes());
        b.extend_from_slice(b"data");
        b.extend_from_slice(&data_len.to_le_bytes());
        b.extend_from_slice(&[0u8; 64]);
        b
    };
    vec![
        json!(""),
        json!("/"),
        json!("/dev/null"),
        json!("/no/such/file.wav"),
        json!(dir.display().to_string()),
        json!(5),
        json!("\u{0}.wav"),
        w("empty.wav", b""),
        w("text.wav", b"hello, this is not audio at all"),
        w("riff.wav", b"RIFF\xff\xff\xff\xffWAVE"),
        w("long.wav", &header(2, 48_000, 0xFFFF_FFF0)),
        w("wide.wav", &header(u16::MAX, 48_000, 64)),
        w("zero-rate.wav", &header(1, 0, 64)),
        w("fast.wav", &header(1, u32::MAX, 64)),
        w("fake.mp3", b"ID3\x04\x00\x00\x7f\x7f\x7f\x7f garbage"),
        w("fake.flac", b"fLaC\x00\x00\x00\x22 garbage garbage garbage"),
        w("fake.ogg", b"OggS\x00\x02 garbage garbage garbage garbage"),
    ]
}

fn hostile_save_paths(dir: &std::path::Path) -> Vec<Value> {
    vec![
        json!(""),
        json!(5),
        Value::Null,
        json!("\u{0}.wav"),
        json!("/no/such/dir/x.wav"),
        json!(dir.display().to_string()),
        json!(dir.join("x.mp3").display().to_string()),
        json!(dir.join("noext").display().to_string()),
        json!(dir.join(".wav").display().to_string()),
    ]
}

/// The node's state and error kind; an `engine` error anywhere in the run is a failure.
fn outcome(reply: &RunReply, case: &str) -> (NodeState, Option<String>) {
    for (id, n) in &reply.nodes {
        if let Some(e) = &n.error {
            assert_ne!(e.kind, "engine", "{case}: node {id} crashed the engine: {}", e.message);
        }
    }
    if let Some(e) = &reply.error {
        assert_ne!(e.kind, "engine", "{case}: {}", e.message);
    }
    let n = &reply.nodes["x"];
    let kind = n.error.as_ref().map(|e| e.kind.clone());
    match n.state {
        NodeState::Done => {}
        NodeState::Error => assert!(matches!(kind.as_deref(), Some("config" | "op")), "{case}: {kind:?} {:?}", n.error),
        other => panic!("{case}: ended {other:?} {:?}", n.error),
    }
    (n.state, kind)
}

#[test]
fn every_node_survives_hostile_parameters_and_audio() {
    let h = hunt_engine();
    let dir = scratch("hunt");
    let save_to = dir.join("out.wav").display().to_string();
    let schema = call(&h, "schema", json!({}));
    let defs: Vec<NodeDef> = serde_json::from_value(schema["nodes"].clone()).unwrap();
    let mut runs = 0u64;
    let mut errors = 0u64;
    let mut n = 0u64;
    let mut go_case = |values: Value, src: Option<(&str, Value)>, def: &NodeDef, case: String| {
        n += 1;
        let sockets: Vec<&Port> = def.inputs.iter().filter(|p| p.ty == "AUDIO").collect();
        let mut nodes = json!({"x": {"kind": def.id, "values": values}});
        let mut wires: Vec<(&str, &str, &str, &str)> = Vec::new();
        if let Some((kind, v)) = src {
            nodes["src"] = json!({"kind": kind, "values": v});
            wires = sockets.iter().map(|p| ("src", "audio", "x", p.name.as_str())).collect();
        }
        let reply = go(&h, &format!("h{n}"), graph(nodes, &wires), &["x"]);
        let (state, _) = outcome(&reply, &case);
        runs += 1;
        if state == NodeState::Error {
            errors += 1;
        }
    };
    let tone = ("audio.tone", json!({"seconds": 0.05, "sample_rate": 8000, "channels": 2, "frequency": 300.0}));
    for def in defs.iter().filter(|d| d.id.starts_with("audio.")) {
        let base = |extra: Value| {
            let mut v = if def.id == "audio.save" { json!({"path": save_to, "overwrite": true}) } else { json!({}) };
            if let (Some(o), Some(x)) = (v.as_object_mut(), extra.as_object()) {
                o.extend(x.clone());
            }
            v
        };
        // Every widget, every hostile value, one at a time.
        for p in def.inputs.iter().filter(|p| p.ty != "AUDIO") {
            let values = match (def.id.as_str(), p.name.as_str()) {
                ("audio.load", "path") => hostile_load_paths(&dir),
                ("audio.save", "path") => hostile_save_paths(&dir),
                _ => hostile_values(),
            };
            for v in values {
                let case = format!("{}.{} = {}", def.id, p.name, v.to_string().chars().take(60).collect::<String>());
                go_case(base(json!({ p.name.clone(): v })), Some(tone.clone()), def, case);
            }
        }
        // Every hostile audio on every audio socket.
        if def.inputs.iter().any(|p| p.ty == "AUDIO") {
            for k in 0..HOSTILE_KINDS {
                go_case(base(json!({})), Some(("test.hostile_audio", json!({"kind": k}))), def, format!("{} on hostile audio {k}", def.id));
            }
            // A value that isn't audio on the socket (no wire, a widget value instead).
            let mut v = base(json!({}));
            for p in def.inputs.iter().filter(|p| p.ty == "AUDIO") {
                v[&p.name] = json!(5);
            }
            go_case(v, None, def, format!("{}: a number on its audio socket", def.id));
        }
    }
    eprintln!("panic hunt: {runs} runs, {errors} refused");
    assert!(runs > 1000, "{runs}");
    assert!(errors > runs / 3, "most hostile values are refused: {errors} of {runs}");
    // Still alive and well.
    let r = go(&h, "alive", chain(&[("t", "audio.tone", json!({})), ("p", "audio.peak", json!({}))]), &["p"]);
    assert!(r.ok);
}

/// Hostile audio is refused as `config` at the first node that reads it, with the reason.
#[test]
fn hostile_audio_is_refused_with_a_reason() {
    let h = hunt_engine();
    let why = |k: i64| {
        let g = graph(json!({"src": {"kind": "test.hostile_audio", "values": {"kind": k}}, "x": {"kind": "audio.gain"}}), &[("src", "audio", "x", "audio")]);
        let r = go(&h, &format!("w{k}"), g, &["x"]);
        let e = r.nodes["x"].error.clone().unwrap_or_else(|| panic!("{k}: accepted"));
        assert_eq!(e.kind, "config", "{k}");
        e.message
    };
    assert!(why(0).contains("empty"));
    assert!(why(1).contains("0 channels"));
    assert!(why(2).contains("divide"));
    assert!(why(4).contains("sample rate"));
    assert!(why(5).contains("sample rate"));
    assert!(why(6).contains("17 channels"));
}
