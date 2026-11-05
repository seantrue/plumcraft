//! Render the editor offscreen (no window, no focus stealing, works with the screen locked) on
//! an engine's demo workflow (the text engine's, or the audio engine's with `--engine audio`),
//! run it, and save a PNG. Adapted from the research software's
//! `crates/ui-egui/examples/snapshot.rs`.
//!
//! ```sh
//! cargo run -p plumcraft-ui --example snapshot -- --out claude-debug/editor/shot.png [--size 1600x900] [--scale 2] [--runs N] [--watch NODE] [--palette QUERY] [--theme paper|pewter|dusk] [--engine audio]
//!     [--tidy] [--settings appearance|engines|cache]
//!     [--open WORKFLOW.json] [--command 'ID {"json": "params"}']...
//! ```
//!
//! `--open` opens a workflow file with `graph.open` instead of the demo; each `--command` runs a
//! session command (an edit, say) after it, before the runs. `--tidy` runs Edit › Tidy
//! (`graph.tidy`, as `ui.invoke` does) after the runs, before the fit; `--settings` opens the
//! Settings window on a tab.

use std::sync::Arc;
use std::time::{Duration, Instant};

use plumcraft_engine::host::Host;
use plumcraft_graph::Graph;
use plumcraft_host::{Connection, InProc};
use plumcraft_session::Session;
use plumcraft_ui::EditorApp;
use plumcraft_ui::link::Link;
use serde_json::json;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

/// Step until no run is in flight (or `limit` passes).
fn settle(h: &mut egui_kittest::Harness<'_, EditorApp>, limit: Duration) {
    let start = Instant::now();
    h.run_steps(2);
    while (h.state().session.run.running || !h.state().session.run.outbox_is_empty()) && start.elapsed() < limit {
        std::thread::sleep(Duration::from_millis(40));
        h.step();
    }
    h.run_steps(8);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = arg(&args, "--out").unwrap_or_else(|| "snapshot.png".into());
    let (w, h) = arg(&args, "--size")
        .and_then(|s| s.split_once('x').and_then(|(a, b)| Some((a.parse::<f32>().ok()?, b.parse::<f32>().ok()?))))
        .unwrap_or((1600.0, 900.0));
    let scale: f32 = arg(&args, "--scale").and_then(|s| s.parse().ok()).unwrap_or(2.0);
    let runs: u32 = arg(&args, "--runs").and_then(|s| s.parse().ok()).unwrap_or(1);
    let watch = arg(&args, "--watch");
    let palette = arg(&args, "--palette");
    let tidy = args.iter().any(|a| a == "--tidy");
    let settings = arg(&args, "--settings");
    let theme = arg(&args, "--theme").and_then(|t| plumcraft_ui::theme::by_name(&t));

    let open = arg(&args, "--open");
    let commands: Vec<String> = args.iter().enumerate().filter(|(_, a)| *a == "--command").filter_map(|(i, _)| args.get(i + 1).cloned()).collect();
    let audio = arg(&args, "--engine").as_deref() == Some("audio");
    let engine = move || if audio { plumcraft_audio::engine() } else { plumcraft_text::engine() };
    let demo = if audio { plumcraft_audio::DEMO_WORKFLOW } else { plumcraft_text::DEMO_WORKFLOW };
    let mut workflow: serde_json::Value = serde_json::from_str(demo).expect("demo workflow");
    plumcraft_ui::layout::place_unplaced(&mut workflow);
    let graph = Graph::from_json(&workflow.to_string()).expect("demo graph");
    let mut harness =
        egui_kittest::Harness::builder().with_size(egui::vec2(w, h)).with_pixels_per_point(scale).with_max_steps(64).wgpu().build_eframe(move |cc| {
            let host = Host::start(Box::new(move || Ok(engine())), None);
            host.wait_ready(Duration::from_secs(5));
            let conn: Arc<dyn Connection> = Arc::new(InProc(host));
            let link = Link::start(conn, "Rust · mode 1 · in-process", Some(cc.egui_ctx.clone())).expect("link");
            let mut app = EditorApp::new(Session::with_graph(engine().schema(), graph)).with_link(link);
            app.document = "Demo".into();
            if let Some(path) = &open {
                app.run("graph.open", &json!({"path": path, "discard": true}));
                println!("graph.open {path}: {:?}", app.last_error);
            }
            for c in &commands {
                let (id, params) = c.split_once(' ').unwrap_or((c.as_str(), "null"));
                let params: serde_json::Value = serde_json::from_str(params).expect("--command params are JSON");
                let r = app.run(id, &params);
                println!("{id}: {r:?} {:?}", app.last_error);
            }
            if let Some(t) = theme {
                app.tokens = t;
            }
            // never a sound from a snapshot
            app.audio.player.set_output(Box::new(plumcraft_ui::playback::NullOutput { rate: 48_000, ..Default::default() }));
            app.setup_context(&cc.egui_ctx);
            app
        });
    harness.run_steps(8);
    if let Some(node) = &watch {
        let r = harness.state_mut().session.execute("run.watch", &json!({"node": node}));
        println!("run.watch {node}: {r:?}");
    }
    for _ in 0..runs {
        harness.state_mut().start_run_if_enabled();
        settle(&mut harness, Duration::from_secs(30));
    }
    if tidy {
        harness.state_mut().invoke("graph.tidy");
        harness.run_steps(8);
    }
    // nodes grew with their results: fit them as they are now
    harness.state_mut().canvas.request_fit();
    harness.run_steps(8);
    if let Some(q) = palette {
        let app = harness.state_mut();
        let center = app.canvas.rect.center();
        app.canvas.open_palette(center);
        if let Some(p) = &mut app.canvas.palette {
            p.query = q;
        }
        harness.run_steps(8);
    }
    if let Some(tab) = &settings {
        let r = harness.state_mut().try_run("app.settings", &json!({"tab": tab}));
        println!("app.settings {tab}: {}", r.map(|v| v.to_string()).unwrap_or_else(|e| e));
        harness.run_steps(8);
    }
    let img = harness.render().expect("render");
    let file = std::fs::File::create(&out).expect("create png");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), img.width(), img.height());
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().expect("png header").write_image_data(img.as_raw()).expect("png data");
    println!("wrote {out} ({}×{})", img.width(), img.height());
    println!("{}", harness.state().session.run.summary(Instant::now()));
    println!("title: {}", harness.state().window_title());
}
