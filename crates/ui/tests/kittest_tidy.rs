//! Edit › Tidy (`graph.tidy`, lifted from the research software) in the editor: the canvas passes the sizes
//! and pins it measured; the result flows left to right with no overlaps, a Vary node sits in
//! the lane beside what it feeds, a second tidy moves nothing, and one undo puts it back. The
//! estimates used without a canvas are held to what the canvas draws.

mod common;

use common::{add, harness, session};
use egui_kittest::Harness;
use plumcraft_graph::NodeId;
use plumcraft_session::layout_cmds::geometry;
use plumcraft_ui::EditorApp;
use serde_json::json;

fn pos(h: &Harness<'static, EditorApp>, id: &str) -> [f32; 2] {
    h.state().session.graph().node(&NodeId::from(id)).unwrap().pos
}

fn wire(s: &mut plumcraft_session::Session, from: (&str, &str), to: (&str, &str)) {
    s.execute("wire.connect", &json!({"from": {"node": from.0, "port": from.1}, "to": {"node": to.0, "port": to.1}})).unwrap();
}

#[test]
fn the_estimates_match_what_the_canvas_draws() {
    let mut s = session();
    let kinds = ["text.literal", "text.upper", "text.replace", "text.repeat", "test.knobs", "text.load", "text.letters", "text.codes", "vary.int"];
    let ids: Vec<String> = kinds.iter().enumerate().map(|(i, k)| add(&mut s, k, 0.0, i as f32 * 300.0)).collect();
    let mut h = harness(EditorApp::new(s));
    h.run_steps(4);
    let (sizes, pins) = h.state().canvas.measured_for_tidy(h.state().session.catalog());
    let (g, catalog) = (h.state().session.graph(), h.state().session.catalog());
    for id in &ids {
        let kind = &g.node(&id.as_str().into()).unwrap().kind;
        let measured = sizes[id].as_array().unwrap().iter().map(|v| v.as_f64().unwrap() as f32).collect::<Vec<_>>();
        let (size, ins, outs) = geometry(g, catalog, &id.as_str().into(), None, None).unwrap();
        assert!((size[1] - measured[1]).abs() <= 6.0, "{kind}: height {} estimated, {} drawn", size[1], measured[1]);
        assert!((size[0] / measured[0] - 1.0).abs() <= 0.25, "{kind}: width {} estimated, {} drawn", size[0], measured[0]);
        for (side, est) in [("inputs", &ins), ("outputs", &outs)] {
            for (port, y) in est {
                let drawn = pins[id][side][port].as_f64().unwrap() as f32;
                assert!((y - drawn).abs() <= 4.0, "{kind}.{port}: pin {y} estimated, {drawn} drawn");
            }
        }
    }
}

#[test]
fn tidy_from_the_menu_lays_out_left_to_right_and_undo_puts_it_back() {
    let mut s = session();
    // a chain scattered right to left, and a Vary Int feeding Repeat's count
    let lit = add(&mut s, "text.literal", 900.0, 500.0);
    let rep = add(&mut s, "text.replace", 600.0, 40.0);
    let up = add(&mut s, "text.upper", 300.0, 700.0);
    let times = add(&mut s, "text.repeat", 0.0, 300.0);
    let vary = add(&mut s, "vary.int", 1200.0, 0.0);
    wire(&mut s, (&lit, "text"), (&rep, "text"));
    wire(&mut s, (&rep, "text"), (&up, "text"));
    wire(&mut s, (&up, "text"), (&times, "text"));
    wire(&mut s, (&vary, "values"), (&times, "times"));
    let mut h = harness(EditorApp::new(s));
    h.run_steps(4);
    let before = h.state().session.graph().clone();

    h.state_mut().invoke("graph.tidy");
    h.run_steps(6);
    let x = |h: &Harness<'static, EditorApp>, id: &str| pos(h, id)[0];
    assert!(x(&h, &lit) < x(&h, &rep) && x(&h, &rep) < x(&h, &up) && x(&h, &up) < x(&h, &times), "left to right");
    // the Vary node is a parameter: in the lane just before Repeat, not in the first column
    assert!(x(&h, &vary) > x(&h, &up) && x(&h, &vary) < x(&h, &times), "vary {} up {} repeat {}", x(&h, &vary), x(&h, &up), x(&h, &times));
    // nothing overlaps, as drawn
    let rects: Vec<(NodeId, egui::Rect)> = h.state().canvas.node_rects_by_id();
    for (i, (a, ra)) in rects.iter().enumerate() {
        for (b, rb) in &rects[i + 1..] {
            assert!(!ra.intersects(*rb), "{a} {ra:?} overlaps {b} {rb:?}");
        }
    }
    // the corner stays
    let corner = |g: &plumcraft_graph::Graph| g.nodes.values().fold([f32::INFINITY; 2], |a, n| [a[0].min(n.pos[0]), a[1].min(n.pos[1])]);
    assert_eq!(corner(h.state().session.graph()), corner(&before));

    // tidying a tidy graph moves nothing
    let once = h.state().session.graph().nodes.clone();
    h.state_mut().invoke("graph.tidy");
    h.run_steps(4);
    assert_eq!(h.state().session.graph().nodes, once);

    // one undo step each
    h.state_mut().invoke("edit.undo");
    h.state_mut().invoke("edit.undo");
    h.run_steps(2);
    assert_eq!(h.state().session.graph().nodes, before.nodes);
}

#[test]
fn a_selection_tidies_alone() {
    let mut s = session();
    let a = add(&mut s, "text.literal", 500.0, 400.0);
    let b = add(&mut s, "text.upper", 0.0, 0.0);
    let c = add(&mut s, "text.lower", 900.0, 900.0);
    wire(&mut s, (&a, "text"), (&b, "text"));
    let mut h = harness(EditorApp::new(s));
    h.run_steps(4);
    let c_was = pos(&h, &c);
    h.state_mut().canvas.picked.set(vec![a.as_str().into(), b.as_str().into()]);
    h.state_mut().invoke("graph.tidy");
    h.run_steps(4);
    assert!(pos(&h, &a)[0] < pos(&h, &b)[0]);
    assert_eq!(pos(&h, &c), c_was, "not selected, not moved");
}

/// The maintainer, 2026-10-30: "Tidy doesn't always leave enough horizontal space". A node's
/// header grew when a run filled in its time, so a graph tidied before a run ended with nodes
/// touching. Headers keep that room now: tidy, run, and every column still has its gap.
#[test]
fn a_run_after_tidy_keeps_the_columns_apart() {
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use plumcraft_engine::host::Host;
    use plumcraft_host::{Connection, InProc};
    use plumcraft_ui::link::Link;

    let mut s = session();
    let a = add(&mut s, "text.literal", 0.0, 0.0);
    let u = add(&mut s, "text.upper", 50.0, 0.0);
    let w = add(&mut s, "text.words", 100.0, 0.0);
    wire(&mut s, (&a, "text"), (&u, "text"));
    wire(&mut s, (&u, "text"), (&w, "text"));
    let host = Host::start(Box::new(|| Ok(plumcraft_text::engine())), None);
    host.wait_ready(Duration::from_secs(5));
    let conn: Arc<dyn Connection> = Arc::new(InProc(host));
    let mut h = harness(EditorApp::new(s).with_link(Link::start(conn, "in-process", None).unwrap()));
    h.state_mut().invoke("graph.tidy");
    h.run_steps(4);
    h.state_mut().start_run_if_enabled();
    let t = Instant::now();
    while (h.state().session.run.running || !h.state().session.run.outbox_is_empty()) && t.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(20));
        h.run_steps(1);
    }
    h.run_steps(4);
    let rects: std::collections::BTreeMap<String, egui::Rect> = h.state().canvas.node_rects_by_id().into_iter().map(|(id, r)| (id.0, r)).collect();
    for (left, right) in [(&a, &u), (&u, &w)] {
        let gap = rects[right].min.x - rects[left].max.x;
        assert!(gap >= 40.0, "{left} -> {right}: {gap} apart after the run ({:?} {:?})", rects[left], rects[right]);
    }
}
