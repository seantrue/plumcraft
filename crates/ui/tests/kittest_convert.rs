//! Convert in the editor (docs/research/20-convert.md): a refused wire offers its converters
//! beside the error, and a Convert node shows the path it keeps.

mod common;

use common::add;
use egui_kittest::kittest::Queryable;
use plumcraft_session::Session;
use plumcraft_ui::EditorApp;
use serde_json::json;

/// The text engine's catalogue, plus a WAV source and a converter from WAV to TEXT.
fn session() -> Session {
    let mut c = common::catalog();
    for node in [
        json!({"id": "test.wav", "display": "A WAV", "family": "test", "inputs": [], "outputs": [{"name": "wav", "type": "WAV"}]}),
        json!({"id": "test.transcribe", "display": "Transcribe", "family": "test",
               "inputs": [{"name": "wav", "type": "WAV", "socket": "socket"}], "outputs": [{"name": "text", "type": "TEXT"}],
               "converts": {"from": "WAV", "to": "TEXT"}}),
    ] {
        c.nodes.push(serde_json::from_value(node).unwrap());
    }
    Session::new(c)
}

#[test]
fn a_refused_wire_offers_insert_and_it_places_the_converter() {
    let mut s = session();
    let wav = add(&mut s, "test.wav", 0.0, 0.0);
    let up = add(&mut s, "text.upper", 600.0, 0.0);
    let mut app = EditorApp::new(s);
    // as a drop on the canvas sends it
    let at = json!({"from": {"node": wav, "port": "wav"}, "to": {"node": up, "port": "text"}});
    assert!(app.try_run("wire.connect", &at).is_err());
    assert!(app.convert_offer.is_some());
    let mut h = common::harness(app);
    h.get_by_label("Insert Transcribe").click();
    h.run_steps(3);
    let g = h.state().session.graph();
    assert_eq!((g.nodes.len(), g.wires.len()), (3, 2), "{:?}", g.wires);
    assert!(g.nodes.values().any(|n| n.kind == "test.transcribe"));
    // not lossless: said
    assert!(h.state().last_error.as_deref().is_some_and(|e| e.contains("isn't lossless")), "{:?}", h.state().last_error);
    assert!(h.state().convert_offer.is_none() && h.query_by_label("Insert Transcribe").is_none(), "the offer goes once taken");
}

#[test]
fn a_convert_node_shows_its_path_and_runs_as_its_steps() {
    let mut s = session();
    let wav = add(&mut s, "test.wav", 0.0, 0.0);
    let conv = add(&mut s, plumcraft_proto::convert::KIND, 300.0, 0.0);
    let up = add(&mut s, "text.upper", 600.0, 0.0);
    let mut h = common::harness(EditorApp::new(s));
    h.get_by_label("wire both ends");
    for (from, to) in [((wav.as_str(), "wav"), (conv.as_str(), "value")), ((conv.as_str(), "value"), (up.as_str(), "text"))] {
        let p = json!({"from": {"node": from.0, "port": from.1}, "to": {"node": to.0, "port": to.1}});
        h.state_mut().try_run("wire.connect", &p).unwrap();
    }
    h.run_steps(3);
    h.get_by_label("WAV → TEXT");
    h.get_by_label("a step isn't lossless");
    assert_eq!(h.state().session.graph().convert_pin(&plumcraft_graph::NodeId(conv)), Some(vec!["test.transcribe".to_owned()]));
}

/// A picture of a Convert node and an Insert offer, for a person to look at:
/// `claude-debug/convert/convert-node.png`. `cargo test -p plumcraft-ui --test kittest_convert -- --ignored`
#[test]
#[ignore = "writes a picture for a person to look at"]
fn convert_picture() {
    let mut s = session();
    let wav = add(&mut s, "test.wav", 0.0, 0.0);
    let conv = add(&mut s, plumcraft_proto::convert::KIND, 380.0, 0.0);
    let up = add(&mut s, "text.upper", 760.0, 0.0);
    let wav2 = add(&mut s, "test.wav", 0.0, 260.0);
    let up2 = add(&mut s, "text.upper", 760.0, 260.0);
    for (f, t) in [((wav.as_str(), "wav"), (conv.as_str(), "value")), ((conv.as_str(), "value"), (up.as_str(), "text"))] {
        s.execute("wire.connect", &json!({"from": {"node": f.0, "port": f.1}, "to": {"node": t.0, "port": t.1}})).unwrap();
    }
    let mut app = EditorApp::new(s);
    let _ = app.try_run("wire.connect", &json!({"from": {"node": wav2, "port": "wav"}, "to": {"node": up2, "port": "text"}}));
    let mut h = egui_kittest::Harness::builder().with_size(egui::vec2(1100.0, 520.0)).wgpu().build_ui_state(|ui, app: &mut EditorApp| app.show(ui), app);
    h.state().setup_context(&h.ctx.clone());
    h.run_steps(4);
    h.state_mut().canvas.request_fit();
    h.run_steps(6);
    let img = h.render().unwrap();
    let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../claude-debug/convert");
    std::fs::create_dir_all(&dir).unwrap();
    img.save(dir.join("convert-node.png")).unwrap();
}
