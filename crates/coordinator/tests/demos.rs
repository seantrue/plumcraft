//! The several-engine demos (`apps/plumcraft-editor/demos/`) through a coordinator, as the editor
//! runs them. **Trace and measure** (array → VectorCraft → array): the picture crosses to
//! VectorCraft as a PNG, the traced PNG comes back, and the squared difference is small but not
//! zero; a rerun is cached in both engines. Skipped when VectorCraft can't start, as the craft
//! tests are.

use std::sync::Arc;
use std::time::{Duration, Instant};

use plumcraft_coordinator::Coordinator;
use plumcraft_engine::host::{Host, PollReply};
use plumcraft_host::{Connection, InProc};
use plumcraft_proto::PortValue;
use plumcraft_proto::event::{Event, NodeState};
use plumcraft_proto::run::RunReply;
use plumcraft_proto::session::EngineState;
use serde_json::{Value, json};

fn root() -> std::path::PathBuf {
    let r = plumcraft_scratch::root().join("demos");
    std::fs::create_dir_all(&r).unwrap();
    r
}

fn host(make: Box<dyn FnMut() -> Result<plumcraft_engine::Engine, String> + Send>) -> Option<Arc<dyn Connection>> {
    let h = Host::start(make, None);
    match h.wait_ready(Duration::from_secs(60)) {
        EngineState::Connected { .. } => Some(Arc::new(InProc(h))),
        other => {
            eprintln!("SKIP: an engine didn't start: {other:?}");
            None
        }
    }
}

fn run(c: &Coordinator, id: &str, graph: &Value, targets: &[&str]) -> RunReply {
    c.call("run", json!({"v": 2, "op": "run", "run_id": id, "generation": 1, "graph": {"kind": "inline", "graph": graph}, "targets": targets, "memo": "use"}))
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(180);
    let mut since = 0;
    while Instant::now() < deadline {
        let p: PollReply = serde_json::from_value(c.call("events.poll", json!({"since": since, "wait_ms": 500, "run_id": id})).unwrap()).unwrap();
        since = p.last_seq;
        for e in p.events {
            if let Event::RunReply(r) = e.event {
                return *r;
            }
        }
    }
    panic!("no reply for {id}");
}

fn floats(c: &Coordinator, v: &PortValue) -> Vec<f32> {
    let PortValue::Blob { data } = v else { panic!("{v:?}") };
    let b = c.resolve(data).expect("the value's bytes");
    b.bytes.as_chunks::<4>().0.iter().map(|x| f32::from_le_bytes(*x)).collect()
}

#[test]
fn trace_and_measure() {
    let demo: Value = serde_json::from_str(include_str!("../../../apps/plumcraft-editor/demos/trace-and-measure.json")).unwrap();
    // One store for both, as every engine on a machine shares the commonkit tree. (Engines
    // with different stores need the coordinator to move the bytes: step 6, not built; today a
    // value crossing to such an engine is a `missing_source` node error.)
    let (r, r2) = (root(), root());
    let array = host(Box::new(move || Ok(plumcraft_array::engine().with_store(Some(r.clone())))));
    let vc = host(Box::new(move || {
        plumcraft_craft::manifest::Manifest::named("vectorcraft").and_then(|m| plumcraft_craft::engine(&m)).map(|e| e.with_store(Some(r2.clone())))
    }));
    let (Some(array), Some(vc)) = (array, vc) else { return };
    let c = Coordinator::new().with_engine("array", array).with_engine("vectorcraft", vc);
    let reply = run(&c, "demo1", &demo, &["svg", "lost"]);
    let bad: Vec<String> = reply
        .nodes
        .iter()
        .filter(|(_, n)| !matches!(n.state, NodeState::Done | NodeState::Cached))
        .map(|(k, n)| format!("{k}: {:?} {:?}", n.state, n.error))
        .collect();
    assert!(reply.ok && bad.is_empty(), "{bad:?} {:?}", reply.error);
    assert_eq!(reply.nodes.len(), demo["nodes"].as_object().unwrap().len(), "every node ran");
    // The trace is close to the picture, not the same.
    let lost = floats(&c, &reply.nodes["lost"].outputs["channel"]);
    let mean = lost.iter().sum::<f32>() / lost.len() as f32;
    eprintln!("trace and measure: mean squared error {mean:.5} over {} pixels", lost.len());
    assert!(mean > 0.0 && mean < 0.1, "{mean}");
    // The SVG came out of VectorCraft.
    let PortValue::Blob { data } = &reply.nodes["svg"].outputs["svg"] else { panic!() };
    assert!(String::from_utf8_lossy(&c.resolve(data).unwrap().bytes).contains("<svg"));
    // Again: cached on both sides of both borders.
    let again = run(&c, "demo2", &demo, &["svg", "lost"]);
    let fresh: Vec<&String> = again.nodes.iter().filter(|(_, n)| n.state != NodeState::Cached).map(|(k, _)| k).collect();
    assert!(fresh.is_empty(), "computed again: {fresh:?}");
}

/// **What a chord looks like** (audio, array and text; no craft app): the spectrum crosses from
/// the audio engine into a 96 × 1024 strip in the array engine, and its three highest peaks are
/// the chord's notes; the text's code points are stretched by the array engine.
#[test]
fn what_a_chord_looks_like() {
    let demo: Value = serde_json::from_str(include_str!("../../../apps/plumcraft-editor/demos/chord.json")).unwrap();
    let (r1, r2, r3) = (root(), root(), root());
    let audio = host(Box::new(move || Ok(plumcraft_audio::engine().with_store(Some(r1.clone()))))).unwrap();
    let array = host(Box::new(move || Ok(plumcraft_array::engine().with_store(Some(r2.clone()))))).unwrap();
    let text = host(Box::new(move || Ok(plumcraft_text::engine().with_store(Some(r3.clone()))))).unwrap();
    let c = Coordinator::new().with_engine("audio", audio).with_engine("array", array).with_engine("text", text);
    let reply = run(&c, "chord1", &demo, &["strip", "scaled", "spec"]);
    let bad: Vec<String> = reply
        .nodes
        .iter()
        .filter(|(_, n)| !matches!(n.state, NodeState::Done | NodeState::Cached))
        .map(|(k, n)| format!("{k}: {:?} {:?}", n.state, n.error))
        .collect();
    assert!(reply.ok && bad.is_empty(), "{bad:?} {:?}", reply.error);
    let PortValue::Blob { data } = &reply.nodes["strip"].outputs["resized"] else { panic!() };
    assert!(matches!(&data.meta.form, plumcraft_proto::Form::Array { shape, .. } if shape == &[96, 1024]), "{:?}", data.meta.form);
    // The three notes are the spectrum's three highest peaks: A3, C4 and E4 (bin k is
    // k × 48000 / 8192 Hz, 5.9 Hz apart).
    let spec = floats(&c, &reply.nodes["spec"].outputs["spectrum"]);
    assert_eq!(spec.len(), 8192 / 2 + 1);
    let mut peaks: Vec<usize> = (1..spec.len() - 1).filter(|&k| spec[k] >= spec[k - 1] && spec[k] > spec[k + 1]).collect();
    peaks.sort_by(|a, b| spec[*b].total_cmp(&spec[*a]));
    let mut found: Vec<f32> = peaks.iter().take(3).map(|&k| k as f32 * 48_000.0 / 8192.0).collect();
    found.sort_by(f32::total_cmp);
    eprintln!("chord peaks at {found:?} Hz");
    for (got, want) in found.iter().zip([220.0f32, 261.63, 329.63]) {
        assert!((got - want).abs() < 6.0, "{found:?}");
    }
    let PortValue::Blob { .. } = &reply.nodes["scaled"].outputs["stretched"] else { panic!() };
}

/// **Analytics in GridCraft** (text, array and GridCraft): two tables, made in two engines and
/// turned into rows where they were made (Table to Rows, pinned), written into one workbook and
/// charted; the workbook's values read back hold both, and the PDF is a PDF. Skipped when
/// GridCraft can't start.
#[test]
fn analytics_in_gridcraft() {
    let demo: Value = serde_json::from_str(include_str!("../../../apps/plumcraft-editor/demos/analytics.json")).unwrap();
    let r = root();
    let (r1, r2, r3) = (r.clone(), r.clone(), r);
    let text = host(Box::new(move || Ok(plumcraft_text::engine().with_store(Some(r1.clone())))));
    let array = host(Box::new(move || Ok(plumcraft_array::engine().with_store(Some(r2.clone())))));
    let grid = host(Box::new(move || {
        plumcraft_craft::manifest::Manifest::named("gridcraft").and_then(|m| plumcraft_craft::engine(&m)).map(|e| e.with_store(Some(r3.clone())))
    }));
    let (Some(text), Some(array), Some(grid)) = (text, array, grid) else { return };
    let c = Coordinator::new().with_engine("text", text).with_engine("array", array).with_engine("gridcraft", grid);
    let reply = run(&c, "an1", &demo, &["pdf", "read"]);
    let bad: Vec<String> = reply
        .nodes
        .iter()
        .filter(|(_, n)| !matches!(n.state, NodeState::Done | NodeState::Cached))
        .map(|(k, n)| format!("{k}: {:?} {:?}", n.state, n.error))
        .collect();
    assert!(reply.ok && bad.is_empty(), "{bad:?} {:?}", reply.error);
    let PortValue::Json { value } = &reply.nodes["read"].outputs["values"] else { panic!("{:?}", reply.nodes["read"].outputs) };
    let rows = value.as_array().unwrap();
    eprintln!("analytics: {} rows read back; first {:?}", rows.len(), rows.first());
    assert_eq!(rows[0][0], "letter");
    assert_eq!(rows[0][3].as_str(), Some("centre"), "{:?}", rows[0]);
    assert_eq!(rows.len(), 27, "26 letters and a header");
    let PortValue::Blob { data } = &reply.nodes["pdf"].outputs["pdf"] else { panic!() };
    let pdf = c.resolve(data).unwrap().bytes;
    assert!(pdf.starts_with(b"%PDF"));
    // `PLUMCRAFT_DEMO_OUT=<dir>`: keep the workbook's PDF, to look at (the editor has no PDF view).
    if let Ok(dir) = std::env::var("PLUMCRAFT_DEMO_OUT") {
        std::fs::write(std::path::Path::new(&dir).join("analytics-workbook.pdf"), &pdf).unwrap();
    }
}

/// **Synesthesia** (three crafts in one graph): SoundCraft's bounce crosses as a WAV into the audio
/// engine beside a spoken line, their spectrograms into the array engine, which colours them apart
/// and adds them, the picture to PhotoCraft as a PNG, back to the array engine for its contours,
/// and the drawing to VectorCraft as an SVG, whose export must still be a drawing: a shape per
/// level and no embedded picture. A rerun is cached everywhere; a different tempo makes a
/// different poster.
#[test]
fn synesthesia() {
    let mut demo: Value = serde_json::from_str(include_str!("../../../apps/plumcraft-editor/demos/synesthesia.json")).unwrap();
    let demos = concat!(env!("CARGO_MANIFEST_DIR"), "/../../apps/plumcraft-editor/demos");
    demo["nodes"]["voice"]["values"]["path"] = json!(format!("{demos}/synesthesia-voice.wav"));
    let r = root();
    let craft = |id: &'static str, r: std::path::PathBuf| {
        host(Box::new(move || plumcraft_craft::manifest::Manifest::named(id).and_then(|m| plumcraft_craft::engine(&m)).map(|e| e.with_store(Some(r.clone())))))
    };
    let (r1, r2) = (r.clone(), r.clone());
    let audio = host(Box::new(move || Ok(plumcraft_audio::engine().with_store(Some(r1.clone())))));
    let array = host(Box::new(move || Ok(plumcraft_array::engine().with_store(Some(r2.clone())))));
    let (sound, photo, vector) = (craft("soundcraft", r.clone()), craft("photocraft", r.clone()), craft("vectorcraft", r));
    let (Some(audio), Some(array), Some(sound), Some(photo), Some(vector)) = (audio, array, sound, photo, vector) else { return };
    let c = Coordinator::new()
        .with_engine("soundcraft", sound)
        .with_engine("audio", audio)
        .with_engine("array", array)
        .with_engine("photocraft", photo)
        .with_engine("vectorcraft", vector);
    let reply = run(&c, "syn1", &demo, &["poster", "print", "together"]);
    let bad: Vec<String> = reply
        .nodes
        .iter()
        .filter(|(_, n)| !matches!(n.state, NodeState::Done | NodeState::Cached))
        .map(|(k, n)| format!("{k}: {:?} {:?}", n.state, n.error))
        .collect();
    assert!(reply.ok && bad.is_empty(), "{bad:?} {:?}", reply.error);
    assert_eq!(reply.nodes.len(), demo["nodes"].as_object().unwrap().len(), "every node ran");
    assert_eq!(reply.engine_stats.len(), 5, "each engine's share of the caches");
    let PortValue::Blob { data } = &reply.nodes["poster"].outputs["svg"] else { panic!() };
    let svg = String::from_utf8_lossy(&c.resolve(data).unwrap().bytes).into_owned();
    assert!(svg.contains("<svg") && !svg.contains("<image") && !svg.contains("base64"), "a drawing, not a picture in a wrapper");
    let levels = demo["nodes"]["contours"]["values"]["levels"].as_u64().unwrap() as usize;
    assert_eq!(svg.matches("<path").count(), levels, "a shape per level");
    assert_eq!(svg.matches("url(#").count(), levels, "each with its gradient");
    assert!(svg.len() < 4 << 20, "{} bytes", svg.len());
    let PortValue::Blob { data: print } = &reply.nodes["print"].outputs["image"] else { panic!() };
    let again = run(&c, "syn2", &demo, &["poster", "print", "together"]);
    assert_eq!(again.stats.as_ref().unwrap().computed, 0, "cached across all five engines");
    demo["nodes"]["tempo"]["values"]["bpm"] = json!(150);
    let faster = run(&c, "syn3", &demo, &["print"]);
    let PortValue::Blob { data: other } = &faster.nodes["print"].outputs["image"] else { panic!() };
    assert_ne!(other.handle, print.handle, "the music changed, so the poster did");
}
