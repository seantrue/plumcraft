//! What the caches did, on screen (crate::cache_view): the status line's summary and warning, and
//! the Cache Report from Settings › Cache and from the summary.

mod common;

use common::{add, harness, session};
use egui_kittest::kittest::Queryable;
use plumcraft_proto::run::{RunReply, RunStats};
use plumcraft_ui::EditorApp;
use serde_json::json;
use std::time::Instant;

fn report(h: &mut egui_kittest::Harness<'static, EditorApp>, stats: RunStats) {
    let run = &mut h.state_mut().session.run;
    let req = run.request(&Default::default(), Vec::new(), Vec::new(), Instant::now());
    let reply = RunReply { run_id: req.run_id, generation: req.generation, stats: Some(stats), ..Default::default() };
    assert!(run.apply_reply(&reply, Instant::now()));
    h.run_steps(3);
}

#[test]
fn the_status_line_summarises_and_opens_the_report() {
    let mut s = session();
    add(&mut s, "text.literal", 0.0, 0.0);
    let mut h = harness(EditorApp::new(s));
    let mut stats = RunStats { computed: 3, from_memory: 8, from_store: 3, from_content: 1, saved_ms: 3200.0, ..Default::default() };
    stats.store.insert("shm_declined_space".into(), 2);
    report(&mut h, stats);
    assert!(h.query_by_label("store refused 2 writes").is_some(), "the warning shows");
    h.get_by_label("cached 12 of 15 · saved 3.20 s").click();
    h.run_steps(4);
    assert!(h.state().session.views.cache_report, "the summary opens the report");
    assert!(h.query_by_label("Latest run").is_some());
    assert!(h.query_by_label("This session, 1 runs").is_some());
    assert_eq!(h.query_all_by_label("By content").count(), 2, "the run and the session");
    let _ = h.state_mut().try_run("run.cacheReport", &json!({"open": false}));
    h.run_steps(3);
    assert!(h.query_by_label("Latest run").is_none());
}
