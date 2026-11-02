//! The audio engine through the interface, in mode 1 (compiled in), mode 2 (the
//! `plumcraft-audio-engine` binary on a socket) and mode 3 (the same binary over HTTP, remote:
//! sources upload, values download by URL). Every case is driven only through
//! [`plumcraft_host::Connection::call`] and never knows its mode.

mod common;

use std::time::{Duration, Instant};

use common::*;
use plumcraft_engine::host::PollReply;
use plumcraft_proto::PortValue;
use plumcraft_proto::catalogue::Media;
use plumcraft_proto::data::{Form, Locator};
use plumcraft_proto::display::Rendition;
use plumcraft_proto::event::{Event, NodeState};
use plumcraft_proto::session::HelloReply;
use serde_json::{Value, json};

fn hello_and_schema(mk: fn() -> Conn) {
    let h = mk();
    let reply: HelloReply =
        serde_json::from_value(call(&h, "hello", json!({"v": 2, "op": "hello", "protocol": {"min": 2, "max": 2}, "caps": ["events.poll", "status.progress"]})))
            .unwrap();
    assert_eq!(reply.protocol, 2);
    assert_eq!(reply.engine.id, "audio");
    assert!(reply.engine.salt.contains(plumcraft_audio::SOUNDCRAFT_REV), "a SoundCraft upgrade changes every memo key");
    let schema = call(&h, "schema", json!({}));
    let nodes = schema["nodes"].as_array().unwrap();
    assert!(nodes.len() >= 40, "{}", nodes.len());
    // its own nodes, and the SDK's building blocks (Vary, sequences, files)
    assert!(nodes.iter().all(|n| n["id"].as_str().is_some_and(|id| id.starts_with("audio.")) || n["category"][0] == "Building blocks"));
    assert!(nodes.iter().any(|n| n["id"] == "vary.range2d"));
    let audio = schema["wire_types"].as_array().unwrap().iter().find(|w| w["id"] == "AUDIO").unwrap().clone();
    assert_eq!(audio["media"], "audio");
    assert_eq!(audio["thumb"], true);
}

/// tone → gain → compressor → peak and loudness: a chain of every kind of node runs, with the
/// status records in order.
fn a_chain_runs(mk: fn() -> Conn) {
    let h = mk();
    let g = graph(
        json!({
            "t": {"kind": "audio.tone", "values": {"seconds": 1.0, "sample_rate": 48000, "channels": 2, "frequency": 1000.0, "level_db": -20.0}},
            "g": {"kind": "audio.gain", "values": {"db": 6.0}},
            "c": {"kind": "audio.fx.compressor"},
            "p": {"kind": "audio.peak"},
            "l": {"kind": "audio.loudness"}
        }),
        &[("t", "audio", "g", "audio"), ("g", "audio", "c", "audio"), ("c", "audio", "p", "audio"), ("c", "audio", "l", "audio")],
    );
    run(&h, "chain", 3, g, json!({"targets": ["p", "l"], "watch": [{"node": "p", "port": "db"}]}));
    let (events, reply) = collect(&h, "chain", 0);
    assert!(reply.ok, "{reply:?}");
    assert_eq!(reply.order, ["t", "g", "c", "l", "p"]);
    let peak = float_out(&reply, "p", "db");
    assert!((-15.0..=-13.0).contains(&peak), "the compressor at its defaults on a -14 dBFS tone: {peak}");
    let lufs = float_out(&reply, "l", "lufs");
    assert!((-25.0..=-10.0).contains(&lufs), "{lufs}");
    assert_eq!(reply.watch[0].value, Some(json!(peak)));
    let pos = |node: &str, state: NodeState| events.iter().position(|e| matches!(&e.event, Event::Status(r) if r.node_id == node && r.state == state)).unwrap();
    for n in ["t", "g", "c", "p", "l"] {
        assert!(pos(n, NodeState::Started) < pos(n, NodeState::Preview) && pos(n, NodeState::Preview) < pos(n, NodeState::Done));
    }
    assert!(pos("g", NodeState::Done) < pos("c", NodeState::Started));
    // The canvas's waveform: a peaks thumb on every audio node.
    for n in ["t", "g", "c"] {
        assert!(matches!(reply.nodes[n].thumb, Some(Rendition::Peaks { channels: 2, .. })), "{n}: {:?}", reply.nodes[n].thumb);
    }
}

/// Audio over the inline limit crosses as an `<f4` blob with `Form::Audio`, named by its handle,
/// and resolves through the connection; its watched display is media `audio` with peaks inline
/// and the full value by reference.
fn audio_crosses_as_a_blob(mk: fn() -> Conn) {
    let h = mk();
    let g = chain(&[("t", "audio.tone", json!({"seconds": 2.0, "sample_rate": 48000, "channels": 2, "frequency": 440.0})), ("r", "audio.reverse", json!({}))]);
    run(&h, "big", 1, g, json!({"targets": ["t", "r"], "watch": [{"node": "r", "port": "audio"}]}));
    let (_, reply) = collect(&h, "big", 0);
    assert!(reply.ok, "{reply:?}");
    let r = blob(&reply, "r", "audio");
    assert_eq!(r.meta.form, Form::Audio { sample_rate: 48000, channels: 2, frames: 96_000, dtype: "<f4".into() });
    let local = r.at.iter().any(|l| matches!(l, Locator::Commonkit { .. } | Locator::Path { .. }));
    let remote = r.at.iter().any(|l| matches!(l, Locator::Url { url, .. } if url.starts_with("/v1/blobs/md5/nd1/")));
    assert!(local || remote, "{:?}", r.at);
    assert!(!r.at.iter().any(|l| matches!(l, Locator::Inline { .. })), "768 KB is over the inline limit");
    let (t, rev) = (sound(&h, &reply, "t"), sound(&h, &reply, "r"));
    let mut back = rev.channels[0].clone();
    back.reverse();
    assert_eq!(back, t.channels[0]);
    let d = &reply.watch[0];
    assert_eq!(d.media, Media::Audio);
    assert_eq!(d.summary, "audio 48000 Hz, 2 ch, 2.00 s");
    assert!(matches!(d.renditions.first(), Some(Rendition::Peaks { channels: 2, .. })));
    assert_eq!(d.full.as_ref().and_then(|f| f.handle.clone()), r.handle);
}

/// Spec 14: a rerun of an unchanged graph with the memo on is all `cached`, nothing started, the
/// same handles.
fn a_cached_rerun_is_all_cached(mk: fn() -> Conn) {
    let h = mk();
    let g = chain(&[
        ("t", "audio.tone", json!({"seconds": 0.5, "sample_rate": 16000})),
        ("f", "audio.fx.plate_reverb", json!({})),
        ("s", "audio.time_stretch", json!({"ratio": 1.25})),
        ("p", "audio.peak", json!({})),
    ]);
    run(&h, "m1", 1, g.clone(), json!({"targets": ["s", "p"], "memo": "use"}));
    let first = collect(&h, "m1", 0).1;
    assert!(first.ok && first.nodes.values().all(|n| n.state == NodeState::Done), "{first:?}");
    run(&h, "m2", 2, g, json!({"targets": ["s", "p"], "memo": "use"}));
    let (events, again) = collect(&h, "m2", 0);
    assert!(again.nodes.values().all(|n| n.state == NodeState::Cached), "{:?}", again.nodes.iter().map(|(k, n)| (k, n.state)).collect::<Vec<_>>());
    assert!(!events.iter().any(|e| matches!(&e.event, Event::Status(r) if r.state == NodeState::Started)));
    assert_eq!(blob(&again, "s", "audio").handle, blob(&first, "s", "audio").handle);
    assert_eq!(again.nodes["p"].outputs, first.nodes["p"].outputs);
}

/// The same graph gives the same handles twice, with the memo off: every node is deterministic
/// (noise is seeded; effects start from a fresh instance).
fn the_same_graph_gives_the_same_handles(mk: fn() -> Conn) {
    let h = mk();
    let g = graph(
        json!({
            "t": {"kind": "audio.tone", "values": {"seconds": 0.3, "sample_rate": 22050, "waveform": "saw", "channels": 2}},
            "n": {"kind": "audio.noise", "values": {"seconds": 0.3, "sample_rate": 22050, "seed": 42, "channels": 2}},
            "m": {"kind": "audio.mix", "values": {"gain_b": -20.0}},
            "c": {"kind": "audio.fx.chorus"},
            "d": {"kind": "audio.fx.dither"},
            "p": {"kind": "audio.pitch_shift", "values": {"semitones": -5.0}},
            "s": {"kind": "audio.spectrum"}
        }),
        &[
            ("t", "audio", "m", "a"),
            ("n", "audio", "m", "b"),
            ("m", "audio", "c", "audio"),
            ("c", "audio", "d", "audio"),
            ("d", "audio", "p", "audio"),
            ("p", "audio", "s", "audio"),
        ],
    );
    let targets = ["t", "n", "m", "c", "d", "p", "s"];
    let handles = |id: &str| {
        let r = go(&h, id, g.clone(), &targets);
        assert!(r.ok, "{r:?}");
        targets.iter().map(|n| blob(&r, n, if *n == "s" { "spectrum" } else { "audio" }).handle).collect::<Vec<_>>()
    };
    assert_eq!(handles("d1"), handles("d2"));
}

/// Errors are data: a bad parameter fails its node as `config`, downstream is skipped, an
/// independent branch still runs.
fn bad_parameters_are_config_errors(mk: fn() -> Conn) {
    let h = mk();
    let g = graph(
        json!({
            "t": {"kind": "audio.tone", "values": {"seconds": 0.1, "sample_rate": 8000}},
            "s": {"kind": "audio.time_stretch", "values": {"ratio": 50.0}},
            "p": {"kind": "audio.peak"},
            "ok": {"kind": "audio.peak"}
        }),
        &[("t", "audio", "s", "audio"), ("s", "audio", "p", "audio"), ("t", "audio", "ok", "audio")],
    );
    let r = go(&h, "bad", g, &["p", "ok"]);
    assert!(!r.ok);
    assert_eq!(r.nodes["s"].error.as_ref().unwrap().kind, "config");
    assert_eq!(r.nodes["p"].state, NodeState::Skipped);
    assert_eq!(r.nodes["ok"].state, NodeState::Done);
}

/// Spec 19 item 6: a cancel mid time-stretch ends the run `cancelled` within 500 ms. The stretch
/// is SoundCraft's monolithic WSOLA, so the node waits on a worker and polls cancel every 20 ms.
fn cancel_mid_stretch(mk: fn() -> Conn) {
    let h = mk();
    let g = chain(&[
        ("t", "audio.tone", json!({"seconds": 240.0, "sample_rate": 48000, "frequency": 220.0})),
        ("s", "audio.time_stretch", json!({"ratio": 2.0})),
        ("p", "audio.peak", json!({})),
    ]);
    run(&h, "c1", 1, g, json!({"targets": ["p"]}));
    // Wait until the stretch reports progress: it is really running.
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut since = 0;
    let mut stretching = false;
    while !stretching && Instant::now() < deadline {
        let p: PollReply = serde_json::from_value(call(&h, "events.poll", json!({"since": since, "wait_ms": 200, "run_id": "c1"}))).unwrap();
        since = p.last_seq;
        stretching = p.events.iter().any(|e| matches!(&e.event, Event::Status(r) if r.node_id == "s" && r.state == NodeState::Progress));
        assert!(!p.events.iter().any(|e| matches!(&e.event, Event::RunReply(_))), "the stretch finished before the cancel: make the input longer");
    }
    assert!(stretching, "the stretch never reported progress");
    std::thread::sleep(Duration::from_millis(100));
    let t = Instant::now();
    call(&h, "run.cancel", json!({"run_id": "c1"}));
    let (_, reply) = collect(&h, "c1", since);
    assert!(t.elapsed() < Duration::from_millis(500), "cancel took {:?}", t.elapsed());
    assert_eq!(reply.error.as_ref().unwrap().kind, "cancelled");
    assert_eq!(reply.nodes["s"].state, NodeState::Cancelled);
    assert_eq!(reply.nodes["p"].state, NodeState::Cancelled);
    // The engine is free for the next run while the abandoned worker finishes.
    let r = go(&h, "after", chain(&[("t", "audio.tone", json!({"seconds": 0.1, "sample_rate": 8000})), ("p", "audio.peak", json!({}))]), &["p"]);
    assert!(r.ok);
}

/// `audio.save` writes a WAV that `audio.load` reads back exactly (float32), keyed by content.
fn save_then_load_round_trips(mk: fn() -> Conn) {
    let h = mk();
    let f = scratch("rt").join("round-trip.wav");
    let path = f.display().to_string();
    let g = chain(&[("t", "audio.noise", json!({"seconds": 0.5, "sample_rate": 44100, "channels": 2, "seed": 3})), ("w", "audio.save", json!({"path": path}))]);
    let saved = go(&h, "save", g, &["t", "w"]);
    assert!(saved.ok, "{saved:?}");
    assert_eq!(saved.nodes["w"].outputs["path"], PortValue::Json { value: json!(path) });
    let loaded = go(&h, "load", chain(&[("l", "audio.load", json!({"path": path}))]), &["l"]);
    assert!(loaded.ok, "{loaded:?}");
    assert_eq!(sound(&h, &loaded, "l"), sound(&h, &saved, "t"));
    assert_eq!(blob(&loaded, "l", "audio").handle, blob(&saved, "t", "audio").handle, "the same samples, the same handle");
}

macro_rules! every_mode {
    ($($case:ident),* $(,)?) => {
        mod mode1_inproc {
            $(#[test] fn $case() { super::$case(super::inproc) })*
        }
        mod mode2_process {
            $(#[test] fn $case() { super::$case(super::process) })*
        }
        mod mode3_http {
            $(#[test] fn $case() { super::$case(super::http) })*
        }
    };
}

every_mode!(
    hello_and_schema,
    a_chain_runs,
    audio_crosses_as_a_blob,
    a_cached_rerun_is_all_cached,
    the_same_graph_gives_the_same_handles,
    bad_parameters_are_config_errors,
    cancel_mid_stretch,
    save_then_load_round_trips,
);

/// Spec 13.2, remote: the file `audio.load` reads uploads once (a rerun finds the engine has it),
/// new bytes upload again, and the decoded audio downloads by URL.
#[test]
fn mode3_load_uploads_the_file_once() {
    let h = spawn_http();
    let dir = scratch("up");
    let f = dir.join("source.wav");
    // Written by the engine itself: the same machine here, as `audio.save` would on another.
    let save = |seed: u64| {
        let g = chain(&[
            ("t", "audio.noise", json!({"seconds": 0.25, "sample_rate": 16000, "seed": seed})),
            ("w", "audio.save", json!({"path": f.display().to_string(), "overwrite": true})),
        ]);
        let r = go(&h, &format!("save{seed}"), g, &["t", "w"]);
        assert!(r.ok, "{r:?}");
        sound(&h, &r, "t")
    };
    let load = |id: &str| {
        let g = chain(&[("l", "audio.load", json!({"path": f.display().to_string()}))]);
        let r = go(&h, id, g, &["l"]);
        assert!(r.ok, "{r:?}");
        assert!(matches!(blob(&r, "l", "audio").at.as_slice(), [Locator::Url { .. }] | [Locator::Inline { .. }]));
        sound(&h, &r, "l")
    };
    let first = save(1);
    let before = h.uploads();
    assert_eq!(load("l1"), first);
    assert_eq!(h.uploads(), before + 1, "uploaded once");
    assert_eq!(load("l2"), first);
    assert_eq!(h.uploads(), before + 1, "the engine had it: nothing uploaded");
    let second = save(2);
    assert_eq!(load("l3"), second);
    assert_eq!(h.uploads(), before + 2, "new bytes upload");
}

/// The demo workflow runs.
#[test]
fn the_demo_workflow_runs() {
    let h = inproc();
    let g: Value = serde_json::from_str(plumcraft_audio::DEMO_WORKFLOW).unwrap();
    run(&h, "demo", 1, g, json!({}));
    let (_, r) = collect(&h, "demo", 0);
    assert!(r.ok, "{r:?}");
    assert_eq!(r.order.len(), 7);
}
