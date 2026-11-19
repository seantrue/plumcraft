//! Rule 6 of `docs/shell-interface.md` 3.1: the UI thread never waits for an engine.
//!
//! [`the_ui_thread_never_calls_an_engine`] wraps every engine connection in [`Watch`], which
//! records each engine call made on the thread that owns the editor (this test's thread: it steps
//! the frames, as eframe's main thread does). It runs, supersedes, cancels, switches engines
//! through the control channel and the window's own paths, and checks that no call was made
//! there, and that no frame waited for a slow engine.
//!
//! [`stalls`] (ignored; `cargo test -p plumcraft-ui --test engine_thread -- --ignored --nocapture`)
//! prints the longest frame while (a) a run starts on a 500-node text graph and (b) the text
//! engine is switched to process mode, on real engines and on slow fake ones.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::channel;
use std::sync::{Arc, Mutex};
use std::thread::ThreadId;
use std::time::{Duration, Instant};

use egui_kittest::Harness;
use plumcraft_editor_core::engines::{Launcher, choices};
use plumcraft_engine::host::Host;
use plumcraft_graph::{Graph, PortRef};
use plumcraft_host::{Connection, InProc};
use plumcraft_proto::data::DataRef;
use plumcraft_proto::error::ProtoError;
use plumcraft_proto::session::EngineState;
use plumcraft_session::Session;
use plumcraft_store::Blob;
use plumcraft_ui::EditorApp;
use plumcraft_ui::link::{Link, repaint_waker};
use serde_json::{Value, json};

/// Which thread owns the editor, once the test says so (building it reads the catalogue there).
type Owner = Arc<Mutex<Option<ThreadId>>>;

/// An engine connection that notes every call made on the owner's thread, and can be slow.
struct Watch {
    inner: Arc<dyn Connection>,
    owner: Owner,
    /// Engine calls made on the owner's thread.
    on_owner: Arc<Mutex<Vec<String>>>,
    /// `resolve`s made on the owner's thread (reported, not refused: section 4 of the slice).
    resolves: Arc<AtomicU64>,
    /// How long a `run` takes to be accepted (a slow engine).
    run_delay: Duration,
}

impl Watch {
    fn mine(&self) -> bool {
        *self.owner.lock().unwrap() == Some(std::thread::current().id())
    }
}

impl Connection for Watch {
    fn call(&self, method: &str, params: Value) -> Result<Value, ProtoError> {
        if method != "events.poll" && self.mine() {
            self.on_owner.lock().unwrap().push(method.to_owned());
        }
        if method == "run" {
            std::thread::sleep(self.run_delay);
        }
        self.inner.call(method, params)
    }
    fn state(&self) -> EngineState {
        self.inner.state()
    }
    fn resolve(&self, r: &DataRef) -> Option<Blob> {
        if self.mine() {
            self.resolves.fetch_add(1, Ordering::Relaxed);
        }
        self.inner.resolve(r)
    }
    fn uploads_pending(&self, params: &Value) -> Vec<(String, String)> {
        self.inner.uploads_pending(params)
    }
    fn allow_uploads(&self, files: &[String]) {
        self.inner.allow_uploads(files)
    }
}

/// What a test watches with.
#[derive(Clone, Default)]
struct Eyes {
    owner: Owner,
    on_owner: Arc<Mutex<Vec<String>>>,
    resolves: Arc<AtomicU64>,
    /// Connects made on the owner's thread (an engine started there).
    connects_on_owner: Arc<Mutex<Vec<String>>>,
}

impl Eyes {
    fn watch(&self, inner: Arc<dyn Connection>, run_delay: Duration) -> Arc<dyn Connection> {
        Arc::new(Watch { inner, owner: Arc::clone(&self.owner), on_owner: Arc::clone(&self.on_owner), resolves: Arc::clone(&self.resolves), run_delay })
    }
    fn arm(&self) {
        *self.owner.lock().unwrap() = Some(std::thread::current().id());
    }
    fn mine(&self) -> bool {
        *self.owner.lock().unwrap() == Some(std::thread::current().id())
    }
}

fn text_host() -> Arc<dyn Connection> {
    let host = Host::start(Box::new(|| Ok(plumcraft_text::engine())), None);
    host.wait_ready(Duration::from_secs(10));
    Arc::new(InProc(host))
}

/// A text graph of `n` nodes and about `1.6 n` wires: Join nodes take two, Upper case one, each
/// wired from earlier nodes (as the FFI's `spike.scene`).
fn scene(s: &mut Session, n: usize) {
    let cat = s.catalog().clone();
    let (lit, join, upper) = (cat.node("text.literal").unwrap().clone(), cat.node("text.join").unwrap().clone(), cat.node("text.upper").unwrap().clone());
    let want = n * 8 / 5;
    let joins = want.saturating_sub(n - 1).min(n - 1);
    s.edit("Scene", move |g, cat| {
        *g = Graph::new();
        let mut ids: Vec<plumcraft_graph::NodeId> = Vec::with_capacity(n);
        let mut wires = 0;
        for i in 0..n {
            let pos = [(i % 25) as f32 * 260.0, (i / 25) as f32 * 160.0];
            let kind = if i == 0 {
                &lit
            } else if i <= joins {
                &join
            } else {
                &upper
            };
            let id = g.add_node(kind, pos);
            if i > 0 {
                let ports: &[&str] = if i <= joins { &["a", "b"] } else { &["text"] };
                for (k, port) in ports.iter().enumerate() {
                    if wires >= want {
                        break;
                    }
                    g.connect(cat, PortRef::new(ids[(i * 7 + k * 13) % i].clone(), "text"), PortRef::new(id.clone(), *port))?;
                    wires += 1;
                }
            }
            ids.push(id);
        }
        Ok(())
    })
    .unwrap();
}

/// A text editor linked to `conn`, offering every text hosting, started by `connector`.
fn editor(conn: Arc<dyn Connection>, nodes: usize, connector: plumcraft_ui::engine_view::Connector) -> EditorApp {
    let mut s = Session::new(serde_json::from_value(conn.call("schema", json!({})).unwrap()).unwrap());
    s.connected = true;
    if nodes > 0 {
        scene(&mut s, nodes);
    } else {
        let lit = s.execute("node.add", &json!({"kind": "text.literal", "pos": [0, 0]})).unwrap()["id"].as_str().unwrap().to_owned();
        let up = s.execute("node.add", &json!({"kind": "text.upper", "pos": [300, 0]})).unwrap()["id"].as_str().unwrap().to_owned();
        s.execute("wire.connect", &json!({"from": {"node": lit, "port": "text"}, "to": {"node": up, "port": "text"}})).unwrap();
    }
    EditorApp::new(s).with_link(Link::start(conn, "in-process", None).unwrap()).with_engines(choices(), Some(("text".into(), "inproc".into())), connector)
}

fn harness(app: EditorApp) -> Harness<'static, EditorApp> {
    let mut h = Harness::builder().with_size(egui::vec2(1400.0, 800.0)).build_ui_state(|ui, app: &mut EditorApp| app.show(ui), app);
    h.run_steps(4);
    h
}

/// Step frames (with time for the engine) until `done`, or `limit`: the longest frame.
fn frames_until(h: &mut Harness<'static, EditorApp>, what: &str, limit: Duration, done: impl Fn(&EditorApp) -> bool) -> Duration {
    let start = Instant::now();
    let mut longest = Duration::ZERO;
    while !done(h.state()) {
        assert!(start.elapsed() < limit, "timed out waiting for {what}: {:?}", h.state().last_error);
        let t = Instant::now();
        h.step();
        longest = longest.max(t.elapsed());
        std::thread::sleep(Duration::from_millis(5));
    }
    longest
}

fn run_done(a: &EditorApp) -> bool {
    !a.session.run.running && a.session.run.outbox_is_empty() && a.session.run.last_reply.is_some()
}

/// Send a control request as the transport does (through `handle_control`, inside no frame),
/// then step frames until it replies: the reply, how long the hand-over took, and the longest
/// frame meanwhile.
fn deferred(h: &mut Harness<'static, EditorApp>, method: &str, params: Value, limit: Duration) -> (Value, Duration, Duration) {
    let (tx, rx) = channel();
    let ctx = h.ctx.clone();
    let t = Instant::now();
    h.state_mut().handle_control(
        &ctx,
        method,
        &params,
        Box::new(move |v| {
            let _ = tx.send(v);
        }),
    );
    let handed = t.elapsed();
    let start = Instant::now();
    let mut longest = Duration::ZERO;
    loop {
        if let Ok(v) = rx.try_recv() {
            return (v, handed, longest);
        }
        assert!(start.elapsed() < limit, "{method}: no reply");
        let t = Instant::now();
        h.step();
        longest = longest.max(t.elapsed());
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Engine › Text › `hosting`, clicked as a person would.
fn pick(h: &mut Harness<'static, EditorApp>, hosting: &str) -> Duration {
    use egui_kittest::kittest::Queryable;
    h.get_by_label("Engine").click();
    h.run_steps(2);
    // "Text ⏵", or "Text · <how it is hosted> ⏵" while it is in use
    match h.query_by_label("Text ⏵") {
        Some(n) => n.hover(),
        None => h.get_by_label_contains("Text · ").hover(),
    }
    h.run_steps(2);
    let t = Instant::now();
    h.get_by_role_and_label(egui::accesskit::Role::RadioButton, hosting).click();
    h.step();
    t.elapsed()
}

/// The text engine's process hosting, faked: in-process, after `delay` (a slow start).
fn slow_connector(eyes: &Eyes, delay: Duration) -> plumcraft_ui::engine_view::Connector {
    let eyes = eyes.clone();
    Box::new(move |engine, mode, ctx| {
        if eyes.mine() {
            eyes.connects_on_owner.lock().unwrap().push(format!("{engine}/{mode}"));
        }
        if mode != "inproc" {
            std::thread::sleep(delay);
        }
        Link::start(eyes.watch(text_host(), Duration::ZERO), format!("fake {mode}"), ctx.cloned())
    })
}

#[test]
fn the_ui_thread_never_calls_an_engine() {
    let eyes = Eyes::default();
    let slow = Duration::from_millis(600);
    let app = editor(eyes.watch(text_host(), slow), 0, slow_connector(&eyes, Duration::from_millis(800)));
    let mut h = harness(app);
    eyes.arm();

    // a run, sent by the frame; the engine is slow to take it, and no frame waits for that
    h.state_mut().run("run.start", &json!({}));
    let longest = frames_until(&mut h, "the run", Duration::from_secs(30), run_done);
    assert!(longest < slow / 2, "a frame waited for the engine: {longest:?}");
    // superseded: a second run while the first is in flight cancels it first
    h.state_mut().run("run.start", &json!({}));
    h.step();
    h.state_mut().run("run.start", &json!({}));
    frames_until(&mut h, "the second run", Duration::from_secs(30), run_done);
    assert!(h.state().session.run.superseded >= 1);
    // cancelled
    h.state_mut().run("run.start", &json!({}));
    h.step();
    h.state_mut().run("run.cancel", &json!({}));
    frames_until(&mut h, "the cancel", Duration::from_secs(30), |a| !a.session.run.running && a.session.run.outbox_is_empty());

    // a switch through the control channel replies when it has switched, and no frame waits
    let (r, handed, longest) =
        deferred(&mut h, "command.run", json!({"command": "engine.switch", "params": {"engine": "text", "mode": "process"}}), Duration::from_secs(30));
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(
        (r["result"]["engine"].as_str(), r["result"]["mode"].as_str(), r["result"]["connected"].as_bool()),
        (Some("text"), Some("process"), Some(true)),
        "{r}"
    );
    assert!(handed < Duration::from_millis(400) && longest < Duration::from_millis(400), "the switch held the thread: {handed:?}, {longest:?}");
    assert!(h.state().engine_label().contains("fake process"), "{}", h.state().engine_label());
    h.state_mut().run("run.start", &json!({}));
    frames_until(&mut h, "the run after the switch", Duration::from_secs(30), run_done);

    // ui.invoke's reconnect, then a clear
    let (r, ..) = deferred(&mut h, "ui.invoke", json!({"command": "engine.reconnect"}), Duration::from_secs(30));
    assert_eq!(r["ok"], true, "{r}");
    assert_eq!(r["result"]["engine"]["connected"], true, "{r}");
    let (r, ..) = deferred(&mut h, "command.run", json!({"command": "engine.clear"}), Duration::from_secs(30));
    assert_eq!(r["result"]["label"], "engine: not connected", "{r}");

    // the window's own path: a pick from the Engine menu runs inside a frame
    let clicked = pick(&mut h, "Rust · process (mode 2)");
    let longest = frames_until(&mut h, "the pick", Duration::from_secs(30), |a| a.session.connected).max(clicked);
    assert!(longest < Duration::from_millis(400), "{longest:?}");
    assert!(h.state().engine_label().contains("fake process"), "{}", h.state().engine_label());

    drop(h);
    assert_eq!(*eyes.on_owner.lock().unwrap(), Vec::<String>::new(), "engine calls on the UI thread");
    assert_eq!(*eyes.connects_on_owner.lock().unwrap(), Vec::<String>::new(), "engines started on the UI thread");
    eprintln!("resolves on the UI thread: {}", eyes.resolves.load(Ordering::Relaxed));
}

/// The longest frame while (a) a run starts on a 500-node text graph and (b) the text engine is
/// switched to process mode, each on a real engine and on a slow fake one.
#[test]
#[ignore = "a measurement: run with --ignored --nocapture"]
fn stalls() {
    let ms = |d: Duration| format!("{:.1} ms", d.as_secs_f64() * 1000.0);
    let launcher = Launcher::default();
    let real: plumcraft_ui::engine_view::Connector = {
        let l = launcher.clone();
        Box::new(move |e, m, ctx| l.connect_ids(e, m, ctx.cloned().map(repaint_waker)).map(Link::from))
    };
    let eyes = Eyes::default();

    // (a) a run on a 500-node graph: in-process, by process, and an engine 500 ms slow to take it
    for (what, conn) in [
        ("in-process", text_host()),
        ("process", launcher.connect_ids("text", "process", None).unwrap().conn.clone()),
        ("slow fake (500 ms)", eyes.watch(text_host(), Duration::from_millis(500))),
    ] {
        let mut h = harness(editor(conn, 500, Box::new(|_, _, _| Err("none".into()))));
        let t = Instant::now();
        h.step();
        let idle = t.elapsed();
        let t = Instant::now();
        h.state_mut().run("run.start", &json!({}));
        let command = t.elapsed();
        let t = Instant::now();
        h.step();
        let first = t.elapsed();
        let longest = frames_until(&mut h, what, Duration::from_secs(120), run_done).max(first);
        let (done, total) = h.state().session.run.progress();
        eprintln!(
            "(a) run, 500 nodes, {what}: a frame before {}, run.start {}, first frame {}, longest frame {} ({done}/{total} nodes)",
            ms(idle),
            ms(command),
            ms(first),
            ms(longest)
        );
    }

    // (b) switching text from in-process to process: real, and a fake 2 s slow to start
    for (what, connector) in [("real process", real), ("slow fake (2 s)", slow_connector(&eyes, Duration::from_secs(2)))] {
        let mut h = harness(editor(text_host(), 0, connector));
        let (r, handed, longest) =
            deferred(&mut h, "command.run", json!({"command": "engine.switch", "params": {"engine": "text", "mode": "process"}}), Duration::from_secs(60));
        assert_eq!(r["ok"], true, "{r}");
        eprintln!("(b) switch to process, {what}: handle_control {}, longest frame after {}", ms(handed), ms(longest));
        // and from the window's own path: a pick from the Engine menu, inside a frame
        h.state_mut().run("engine.switch", &json!({"engine": "text", "mode": "inproc"}));
        h.run_steps(2);
        let clicked = pick(&mut h, "Rust · process (mode 2)");
        let longest = frames_until(&mut h, what, Duration::from_secs(60), |a| a.session.connected && a.engine_label().contains("process")).max(clicked);
        eprintln!("(b) switch to process inside a frame, {what}: longest {}", ms(longest));
    }
}

// ---------- values (slice 6): no frame waits for an engine's `resolve` ----------

/// A connection whose `resolve` takes `delay`, as an HTTP engine's fetch of a value does, and
/// which counts every `resolve` made on the thread that owns the editor.
struct SlowValues {
    inner: Arc<dyn Connection>,
    owner: Owner,
    on_owner: Arc<AtomicU64>,
    total: Arc<AtomicU64>,
    delay: Duration,
}

impl Connection for SlowValues {
    fn call(&self, method: &str, params: Value) -> Result<Value, ProtoError> {
        self.inner.call(method, params)
    }
    fn state(&self) -> EngineState {
        self.inner.state()
    }
    fn resolve(&self, r: &DataRef) -> Option<Blob> {
        if *self.owner.lock().unwrap() == Some(std::thread::current().id()) {
            self.on_owner.fetch_add(1, Ordering::Relaxed);
        }
        self.total.fetch_add(1, Ordering::Relaxed);
        std::thread::sleep(self.delay);
        self.inner.resolve(r)
    }
}

/// A node that outputs a fixed value.
struct Fixed {
    id: &'static str,
    ty: &'static str,
    make: fn() -> plumcraft_engine::Data,
}

impl plumcraft_engine::Node for Fixed {
    fn def(&self) -> plumcraft_proto::catalogue::NodeDef {
        use plumcraft_proto::catalogue::{NodeDef, Port};
        NodeDef {
            id: self.id.into(),
            display: self.id.into(),
            category: vec!["Testing".into()],
            family: "test".into(),
            outputs: vec![Port { name: "out".into(), ty: self.ty.into(), ..Default::default() }],
            ..Default::default()
        }
    }
    fn run(&self, _: &plumcraft_engine::Inputs, _: &mut plumcraft_engine::Cx) -> Result<plumcraft_engine::Outputs, plumcraft_proto::error::NodeError> {
        Ok(plumcraft_engine::Outputs::from([("out".to_owned(), (self.make)())]))
    }
}

fn f32_array(h: usize, w: usize, c: usize, k: f32) -> plumcraft_engine::Data {
    let v: Vec<u8> = (0..h * w * c).flat_map(|i| ((i % 97) as f32 * k).to_le_bytes()).collect();
    let shape = if c == 1 { vec![h as u64, w as u64] } else { vec![h as u64, w as u64, c as u64] };
    plumcraft_engine::Data::Array(plumcraft_engine::Array::new("<f4", shape, v))
}

fn values_engine() -> plumcraft_engine::Engine {
    use plumcraft_engine::{Audio, BlobSource, Data};
    use plumcraft_proto::catalogue::core_types;
    use plumcraft_proto::data::{BlobMeta, Form};
    let info =
        plumcraft_proto::session::EngineInfo { id: "values".into(), name: "Values".into(), version: "1".into(), salt: "values".into(), ..Default::default() };
    let png = || {
        let img = image::RgbImage::from_fn(640, 480, |x, y| image::Rgb([(x * 7 % 251) as u8, (y * 3 % 241) as u8, ((x ^ y) % 256) as u8]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        img.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        let bytes = bytes.into_inner();
        let meta = BlobMeta { media: "image/png".into(), size: bytes.len() as u64, form: Form::Image { width: 640, height: 480 }, aliases: Vec::new() };
        Data::Blob(Box::new(plumcraft_engine::Blob {
            handle: plumcraft_proto::Handle::raw(&bytes),
            meta,
            source: BlobSource::Bytes(Arc::from(bytes)),
            publish: true,
            thumb: None,
        }))
    };
    let mut e = plumcraft_engine::Engine::new(info).with_wire_type(core_types::array()).with_wire_type(core_types::image()).with_wire_type(core_types::audio());
    for (id, ty, make) in [
        ("v.planes", core_types::ARRAY, (|| f32_array(192, 256, 3, 0.01)) as fn() -> Data),
        ("v.planes2", core_types::ARRAY, || f32_array(192, 256, 3, 0.0101)),
        ("v.grey", core_types::ARRAY, || f32_array(192, 256, 1, 0.02)),
        ("v.png", core_types::IMAGE, png),
        ("v.audio", core_types::AUDIO, || {
            let samples: Vec<f32> = (0..40_000).map(|i| ((i as f32) * 0.05).sin() * 0.5).collect();
            Data::Audio(Audio { sample_rate: 8000, channels: 2, samples: Arc::from(samples) })
        }),
    ] {
        e = e.with_node(Box::new(Fixed { id, ty, make }));
    }
    e
}

/// An editor over [`values_engine`] whose `resolve` takes `delay`: one node of each kind.
fn values_editor(eyes: &Eyes, on_owner: &Arc<AtomicU64>, total: &Arc<AtomicU64>, delay: Duration) -> (EditorApp, Vec<String>) {
    let host = Host::start(Box::new(|| Ok(values_engine())), None);
    host.wait_ready(Duration::from_secs(10));
    let catalog = serde_json::from_value(host.schema(None).unwrap()).unwrap();
    let mut s = Session::new(catalog);
    s.connected = true;
    let kinds = ["v.planes", "v.planes2", "v.grey", "v.png", "v.audio"];
    let ids = kinds
        .iter()
        .enumerate()
        .map(|(i, k)| s.execute("node.add", &json!({"kind": k, "pos": [40 + 260 * (i % 3), 40 + 240 * (i / 3)]})).unwrap()["id"].as_str().unwrap().to_owned())
        .collect();
    let conn: Arc<dyn Connection> =
        Arc::new(SlowValues { inner: Arc::new(InProc(host)), owner: Arc::clone(&eyes.owner), on_owner: Arc::clone(on_owner), total: Arc::clone(total), delay });
    (EditorApp::new(s).with_link(Link::start(conn, "in-process", None).unwrap()), ids)
}

/// Step frames for at least `min` and until `done` (or `limit`): the longest frame.
fn frames_for(
    h: &mut Harness<'static, EditorApp>,
    min: Duration,
    limit: Duration,
    done: impl Fn(&mut Harness<'static, EditorApp>) -> bool,
) -> (Duration, bool) {
    let start = Instant::now();
    let mut longest = Duration::ZERO;
    loop {
        let t = Instant::now();
        h.step();
        longest = longest.max(t.elapsed());
        let ok = done(h);
        if (ok && start.elapsed() >= min) || start.elapsed() >= limit {
            return (longest, ok);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn inspect(h: &mut Harness<'static, EditorApp>) -> Value {
    let ctx = h.ctx.clone();
    h.state().inspect(&ctx)
}

/// The values scenario: run (thumbnails and waveforms on the canvas), watch each value (the
/// Watched panel reads it in full), pop one out, and compare two. Returns each phase's longest
/// frame and whether what it waited for arrived.
fn values_scenario(h: &mut Harness<'static, EditorApp>, ids: &[String], min: Duration) -> Vec<(String, Duration, bool)> {
    let limit = Duration::from_secs(20);
    let mut out = Vec::new();
    h.state_mut().run("run.start", &json!({}));
    let (l, ok) = frames_for(h, min, limit, |h| run_done(h.state()));
    out.push(("run, then the canvas's thumbnails and waveform".to_owned(), l, ok));
    for id in ids {
        h.state_mut().run("run.watch", &json!({"node": id}));
        let (l, ok) = frames_for(h, min, limit, |h| {
            let v = inspect(h);
            let w = &v["watched"];
            w["node"] == id.as_str() && w["full"] == true
        });
        out.push((format!("watch {id}"), l, ok));
    }
    let a = ids[0].clone();
    h.state_mut().run("run.watch", &json!({"node": a}));
    h.state_mut().run("view.popout", &json!({"node": a, "port": "out"}));
    let (l, ok) = frames_for(h, min, limit, |h| inspect(h)["popouts"].as_array().and_then(|p| p.first()).is_some_and(|p| p["pane"]["full"] == true));
    out.push(("a pop-out".to_owned(), l, ok));
    h.state_mut().run("viewer.pin", &json!({"slot": "a"}));
    h.state_mut().run("run.watch", &json!({"node": ids[1]}));
    h.state_mut().run("viewer.pin", &json!({"slot": "b"}));
    h.state_mut().run("viewer.compare", &json!({"on": true, "how": "difference"}));
    let (l, ok) = frames_for(h, min, limit, |h| inspect(h)["watched"]["compare"]["result"]["state"] == "ready");
    out.push(("compare A and B".to_owned(), l, ok));
    out
}

/// Rule 6 for values: thumbnails, the Watched panel, a pop-out and compare never call `resolve`
/// on the editor's thread, and no frame waits for an engine whose `resolve` takes 300 ms.
#[test]
fn values_never_wait_for_an_engine() {
    let eyes = Eyes::default();
    let (on_owner, total) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0)));
    let slow = Duration::from_millis(300);
    let (app, ids) = values_editor(&eyes, &on_owner, &total, slow);
    let mut h = harness(app);
    eyes.arm();
    let phases = values_scenario(&mut h, &ids, Duration::ZERO);
    for (what, longest, ok) in &phases {
        eprintln!("{what}: longest frame {:.1} ms, arrived: {ok}", longest.as_secs_f64() * 1000.0);
    }
    for (what, longest, ok) in &phases {
        assert!(*ok, "{what}: never arrived: {}", inspect(&mut h)["watched"]);
        assert!(*longest < slow / 2, "{what}: a frame waited for the engine's resolve: {longest:?}");
    }
    drop(h);
    assert_eq!(on_owner.load(Ordering::Relaxed), 0, "resolve called on the editor's thread ({} in all)", total.load(Ordering::Relaxed));
    assert!(total.load(Ordering::Relaxed) > 0, "values were read");
}

/// The longest frame in each phase of [`values_scenario`], on an engine whose `resolve` takes 300
/// ms and on the same engine with no delay.
#[test]
#[ignore = "a measurement: run with --ignored --nocapture"]
fn value_stalls() {
    for delay in [Duration::ZERO, Duration::from_millis(300)] {
        let eyes = Eyes::default();
        let (on_owner, total) = (Arc::new(AtomicU64::new(0)), Arc::new(AtomicU64::new(0)));
        let (app, ids) = values_editor(&eyes, &on_owner, &total, delay);
        let mut h = harness(app);
        eyes.arm();
        for (what, longest, ok) in values_scenario(&mut h, &ids, Duration::from_millis(300)) {
            eprintln!("resolve {} ms · {what}: longest frame {:.1} ms (arrived: {ok})", delay.as_millis(), longest.as_secs_f64() * 1000.0);
        }
        drop(h);
        eprintln!(
            "resolve {} ms · resolves on the editor's thread: {} of {}",
            delay.as_millis(),
            on_owner.load(Ordering::Relaxed),
            total.load(Ordering::Relaxed)
        );
    }
}

/// A node whose 96 × 128 × 3 array depends on its `seed`: every node's thumbnail is its own.
struct Seeded;

impl plumcraft_engine::Node for Seeded {
    fn def(&self) -> plumcraft_proto::catalogue::NodeDef {
        use plumcraft_proto::catalogue::{NodeDef, Port, core_types};
        NodeDef {
            id: "v.seeded".into(),
            display: "Seeded".into(),
            category: vec!["Testing".into()],
            family: "test".into(),
            inputs: vec![Port { name: "seed".into(), ty: "INT".into(), default: json!(0), ..Default::default() }],
            outputs: vec![Port { name: "out".into(), ty: core_types::ARRAY.into(), ..Default::default() }],
            ..Default::default()
        }
    }
    fn run(&self, inputs: &plumcraft_engine::Inputs, _: &mut plumcraft_engine::Cx) -> Result<plumcraft_engine::Outputs, plumcraft_proto::error::NodeError> {
        let seed = match inputs.get("seed") {
            Some(plumcraft_engine::Data::Json(v)) => v.as_f64().unwrap_or(0.0) as f32,
            _ => 0.0,
        };
        let v: Vec<u8> = (0..96 * 128 * 3).flat_map(|i| (((i % 113) as f32 + seed) * 0.003).sin().to_le_bytes()).collect();
        Ok(plumcraft_engine::Outputs::from([("out".to_owned(), plumcraft_engine::Data::Array(plumcraft_engine::Array::new("<f4", vec![96, 128, 3], v)))]))
    }
}

/// Memory with a 500-node graph's thumbnails on the canvas: run it under `/usr/bin/time -l` for
/// the peak (`cargo test -p plumcraft-ui --test engine_thread -- --ignored value_memory`).
#[test]
#[ignore = "a measurement: run with --ignored --nocapture under /usr/bin/time -l"]
fn value_memory() {
    use plumcraft_proto::catalogue::core_types;
    let info =
        plumcraft_proto::session::EngineInfo { id: "seeded".into(), name: "Seeded".into(), version: "1".into(), salt: "seeded".into(), ..Default::default() };
    let host =
        Host::start(Box::new(move || Ok(plumcraft_engine::Engine::new(info.clone()).with_wire_type(core_types::array()).with_node(Box::new(Seeded)))), None);
    host.wait_ready(Duration::from_secs(10));
    let catalog = serde_json::from_value(host.schema(None).unwrap()).unwrap();
    let mut s = Session::new(catalog);
    s.connected = true;
    for i in 0..500 {
        let id = s.execute("node.add", &json!({"kind": "v.seeded", "pos": [(i % 25) * 260, (i / 25) * 200]})).unwrap()["id"].as_str().unwrap().to_owned();
        s.execute("node.set", &json!({"node": id, "port": "seed", "value": i})).unwrap();
    }
    let app = EditorApp::new(s).with_link(Link::start(Arc::new(InProc(host)) as Arc<dyn Connection>, "in-process", None).unwrap());
    let mut h = harness(app);
    let t = Instant::now();
    h.state_mut().run("run.start", &json!({}));
    frames_until(&mut h, "the run", Duration::from_secs(300), run_done);
    let ran = t.elapsed();
    let settle = Instant::now();
    let mut frames = 0;
    while settle.elapsed() < Duration::from_secs(4) {
        h.step();
        frames += 1;
    }
    eprintln!("value_memory: 500 nodes ran in {:.1} s, then {frames} frames", ran.as_secs_f64());
}
