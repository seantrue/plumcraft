//! Every value can be looked at (spec 10.1): run each of PlumCraft's engines' demos with every
//! output of every node watched, and each one must offer a viewer other than the hex dump.

use std::collections::BTreeSet;
use std::time::Duration;

use plumcraft_engine::Engine;
use plumcraft_engine::host::Host;
use plumcraft_proto::catalogue::SchemaReply;
use plumcraft_proto::display::WatchRequest;
use plumcraft_proto::run::RunRequest;
use plumcraft_proto::session::EngineState;
use plumcraft_ui::viewers::viewable;
use serde_json::Value;

fn all_outputs_viewable(name: &str, engine: fn() -> Engine, demo: &str) -> BTreeSet<String> {
    let root = plumcraft_scratch::dir("no-hex");
    let h = Host::start(Box::new(move || Ok(engine().with_store(Some(root.clone())))), None);
    assert!(matches!(h.wait_ready(Duration::from_secs(10)), EngineState::Connected { .. }));
    let schema: SchemaReply = serde_json::from_value(h.schema(None).unwrap()).unwrap();
    let graph: Value = serde_json::from_str(demo).unwrap();
    let mut req = RunRequest::new(name, 1, graph.clone());
    let mut types = Vec::new();
    for (id, n) in graph["nodes"].as_object().unwrap() {
        let Some(def) = schema.node(n["kind"].as_str().unwrap()) else { continue };
        for p in &def.outputs {
            req.watch.push(WatchRequest { node: id.clone(), port: p.name.clone(), ..Default::default() });
            types.push(p.ty.clone());
        }
    }
    assert!(!req.watch.is_empty(), "{name}'s demo watches nothing");
    h.run(req).unwrap();
    let mut reply = None;
    for _ in 0..3000 {
        reply = h.get(name).unwrap().reply;
        if reply.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let reply = reply.expect("the demo finished");
    assert_eq!(reply.watch.len(), types.len(), "every watch answered");
    let mut bad = Vec::new();
    for (d, ty) in reply.watch.iter().zip(&types) {
        if d.error.is_none() && !viewable(d) {
            bad.push(format!("{}.{} ({ty}): {:?}, {:?}", d.node, d.port, d.media, d.summary));
        }
    }
    assert!(bad.is_empty(), "{name}: only hex for {bad:#?}");
    types.into_iter().collect()
}

#[test]
fn bytes_with_no_picture_are_not_viewable() {
    use plumcraft_proto::catalogue::Media;
    use plumcraft_proto::data::{BlobMeta, DataRef};
    use plumcraft_proto::display::Display;
    let full = Some(DataRef { handle: None, meta: BlobMeta::default(), at: Vec::new() });
    assert!(!viewable(&Display { media: Media::Bytes, full, ..Default::default() }));
    assert!(viewable(&Display { media: Media::Scalar, value: Some(1.into()), ..Default::default() }));
}

#[test]
fn every_output_of_the_text_demo_has_a_viewer() {
    all_outputs_viewable("text", plumcraft_text::engine, plumcraft_text::DEMO_WORKFLOW);
}

#[test]
fn every_output_of_the_audio_demo_has_a_viewer() {
    all_outputs_viewable("audio", plumcraft_audio::engine, plumcraft_audio::DEMO_WORKFLOW);
}

#[test]
fn every_output_of_the_array_demo_has_a_viewer() {
    all_outputs_viewable("array", plumcraft_array::engine, plumcraft_array::DEMO_WORKFLOW);
}
