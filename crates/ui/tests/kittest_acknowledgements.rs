//! FR4: the Acknowledgements window, from Help and from About: the editor's licences, sectioned
//! by licence, searched by the core, closed by its command.

mod common;

use common::{catalog, harness_pictured, shot};
use egui_kittest::kittest::Queryable;
use plumcraft_session::{About, Session};
use plumcraft_ui::EditorApp;

#[test]
fn acknowledgements_open_from_about_and_from_help_and_search() {
    let mut app = EditorApp::new(Session::new(catalog()));
    app.session.about = About { title: "PlumThing".into(), version: "1.2.3".into(), parts: Vec::new(), build: None };
    let mut h = harness_pictured(app, egui::vec2(1100.0, 800.0));
    h.get_by_label("Help").click();
    h.run_steps(2);
    h.get_by_label("About").click();
    h.run_steps(4);
    h.get_by_label("Acknowledgements…").click();
    h.run_steps(4);
    assert!(h.state().session.views.acknowledgements);
    h.get_by_label("Acknowledgements");
    assert_eq!(h.get_all_by_label("PlumThing").count(), 2, "About's heading and the list's");
    h.get_by_label_contains(" packages");
    h.get_by_label_contains("MPL-2.0: symphonia");
    shot(&mut h, "fr4-egui-acknowledgements");
    // the core's search: only the licences a package's name matches
    let at = h.get_all_by_role(egui::accesskit::Role::TextInput).last().map(|n| n.rect().center()).unwrap();
    common::click_at(&mut h, egui::pos2(at.x as f32, at.y as f32));
    h.event(egui::Event::Text("symphonia".into()));
    h.run_steps(4);
    h.get_by_label_contains("MPL-2.0: symphonia");
    assert!(h.query_by_label_contains("Apache-2.0: ").is_none(), "only the licences symphonia is under");
    h.get_by_label_contains(" of ");
    h.get_by_label_contains("MPL-2.0: symphonia").click();
    h.run_steps(4);
    h.get_by_label_contains("Mozilla Public License Version 2.0");
    shot(&mut h, "fr4-egui-acknowledgements-search");
    // one row per licence: Apache-2.0's text once, each package's own copyright line under it
    common::click_at(&mut h, egui::pos2(at.x as f32, at.y as f32));
    h.key_press(egui::Key::End);
    for _ in 0.."symphonia".len() {
        h.key_press(egui::Key::Backspace);
    }
    h.event(egui::Event::Text("zopfli".into()));
    h.run_steps(4);
    h.get_by_label_contains("Apache-2.0: ").click();
    h.run_steps(4);
    shot(&mut h, "fr4-egui-acknowledgements-apache");
    h.get_by_label("Copyright 2011 Google Inc.");
    assert_eq!(h.get_all_by_label_contains("TERMS AND CONDITIONS FOR USE").count(), 1, "the text once");
    // B175: a crate under two licences at once (brotli: BSD-3-Clause AND MIT) is in both rows,
    // each saying where else it is
    common::click_at(&mut h, egui::pos2(at.x as f32, at.y as f32));
    h.key_press(egui::Key::End);
    for _ in 0.."zopfli".len() {
        h.key_press(egui::Key::Backspace);
    }
    h.event(egui::Event::Text("brotli".into()));
    h.run_steps(4);
    h.get_by_label_contains("BSD-3-Clause: ").click();
    h.run_steps(4);
    h.get_by_label_contains("MIT: ").click();
    h.run_steps(4);
    shot(&mut h, "b175-egui-also-under");
    h.get_by_label_contains("(also under MIT)");
    h.get_by_label_contains("(also under BSD-3-Clause)");
    h.state_mut().run("app.acknowledgements", &serde_json::json!({"open": false}));
    h.run_steps(4);
    assert!(h.query_by_label("Acknowledgements").is_none());
    // Help › Acknowledgements opens it again
    h.get_by_label("Help").click();
    h.run_steps(2);
    h.get_by_label("Acknowledgements").click();
    h.run_steps(4);
    assert!(h.state().session.views.acknowledgements);
    // About is still open behind it: the last Close is the window drawn last, Acknowledgements'
    h.get_all_by_label("Close").last().unwrap().click();
    h.run_steps(4);
    assert!(!h.state().session.views.acknowledgements);
    assert!(h.state().session.views.about, "About stays open");
}
