//! `plumcraft-view`: a minimal window onto the interface. It runs a workflow on the text engine,
//! in-process (mode 1) or as a separate process (mode 2), written in Rust or in Python, and shows
//! what crosses the interface
//! live: each node's state, progress, time, error and thumbnail, the watched value as it arrives,
//! and the event log. Widgets come only from the engine's catalogue.
//!
//! ```text
//! plumcraft-view [WORKFLOW.json] [--process | --python | --http [--python]] [--watch NODE.PORT] [--screenshot OUT.png [--shot-runs N]]
//! ```
//!
//! `--process` starts the Rust engine binary; `--python` the Python engine (`python/`), with
//! `uv run`. The Engine menu switches between all three while running.
//!
//! `--screenshot` runs the workflow once (or `--shot-runs N` times, to see the memo's `cached`
//! cards), saves the window after the last reply, and exits (for
//! checking the UI without a person).
//!
//! Colours and sizes are hard-coded here for now; they move to theme tokens when this window
//! becomes the editor (`ROADMAP.md`, "Externalize colours and theme materials").

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use egui::{Color32, RichText};
use plumcraft_engine::host::{Host, PollReply};
use plumcraft_host::http::{HttpHost, HttpOptions};
use plumcraft_host::process::{Manifest, ProcessHost};
use plumcraft_host::{Connection, InProc};
use plumcraft_proto::catalogue::{Media, NodeDef, SchemaReply};
use plumcraft_proto::data::Locator;
use plumcraft_proto::display::{Display, Rendition};
use plumcraft_proto::event::{Envelope, Event, NodeState};
use plumcraft_proto::reducer::{NodeView, Reducer};
use plumcraft_proto::run::RunReply;
use plumcraft_proto::session::EngineState;
use plumcraft_store::Blob;
use serde_json::{Value, json};

const LOG_LINES: usize = 400;
const CARD_W: f32 = 230.0;

/// Events pulled off the connection by the pump thread, waiting for the next frame.
#[derive(Default)]
struct Inbox {
    events: Vec<Envelope>,
}

/// A live connection and the thread pumping its events.
struct Link {
    conn: Arc<dyn Connection>,
    process: Option<Arc<ProcessHost>>,
    inbox: Arc<Mutex<Inbox>>,
    stop: Arc<AtomicBool>,
}

impl Drop for Link {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

/// Which engine, hosted how.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// Mode 1: the Rust engine compiled in.
    InProc,
    /// Mode 2: the Rust engine binary as a separate process.
    Rust,
    /// Mode 2: the same engine written in Python, as a separate process.
    Python,
    /// Mode 3: the Rust engine over HTTP, talked to as a UI on another machine would.
    HttpRust,
    /// Mode 3: the Python engine over HTTP, likewise.
    HttpPython,
}

impl Mode {
    const ALL: [Mode; 5] = [Mode::InProc, Mode::Rust, Mode::Python, Mode::HttpRust, Mode::HttpPython];

    fn label(self) -> &'static str {
        match self {
            Mode::InProc => "Rust, in-process (mode 1)",
            Mode::Rust => "Rust, separate process (mode 2)",
            Mode::Python => "Python, separate process (mode 2)",
            Mode::HttpRust => "Rust over HTTP, remote (mode 3)",
            Mode::HttpPython => "Python over HTTP, remote (mode 3)",
        }
    }

    fn python(self) -> bool {
        matches!(self, Mode::Python | Mode::HttpPython)
    }
}

fn connect(mode: Mode, ctx: &egui::Context) -> Result<Link, String> {
    let manifest = match mode {
        Mode::InProc => None,
        Mode::Rust | Mode::HttpRust => {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?.with_file_name("plumcraft-text-engine");
            Some(Manifest {
                name: "text".into(),
                command: vec![exe.display().to_string()],
                env: Vec::new(),
                hint: "cargo build -p plumcraft-text".into(),
                store: None,
                cwd: None,
            })
        }
        Mode::Python | Mode::HttpPython => {
            let (command, env) = plumcraft_text::python_command();
            Some(Manifest {
                name: "text (Python)".into(),
                command,
                env,
                hint: "needs uv (cd python && uv sync), or set PLUMCRAFT_UV".into(),
                store: None,
                cwd: None,
            })
        }
    };
    let (conn, proc_host): (Arc<dyn Connection>, Option<Arc<ProcessHost>>) =
        if let (Some(m), true) = (&manifest, matches!(mode, Mode::HttpRust | Mode::HttpPython)) {
            let h = HttpHost::spawn(m, HttpOptions { remote: true, ..Default::default() })?;
            (Arc::new(h) as Arc<dyn Connection>, None)
        } else if let Some(m) = manifest {
            let h = Arc::new(ProcessHost::start(m).map_err(|e| e.to_string())?);
            h.wait_ready(Duration::from_secs(10));
            (Arc::clone(&h) as Arc<dyn Connection>, Some(h))
        } else {
            let h = Host::start(Box::new(|| Ok(plumcraft_text::engine())), None);
            h.wait_ready(Duration::from_secs(5));
            (Arc::new(InProc(h)), None)
        };
    let inbox = Arc::new(Mutex::new(Inbox::default()));
    let stop = Arc::new(AtomicBool::new(false));
    let (pump_conn, pump_inbox, pump_stop, pump_ctx) = (Arc::clone(&conn), Arc::clone(&inbox), Arc::clone(&stop), ctx.clone());
    std::thread::Builder::new()
        .name("plumcraft-view-pump".into())
        .spawn(move || {
            let mut since = 0;
            while !pump_stop.load(Ordering::SeqCst) {
                let Ok(Ok(p)) = pump_conn.call("events.poll", json!({"since": since, "wait_ms": 250})).map(serde_json::from_value::<PollReply>) else {
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                };
                since = p.last_seq;
                if !p.events.is_empty() {
                    pump_inbox.lock().unwrap_or_else(|e| e.into_inner()).events.extend(p.events);
                    pump_ctx.request_repaint();
                }
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(Link { conn, process: proc_host, inbox, stop })
}

struct App {
    link: Option<Link>,
    mode: Mode,
    schema: SchemaReply,
    workflow: Value,
    columns: Vec<Vec<String>>,
    reducer: Reducer,
    runs: u64,
    current: Option<String>,
    reply: Option<RunReply>,
    watch: Option<(String, String)>,
    watched: Option<Display>,
    /// The watched display's `full` value, read once when the display arrives.
    full: Option<Blob>,
    /// The watched value's blob was gone: rerun once (F58), then say so.
    missing: bool,
    reran_for_missing: bool,
    rerun: bool,
    log: VecDeque<String>,
    error: Option<String>,
    shot: Option<Shot>,
}

struct Shot {
    path: PathBuf,
    frames_after_reply: u32,
    runs: u64,
    requested: bool,
}

impl App {
    fn new(ctx: &egui::Context, workflow: Value, path: Option<PathBuf>, mode: Mode, shot: Option<(PathBuf, u64)>, watch: Option<(String, String)>) -> App {
        let mut app = App {
            link: None,
            mode,
            schema: SchemaReply::default(),
            columns: columns(&workflow),
            workflow,
            reducer: Reducer::new("", 0),
            runs: 0,
            current: None,
            reply: None,
            watch: None,
            watched: None,
            full: None,
            missing: false,
            reran_for_missing: false,
            rerun: false,
            log: VecDeque::new(),
            error: None,
            shot: shot.map(|(path, runs)| Shot { path, frames_after_reply: 0, runs, requested: false }),
        };
        app.reconnect(ctx);
        // A workflow file's relative paths are relative to its folder, as in the editor.
        if let Some(p) = &path {
            app.workflow = plumcraft_session::paths::anchor_workflow(&app.workflow, &app.schema, p);
        }
        app.watch = watch.or_else(|| app.default_watch());
        if app.shot.is_some() {
            app.run();
        }
        app
    }

    fn reconnect(&mut self, ctx: &egui::Context) {
        self.link = None;
        match connect(self.mode, ctx) {
            Ok(link) => {
                match link.conn.call("schema", json!({})).map(serde_json::from_value::<SchemaReply>) {
                    Ok(Ok(s)) => self.schema = s,
                    Ok(Err(e)) => self.error = Some(e.to_string()),
                    Err(e) => self.error = Some(e.to_string()),
                }
                self.link = Some(link);
                self.log(format!("connected: {}", self.mode.label()));
            }
            Err(e) => self.error = Some(e),
        }
    }

    /// A last-column node's first output (its port from the catalogue), so a fresh window shows
    /// something.
    fn default_watch(&self) -> Option<(String, String)> {
        let last = self.columns.last()?;
        last.iter().rev().find_map(|n| Some((n.clone(), self.def_of(n)?.outputs.first()?.name.clone())))
    }

    fn log(&mut self, line: String) {
        self.log.push_back(line);
        while self.log.len() > LOG_LINES {
            self.log.pop_front();
        }
    }

    fn run(&mut self) {
        let Some(link) = &self.link else { return };
        if let Some(cur) = &self.current {
            let _ = link.conn.call("run.cancel", json!({"run_id": cur})); // supersede (spec 8.2)
        }
        self.runs += 1;
        // Unique per client: an engine may outlive this window and remember its runs.
        let run_id = format!("view-{}-{}", std::process::id(), self.runs);
        let watch: Vec<Value> = self.watch.iter().map(|(n, p)| json!({"node": n, "port": p})).collect();
        let req = json!({"v": 2, "op": "run", "run_id": run_id, "generation": self.runs, "graph": {"kind": "inline", "graph": self.workflow}, "watch": watch});
        match link.conn.call("run", req) {
            Ok(_) => {
                self.reducer = Reducer::new(&run_id, self.runs);
                self.current = Some(run_id.clone());
                self.reply = None;
                self.watched = None;
                self.error = None;
                self.log(format!("run {run_id} accepted"));
            }
            Err(e) => self.error = Some(format!("run: {}", e.message)),
        }
    }

    fn cancel(&mut self) {
        if let (Some(link), Some(cur)) = (&self.link, &self.current) {
            let _ = link.conn.call("run.cancel", json!({"run_id": cur}));
            let cur = cur.clone();
            self.log(format!("cancel {cur} sent"));
        }
    }

    fn drain(&mut self) {
        let events = match &self.link {
            Some(l) => std::mem::take(&mut l.inbox.lock().unwrap_or_else(|e| e.into_inner()).events),
            None => return,
        };
        for e in events {
            let (run, generation) = (e.run_id.clone().unwrap_or_default(), e.generation.unwrap_or(0));
            match &e.event {
                Event::Status(rec) => {
                    if rec.state != NodeState::Progress {
                        let detail = rec.error.as_ref().map(|x| format!(" ({})", x.message)).unwrap_or_default();
                        self.log(format!("#{} {} {} {:?}{}", e.seq, run, rec.node_id, rec.state, detail));
                    }
                    self.reducer.apply(&run, generation, rec);
                    if let Some(d) = rec.display.first()
                        && Some(&run) == self.current.as_ref()
                    {
                        self.set_watched(d.clone());
                    }
                }
                Event::RunReply(r) => {
                    self.log(format!("#{} {} reply ok={} {:.1} ms", e.seq, r.run_id, r.ok, r.elapsed_ms));
                    if Some(&r.run_id) == self.current.as_ref() {
                        self.reducer.finish(r);
                        if let Some(d) = r.watch.first() {
                            self.set_watched(d.clone());
                        }
                        self.reply = Some((**r).clone());
                    }
                }
                Event::EngineState(s) => {
                    let s = format!("#{} engine {}", e.seq, state_text(s));
                    self.log(s);
                }
                _ => {}
            }
        }
        // A rerun asked for while handling events (a missing blob), once the run has its reply.
        if self.rerun && self.reply.is_some() {
            self.rerun = false;
            self.run();
        }
    }

    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let state = self.link.as_ref().map_or(EngineState::Off, |l| l.conn.state());
        ui.horizontal(|ui| {
            ui.heading("PlumCraft");
            ui.separator();
            let name =
                if self.schema.engine.name.is_empty() { "no engine".to_owned() } else { format!("{} {}", self.schema.engine.name, self.schema.engine.version) };
            ui.label(name);
            let lang = if self.mode.python() { "Python" } else { "Rust" };
            let mode = match (&self.link, self.mode) {
                (_, Mode::InProc) => "Rust · mode 1 · in-process".into(),
                (_, Mode::HttpRust | Mode::HttpPython) => format!("{lang} · mode 3 · HTTP, remote"),
                (Some(l), _) => format!("{lang} · mode 2 · process {}", l.process.as_ref().and_then(|p| p.pid()).unwrap_or(0)),
                (None, _) => format!("{lang} · mode 2"),
            };
            ui.label(RichText::new(mode).weak());
            ui.separator();
            let colour = match &state {
                EngineState::Connected { .. } => Color32::from_rgb(80, 180, 90),
                EngineState::Busy { .. } | EngineState::Connecting { .. } | EngineState::LoadingSchema => Color32::from_rgb(220, 170, 40),
                _ => Color32::from_rgb(210, 70, 60),
            };
            let (dot, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), egui::Sense::hover());
            ui.painter().circle_filled(dot.center(), 5.0, colour);
            ui.label(state_text(&state));
            ui.separator();
            if ui.button("▶ Run").clicked() {
                self.run();
            }
            let running = self.current.is_some() && self.reply.is_none();
            if ui.add_enabled(running, egui::Button::new("■ Cancel")).clicked() {
                self.cancel();
            }
            if matches!(self.mode, Mode::Rust | Mode::Python)
                && ui.button("Kill engine").on_hover_text("SIGKILL the engine process, to watch the host recover").clicked()
                && let Some(p) = self.link.as_ref().and_then(|l| l.process.clone())
            {
                p.kill_engine();
                self.log("engine killed (SIGKILL)".into());
            }
            let mut chosen = self.mode;
            egui::ComboBox::from_id_salt("engine-mode").selected_text("Engine…").show_ui(ui, |ui| {
                for m in Mode::ALL {
                    ui.selectable_value(&mut chosen, m, m.label());
                }
            });
            if chosen != self.mode {
                self.mode = chosen;
                self.current = None;
                self.reply = None;
                self.reducer = Reducer::new("", 0);
                let ctx = ui.ctx().clone();
                self.reconnect(&ctx);
            }
            if let Some(r) = &self.reply {
                ui.separator();
                let text = match &r.error {
                    Some(e) => format!("{}: {}", e.kind, e.message),
                    None if r.ok => format!("ok · {:.0} ms", r.elapsed_ms),
                    None => format!("finished with errors · {:.0} ms", r.elapsed_ms),
                };
                ui.label(RichText::new(text).strong());
            }
        });
        if let EngineState::EngineDown { reason, stderr_tail, hint, .. } = &state {
            ui.colored_label(Color32::from_rgb(210, 70, 60), format!("Engine down: {reason}"));
            if !hint.is_empty() {
                ui.label(format!("Fix: {hint}"));
            }
            if let Some(t) = stderr_tail.as_ref().filter(|t| !t.is_empty()) {
                ui.label(RichText::new(t).monospace().small());
            }
        }
        if let Some(e) = &self.error {
            ui.colored_label(Color32::from_rgb(210, 70, 60), e);
        }
    }

    fn graph(&mut self, ui: &mut egui::Ui) {
        let mut rects: BTreeMap<String, egui::Rect> = BTreeMap::new();
        let mut clicked = None;
        let columns = self.columns.clone();
        egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal_top(|ui| {
                for col in &columns {
                    ui.vertical(|ui| {
                        ui.set_width(CARD_W);
                        for id in col {
                            let (rect, header) = self.card(ui, id);
                            if header.clicked() {
                                clicked = Some(id.clone());
                            }
                            rects.insert(id.clone(), rect);
                            ui.add_space(12.0);
                        }
                    });
                    ui.add_space(48.0);
                }
            });
            let painter = ui.painter();
            let stroke = egui::Stroke::new(1.5, ui.visuals().weak_text_color());
            for w in self.workflow["wires"].as_array().into_iter().flatten() {
                let (Some(a), Some(b)) = (w["from"]["node"].as_str().and_then(|n| rects.get(n)), w["to"]["node"].as_str().and_then(|n| rects.get(n))) else {
                    continue;
                };
                let (p0, p3) = (a.right_center(), b.left_center());
                let dx = (p3.x - p0.x).abs() * 0.5;
                let curve = egui::epaint::CubicBezierShape::from_points_stroke(
                    [p0, p0 + egui::vec2(dx, 0.0), p3 - egui::vec2(dx, 0.0), p3],
                    false,
                    Color32::TRANSPARENT,
                    stroke,
                );
                painter.add(curve);
            }
        });
        if let Some(id) = clicked {
            let port = self.def_of(&id).and_then(|d| d.outputs.first().map(|p| p.name.clone())).unwrap_or_else(|| "text".into());
            self.watch = Some((id, port));
            self.run();
        }
    }

    fn def_of(&self, id: &str) -> Option<NodeDef> {
        let kind = self.workflow["nodes"][id]["kind"].as_str()?;
        self.schema.node(kind).cloned()
    }

    /// A node's card: its rectangle (for wires) and its title row's response. Only the title row
    /// is clickable: a click area over the whole card would sit on top of its text fields and
    /// take their clicks (found by the maintainer, 2026-10-27: "Can't edit the text fields").
    fn card(&mut self, ui: &mut egui::Ui, id: &str) -> (egui::Rect, egui::Response) {
        let def = self.def_of(id);
        let view = self.reducer.node(id).cloned().unwrap_or_default();
        let watched = self.watch.as_ref().is_some_and(|(n, _)| n == id);
        let colour = state_colour(view.state);
        let frame = egui::Frame::group(ui.style()).stroke(egui::Stroke::new(if watched { 3.0 } else { 1.5 }, colour)).inner_margin(8.0);
        let inner = frame.show(ui, |ui| {
            ui.set_width(CARD_W - 20.0);
            let header = ui.horizontal(|ui| {
                ui.label(RichText::new(id).strong());
                ui.label(RichText::new(def.as_ref().map_or("?", |d| d.display.as_str())).weak());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(state_name(view.state)).color(colour).strong());
                });
            });
            let header = ui
                .interact(header.response.rect, egui::Id::new(("card", id)), egui::Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("Click to watch this node's output and run");
            self.widgets(ui, id, def.as_ref());
            status_lines(ui, &view);
            header
        });
        (inner.response.rect, inner.inner)
    }

    /// Widgets for the node's widget ports, straight from the catalogue.
    fn widgets(&mut self, ui: &mut egui::Ui, id: &str, def: Option<&NodeDef>) {
        let Some(def) = def else { return };
        for port in def.inputs.iter().filter(|p| p.has_widget()) {
            let wired = self.workflow["wires"].as_array().into_iter().flatten().any(|w| w["to"]["node"] == id && w["to"]["port"] == port.name.as_str());
            if wired {
                continue;
            }
            let slot = &mut self.workflow["nodes"][id]["values"][&port.name];
            if slot.is_null() {
                *slot = port.default.clone();
            }
            ui.horizontal(|ui| {
                ui.label(port.display_label());
                match port.ty.as_str() {
                    "FLOAT" => {
                        let mut v = slot.as_f64().unwrap_or(0.0);
                        let range = port.min.unwrap_or(f64::MIN)..=port.max.unwrap_or(f64::MAX);
                        if ui.add(egui::DragValue::new(&mut v).speed(0.1).range(range)).changed() {
                            *slot = json!(v);
                        }
                    }
                    "INT" => {
                        let mut v = slot.as_i64().unwrap_or(0);
                        if ui.add(egui::DragValue::new(&mut v)).changed() {
                            *slot = json!(v);
                        }
                    }
                    _ => {
                        let mut v = slot.as_str().unwrap_or_default().to_owned();
                        let multiline = v.contains('\n');
                        let edit = if multiline { ui.add(egui::TextEdit::multiline(&mut v).desired_rows(2)) } else { ui.text_edit_singleline(&mut v) };
                        if edit.changed() {
                            *slot = json!(v);
                        }
                    }
                }
            });
        }
    }

    /// Take a new watched display and read its full value, if it has one. A blob that is gone
    /// greys the value and reruns once (spec 4.6, F58): the engine publishes it again.
    fn set_watched(&mut self, d: Display) {
        let same = self.watched.as_ref().and_then(|w| w.full.as_ref()).map(|f| &f.handle) == d.full.as_ref().map(|f| &f.handle);
        if !same || self.full.is_none() {
            self.full = d.full.as_ref().and_then(|f| self.link.as_ref().and_then(|l| l.conn.resolve(f)));
        }
        self.missing = d.full.is_some() && self.full.is_none();
        if self.missing && !self.reran_for_missing {
            self.reran_for_missing = true;
            self.log("the watched value's blob is gone; rerunning once to publish it again".into());
            self.rerun = true;
        } else if !self.missing {
            self.reran_for_missing = false;
        }
        self.watched = Some(d);
    }

    fn watched_panel(&self, ui: &mut egui::Ui) {
        ui.heading("Watched");
        let Some((n, p)) = &self.watch else {
            ui.label("Click a node to watch it.");
            return;
        };
        ui.label(RichText::new(format!("{n}.{p}")).monospace());
        let Some(d) = &self.watched else {
            ui.label(RichText::new("waiting for the value…").weak());
            return;
        };
        if let Some(e) = &d.error {
            ui.colored_label(Color32::from_rgb(210, 70, 60), &e.message);
            return;
        }
        ui.label(RichText::new(&d.summary).weak());
        if let Some(f) = &d.full {
            let how = match (&self.full, f.at.first()) {
                (Some(_), Some(Locator::Inline { .. })) => "inline".to_owned(),
                (Some(_), Some(Locator::Url { .. })) => format!("{} bytes downloaded by URL, verified, kept in the UI's CAS", f.meta.size),
                (Some(_), _) => format!("{} bytes from the store", f.meta.size),
                (None, _) => "gone (reaped); rerun to publish it again".to_owned(),
            };
            let handle = f.handle.as_ref().map(|h| h.to_string()).unwrap_or_default();
            ui.label(RichText::new(format!("{how} · {}", handle.get(..20).unwrap_or(&handle))).small().weak()).on_hover_text(handle);
        }
        egui::ScrollArea::vertical().show(ui, |ui| match d.media {
            Media::Table => {
                let table = d.value.clone().or_else(|| self.full.as_ref().and_then(|b| serde_json::from_slice(&b.bytes).ok()));
                match table {
                    Some(t) => table_view(ui, &t),
                    None => {
                        ui.label(RichText::new("no table").weak());
                    }
                }
            }
            Media::Ndarray => match self.full.as_ref().and_then(numbers) {
                Some(v) => plot(ui, &v),
                None => {
                    ui.label(RichText::new("no values").weak());
                }
            },
            _ => {
                let full_text = self.full.as_ref().filter(|_| d.media == Media::Text).map(|b| String::from_utf8_lossy(&b.bytes).into_owned());
                match full_text {
                    Some(t) => {
                        ui.label(RichText::new(t).monospace());
                    }
                    None => {
                        for r in &d.renditions {
                            if let Rendition::Excerpt { text, .. } = r {
                                ui.label(RichText::new(text).monospace().color(if self.missing {
                                    ui.visuals().weak_text_color()
                                } else {
                                    ui.visuals().text_color()
                                }));
                            }
                        }
                    }
                }
                if let Some(v) = &d.value {
                    ui.label(RichText::new(v.to_string()).monospace().size(28.0));
                }
            }
        });
    }

    fn screenshot(&mut self, ctx: &egui::Context) {
        let Some(shot) = &mut self.shot else { return };
        if self.reply.is_some() && self.runs < shot.runs {
            self.run();
            return;
        }
        if self.reply.is_some() {
            shot.frames_after_reply += 1;
            ctx.request_repaint();
        }
        if !shot.requested && shot.frames_after_reply >= 5 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
            shot.requested = true;
        }
        let image = ctx.input(|i| {
            i.raw.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        if let Some(image) = image {
            match save_png(&image, &shot.path) {
                Ok(()) => println!("wrote {}", shot.path.display()),
                Err(e) => eprintln!("plumcraft-view: {}: {e}", shot.path.display()),
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.drain();
        egui::Panel::top("plumcraft-top").show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("plumcraft-log").resizable(true).default_size(150.0).show(ui, |ui| {
            ui.label(RichText::new("Events").strong());
            egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
                for l in &self.log {
                    ui.label(RichText::new(l).monospace().small());
                }
            });
        });
        egui::Panel::right("plumcraft-watched").resizable(true).default_size(320.0).show(ui, |ui| self.watched_panel(ui));
        egui::CentralPanel::default().show(ui, |ui| self.graph(ui));
        if self.current.is_some() && self.reply.is_none() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        self.screenshot(&ctx);
    }
}

fn status_lines(ui: &mut egui::Ui, v: &NodeView) {
    if v.state == NodeState::Progress {
        let f = v.fraction.unwrap_or(0.0) as f32;
        ui.add(egui::ProgressBar::new(f).show_percentage().desired_width(CARD_W - 24.0));
        if let Some(s) = &v.stage {
            ui.label(RichText::new(s).weak().small());
        }
    }
    if let Some(ms) = v.ms {
        ui.label(RichText::new(format!("{ms:.1} ms")).weak().small());
    }
    if let Some(e) = &v.error {
        ui.label(RichText::new(&e.message).color(Color32::from_rgb(210, 70, 60)).small());
    }
    if let Some(Rendition::Excerpt { text, .. }) = &v.thumb {
        // Three lines and at most 200 characters: one long line must not stretch the card.
        let mut short: String = text.lines().take(3).collect::<Vec<_>>().join("\n");
        if short.chars().count() > 200 {
            short = short.chars().take(200).collect::<String>() + "…";
        }
        ui.label(RichText::new(short).monospace().small());
    }
}

fn state_name(s: NodeState) -> &'static str {
    match s {
        NodeState::Queued => "queued",
        NodeState::Started => "running",
        NodeState::Progress => "running",
        NodeState::Preview => "running",
        NodeState::Done => "done",
        NodeState::Cached => "cached",
        NodeState::Error => "error",
        NodeState::Skipped => "skipped",
        NodeState::Cancelled => "cancelled",
        NodeState::Unknown => "",
    }
}

/// A table (`{"columns": [{name, type, values}]}`) as a grid, with a bar for the last numeric
/// column.
fn table_view(ui: &mut egui::Ui, t: &Value) {
    let cols = t["columns"].as_array().cloned().unwrap_or_default();
    let rows = cols.first().and_then(|c| c["values"].as_array()).map_or(0, Vec::len);
    let bars = cols.iter().rev().find(|c| c["values"].as_array().is_some_and(|v| v.iter().all(Value::is_number)));
    let max = bars.and_then(|c| c["values"].as_array()).map_or(0.0, |v| v.iter().filter_map(Value::as_f64).fold(0.0, f64::max));
    egui::Grid::new("watched-table").striped(true).show(ui, |ui| {
        for c in &cols {
            ui.label(RichText::new(c["name"].as_str().unwrap_or("?")).strong());
        }
        ui.end_row();
        for r in 0..rows.min(1000) {
            for c in &cols {
                let v = &c["values"][r];
                ui.label(v.as_str().map_or_else(|| v.to_string(), str::to_owned));
            }
            if let Some(b) = bars.and_then(|c| c["values"][r].as_f64()).filter(|_| max > 0.0) {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(120.0, 12.0), egui::Sense::hover());
                let w = rect.width() * (b / max) as f32;
                ui.painter().rect_filled(egui::Rect::from_min_size(rect.min, egui::vec2(w, rect.height())), 2.0, Color32::from_rgb(79, 134, 198));
            }
            ui.end_row();
        }
    });
    if rows > 1000 {
        ui.label(RichText::new(format!("… {} more rows", rows - 1000)).weak());
    }
}

/// A 1-d array's values as numbers, for the dtypes the text engine makes and a few more.
fn numbers(b: &Blob) -> Option<Vec<f64>> {
    if b.shape.len() != 1 {
        return None;
    }
    let v: Vec<f64> = match b.dtype.as_str() {
        "|u1" => b.bytes.iter().map(|x| f64::from(*x)).collect(),
        "<u4" => b.bytes.as_chunks::<4>().0.iter().map(|c| f64::from(u32::from_le_bytes(*c))).collect(),
        "<i4" => b.bytes.as_chunks::<4>().0.iter().map(|c| f64::from(i32::from_le_bytes(*c))).collect(),
        "<f4" => b.bytes.as_chunks::<4>().0.iter().map(|c| f64::from(f32::from_le_bytes(*c))).collect(),
        "<f8" => b.bytes.as_chunks::<8>().0.iter().map(|c| f64::from_le_bytes(*c)).collect(),
        _ => return None,
    };
    Some(v)
}

/// A 1-d plot: a bar per value when they fit, otherwise each pixel column spans the min and max
/// of its values; the range is labelled.
fn plot(ui: &mut egui::Ui, v: &[f64]) {
    let (lo, hi) = v.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), x| (a.min(*x), b.max(*x)));
    ui.label(RichText::new(format!("{} values, {lo} … {hi}", v.len())).small().weak());
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width().max(100.0), 160.0), egui::Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_stroke(rect, 2.0, egui::Stroke::new(1.0, ui.visuals().weak_text_color()), egui::StrokeKind::Inside);
    if v.is_empty() || !lo.is_finite() || !hi.is_finite() {
        return;
    }
    // Bars rise from zero when the range includes it, else from the minimum.
    let base = if lo <= 0.0 && hi >= 0.0 { 0.0 } else { lo };
    let (top, bottom) = (hi.max(base), lo.min(base));
    let span = if top > bottom { top - bottom } else { 1.0 };
    let y = |x: f64| rect.bottom() - 4.0 - ((x - bottom) / span) as f32 * (rect.height() - 8.0);
    let colour = Color32::from_rgb(138, 92, 194);
    let inner = rect.shrink2(egui::vec2(4.0, 0.0));
    let px = inner.width().max(1.0) as usize;
    if v.len() <= px {
        let w = inner.width() / v.len() as f32;
        for (i, x) in v.iter().enumerate() {
            let left = inner.left() + i as f32 * w;
            let (a, b) = (y(*x).min(y(base)), y(*x).max(y(base)));
            p.rect_filled(egui::Rect::from_min_max(egui::pos2(left + w * 0.1, a), egui::pos2(left + w * 0.9, b.max(a + 1.0))), 1.0, colour);
        }
        return;
    }
    for col in 0..px {
        let (a, b) = (col * v.len() / px, ((col + 1) * v.len() / px).max(col * v.len() / px + 1));
        let slice = v.get(a..b.min(v.len())).unwrap_or_default();
        let (mn, mx) = slice.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(m, n), x| (m.min(*x), n.max(*x)));
        let x = inner.left() + col as f32 + 0.5;
        p.line_segment([egui::pos2(x, y(mn) + 0.5), egui::pos2(x, y(mx) - 0.5)], egui::Stroke::new(1.0, colour));
    }
}

fn state_colour(s: NodeState) -> Color32 {
    match s {
        NodeState::Done => Color32::from_rgb(80, 180, 90),
        NodeState::Cached => Color32::from_rgb(60, 150, 175),
        NodeState::Started | NodeState::Progress | NodeState::Preview | NodeState::Queued => Color32::from_rgb(220, 170, 40),
        NodeState::Error => Color32::from_rgb(210, 70, 60),
        NodeState::Skipped | NodeState::Cancelled => Color32::from_rgb(140, 140, 150),
        NodeState::Unknown => Color32::from_rgb(90, 90, 100),
    }
}

fn state_text(s: &EngineState) -> String {
    match s {
        EngineState::Off => "off".into(),
        EngineState::Connecting { url } => format!("connecting {url}"),
        EngineState::Unreachable { reason, .. } => format!("unreachable: {reason}"),
        EngineState::EngineDown { reason, .. } => format!("down: {reason}"),
        EngineState::LoadingSchema => "loading schema".into(),
        EngineState::Unavailable { reason } => format!("unavailable: {reason}"),
        EngineState::Busy { .. } => "busy".into(),
        EngineState::Connected { nodes, .. } => format!("connected · {nodes} nodes"),
    }
}

/// Nodes in columns by depth (longest path from a source), each column in id order.
fn columns(workflow: &Value) -> Vec<Vec<String>> {
    let nodes: Vec<String> = workflow["nodes"].as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
    let wires: Vec<(String, String)> = workflow["wires"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|w| Some((w["from"]["node"].as_str()?.to_owned(), w["to"]["node"].as_str()?.to_owned())))
        .collect();
    let mut depth: BTreeMap<String, usize> = nodes.iter().map(|n| (n.clone(), 0)).collect();
    // Relax at most |nodes| times; a cycle just stops growing.
    for _ in 0..nodes.len() {
        let mut changed = false;
        for (a, b) in &wires {
            let next = depth.get(a).copied().unwrap_or(0) + 1;
            if let Some(d) = depth.get_mut(b)
                && *d < next
                && next <= nodes.len()
            {
                *d = next;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    let mut cols: Vec<Vec<String>> = Vec::new();
    for (n, d) in depth {
        if cols.len() <= d {
            cols.resize(d + 1, Vec::new());
        }
        if let Some(c) = cols.get_mut(d) {
            c.push(n);
        }
    }
    cols
}

fn save_png(image: &egui::ColorImage, path: &std::path::Path) -> Result<(), String> {
    let [w, h] = image.size;
    let bytes: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), u32::try_from(w).map_err(|e| e.to_string())?, u32::try_from(h).map_err(|e| e.to_string())?);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(&bytes).map_err(|e| e.to_string())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (python, http) = (args.iter().any(|a| a == "--python"), args.iter().any(|a| a == "--http"));
    let mode = match (python, http, args.iter().any(|a| a == "--process")) {
        (true, true, _) => Mode::HttpPython,
        (false, true, _) => Mode::HttpRust,
        (true, false, _) => Mode::Python,
        (false, false, true) => Mode::Rust,
        _ => Mode::InProc,
    };
    let watch =
        args.iter().position(|a| a == "--watch").and_then(|i| args.get(i + 1)).and_then(|w| w.split_once('.')).map(|(n, p)| (n.to_owned(), p.to_owned()));
    let shot_runs = args.iter().position(|a| a == "--shot-runs").and_then(|i| args.get(i + 1)).and_then(|n| n.parse().ok()).unwrap_or(1);
    let shot = args.iter().position(|a| a == "--screenshot").and_then(|i| args.get(i + 1)).map(|p| (PathBuf::from(p), shot_runs));
    let path = args
        .iter()
        .enumerate()
        .find(|(i, a)| !a.starts_with("--") && (*i == 0 || args.get(i - 1).is_none_or(|p| p != "--screenshot" && p != "--shot-runs" && p != "--watch")))
        .map(|(_, a)| a.clone());
    let text = match &path {
        Some(p) => match std::fs::read_to_string(p) {
            Ok(t) => t,
            Err(e) => {
                eprintln!("plumcraft-view: {p}: {e}");
                return ExitCode::from(2);
            }
        },
        None => plumcraft_text::DEMO_WORKFLOW.to_owned(),
    };
    let workflow: Value = match serde_json::from_str(&text) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("plumcraft-view: workflow: {e}");
            return ExitCode::from(2);
        }
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_title("PlumCraft").with_inner_size([1360.0, 760.0]).with_min_inner_size([640.0, 400.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    match eframe::run_native(
        "PlumCraft",
        options,
        Box::new(move |cc| Ok(Box::new(App::new(&cc.egui_ctx, workflow, path.map(PathBuf::from), mode, shot, watch)))),
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("plumcraft-view: {e}");
            ExitCode::FAILURE
        }
    }
}
