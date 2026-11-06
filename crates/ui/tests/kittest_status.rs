//! The status bar's messages: a chip each, its background by severity and its text in the
//! ordinary colour; one cut short shows "more…", and a click opens the Messages window with every
//! message in full.

mod common;

use common::{harness, session};
use egui_kittest::kittest::Queryable;
use plumcraft_session::commands::Tone;
use plumcraft_ui::EditorApp;
use plumcraft_ui::status_msgs::{self, MORE, TITLE};
use plumcraft_ui::theme::mix;

const TAIL: &str = "and this is the very end of it";

fn long() -> String {
    format!("the engine said something long: {} {TAIL}", "a word or two, ".repeat(30))
}

#[test]
fn a_short_message_shows_whole_with_no_more_button() {
    let mut app = EditorApp::new(session());
    app.last_error = Some("select nodes first".into());
    let h = harness(app);
    assert!(h.query_by_label("select nodes first").is_some());
    assert!(h.query_all_by_label_contains(MORE).next().is_none());
}

#[test]
fn a_long_message_is_cut_short_and_a_click_shows_it_in_full() {
    let mut app = EditorApp::new(session());
    app.last_error = Some(long());
    let mut h = harness(app);
    assert!(h.query_all_by_label_contains(MORE).next().is_some(), "the bar says something isn't shown");
    assert!(h.query_by_label(TITLE).is_none(), "no window yet");
    h.get_all_by_label_contains(MORE).next().unwrap().click();
    h.run_steps(4);
    assert!(h.state().session.views.messages);
    assert!(h.query_by_label(TITLE).is_some(), "the Messages window");
    // the bar's chip and the window's whole message
    assert!(h.query_all_by_label_contains(TAIL).count() >= 2, "the whole message, in the window");
    // nothing left to show closes it
    h.state_mut().last_error = None;
    h.run_steps(4);
    assert!(!h.state().session.views.messages);
}

#[test]
fn clicking_a_message_opens_the_window_too() {
    let mut app = EditorApp::new(session());
    app.last_error = Some("select nodes first".into());
    let mut h = harness(app);
    h.get_by_label("select nodes first").click();
    h.run_steps(4);
    assert!(h.state().session.views.messages);
}

#[test]
fn the_window_is_a_command() {
    let mut app = EditorApp::new(session());
    app.last_error = Some("select nodes first".into());
    let mut h = harness(app);
    h.state_mut().try_run("view.messages", &serde_json::json!({"open": true})).unwrap();
    h.run_steps(4);
    assert!(h.query_by_label(TITLE).is_some());
    h.state_mut().try_run("view.messages", &serde_json::json!({})).unwrap();
    h.run_steps(4);
    assert!(h.query_by_label(TITLE).is_none(), "toggled shut");
}

#[test]
fn the_most_serious_comes_first() {
    use status_msgs::{Msg, by_severity};
    let msgs = [Msg::new(Tone::Info, "a"), Msg::new(Tone::Error, "b"), Msg::new(Tone::Warn, "c"), Msg::new(Tone::Error, "d")];
    let order: Vec<&str> = by_severity(&msgs).iter().map(|m| m.text.as_str()).collect();
    assert_eq!(order, ["b", "d", "c", "a"]);
}

#[test]
fn backgrounds_differ_by_severity_and_text_keeps_its_colour() {
    let app = EditorApp::new(session());
    let t = &app.tokens;
    let (i, w, e) = (status_msgs::background(Tone::Info, t), status_msgs::background(Tone::Warn, t), status_msgs::background(Tone::Error, t));
    assert!(i != w && w != e && i != e);
    assert_eq!(e, mix(t.panel, t.error, 0.28));
    // the messages carry their severity; the run's and the editor's errors are errors
    let mut app = app;
    app.last_error = Some("x".into());
    assert_eq!(app.status_messages()[0].tone, Tone::Error);
}
