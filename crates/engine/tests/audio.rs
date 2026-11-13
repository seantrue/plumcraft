//! Audio values (plan 10.4, F1): a node that returns audio, in-process. Small audio crosses
//! inline, large audio through the store; both are `<f4` (frames, channels) blobs with
//! `Form::Audio`, a peaks thumbnail, and a display of media `audio`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use plumcraft_engine::host::{Host, PollReply};
use plumcraft_engine::{Audio, Cx, Data, Engine, Inputs, Node, Outputs, data};
use plumcraft_proto::PortValue;
use plumcraft_proto::catalogue::{Media, NodeDef, Port, WireType};
use plumcraft_proto::data::{Form, Locator};
use plumcraft_proto::display::Rendition;
use plumcraft_proto::error::NodeError;
use plumcraft_proto::handle::Handle;
use plumcraft_proto::run::RunReply;
use plumcraft_proto::session::{EngineInfo, EngineState};
use serde_json::{Value, json};

/// A sine at 440 Hz for `seconds`, stereo (right channel inverted), at 8 kHz.
struct Tone;

impl Node for Tone {
    fn def(&self) -> NodeDef {
        NodeDef {
            id: "test.tone".into(),
            inputs: vec![Port { name: "seconds".into(), ty: "FLOAT".into(), default: json!(0.01), ..Default::default() }],
            outputs: vec![Port { name: "audio".into(), ty: "AUDIO".into(), ..Default::default() }],
            ..Default::default()
        }
    }

    fn run(&self, inputs: &Inputs, _: &mut Cx) -> Result<Outputs, NodeError> {
        let secs = inputs.get("seconds").and_then(Data::json).and_then(Value::as_f64).unwrap_or(0.01);
        let frames = (secs * 8000.0) as usize;
        let left: Vec<f32> = (0..frames).map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / 8000.0).sin()).collect();
        let right: Vec<f32> = left.iter().map(|s| -s).collect();
        Ok(Outputs::from([("audio".to_owned(), Data::Audio(Audio::from_channels(8000, &[left, right])))]))
    }
}

fn root() -> std::path::PathBuf {
    let r = plumcraft_scratch::root().join("audio-test");
    let _ = std::fs::create_dir_all(&r);
    r
}

fn run(h: &Host, id: &str, seconds: f64) -> RunReply {
    let g = json!({"version": 1, "nodes": {"t": {"kind": "test.tone", "values": {"seconds": seconds}}}, "wires": []});
    let req = json!({"run_id": id, "generation": 1, "graph": {"kind": "inline", "graph": g}, "targets": ["t"], "watch": [{"node": "t", "port": "audio"}], "memo": "off"});
    h.dispatch("run", req).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut since = 0;
    while Instant::now() < deadline {
        let p: PollReply = serde_json::from_value(h.dispatch("events.poll", json!({"since": since, "wait_ms": 200})).unwrap()).unwrap();
        since = p.last_seq;
        if let Some(r) = p.events.iter().find_map(|e| match &e.event {
            plumcraft_proto::event::Event::RunReply(r) if r.run_id == id => Some((**r).clone()),
            _ => None,
        }) {
            return r;
        }
    }
    panic!("no reply");
}

fn host() -> Host {
    let engine = || {
        Ok(Engine::new(EngineInfo { id: "audio-test".into(), ..Default::default() })
            .with_wire_type(WireType { id: "AUDIO".into(), media: Media::Audio, ..Default::default() })
            .with_wire_type(WireType { id: "FLOAT".into(), media: Media::Scalar, ..Default::default() })
            .with_node(Box::new(Tone))
            .with_store(Some(root())))
    };
    let h = Host::start(Box::new(engine), None);
    assert!(matches!(h.wait_ready(Duration::from_secs(5)), EngineState::Connected { .. }));
    h
}

#[test]
fn audio_crosses_as_an_f4_blob_with_peaks() {
    let h = host();
    for (id, secs, inline) in [("small", 0.01, true), ("large", 2.5, false)] {
        let reply = run(&h, id, secs);
        assert!(reply.ok, "{reply:?}");
        let PortValue::Blob { data: r } = &reply.nodes["t"].outputs["audio"] else { panic!("not a blob") };
        let frames = (secs * 8000.0) as u64;
        assert_eq!(r.meta.form, Form::Audio { sample_rate: 8000, channels: 2, frames, dtype: "<f4".into() });
        assert_eq!(matches!(r.at.as_slice(), [Locator::Inline { .. }]), inline, "{id}: {:?}", r.at);
        let blob = plumcraft_store::Store::new(root()).resolve(r, true).expect("resolves");
        assert_eq!(blob.shape, [frames, 2]);
        assert_eq!(r.handle, Some(Handle::nd1("<f4", &[frames, 2], &blob.bytes).unwrap()));
        // The thumb and the watched display: peaks inline, the display media audio.
        let Some(Rendition::Peaks { bins, channels, .. }) = &reply.nodes["t"].thumb else { panic!("no peaks thumb") };
        assert_eq!(*channels, 2);
        assert!(*bins as u64 <= frames && *bins > 0);
        let d = &reply.watch[0];
        assert_eq!(d.media, Media::Audio);
        assert_eq!(d.summary, format!("audio 8000 Hz, 2 ch, {secs:.2} s"));
        assert_eq!(d.full.as_ref().and_then(|f| f.handle.clone()), r.handle);
    }
}

#[test]
fn peaks_are_min_and_max_per_bin_within_four_kib() {
    let a = Audio::from_channels(48_000, &[vec![0.0, 1.0, -1.0, 0.5], vec![0.25; 4]]);
    let Some(Rendition::Peaks { data: r, bins, channels }) = data::peaks(&a) else { panic!() };
    assert_eq!((bins, channels), (4, 2), "never more bins than frames");
    let b = plumcraft_store::Store::new(root()).resolve(&r, false).expect("inline");
    let v: Vec<f32> = b.bytes.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect();
    assert_eq!(&v[..8], &[0.0, 0.0, 1.0, 1.0, -1.0, -1.0, 0.5, 0.5], "left: one frame per bin");
    let long = Audio { sample_rate: 48_000, channels: 2, samples: Arc::from(vec![0.1f32; 96_000]) };
    let Some(Rendition::Peaks { data: r, bins, .. }) = data::peaks(&long) else { panic!() };
    assert_eq!(bins, 256);
    assert!(r.meta.size <= data::PEAKS_BYTES as u64);
    assert!(data::peaks(&Audio { sample_rate: 1, channels: 0, samples: Arc::from(Vec::new()) }).is_none());
    // Channels round-trip.
    assert_eq!(a.to_channels()[0], vec![0.0, 1.0, -1.0, 0.5]);
}
