//! The SoundCraft pack (`packs/soundcraft.toml`): it parses, its demo uses only its nodes, and,
//! when `soundcraft-cli` is on PATH, its curated nodes all pass the catalogue check and the
//! demo's chain (new → instrument track → notes → transpose → reverb → bounce WAV) makes a
//! non-silent WAV, warm and after a cold reopen from the CAS.

mod common;

use std::time::Duration;

use common::*;
use plumcraft_craft::manifest::{Kind, builtin, demo};
use plumcraft_proto::data::Form;
use serde_json::{Value, json};

const CURATED: &[&str] = &[
    "sc.new",
    "sc.inspect",
    "sc.instrument_track",
    "sc.notes",
    "sc.transpose",
    "sc.quantize",
    "sc.velocity",
    "sc.tempo",
    "sc.meter",
    "sc.marker",
    "sc.volume",
    "sc.pan",
    "sc.insert",
    "sc.reverb",
    "sc.delay",
    "sc.compressor",
    "sc.export_wav",
    "sc.export_flac",
    "sc.export_aiff",
    "sc.export_midi",
    "sc.export_musicxml",
];

#[test]
fn the_soundcraft_pack_parses_and_its_demo_uses_its_nodes() {
    let m = builtin("soundcraft").unwrap().unwrap();
    let ids: Vec<&str> = m.nodes.iter().map(|n| n.id.as_str()).collect();
    assert_eq!(ids, CURATED);
    assert!(m.nodes.iter().any(|n| n.kind == Kind::New));
    assert!(m.nodes.iter().filter(|n| n.kind == Kind::Export).all(|n| (n.format.is_some() || n.command.is_some()) && n.ext.is_some()));
    assert_eq!(m.actions.export.as_ref().and_then(|a| a.tool.as_deref()), Some("bounce_mix"));
    assert_eq!(m.gui.as_ref().and_then(|g| g.env.as_deref()), Some("PLUMCRAFT_SOUNDCRAFT_APP"));
    let d: Value = serde_json::from_str(demo("soundcraft").unwrap()).unwrap();
    for (name, n) in d["nodes"].as_object().unwrap() {
        let kind = n["kind"].as_str().unwrap();
        assert!(ids.contains(&kind), "demo node {name} is {kind}");
    }
}

fn wav_peak(h: &Conn, r: &plumcraft_proto::run::RunReply, node: &str) -> f64 {
    let wav = blob(r, node, "audio");
    assert_eq!(wav.meta.media, "audio/wav");
    assert!(wav.meta.size > 44_100, "a second of audio at least: {} bytes", wav.meta.size);
    let bytes = h.resolve(&wav).expect("the WAV resolves").bytes;
    assert_eq!(bytes.len() as u64, wav.meta.size);
    assert_eq!((&bytes[0..4], &bytes[8..12]), (&b"RIFF"[..], &b"WAVE"[..]));
    let res = json_out(r, node, "result");
    res["peak_db"].as_f64().unwrap_or_else(|| panic!("no peak_db: {res}"))
}

#[test]
fn the_soundcraft_demo_chain_bounces_a_wav() {
    let Some(m) = soundcraft() else { return };
    let root = craft_root("pack-soundcraft");
    let opts = plumcraft_craft::Options { root: Some(root), store: Some(Some(store_root())), include_excluded: false };
    let (engine, report, _) = plumcraft_craft::build(&m, &opts).unwrap();
    eprintln!("SoundCraft {}: nodes {:?}; hatch {:?}; dropped {:?}", report.server_version, report.curated, report.hatch, report.dropped);
    assert!(report.dropped.is_empty(), "{:?}", report.dropped);
    assert_eq!(report.curated, CURATED);
    assert_eq!(report.hatch, ["sc.run"]);
    let mut once = Some(engine);
    let h = plumcraft_engine::host::Host::start(Box::new(move || once.take().ok_or_else(|| "built once".to_owned())), None);
    let _ = h.wait_ready(Duration::from_secs(30));
    let h: Conn = Box::new(plumcraft_host::InProc(h));

    let steps = |wav: Value| {
        chain(&[
            ("n", "sc.new", json!({"sample_rate": 48000})),
            ("t", "sc.instrument_track", json!({"name": "Synth", "instrument": "subtractive_synth"})),
            ("m", "sc.notes", json!({"track": "Synth"})),
            ("x", "sc.transpose", json!({"semitones": 5})),
            ("r", "sc.reverb", json!({"track": "Synth", "plugin": "plate_reverb", "mix": 35.0})),
            ("w", "sc.export_wav", wav),
        ])
    };
    // Warm: one process holds the session from New to the bounce.
    let r = go(&h, "sc-warm", steps(json!({})), &["t", "r", "w"], "use");
    assert!(r.ok, "{:?}", states(&r));
    let doc = blob(&r, "r", "doc");
    assert_eq!(doc.meta.form, Form::Doc { app: "soundcraft".into() });
    let warm = wav_peak(&h, &r, "w");
    assert!(warm > -60.0 && warm <= 0.0, "the synth is audible: peak {warm} dBFS");
    assert_eq!(json_out(&r, "t", "tracks"), json!([1]));

    // The session says what the chain did.
    let q = go(&h, "sc-inspect", chain(&[("n", "sc.new", json!({"sample_rate": 48000})), ("i", "sc.inspect", json!({}))]), &["i"], "off");
    assert!(q.ok, "{:?}", states(&q));
    assert_eq!(json_out(&q, "i", "result")["sample_rate"], 48000);

    // Cold: the process now holds another session, so the bounce reopens the cached reverb
    // session from the CAS (its MIDI and plug-ins live in the .scraft) and still sounds.
    let other = go(&h, "sc-other", chain(&[("n", "sc.new", json!({"sample_rate": 44100}))]), &["n"], "off");
    assert!(other.ok);
    let cold = go(&h, "sc-cold", steps(json!({"bit_depth": "16"})), &["r", "w"], "use");
    assert!(cold.ok, "{:?}", states(&cold));
    assert_eq!(blob(&cold, "r", "doc").handle, doc.handle, "upstream came from the memo");
    let peak = wav_peak(&h, &cold, "w");
    assert!((peak - warm).abs() < 1.0, "the cold bounce matches the warm one: {peak} vs {warm}");
}

/// Every other curated node and the escape hatch, with their defaults or a value each, in one
/// chain bounced to FLAC and AIFF.
#[test]
fn every_soundcraft_node_runs() {
    let Some(m) = soundcraft() else { return };
    let root = craft_root("pack-soundcraft-all");
    let opts = plumcraft_craft::Options { root: Some(root), store: Some(Some(store_root())), include_excluded: false };
    let (engine, _, _) = plumcraft_craft::build(&m, &opts).unwrap();
    let mut once = Some(engine);
    let h = plumcraft_engine::host::Host::start(Box::new(move || once.take().ok_or_else(|| "built once".to_owned())), None);
    let _ = h.wait_ready(Duration::from_secs(30));
    let h: Conn = Box::new(plumcraft_host::InProc(h));
    let steps: Vec<(&str, &str, Value)> = vec![
        ("a", "sc.new", json!({"sample_rate": 44100})),
        ("b", "sc.instrument_track", json!({})),
        ("c", "sc.notes", json!({})),
        ("d", "sc.quantize", json!({"grid": "1/8"})),
        ("e", "sc.velocity", json!({"scale": 80.0})),
        ("f", "sc.tempo", json!({"bpm": 90.0})),
        ("g", "sc.meter", json!({"numerator": 3})),
        ("h", "sc.marker", json!({"name": "Intro"})),
        ("i", "sc.volume", json!({"db": -3.0})),
        ("j", "sc.pan", json!({"pan": -0.3})),
        ("k", "sc.insert", json!({"plugin": "chorus"})),
        ("l", "sc.delay", json!({"mix": 20.0})),
        ("m", "sc.compressor", json!({"threshold": -20.0})),
        ("n", "sc.run", json!({"command": "mix.mute", "params": {"tracks": ["Synth"], "value": false}})),
        ("o", "sc.export_flac", json!({})),
    ];
    let mut g = chain(&steps);
    g["nodes"]["p"] = json!({"kind": "sc.export_aiff", "values": {"bit_depth": "16"}});
    g["nodes"]["q"] = json!({"kind": "sc.inspect", "values": {}});
    g["nodes"]["mid"] = json!({"kind": "sc.export_midi", "values": {}});
    g["nodes"]["xml"] = json!({"kind": "sc.export_musicxml", "values": {"tracks": ["Synth"]}});
    let wires = g["wires"].as_array_mut().unwrap();
    wires.push(json!({"from": {"node": "n", "port": "doc"}, "to": {"node": "p", "port": "doc"}}));
    wires.push(json!({"from": {"node": "n", "port": "doc"}, "to": {"node": "q", "port": "doc"}}));
    wires.push(json!({"from": {"node": "n", "port": "doc"}, "to": {"node": "mid", "port": "doc"}}));
    wires.push(json!({"from": {"node": "n", "port": "doc"}, "to": {"node": "xml", "port": "doc"}}));
    let r = go(&h, "sc-all", g, &["o", "p", "q", "mid", "xml"], "off");
    assert!(r.ok, "{:?}", r.nodes.iter().map(|(k, n)| (k, n.state, n.error.clone())).collect::<Vec<_>>());
    let flac = h.resolve(&blob(&r, "o", "audio")).unwrap().bytes;
    assert_eq!(&flac[0..4], b"fLaC");
    let aiff = h.resolve(&blob(&r, "p", "audio")).unwrap().bytes;
    assert_eq!((&aiff[0..4], &aiff[8..12]), (&b"FORM"[..], &b"AIFF"[..]));
    let mid = h.resolve(&blob(&r, "mid", "midi")).unwrap().bytes;
    assert_eq!(&mid[0..4], b"MThd", "a Standard MIDI File");
    assert!(mid.windows(4).any(|w| w == b"MTrk"));
    let xml = String::from_utf8(h.resolve(&blob(&r, "xml", "score")).unwrap().bytes.into_vec()).unwrap();
    assert!(xml.contains("<score-partwise") && xml.contains("<note"), "{}", &xml[..xml.len().min(400)]);
    let s = json_out(&r, "q", "result");
    let t = &s["tracks"][0];
    assert_eq!(t["volume_db"], -3.0);
    let plugins: Vec<&str> = t["inserts"].as_array().unwrap().iter().filter_map(|i| i["plugin"].as_str()).collect();
    assert_eq!(plugins, ["chorus", "mod_delay", "compressor"]);
    assert_eq!(s["tempo"][0]["bpm"], 90.0);
    assert!(s["markers"].as_array().unwrap().iter().any(|mk| mk["name"] == "Intro"), "{}", s["markers"]);
}

/// Every export is a converter from the document (docs/research/20-convert.md), so a SoundCraft
/// session reaches Audio in two steps: Bounce WAV, then WAV to Audio.
#[test]
fn exports_are_converters_from_the_document() {
    let m = builtin("soundcraft").unwrap().unwrap();
    if m.executable().is_none() {
        eprintln!("SKIP: soundcraft-cli is not on PATH; {}", m.hint);
        return;
    }
    let opts = plumcraft_craft::Options { root: Some(craft_root("soundcraft")), store: Some(Some(store_root())), include_excluded: false };
    let (engine, _, _) = plumcraft_craft::build(&m, &opts).unwrap();
    let cat = engine.schema();
    let wav = cat.converters().into_iter().find(|c| c.kind == "sc.export_wav").expect("Bounce WAV converts");
    assert_eq!((wav.from, wav.to, wav.input, wav.output, wav.lossless), ("soundcraft.session", "WAV", "doc", "audio", false));
    let exports = cat.nodes.iter().filter(|n| n.id.starts_with("sc.export_")).count();
    assert_eq!(cat.converters().len(), exports, "every export, and nothing else");
}

/// Insert plug-in's choice brings that plug-in's parameters (`brings`, from `engine.plugins`):
/// fields shown only while it is chosen, a toggle as a checkbox, a choice by name, and only the
/// chosen plug-in's values sent.
#[test]
fn a_plugin_choice_brings_its_parameters() {
    let Some(m) = soundcraft() else { return };
    let root = craft_root("pack-soundcraft-brings");
    let opts = plumcraft_craft::Options { root: Some(root), store: Some(Some(store_root())), include_excluded: false };
    let (engine, _, _) = plumcraft_craft::build(&m, &opts).unwrap();
    let def = engine.schema().nodes.into_iter().find(|n| n.id == "sc.insert").unwrap();
    let port = |name: &str| def.inputs.iter().find(|p| p.name == name).cloned().unwrap_or_else(|| panic!("no {name}"));
    let threshold = port("compressor_threshold");
    assert_eq!((threshold.ty.as_str(), threshold.default.clone(), threshold.label.as_deref()), ("FLOAT", json!(-20.0), Some("Threshold (dB)")));
    assert_eq!(threshold.active_when, Some([("plugin".to_owned(), vec![json!("compressor")])].into_iter().collect()));
    assert_eq!(port("compressor_sc_hpf_on").ty, "BOOL", "a Toggle is a checkbox");
    let eq_type = port("eq_1band_type");
    assert_eq!((eq_type.ty.as_str(), eq_type.default.clone()), ("ENUM", json!("Peak")), "a choice by name");
    assert!(def.inputs.iter().all(|p| p.name != "slot"), "slot stays hidden");
    let mut once = Some(engine);
    let h = plumcraft_engine::host::Host::start(Box::new(move || once.take().ok_or_else(|| "built once".to_owned())), None);
    let _ = h.wait_ready(Duration::from_secs(30));
    let h: Conn = Box::new(plumcraft_host::InProc(h));
    let steps: Vec<(&str, &str, Value)> = vec![
        ("n", "sc.new", json!({"sample_rate": 48000})),
        ("t", "sc.instrument_track", json!({"name": "Synth", "instrument": "subtractive_synth"})),
        ("c", "sc.insert", json!({"track": "Synth", "plugin": "compressor", "compressor_threshold": -30.0, "compressor_sc_hpf_on": true})),
        ("e", "sc.insert", json!({"track": "Synth", "plugin": "eq_1band", "eq_1band_type": "High Pass", "compressor_threshold": -5.0})),
        ("i", "sc.inspect", json!({"detail": "full"})),
    ];
    let r = go(&h, "sc-brings", chain(&steps), &["i"], "off");
    assert!(r.ok, "{:?}", r.nodes.iter().map(|(k, n)| (k, n.state, n.error.clone())).collect::<Vec<_>>());
    let s = json_out(&r, "i", "result");
    let synth = s["tracks"].as_array().unwrap().iter().find(|t| t["name"] == "Synth").unwrap();
    let insert = |plugin: &str| synth["inserts"].as_array().unwrap().iter().find(|i| i["plugin"] == plugin).cloned().unwrap();
    let comp = insert("compressor");
    assert_eq!((comp["params"]["threshold"].clone(), comp["params"]["sc_hpf_on"].clone()), (json!(-30.0), json!(1.0)), "{comp}");
    // the EQ's own choice went as its index; the compressor value on the EQ node wasn't sent
    assert_eq!(insert("eq_1band")["params"]["type"], json!(3.0), "High Pass is choice 3");
}
