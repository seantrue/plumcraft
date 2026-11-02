//! Node headers: the collapse chevron and the actions menu (the research software's `kittest_nodes.rs`,
//! without groups and frames).

mod common;

use common::{add, click, click_in_node, harness, session};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use plumcraft_graph::{NodeId, NodeView};
use plumcraft_ui::EditorApp;

fn view(h: &Harness<'static, EditorApp>, id: &str) -> NodeView {
    h.state().session.graph().node(&NodeId::from(id)).unwrap().view
}

#[test]
fn the_header_menu_offers_the_node_actions() {
    let mut s = session();
    let a = add(&mut s, "text.upper", 200.0, 200.0);
    let mut h = harness(EditorApp::new(s));
    click_in_node(&mut h, "Node actions");
    h.get_by_label("Watch text");
    h.get_by_label("Duplicate");
    click(&mut h, "Remove");
    assert!(h.state().session.graph().node(&NodeId::from(a.as_str())).is_none(), "removed from its header menu");
}

#[test]
fn the_header_menu_watches_an_output() {
    let mut s = session();
    let a = add(&mut s, "text.words", 200.0, 200.0);
    let mut h = harness(EditorApp::new(s));
    click_in_node(&mut h, "Node actions");
    click(&mut h, "Watch count");
    let w = h.state().session.run.watch.first().cloned().unwrap();
    assert_eq!((w.node.as_str(), w.port.as_str()), (a.as_str(), "count"));
    h.get_by_label(&format!("{a}.count"));
    h.get_by_label("not run yet: run to see it");
}

#[test]
fn the_chevron_collapses_and_expands_a_node() {
    let mut s = session();
    let a = add(&mut s, "text.upper", 200.0, 200.0);
    let mut h = harness(EditorApp::new(s));
    click_in_node(&mut h, "Collapse node");
    // (snarl animates the collapse)
    h.run_steps(30);
    assert_eq!(view(&h, &a), NodeView::Collapsed);
    click_in_node(&mut h, "Expand node");
    h.run_steps(3);
    assert_eq!(view(&h, &a), NodeView::Full);
}
