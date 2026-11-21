//! A document: one open graph with everything it needs to edit and run it, for every shell
//! (stage N1, slice 7; `docs/shell-interface.md` 4.3, `docs/research/24-native-shells.md` 4.3).
//!
//! Before this, the pieces of an open document were fields of the egui editor (`EditorApp`), and
//! the C ABI's core held a second assembly of the same pieces with its own copies of the per-turn
//! work. A [`Document`] owns them once: the [`Session`], the engine [`Link`] (and the coordinator
//! with several engines), the engine worker and the switch in flight, the asks, the media service
//! following the link, the last error and the convert offer, the save in flight, and a run held
//! for an uploads prompt. Each shell holds one per window (the egui editor) or several (the C ABI's
//! core, by [`DocId`]), and calls it once per frame or wake.
//!
//! **The turn.** [`turn`] does what a frame or a wake does for a document, in order:
//!
//! 1. engine switching ([`control::step`]): what the worker brought back, finished; what the
//!    session asked for next, begun; old engines retired on the worker;
//! 2. startup ([`Document::start_engines`]): once the first engine has connected (or failed to),
//!    the graph it was waiting for opens, and the commands held meanwhile run;
//! 3. the engine's events into the session's run state ([`Document::drain_engine`]);
//! 4. [`pump`]: a due auto-run started, a review or uploads prompt raised, the session's runs,
//!    cancels and releases handed to the worker, a save sent on a thread of its own, the media
//!    service's frame;
//! 5. [`after`]: "Open in …" and documents edited in their apps, a rerun asked for.
//!
//! It returns a [`Turn`]: the events applied (the C ABI forwards them), the engine operations
//! that finished, what the shell must redo for what it keeps beside the core ([`Effects`]),
//! whether startup finished, and when to look again though nothing wakes. The egui shell calls the
//! same steps at the points of its frame where it called them before ([`pump`] twice a frame),
//! so nothing moves within a frame.
//!
//! **Commands.** [`execute`] is what `EditorApp::try_run` was: the session runs the command, then
//! the follow-ups every shell needs (the document's name, "opened with N problems", a watch that
//! runs, the convert offer, the notes of bundle, detach, expand, recipe copies), what only a shell
//! does (its [`Holder::shell_command`] and [`Holder::after_command`]: the egui shell's look,
//! fit and selection), and an engine switch asked for: begun on the worker, or waited for by a
//! caller that may wait ([`Holder::may_wait`]: tests and scripts outside a frame).
//!
//! **Starting without waiting.** [`Document::start_engines`] records the first engine as a switch:
//! the shell shows its window at once, in the "connecting" state, and the switch's waiting part
//! runs on the worker like any other. What needs the catalogue waits for it ([`Opening`]: the
//! demo, a workflow file), and a `graph.open` or `graph.new` asked for meanwhile is held, then run
//! in order, never refused.
//!
//! **Closing.** [`Document::close`] (and dropping one) withdraws its asks, answers the requests
//! waiting on it, cancels its run, and hands the engine link and coordinator to the worker, which
//! tells the engine and stops it there; the worker is then let go without a join, so the caller
//! never waits for an engine's shutdown (up to 250 ms for a pump, 2 s for a process engine's
//! grace). [`wait_closed`] waits (bounded) for what closed documents left their workers to do, for
//! a program about to exit, after its window has gone.
//!
//! **Several documents and one engine** (decision 11B). Each document has its own session, link,
//! worker, run ids and graph scopes. An engine compiled in may be shared ([`Starter`] decides):
//! the C ABI's core starts one per engine and gives every document a link of its own to it, so
//! the engine's single worker runs their runs in turn, its store and cache are common, and each
//! document applies only its own runs' events ([`Document::owns_run`]).

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use plumcraft_coordinator::Coordinator;
use plumcraft_graph::Graph;
use plumcraft_media::MediaService;
use plumcraft_proto::catalogue::SchemaReply;
use plumcraft_proto::event::{Envelope, Event};
use plumcraft_proto::run::{RunRequest, SaveReply};
use plumcraft_proto::session::EngineState;
use plumcraft_session::run::Outgoing;
use plumcraft_session::{EngineChange, EngineTarget, Session};
use serde_json::{Value, json};

use crate::asks::{Asks, By};
use crate::control::{self, Driver, Parked, Shell, View};
use crate::install::{Installer, Installing};
use crate::invoke::{self, Cont, Host, Prompt};
use crate::link::{self, Link, Waker};
use crate::prefs;
use crate::switchboard::{self, Effects, Reloader, Switchboard, Switching, Work, Worked};
use crate::values::Follow;
use crate::worker::EngineWorker;

/// A document's number: unique in the process, never reused.
pub type DocId = u64;

static NEXT_DOC: AtomicU64 = AtomicU64::new(1);

/// The next document's number.
pub fn next_id() -> DocId {
    NEXT_DOC.fetch_add(1, Ordering::Relaxed)
}

/// Starts engine `engine` hosted as `mode` and links to it, the link waking `waker`. It runs on
/// the document's engine worker (it is `Send`), unless the document starts engines compiled in
/// where it stands ([`Document::start_in_place`]).
pub type Starter = Box<dyn FnMut(&str, &str, Option<Waker>) -> Result<Link, String> + Send>;

/// Where the asks a document raised and ended go, as the C ABI's feed carries them (each item
/// with its `doc`).
pub type AsksSink = Box<dyn FnMut(Vec<Value>, Vec<Value>) + Send>;

/// The longest a caller that may wait (outside a frame) waits for an engine to start: the
/// slowest start the launcher allows (60 s), and then some.
pub const START_WAIT: Duration = Duration::from_secs(120);

/// How soon to look again at a run or save the application holds back (`Hooks::hold_runs`), whose
/// letting go wakes nothing.
pub const HELD_OUTBOX: Duration = Duration::from_millis(30);

/// How often to look for a save's answer, and at a run in flight.
const SOON: Duration = Duration::from_millis(100);

/// The most run ids a document remembers as its own ([`Document::owns_run`]).
const OWN_RUNS: usize = 64;

/// Commands that replace the graph, held while the first engine connects (they are titled and
/// checked against its catalogue).
const HELD_AT_STARTUP: &[&str] = &["graph.open", "graph.new"];

/// The most commands held at startup.
const MAX_HELD: usize = 64;

/// What opens once the first engine's catalogue has arrived ([`Document::start_engines`]).
#[derive(Debug)]
pub enum Opening {
    /// Nothing: the graph the session has stays.
    Nothing,
    /// This graph, as the session was made with it ([`Session::start_graph`]): the demo, a
    /// workflow read at launch.
    Graph(Graph),
    /// This graph, as one undo step named `label` (the C ABI's demo).
    Edit(String, Graph),
    /// A workflow file, opened as File › Open opens it (`graph.open`).
    Path(PathBuf),
}

/// A document waiting for its first engine.
#[derive(Debug)]
struct Startup {
    opening: Opening,
    /// Commands that replace the graph, asked for meanwhile: run in order once it is open.
    held: Vec<(String, Value)>,
}

/// What one turn did, for the shell to redraw, forward or schedule.
#[derive(Debug, Default)]
pub struct Turn {
    /// The engine's events applied to the run state, as they came.
    pub events: Vec<Envelope>,
    /// Engine operations (a switch, an add, a remove, …) that finished.
    pub outcomes: Vec<Result<Value, String>>,
    /// What the shell must redo for what it keeps beside the core.
    pub effects: Effects,
    /// A run's reply came: values reaped before may be there again.
    pub reread: bool,
    /// Startup finished: the first engine connected (or failed to) and the graph opened.
    pub ready: bool,
    /// Something was asked of the shell (a prompt raised).
    pub asked: bool,
    /// Look again within this long, though nothing wakes: an auto-run due, a held run, a save or
    /// a run in flight.
    pub again_in: Option<Duration>,
}

impl Turn {
    fn again(&mut self, d: Duration) {
        self.again_in = Some(self.again_in.map_or(d, |a| a.min(d)));
    }
}

/// One open document (see the module docs).
pub struct Document {
    pub id: DocId,
    pub session: Session,
    /// The engine connection, when one was started.
    pub link: Option<Link>,
    /// With engines added beside the first: the coordinator the link goes through.
    pub coordinator: Option<Arc<Coordinator>>,
    /// The engine's state as last read.
    pub engine: EngineState,
    /// The last command error, shown in the status bar.
    pub last_error: Option<String>,
    /// A refused wire's way through (`wire.convert`'s params and the path's label), offered
    /// beside the error (docs/research/20-convert.md).
    pub convert_offer: Option<(Value, String)>,
    /// The title's name for a graph with no file (`Untitled`, the demo, what it came from).
    pub document: String,
    /// What the core is asking the shell: file dialogs, prompts, forms, the clipboard.
    pub asks: Asks<Cont>,
    /// The apps the session asked to start, until they become `open.external` asks.
    pub launches: Option<invoke::Launches>,
    /// Every value shown, read and decoded off the shell's thread ([`MediaService`]).
    pub media_service: MediaService,
    /// Keeps the media service reading through the current engine link.
    pub media_follow: Follow,
    /// Whether the media service follows this document's link (a service shared by several
    /// documents has a resolver of its own).
    follows_media: bool,
    /// Every engine call the document makes, made off the shell's thread.
    pub engine_worker: EngineWorker,
    /// An engine switch under way.
    pub switching: Switching,
    /// Control requests waiting for their reply, for a shell that keeps none of its own.
    pub parked: Parked,
    /// Reads the engine manifests again (`engine.manifests.reload`).
    pub reloader: Option<Reloader>,
    /// Installs engines from GitHub (`engine.install`).
    pub installer: Option<Installer>,
    /// An install running in the background.
    pub installing: Option<Installing>,
    /// Starts the engines the Engine menu offers; with a switch on the worker, it travels with it.
    pub starter: Option<Starter>,
    /// Start an engine compiled in where the document stands, not on the worker (it waits for no
    /// process and no network). The egui shell keeps this for two kittests; see slice 3.
    pub start_in_place: bool,
    /// A rerun asked for while a run was in flight (a watched value's bytes were gone).
    pub rerun_pending: bool,
    /// The run waiting on [`Prompt::Uploads`].
    pub upload_held: Option<Box<RunRequest>>,
    /// The answer to the save in flight.
    saving: Option<Receiver<Result<SaveReply, String>>>,
    /// What wakes the shell: new links, the worker, the media service, a save's thread.
    waker: Option<Waker>,
    /// Quit or close was asked for, unsaved changes dealt with.
    pub close_asked: bool,
    /// The folders each kind of file ask last used, for a shell that keeps none (in memory).
    pub folders: prefs::Store,
    /// Saving the document by itself (`document.open {autosave: true}`, N4 slice 4).
    pub autosave: crate::autosave::Autosave,
    /// The look a core shares among its documents (preferences, theme, the system's appearance;
    /// the C ABI's, N4 slice 4): settings commands are carried out on it, and the scenes are built
    /// in its tokens. None: the shell keeps its own (the egui editor), or none (tests).
    pub look: Option<crate::settings::SharedLook>,
    /// The look's generation this document last took ([`crate::settings::follow`]).
    pub look_seen: u64,
    /// Watching a port pops its value out (N4 slice 4: a shell with no Watched panel, the Apple
    /// apps, where a watch with no window showed nothing): `run.watch {node, port}` from a tap on
    /// a pin or a Watch menu item ends with the value in a pop-out, brought forward if open.
    pub watch_pops_out: bool,
    /// What the shell must redo, gathered from the switchboard until the turn hands it over.
    effects: Effects,
    /// Waiting for the first engine.
    startup: Option<Startup>,
    /// The runs this document sent (newest last).
    own_runs: VecDeque<String>,
    /// Where raised and ended asks go, for a shell with a feed.
    asks_sink: Option<AsksSink>,
    /// The last command asked for an engine switch (a control request replies once it is done).
    engine_asked: bool,
    closed: bool,
    /// What the platform can start (the Engine menu lists what it can't, disabled with why).
    pub caps: crate::engines::Capabilities,
    /// The menu bar's generation (`menu.get`'s `gen`), up when what it holds changed.
    pub menus: crate::menu::Revision,
    /// What is selected on the canvas ([`crate::selection`]); changed only by the canvas
    /// commands ([`crate::canvas_cmds`]) and dropped as the graph loses what it named.
    pub selection: crate::selection::SelectionCell,
    /// The group entered (`canvas.focus`): only its members are shown and selectable.
    pub focus: Option<plumcraft_graph::GroupId>,
    /// The scene last published, with its revisions (`scene.get`, [`crate::scene`]).
    pub scene: crate::scene::Published,
    /// What the shell measured of the faces (`geometry.set`, [`crate::geometry`]; N2 slice 3).
    pub geometry: crate::geometry::Measured,
    /// The latest fit the core asked the shell's view for (`canvas.fit`): the scene's `view`.
    pub fit: Option<crate::scene::Fit>,
    /// A fit is owed once every face is measured (N2 slice 4): set by `graph.new` and
    /// `graph.open` with something to show, paid by the geometry report that measures the last
    /// face ([`Document::apply_geometry`]), since the core can't size a face until it is drawn.
    pub fit_owed: bool,
    /// What the hot calls read ([`crate::geometry::Snapshot`]), made again after a publish when
    /// what it is made from changed ([`Document::publish_scene`]).
    pub snapshot: std::sync::Arc<crate::geometry::Snapshot>,
    /// A drag under way (`ui.gesture`, N2 slice 4), shared with the C ABI's `plum_gesture_move`.
    pub drag: crate::gesture::DragCell,
    /// The node palette while it is open (`canvas.palette`, `palette.choose`; core state since N3
    /// slice 1): the scene's `view.palette`.
    pub palette: Option<crate::scene::Palette>,
    /// The row a field editor is open on (`row.edit`, N4 slice 2): the scene's `view.edit`.
    pub editing: Option<crate::inline::RowEdit>,
    /// The latest reveal asked for (`canvas.reveal`, N4 slice 3): the scene's `view.reveal`.
    pub reveal: Option<crate::scene::Reveal>,
    /// Select mode (`canvas.selectMode`, N4 slice 3): the scene's `view.select`.
    pub select_mode: bool,
    /// A name edited in place (`name.edit`, N4 slice 3): the scene's `view.name`.
    pub naming: Option<crate::naming::NameEdit>,
    /// Which use of each net Go to Next Use goes to next (`canvas.reveal {net}`), by net id.
    pub reveal_next: std::collections::HashMap<String, usize>,
    /// The latest snapshot, for readers on other threads (the C ABI's hot calls), set as it is made.
    pub snapshot_out: crate::geometry::SnapshotCell,
    /// The run animation ([`crate::run_anim`]): each node's body washed by its state while a run
    /// computes, and View › Animate Last Run's replay. Kept up to date every turn
    /// ([`Document::pump_run_anim`]); the egui shell draws from it, and a scene a native shell
    /// fetches carries it while it shows (N3 slice 4).
    pub run_anim: crate::run_anim::RunAnim,
    /// Who the `ui.invoke` being handled is for ([`Shell::invoke_by`]): the C ABI sets it to the
    /// person for the request a shell sends as `ui.invoke {by: "person"}`, and back after.
    pub invoke_by: crate::asks::By,
}

impl std::fmt::Debug for Document {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Document").field("id", &self.id).field("document", &self.document).field("engine", &self.engine).finish_non_exhaustive()
    }
}

impl Document {
    /// A document on `session`, with no engine yet, reading values with the default media limits.
    pub fn new(session: Session) -> Document {
        Document::with_media(session, MediaService::new(plumcraft_media::Config::default()), true)
    }

    /// A document reading values through `media`; `follow`: point it at this document's link
    /// (false for a service shared by several documents, with a resolver of its own).
    pub fn with_media(mut session: Session, media: MediaService, follow: bool) -> Document {
        let id = next_id();
        session.set_doc_id(id);
        // the session's app starts are asked for, and the shell starts them
        let launches = invoke::route_launches(&mut session);
        Document {
            id,
            session,
            link: None,
            coordinator: None,
            engine: EngineState::Off,
            last_error: None,
            convert_offer: None,
            document: control::UNTITLED.into(),
            asks: Asks::default(),
            launches,
            media_service: media,
            media_follow: Follow::default(),
            follows_media: follow,
            engine_worker: EngineWorker::default(),
            switching: Switching::default(),
            parked: Parked::default(),
            reloader: None,
            installer: None,
            installing: None,
            starter: None,
            start_in_place: false,
            rerun_pending: false,
            upload_held: None,
            saving: None,
            waker: None,
            close_asked: false,
            folders: prefs::Store::default(),
            autosave: crate::autosave::Autosave::default(),
            look: None,
            look_seen: 0,
            watch_pops_out: false,
            effects: Effects::default(),
            startup: None,
            own_runs: VecDeque::new(),
            asks_sink: None,
            engine_asked: false,
            closed: false,
            caps: crate::engines::Capabilities::DESKTOP,
            menus: crate::menu::Revision::default(),
            selection: crate::selection::SelectionCell::default(),
            focus: None,
            scene: crate::scene::Published::default(),
            geometry: crate::geometry::Measured::default(),
            fit: None,
            run_anim: crate::run_anim::RunAnim::default(),
            invoke_by: crate::asks::By::Script,
            fit_owed: false,
            snapshot: std::sync::Arc::default(),
            drag: crate::gesture::DragCell::default(),
            palette: None,
            select_mode: false,
            reveal: None,
            naming: None,
            reveal_next: std::collections::HashMap::new(),
            editing: None,
            snapshot_out: crate::geometry::SnapshotCell::default(),
        }
    }

    /// Use these limits for the media service; what it held is dropped.
    pub fn set_media(&mut self, config: plumcraft_media::Config) {
        self.media_service = MediaService::new(config);
        self.media_follow = Follow::default();
        self.follows_media = true;
        if let Some(w) = &self.waker {
            self.media_service.set_wake(Some(media_wake(w)));
        }
        self.follow_link();
    }

    /// What wakes the shell from now on (a repaint, a message to the core thread). Given to the
    /// engine worker and the media service, and to every link started after.
    pub fn set_waker(&mut self, waker: Option<Waker>) {
        self.engine_worker.set_waker(waker.clone());
        // (a service shared by several documents wakes its owner its own way)
        if self.follows_media {
            self.media_service.set_wake(waker.as_ref().map(media_wake));
        }
        self.waker = waker;
    }

    pub fn waker(&self) -> Option<&Waker> {
        self.waker.as_ref()
    }

    /// Send the asks raised (each with this document's id) and ended to `sink`, every turn and
    /// whenever a request may have asked something.
    pub fn set_asks_sink(&mut self, sink: Option<AsksSink>) {
        self.asks_sink = sink;
    }

    /// Link to an engine that is already running, reading its catalogue here and now: for
    /// callers that may wait (tests, scripts, an application's own setup), which read the
    /// catalogue at once. A shell's own startup uses [`Document::start_engines`] instead.
    pub fn link_now(&mut self, link: Link) -> Option<bool> {
        let first = match link.conn.call("schema", json!({})).map(serde_json::from_value::<SchemaReply>) {
            Ok(Ok(catalog)) => Some(switchboard::set_catalog(&mut self.session, catalog)),
            Ok(Err(e)) => {
                self.last_error = Some(format!("the engine's catalogue: {e}"));
                None
            }
            Err(e) => {
                self.last_error = Some(format!("schema: {}", e.message));
                None
            }
        };
        self.engine = link.state();
        switchboard::sync_state(&mut self.session, &self.engine);
        self.link = Some(link);
        self.follow_link();
        switchboard::share_connection(&mut self.session, &self.link);
        first
    }

    /// Start the first engine without waiting for it: `target` is recorded as a switch, which the
    /// next turn begins on the worker (the shell shows "connecting" meanwhile), `extras` are added
    /// after it, and `opening` opens once its catalogue has arrived (or it failed to start).
    pub fn start_engines(&mut self, target: EngineTarget, extras: Vec<EngineTarget>, opening: Opening) {
        self.session.engines.ask_switch(target);
        for t in extras {
            self.session.engines.ask_change(EngineChange::Add(t));
        }
        self.startup = Some(Startup { opening, held: Vec::new() });
    }

    /// Still waiting for the first engine?
    pub fn starting(&self) -> bool {
        self.startup.is_some()
    }

    /// Is an engine switch asked for, or under way?
    pub fn engine_busy(&self) -> bool {
        self.switching.busy() || self.session.engines.is_pending()
    }

    /// Was run `run_id` sent by this document? (Every document linked to one engine sees every
    /// run's events; each applies its own.)
    pub fn owns_run(&self, run_id: &str) -> bool {
        self.own_runs.iter().any(|r| r == run_id)
    }

    /// What the shell must redo, gathered since the last call.
    pub fn take_effects(&mut self) -> Effects {
        std::mem::take(&mut self.effects)
    }

    /// The media service reads through the current link (none: no engine), and the session's own
    /// reads (`doc.open`, `value.open`) share what it has read.
    fn follow_link(&mut self) {
        if self.follows_media {
            self.media_follow.follow(&self.media_service, self.link.as_ref().map(|l| &l.conn));
        }
        self.session.docs.resolver = self.link.as_ref().map(|_| crate::values::docs_resolver(&self.media_service));
    }

    /// The engine parts, lent to the core's switchboard for `f`; what it says to redo is kept for
    /// the turn ([`Document::take_effects`]).
    pub fn board<R>(&mut self, f: impl FnOnce(&mut Switchboard<'_>) -> R) -> R {
        let mut b = Switchboard {
            session: &mut self.session,
            link: &mut self.link,
            coordinator: &mut self.coordinator,
            state: &mut self.engine,
            last_error: &mut self.last_error,
            reloader: &mut self.reloader,
            installer: &self.installer,
            installing: &mut self.installing,
            switching: &mut self.switching,
            waker: self.waker.clone(),
            effects: Effects::default(),
        };
        let out = f(&mut b);
        let fx = b.effects;
        self.effects = merge(self.effects, fx);
        out
    }

    /// The engine worker, waking the shell.
    fn worker(&mut self) -> &mut EngineWorker {
        if !self.engine_worker.has_waker()
            && let Some(w) = &self.waker
        {
            self.engine_worker.set_waker(Some(w.clone()));
        }
        &mut self.engine_worker
    }

    /// A switch's waiting part: in place for an engine compiled in when the document starts
    /// those in place, else on the worker with the starter (which comes back with the result).
    pub fn start(&mut self, mut w: Work) -> Option<Worked> {
        if self.start_in_place && w.in_process() {
            let retired = w.take_retired();
            self.worker().retire(retired);
            let waker = self.waker.clone();
            let starter = &mut self.starter;
            return Some(w.run(&mut |engine: &str, mode: &str| match starter {
                Some(s) => s(engine, mode, waker.clone()),
                None => Err("this editor can't start engines".into()),
            }));
        }
        let carry: (Option<Starter>, Option<Waker>) = (self.starter.take(), self.waker.clone());
        self.worker().switch(w, carry, |(starter, waker): &mut (Option<Starter>, Option<Waker>), engine: &str, mode: &str| match starter {
            Some(s) => s(engine, mode, waker.clone()),
            None => Err("this editor can't start engines".into()),
        });
        None
    }

    /// The starter a switch carried, given back.
    pub fn take_back(&mut self, back: Box<dyn std::any::Any + Send>) {
        if let Ok(carry) = back.downcast::<(Option<Starter>, Option<Waker>)>() {
            self.starter = carry.0;
        }
    }

    /// An engine that just went down or away is logged, with what its stderr said last (a process
    /// engine's stderr goes to a tail the host keeps; this is where it reaches the log).
    fn log_engine_down(&self) {
        let source = match &self.session.engines.current {
            Some(EngineTarget::Choice { engine, .. }) => format!("engine.{engine}"),
            _ => "engine".to_owned(),
        };
        let text = match &self.engine {
            EngineState::EngineDown { stderr_tail: Some(tail), .. } if !tail.trim().is_empty() => {
                format!("{}\n{}", link::state_text(&self.engine), tail.trim_end())
            }
            EngineState::EngineDown { .. } | EngineState::Unavailable { .. } => link::state_text(&self.engine),
            _ => return,
        };
        crate::log::write(crate::log::Level::Warn, &source, &text, Some(self.id));
    }

    /// Engine housekeeping: the engine's state, its events into the session's run state. The
    /// events applied, and whether a run's reply came (values reaped before may be there again).
    pub fn drain_engine(&mut self) -> (Vec<Envelope>, bool) {
        switchboard::share_connection(&mut self.session, &self.link);
        let Some(link) = &self.link else {
            self.session.connected = false;
            self.session.docs.resolver = None;
            if self.follows_media {
                self.media_follow.follow(&self.media_service, None);
            }
            return (Vec::new(), false);
        };
        let was_down = matches!(self.engine, EngineState::EngineDown { .. } | EngineState::Unavailable { .. });
        self.engine = link.state();
        if !was_down {
            self.log_engine_down();
        }
        switchboard::sync_state(&mut self.session, &self.engine);
        self.follow_link();
        let Some(link) = &self.link else { return (Vec::new(), false) };
        let events = link.drain();
        let now = Instant::now();
        let mut reread = false;
        for e in &events {
            if self.session.run.apply_event(e, now) && matches!(e.event, Event::RunReply(_)) {
                reread = true;
                self.media_service.forget_misses();
            }
        }
        if !events.is_empty() {
            // what an application's value editor will read, asked for now, off this thread
            crate::values::read_ahead(&self.media_service, &self.session.run);
        }
        if self.session.run.running && matches!(self.engine, EngineState::EngineDown { .. } | EngineState::Unavailable { .. }) {
            self.session.run.fail(&format!("the engine went away: {}", link::state_text(&self.engine)));
        }
        (events, reread)
    }

    /// Is command `id` enabled now?
    /// Forget the selected nodes, groups and net, and the group entered, that the graph no
    /// longer has (after every command; an undo can take them away too).
    pub fn forget_gone(&mut self) {
        let g = self.session.graph();
        self.selection.update(|s| s.retain_existing(g));
        if self.focus.as_ref().is_some_and(|f| g.group(f).is_none()) {
            self.focus = None;
        }
        // the field editor's row goes with its node
        if self.editing.as_ref().is_some_and(|e| !g.nodes.contains_key(&plumcraft_graph::NodeId::from(e.node.as_str()))) {
            self.editing = None;
        }
    }

    /// What Ungroup acts on: the first selected collapsed group, else the innermost group of the
    /// first selected node in one, else the group entered.
    pub fn group_to_ungroup(&self) -> Option<plumcraft_graph::GroupId> {
        let g = self.session.graph();
        let sel = self.selection.get();
        sel.groups
            .iter()
            .find(|id| g.group(id).is_some())
            .cloned()
            .or_else(|| sel.nodes.iter().find_map(|n| g.group_of(n).cloned()))
            .or_else(|| self.focus.clone().filter(|f| g.group(f).is_some()))
    }

    pub fn enabled(&self, id: &str) -> bool {
        plumcraft_session::find(id).is_some_and(|c| (c.enabled)(&self.session).is_ok())
    }

    /// What `wire.convert` would place for a refused `wire.connect`, if anything.
    fn offer_for(&self, params: &Value) -> Option<(Value, String)> {
        let port = |k: &str| -> Option<plumcraft_graph::PortRef> {
            let v = params.get(k)?;
            Some(plumcraft_graph::PortRef::new(plumcraft_graph::NodeId(v.get("node")?.as_str()?.to_owned()), v.get("port")?.as_str()?))
        };
        let (from, to) = (port("from")?, port("to")?);
        let best = plumcraft_session::convert::offers(&self.session, &from, &to).into_iter().next()?;
        let label = best.get("label")?.as_str()?.to_owned();
        let p = json!({"from": params["from"], "to": params["to"], "kinds": best["kinds"]});
        Some((p, label))
    }

    /// Send the asks raised and ended since the last look to the sink, if there is one.
    pub fn feed_asks(&mut self) {
        if self.asks_sink.is_none() {
            return;
        }
        invoke::tidy(self);
        let (mut raised, ended) = self.asks.take_feed();
        if raised.is_empty() && ended.is_empty() {
            return;
        }
        // an ask raised names its document (`docs/shell-interface.md` 11.1); an ended one is
        // `{ask, ended}` as before (ask ids are the document's own)
        for v in &mut raised {
            if let Some(o) = v.as_object_mut() {
                o.insert("doc".into(), json!(self.id));
            }
        }
        if let Some(sink) = &mut self.asks_sink {
            sink(raised, ended);
        }
    }

    /// Close: withdraw the asks, answer what waits on the document, cancel its run, and hand its
    /// engine to the worker to tell and stop, off the caller's thread; then let the worker go
    /// without waiting for it ([`wait_closed`] can, bounded). Nothing here waits for an engine.
    /// Done once; dropping a document closes it.
    pub fn close(&mut self) {
        if std::mem::replace(&mut self.closed, true) {
            return;
        }
        drop(self.asks.withdraw_all());
        self.feed_asks();
        self.parked.close("the document closed");
        self.startup = None;
        // the run in flight, and anything queued, go to the engine with it: its cancel first
        if self.session.run.running {
            self.session.run.cancel();
        }
        self.board(|b| {
            b.retire_coordinator();
            b.forget_runs(Instant::now());
        });
        let retired = self.switching.take_retired();
        self.engine_worker.retire(retired);
        self.session.set_connection(None);
        self.session.docs.resolver = None;
        if self.follows_media {
            self.media_follow.follow(&self.media_service, None);
            self.media_service.set_wake(None);
        }
        // a switch's link that comes back later is dropped on the worker, never here
        self.engine_worker.close();
    }
}

impl Drop for Document {
    fn drop(&mut self) {
        self.close();
    }
}

/// Wait at most `timeout` for what closed documents left their engine workers to do (cancelling,
/// stopping engines): whether all of it is done. For a program about to exit, once its windows
/// have gone; never a frame.
pub fn wait_closed(timeout: Duration) -> bool {
    crate::worker::wait_closed(timeout)
}

fn merge(a: Effects, b: Effects) -> Effects {
    Effects {
        runs_forgotten: a.runs_forgotten || b.runs_forgotten,
        catalog_set: match (a.catalog_set, b.catalog_set) {
            (Some(x), Some(y)) => Some(x || y),
            (x, y) => x.or(y),
        },
        redraw: a.redraw || b.redraw,
    }
}

fn media_wake(w: &Waker) -> Arc<dyn Fn() + Send + Sync> {
    let w = w.clone();
    Arc::new(move || w.wake())
}

/// What holds a document: the document itself (the C ABI's core, tests), or a shell around it
/// (the egui editor), which adds what only it does around a command.
pub trait Holder: Driver {
    fn doc(&self) -> &Document;
    fn doc_mut(&mut self) -> &mut Document;
    /// A command the shell answers itself once the session has run it (the egui shell's look).
    fn shell_command(&mut self, _id: &str, v: Value) -> Result<Value, String> {
        Ok(v)
    }
    /// What the shell redoes after a command succeeded (the egui shell: fit, selection, watched).
    fn after_command(&mut self, _id: &str, _v: &Value) {}
    /// May this call wait for an engine (tests and scripts outside a frame)?
    fn may_wait(&self) -> bool {
        false
    }
    /// The last command asked for an engine switch (or didn't).
    fn note_engine_asked(&mut self, _asked: bool) {}
}

/// A command, as the editor runs any (see the module docs): its error is kept for the status bar.
pub fn execute<H: Holder>(h: &mut H, id: &str, params: &Value) -> Result<Value, String> {
    let doc_id = h.doc().id;
    if let Some(st) = &mut h.doc_mut().startup
        && HELD_AT_STARTUP.contains(&id)
    {
        if st.held.len() >= MAX_HELD {
            return Err("still connecting the engine: too many commands are waiting".into());
        }
        st.held.push((id.to_owned(), params.clone()));
        return Ok(json!({"held": true, "until": "the engine has connected"}));
    }
    // the canvas's own commands (the selection, the group entered, fit, the palette): the core's
    // and the shell's, not the session's
    if let Some(c) = crate::canvas_cmds::find(id) {
        return crate::canvas_cmds::execute(h, c, params);
    }
    let result = plumcraft_session::commands::with_doc(doc_id, || h.doc_mut().session.execute(id, params));
    // reading the messages in full leaves them (and the offer) as they are
    let keeps = id == "view.messages";
    // an offer lasts until the next command
    if !keeps {
        h.doc_mut().convert_offer = None;
    }
    let v = match result {
        Ok(v) => v,
        Err(e) => {
            let e = e.to_string();
            let d = h.doc_mut();
            d.last_error = Some(e.clone());
            if id == "wire.connect" {
                d.convert_offer = d.offer_for(params);
            }
            return Err(e);
        }
    };
    if !keeps {
        h.doc_mut().last_error = None;
    }
    // the shell's own (the egui shell's look)
    let v = match h.shell_command(id, v) {
        Ok(v) => v,
        Err(e) => {
            h.doc_mut().last_error = Some(e.clone());
            return Err(e);
        }
    };
    let d = h.doc_mut();
    if id == "wire.convert"
        && let Some(w) = v.get("warning").and_then(Value::as_str)
    {
        d.last_error = Some(w.to_owned());
    }
    if id == "graph.new" || id == "graph.open" {
        if id == "graph.new" {
            d.document = control::UNTITLED.into();
        }
        if let Some(n) = v.get("problemCount").and_then(Value::as_u64).filter(|n| *n > 0) {
            let first = v.get("problems").and_then(|p| p.get(0)).and_then(Value::as_str).unwrap_or_default();
            d.last_error = Some(format!("opened with {n} problem(s): {first}"));
        }
        // what's missing is the banner's, for as long as it is; that says more than the first
        // node it left unknown
        if v.get("missingEngines").is_some() {
            d.last_error = None;
        }
        // a graph with no file of its own is still known by what it came from
        if let Some(from) = v.get("openedFrom").and_then(Value::as_str) {
            d.document = std::path::Path::new(from).file_name().map_or_else(|| from.to_owned(), |n| n.to_string_lossy().into_owned());
        }
    }
    // nothing of the old graph stays selected or entered; and what the graph lost goes
    let d = h.doc_mut();
    if id == "graph.new" || id == "graph.open" {
        d.selection.update(crate::selection::Selection::clear);
        d.selection.update(|s| s.net = None);
        d.focus = None;
        // a new graph's faces are measured afresh, and no drag goes on across it; a fit is owed
        // once they are measured, when there is anything to show
        d.geometry.clear();
        d.drag.set(None);
        d.palette = None;
        d.editing = None;
        d.naming = None;
        let graph = d.session.graph();
        d.fit_owed = !graph.nodes.is_empty() || !graph.frames.is_empty();
    } else {
        d.forget_gone();
    }
    h.after_command(id, &v);
    if id == "run.watch" && h.doc().watch_pops_out && params.get("port").and_then(Value::as_str).is_some() {
        // no Watched panel here: the value is shown in its pop-out (opened, or brought forward)
        let mut p = json!({"node": params.get("node").cloned().unwrap_or(Value::Null), "port": params["port"].clone()});
        if let Some(i) = params.get("index").filter(|i| !i.is_null()) {
            p["index"] = i.clone();
        }
        let _ = h.run("view.popout", &p);
    }
    if id == "run.watch" {
        // only what this node needs: a click to look at it runs nothing downstream
        if h.doc().enabled("run.start") {
            let node = v.get("node").cloned().unwrap_or(Value::Null);
            let _ = h.run("run.start", &json!({"targets": [node]}));
        }
    }
    let d = h.doc_mut();
    if id == "bundle.detach" {
        let unresolved: Vec<&str> = v.get("unresolved").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_str).collect();
        if !unresolved.is_empty() {
            d.last_error = Some(format!("detached, but these came from wires upstream and weren't copied: {}", unresolved.join(", ")));
        }
    }
    if id == "viewer.recipe"
        && params.get("copy").and_then(Value::as_bool) == Some(true)
        && let Some(text) = v.get("recipe").filter(|r| !r.is_null()).and_then(|r| serde_json::to_string_pretty(r).ok())
    {
        // on the clipboard next frame (the core asks; the shell answers)
        invoke::copy(h, &text, "text", By::Person);
    }
    let d = h.doc_mut();
    if id == "graph.bundle"
        && let Some(n) = v.get("missing").and_then(Value::as_array).map(Vec::len).filter(|n| *n > 0)
    {
        d.last_error = Some(format!("bundled, but {n} path(s) couldn't go in: {}", v["missing"][0]["path"].as_str().unwrap_or_default()));
    }
    if id == "node.expand"
        && let Some(said) = expand_note(&v)
    {
        d.last_error = Some(said);
    }
    // The session recorded an engine switch: perform it, so the caller gets its outcome. A pick
    // can take steps (out, then in again the new way); the caller gets the last, and how the pick
    // was taken. A caller that may not wait has what waits for an engine go to the worker, and
    // the outcome comes later (a control request waits for it); one that may, waits.
    let asked = d.session.engines.is_pending();
    h.note_engine_asked(asked);
    let done = if !asked {
        None
    } else if !h.may_wait() {
        let mut done = None;
        let mut out = Vec::new();
        control::step(h, &mut out);
        for r in out {
            done = Some(r?);
        }
        done
    } else {
        control::engines_now(h, START_WAIT).transpose()?
    };
    match done {
        Some(mut out) => {
            if let (Some(how), Some(o)) = (v.get("how"), out.as_object_mut()) {
                o.insert("how".into(), how.clone());
            }
            Ok(out)
        }
        None => Ok(v),
    }
}

/// What an expansion couldn't carry over, for the status bar. `None` when it all came across.
pub fn expand_note(v: &Value) -> Option<String> {
    let list = |key: &str| v.get(key).and_then(Value::as_array).map_or(0, Vec::len);
    let mut parts = Vec::new();
    if list("dropped") > 0 {
        let why = v["dropped"][0]["reason"].as_str().unwrap_or_default();
        parts.push(format!("{} wire(s) dropped ({why})", list("dropped")));
    }
    if list("lost") > 0 {
        parts.push(format!("{} group parameter(s) or result(s) lost", list("lost")));
    }
    if list("unresolved") > 0 {
        parts.push(format!("{} setting(s) from wires upstream used the node's own value", list("unresolved")));
    }
    (!parts.is_empty()).then(|| format!("expanded, but: {}", parts.join("; ")))
}

/// `run.start`, when it is enabled (an engine connected, a graph to run).
pub fn start_run_if_enabled<H: Holder>(h: &mut H) -> Option<Value> {
    if h.doc().enabled("run.start") { h.run("run.start", &json!({})).ok() } else { None }
}

/// Once the first engine has connected (or failed to start) and nothing more is asked of the
/// engines: open what waited for it, and run the commands held meanwhile. True when this was it.
pub fn settle_startup<H: Holder>(h: &mut H) -> bool {
    if h.doc().startup.is_none() || h.doc().engine_busy() {
        return false;
    }
    let Some(st) = h.doc_mut().startup.take() else { return false };
    let d = h.doc_mut();
    match st.opening {
        Opening::Nothing => {}
        Opening::Graph(g) => {
            d.session.start_graph(g);
            d.effects.redraw = true;
            h.fit();
        }
        Opening::Edit(label, g) => {
            let r = d.session.edit(&label, |graph, _| {
                *graph = g;
                Ok(())
            });
            if let Err(e) = r {
                d.last_error = Some(e.to_string());
            }
            d.effects.redraw = true;
        }
        Opening::Path(p) => {
            let _ = h.run("graph.open", &json!({"path": p}));
        }
    }
    // picking engines automatically: what the graph opened names, started now it is there
    if h.doc().session.engines.auto {
        let _ = h.run("engine.auto", &json!({"on": true}));
    }
    for (id, p) in st.held {
        let _ = h.run(&id, &p);
    }
    true
}

/// Run housekeeping before the shell's own (see the module docs): a due auto-run started, a review
/// prompt raised, what the session queued handed to the worker (or held for the uploads prompt),
/// a save sent, the media service's frame.
pub fn pump<H: Holder>(h: &mut H) -> Turn {
    let mut t = Turn::default();
    let now = Instant::now();
    match h.doc().session.run.auto_due_in(now) {
        Some(d) if d.is_zero() => {
            // (when nothing can run now, no engine or an empty graph, it stays due and runs on a
            // later turn once it can; nothing is scheduled meanwhile)
            start_run_if_enabled(h);
        }
        Some(d) => t.again(d),
        None => {}
    }
    if h.doc().session.run.awaiting_review() && invoke::open_prompt(&h.doc().asks).is_none() {
        let review = Box::new(h.doc().session.review());
        invoke::raise_prompt(h, Prompt::Review(review), By::Person);
        t.asked = true;
    }
    // Sent by the engine worker, in order (a cancel after the run it cancels), never here: the
    // shell doesn't wait for an engine to take a run. A refusal comes back to the step, which
    // fails the run if it is still the latest.
    let outbox = h.doc_mut().session.run.take_outbox();
    for out in outbox {
        let d = h.doc_mut();
        match out {
            Outgoing::Run(req) => {
                let body = serde_json::to_value(&*req);
                // files for an engine nobody here started: ask first, and send this run after
                // (the host's own list: no call to the engine)
                let pending = match (&d.link, &body) {
                    (Some(link), Ok(body)) => link.conn.uploads_pending(body),
                    _ => Vec::new(),
                };
                if !pending.is_empty() {
                    d.upload_held = Some(req);
                    invoke::raise_prompt(h, Prompt::Uploads(pending), By::Person);
                    t.asked = true;
                    continue;
                }
                match (d.link.as_ref().map(|l| Arc::clone(&l.conn)), body) {
                    (Some(conn), Ok(body)) => {
                        if d.own_runs.len() >= OWN_RUNS {
                            d.own_runs.pop_front();
                        }
                        d.own_runs.push_back(req.run_id.clone());
                        d.worker().run(conn, body, req.run_id.clone(), req.generation);
                    }
                    (None, _) => d.session.run.fail("run: the engine is not connected"),
                    (_, Err(e)) => d.session.run.fail(&format!("run: {e}")),
                }
            }
            Outgoing::Release(scope) => {
                if let Some(conn) = d.link.as_ref().map(|l| Arc::clone(&l.conn)) {
                    // an engine without the capability has nothing to release
                    d.worker().call(conn, "graph.release", json!({"scope": scope}));
                }
            }
            Outgoing::Cancel(run_id) => {
                if let Some(conn) = d.link.as_ref().map(|l| Arc::clone(&l.conn)) {
                    // the run's late records are ignored by generation whatever the reply
                    d.worker().cancel(conn, run_id);
                }
            }
        }
    }
    if !h.doc().session.run.outbox_is_empty() {
        t.again(HELD_OUTBOX);
    }
    pump_saves(h, &mut t);
    // the document saved by itself once a change has settled (N4 slice 4)
    if let Some(again) = crate::autosave::tick(h.doc_mut(), now) {
        t.again(again);
    }
    // a value arriving from the media service's workers wakes the shell, and what no turn asks
    // for any more is dropped before it is read
    h.doc().media_service.frame();
    t
}

/// Send a queued save on a thread of its own, and take its answer when it comes.
fn pump_saves<H: Holder>(h: &mut H, t: &mut Turn) {
    for req in h.doc_mut().session.save.take_outbox() {
        let d = h.doc_mut();
        let Some(conn) = d.link.as_ref().map(|l| Arc::clone(&l.conn)) else {
            d.session.save.fail("the engine is not connected");
            continue;
        };
        let pending = serde_json::to_value(&req).map(|b| conn.uploads_pending(&b)).unwrap_or_default();
        if !pending.is_empty() {
            d.session.save.fail("this save would send files to the engine: allow them, then save again");
            invoke::raise_prompt(h, Prompt::Uploads(pending), By::Person);
            t.asked = true;
            continue;
        }
        let (tx, rx) = channel();
        let wake = d.waker.clone();
        let sent = std::thread::Builder::new().name("plumcraft-save".into()).spawn(move || {
            let answer = serde_json::to_value(&req)
                .map_err(|e| e.to_string())
                .and_then(|body| conn.call("output.save", body).map_err(|e| e.message))
                .and_then(|v| serde_json::from_value::<SaveReply>(v).map_err(|e| format!("the save reply: {e}")));
            let _ = tx.send(answer);
            if let Some(w) = wake {
                w.wake();
            }
        });
        match sent {
            Ok(_) => d.saving = Some(rx),
            Err(e) => d.session.save.fail(&e.to_string()),
        }
    }
    let d = h.doc_mut();
    if !d.session.save.outbox_is_empty() {
        t.again(HELD_OUTBOX);
    }
    let Some(rx) = &d.saving else { return };
    match rx.try_recv() {
        Ok(Ok(reply)) => d.session.save.apply_reply(reply),
        Ok(Err(why)) => d.session.save.fail(&why),
        Err(TryRecvError::Disconnected) => d.session.save.fail("the save stopped without an answer"),
        Err(TryRecvError::Empty) => {
            t.again(SOON);
            return;
        }
    }
    d.saving = None;
}

/// Run housekeeping after the shell's own: an "Open in …" or Open With waiting for its value, a
/// document saved in its app, a rerun asked for. When to look again, though nothing wakes.
pub fn after<H: Holder>(h: &mut H) -> Option<Duration> {
    let mut t = Turn::default();
    let d = h.doc_mut();
    // an "Open in …" waiting for its value (session doc.open)
    match plumcraft_session::doc_cmds::poll_doc_open(&mut d.session) {
        Some(Err(e)) => d.last_error = Some(e),
        Some(Ok(_)) => d.last_error = None,
        None => {}
    }
    // a document saved in its app: fed downstream and run (session doc_edits)
    match plumcraft_session::doc_edits::poll_doc_edits(&mut d.session, Instant::now()) {
        Some(Err(e)) => d.last_error = Some(e),
        Some(Ok(_)) => d.last_error = None,
        None => {}
    }
    if plumcraft_session::doc_edits::watching(&d.session) {
        t.again(plumcraft_session::doc_edits::POLL);
    }
    // an Open or Open with… waiting for its value (session value.open)
    match plumcraft_session::open_cmds::poll_value_open(&mut d.session) {
        Some(Err(e)) => d.last_error = Some(e),
        Some(Ok(_)) => d.last_error = None,
        None => {}
    }
    if d.rerun_pending && !d.session.run.running {
        d.rerun_pending = false;
        start_run_if_enabled(h);
    }
    if h.doc().session.run.running {
        t.again(SOON);
    }
    t.again_in
}

/// One turn of a document (see the module docs): what a frame or a wake does.
pub fn turn<H: Holder>(h: &mut H) -> Turn {
    let mut outcomes = Vec::new();
    control::step(h, &mut outcomes);
    let ready = settle_startup(h);
    let (events, reread) = plumcraft_session::commands::with_doc(h.doc().id, || h.doc_mut().drain_engine());
    let mut t = pump(h);
    h.doc_mut().pump_run_anim(std::time::Instant::now());
    if let Some(d) = after(h) {
        t.again(d);
    }
    let d = h.doc_mut();
    d.feed_asks();
    t.events = events;
    t.reread = reread;
    t.outcomes = outcomes;
    t.ready = ready;
    t.effects = d.take_effects();
    t
}

/// A document is its own shell, with nothing drawn: no canvas, so no selection or geometry; asks
/// raised go to its sink ([`Document::set_asks_sink`]).
impl Host for Document {
    fn session(&self) -> &Session {
        &self.session
    }
    fn session_mut(&mut self) -> &mut Session {
        &mut self.session
    }
    fn asks(&mut self) -> &mut Asks<Cont> {
        &mut self.asks
    }
    fn run(&mut self, id: &str, params: &Value) -> Result<Value, String> {
        execute(self, id, params)
    }
    fn say(&mut self, text: String) {
        self.last_error = Some(text);
    }
    fn prefs(&mut self) -> Option<&mut prefs::Store> {
        Some(&mut self.folders)
    }
    fn selection(&self) -> (Vec<String>, Vec<plumcraft_graph::GroupId>) {
        let s = self.selection.get();
        (s.nodes.into_iter().map(|n| n.0).collect(), s.groups)
    }
    fn group_to_ungroup(&self) -> Option<plumcraft_graph::GroupId> {
        Document::group_to_ungroup(self)
    }
    fn sizes(&self, nodes: &[String], groups: &[plumcraft_graph::GroupId]) -> std::collections::BTreeMap<String, [f32; 2]> {
        let nodes: Vec<plumcraft_graph::NodeId> = nodes.iter().map(|n| plumcraft_graph::NodeId::from(n.as_str())).collect();
        self.geometry.sizes_for(&nodes, groups)
    }
    /// Tidy's measurements, from what the shell reported (`geometry.set`).
    fn measured(&self) -> (serde_json::Map<String, Value>, serde_json::Map<String, Value>) {
        let texts = std::collections::HashMap::new();
        let faces = crate::scene::faces(&self.scene_inputs(&crate::theme::DEFAULT, None, &texts, &[]));
        self.geometry.for_tidy(&faces)
    }
    fn close(&mut self) {
        self.close_asked = true;
    }
    fn closing(&self) -> bool {
        self.close_asked
    }
    fn allow_uploads(&mut self, files: &[String]) {
        if let Some(link) = &self.link {
            link.conn.allow_uploads(files);
        }
    }
    fn take_held_run(&mut self) -> Option<Box<RunRequest>> {
        self.upload_held.take()
    }
    fn take_launches(&mut self) -> Vec<(PathBuf, Vec<String>, PathBuf)> {
        invoke::drain_launches(self.launches.as_ref())
    }
}

impl Shell for Document {
    fn view(&self) -> View<'_> {
        View {
            link: self.link.as_ref(),
            state: &self.engine,
            coordinator: self.coordinator.as_ref(),
            switching: &self.switching,
            last_error: self.last_error.as_deref(),
            asks: &self.asks,
            document: &self.document,
            app_title: control::APP_TITLE,
            run_anim: None,
            prefs: Some(&self.folders),
            theme: None,
        }
    }
    fn parked(&mut self) -> &mut Parked {
        &mut self.parked
    }
    fn clear_last_error(&mut self) {
        self.last_error = None;
    }
    fn presented(&mut self) {
        self.feed_asks();
    }
    fn request<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        self.engine_asked = false;
        f(self)
    }
    fn engine_asked(&self) -> bool {
        self.engine_asked
    }
    fn invoke_by(&self) -> crate::asks::By {
        self.invoke_by
    }
    fn menu_facts(&self) -> crate::menu::Facts {
        let s = self.selection.get();
        crate::menu::Facts {
            selected: s.nodes,
            selected_groups: s.groups,
            live: self.link.is_some(),
            caps: self.caps,
            select_mode: self.select_mode,
            ..crate::menu::Facts::default()
        }
    }
    fn menu_revision(&mut self) -> Option<&mut crate::menu::Revision> {
        Some(&mut self.menus)
    }
    /// The scene in the document's theme ([`Document::scene_theme`]), with the run animation
    /// while it shows ([`Document::shown_anim`]) and no application editors (this shell draws
    /// none).
    fn scene(&self) -> Option<crate::scene::Scene> {
        Some(self.build_scene(&self.scene_theme(), self.shown_anim(std::time::Instant::now()), &crate::scene::app_texts(), &[]))
    }
    /// `theme.get`: the tokens of the theme the scenes are built in.
    fn theme_tokens(&self) -> Option<Value> {
        let t = self.scene_theme();
        let mut v = control::theme_reply(t.name, &t);
        if let Some(look) = &self.look {
            let l = crate::settings::lock(look);
            v["chosen"] = json!(l.chosen());
            v["system"] = l.system.json();
            v["gen"] = json!(l.generation());
        }
        Some(v)
    }
    fn scene_published(&mut self) -> Option<&mut crate::scene::Published> {
        Some(&mut self.scene)
    }
    fn document_mut(&mut self) -> Option<&mut Document> {
        Some(self)
    }
}

impl Document {
    /// The theme this document's scenes are built in when the shell has none of its own (the C
    /// ABI's core, N3 slice 3): the one `view.theme {name}` last named, so a native shell that
    /// follows the system's appearance names Paper or Dusk; else the default. A name that is no
    /// theme is the default. Not kept between runs (the egui shell keeps its own in preferences).
    /// Bring the run animation up to date with the session's run: a new run is shown as it
    /// happens, View › Animate Last Run (`view.animateRun`) replays the last one slowly, and the
    /// real run's end is noted. Called every turn ([`turn`]); doing it twice changes nothing.
    /// Whether it still shows (the shell keeps drawing while it does).
    pub fn pump_run_anim(&mut self, now: std::time::Instant) -> bool {
        use crate::run_anim;
        if std::mem::take(&mut self.session.views.animate_asked) && !self.session.run.running {
            let run = &self.session.run;
            let plan = run_anim::plan(&self.session, &run.needed);
            let slow = std::time::Duration::from_millis(u64::from(self.session.views.anim_ms()));
            self.run_anim.replay(run.generation, &run.needed, plan, slow, &run.log, now);
        }
        let run = &self.session.run;
        if run.generation != self.run_anim.generation() && !run.needed.is_empty() {
            let plan = run_anim::plan(&self.session, &run.needed);
            self.run_anim.begin(run.generation, &run.needed, plan, std::time::Duration::ZERO);
        }
        let run = &self.session.run;
        if run.generation == self.run_anim.generation() {
            self.run_anim.feed(&run.log);
            if !run.running {
                self.run_anim.end(now);
            }
        }
        (!self.session.views.run_colors_off || self.run_anim.is_replay()) && self.run_anim.is_active(now)
    }

    /// The run animation for a scene a native shell fetches: while it shows at `now` (View ›
    /// Colour Nodes While Running on, or a replay, and not yet faded), else none, so the faces'
    /// `shown` goes once the colours have faded and a scene at rest carries no animation (N3
    /// slice 4). The shell computes the wash, its pulse and its fade from the scene's `anim`
    /// (contract 8.6); the core never publishes per frame.
    pub fn shown_anim(&self, now: std::time::Instant) -> Option<&crate::run_anim::RunAnim> {
        ((!self.session.views.run_colors_off || self.run_anim.is_replay()) && self.run_anim.is_active(now)).then_some(&self.run_anim)
    }

    /// The tokens this document's scenes are built in: the shared look's ([`crate::settings::Look::tokens`],
    /// the theme chosen or the system's, with the person's changes and the system's
    /// accessibility settings), else (no look) the theme `view.theme {name}` last named.
    pub fn scene_theme(&self) -> crate::theme::Tokens {
        if let Some(look) = &self.look {
            return crate::settings::lock(look).tokens();
        }
        self.session.views.theme_asked.as_deref().and_then(crate::theme::by_name).unwrap_or(crate::theme::DEFAULT)
    }

    /// The scene of this document now ([`crate::scene::build`]) in `tokens`' colours: the
    /// selection, the group entered and the Watched panel's node are the document's; the run
    /// animation, the application's lines and its editors are what the shell shows.
    pub fn build_scene(
        &self,
        tokens: &crate::theme::Tokens,
        anim: Option<&crate::run_anim::RunAnim>,
        app_text: &std::collections::HashMap<String, String>,
        editors: &[crate::scene::Editor],
    ) -> crate::scene::Scene {
        crate::scene::build(&self.scene_inputs(tokens, anim, app_text, editors))
    }

    /// What [`Document::build_scene`] builds from.
    pub fn scene_inputs<'a>(
        &'a self,
        tokens: &'a crate::theme::Tokens,
        anim: Option<&'a crate::run_anim::RunAnim>,
        app_text: &'a std::collections::HashMap<String, String>,
        editors: &'a [crate::scene::Editor],
    ) -> crate::scene::Inputs<'a> {
        let watched = self.session.run.panel().map(|(w, _)| plumcraft_graph::NodeId(w.node.clone()));
        crate::scene::Inputs {
            doc: self.id,
            graph: self.session.graph(),
            catalog: self.session.catalog(),
            run: &self.session.run,
            selection: self.selection.get(),
            focus: self.focus.as_ref(),
            watched,
            tokens,
            anim,
            now: std::time::Instant::now(),
            app_text,
            editors,
            view: crate::scene::View {
                fit: self.fit.clone(),
                palette: self.palette.clone(),
                edit: self.editing.clone(),
                select: self.select_mode,
                reveal: self.reveal.clone(),
                windows: crate::windows::of(&self.session),
                name: self.naming.clone(),
            },
            geometry: Some(&self.geometry),
        }
    }
}

impl Document {
    /// Publish `scene` (a build of this document's), and make the hot calls' snapshot again if
    /// what it is made from changed. Every publish goes through here (`scene.get`, the C ABI's
    /// feed, the `geometry.*` queries). Faces gone from the scene are forgotten by the geometry.
    pub fn publish_scene(&mut self, scene: crate::scene::Scene) -> u64 {
        let generation = self.scene.publish(scene);
        if self.geometry.len() > self.scene.face_count() {
            let scene = &self.scene;
            self.geometry.retain(|id| scene.face(id).is_some());
        }
        let key = self.snapshot_key();
        if self.snapshot.key != key || self.snapshot.doc != self.id || (self.snapshot.generation == 0 && generation > 0) {
            self.snapshot = std::sync::Arc::new(self.scene.snapshot(key, self.session.graph(), self.session.catalog(), &self.geometry));
            if self.snapshot.generation > 0 {
                self.snapshot_out.set(std::sync::Arc::clone(&self.snapshot));
            }
        }
        generation
    }

    /// What the snapshot is made from: the session's generation (every command, a new
    /// catalogue), the graph's revision, the geometry's generation, the group entered.
    fn snapshot_key(&self) -> (u64, u64, u64, Option<String>) {
        (self.session.generation(), self.session.graph().revision(), self.geometry.generation(), self.focus.as_ref().map(|g| g.0.clone()))
    }

    /// Keep what the shell measured (`geometry.set`): `{applied, stale, unknown}`. A face the
    /// graph doesn't have is skipped (`unknown`); one measured at an older revision than the
    /// scene's is kept all the same (an older measurement beats the default) and listed
    /// (`stale`), so the shell measures it again.
    pub fn apply_geometry(&mut self, reports: Vec<crate::geometry::Report>) -> Value {
        let (mut applied, mut stale, mut unknown) = (0u64, Vec::new(), Vec::new());
        let graph = self.session.graph();
        let known = |id: &str| match id.strip_prefix("group:") {
            Some(g) => graph.group(&plumcraft_graph::GroupId::from(g)).is_some(),
            None => graph.nodes.contains_key(&plumcraft_graph::NodeId::from(id)),
        };
        let mut keep = Vec::with_capacity(reports.len());
        for r in reports {
            if !known(&r.face) {
                if unknown.len() < 64 {
                    unknown.push(r.face);
                }
                continue;
            }
            if let (Some(rev), Some(now)) = (r.rev, self.scene.face_rev(&r.face))
                && rev < now
            {
                stale.push(r.face.clone());
            }
            keep.push(r);
        }
        for r in keep {
            self.geometry.set(&r.face, r.geometry);
            applied += 1;
        }
        // a shell that measures draws the scene: the feed says when it changes
        self.scene.watched = true;
        // the fit a new graph owed, once every face it shows is measured (the scene's view.fit)
        if self.fit_owed && self.scene.face_count() > 0 && self.scene.face_ids().values().all(|id| self.geometry.get(id).is_some()) {
            self.fit_owed = false;
            self.ask_fit();
        }
        json!({"applied": applied, "stale": stale, "unknown": unknown})
    }

    /// The rect a fit shows now ([`crate::geometry::fit_rect`]): every face shown at the size the
    /// core has for it, and the frames outside an entered group.
    pub fn fit_rect(&self) -> Option<[f64; 4]> {
        let texts = std::collections::HashMap::new();
        let faces = crate::scene::faces(&self.scene_inputs(&crate::theme::DEFAULT, None, &texts, &[]));
        let rects = faces.iter().map(|f| crate::geometry::face_rect(f.pos, self.geometry.get(&f.id)));
        let graph = self.session.graph();
        let frames = graph
            .frames
            .values()
            .filter(|_| self.focus.is_none())
            .map(|f| crate::geometry::face_rect(f.pos, Some(&crate::geometry::FaceGeometry { size: f.size, ..Default::default() })));
        crate::geometry::fit_rect(rects, frames)
    }

    /// Ask the shell's view for a fit (`canvas.fit`): `{fit: true, seq, rect}` (rect null when
    /// there is nothing to fit, and then nothing is asked).
    pub fn ask_fit(&mut self) -> Value {
        let Some(rect) = self.fit_rect() else { return json!({"fit": true, "seq": self.fit.as_ref().map_or(0, |f| f.seq), "rect": null}) };
        let seq = self.fit.as_ref().map_or(0, |f| f.seq).wrapping_add(1);
        self.fit = Some(crate::scene::Fit { seq, rect });
        json!({"fit": true, "seq": seq, "rect": rect})
    }
}

impl Driver for Document {
    fn worker(&mut self) -> &mut EngineWorker {
        Document::worker(self)
    }
    fn switching(&mut self) -> &mut Switching {
        &mut self.switching
    }
    fn board<R>(&mut self, f: impl FnOnce(&mut Switchboard<'_>) -> R) -> R {
        Document::board(self, f)
    }
    fn start(&mut self, work: Work) -> Option<Worked> {
        Document::start(self, work)
    }
    fn take_back(&mut self, back: Box<dyn std::any::Any + Send>) {
        Document::take_back(self, back);
    }
}

impl Holder for Document {
    fn doc(&self) -> &Document {
        self
    }
    fn doc_mut(&mut self) -> &mut Document {
        self
    }
    /// No canvas is drawn here: fit, the palette and reveal are refused, as `ui.set` is.
    fn shell_command(&mut self, id: &str, v: Value) -> Result<Value, String> {
        if crate::canvas_cmds::is_shells(id) {
            return Err(control::not_here(id));
        }
        // a core with a shared look carries the settings commands out itself (N4 slice 4)
        match self.look.clone() {
            Some(look) => crate::settings::after_command(self, &look, id, v),
            None => Ok(v),
        }
    }
    fn note_engine_asked(&mut self, asked: bool) {
        self.engine_asked = asked;
    }
}

/// Engines compiled in, shared by the documents of one core (decision 11B): a [`Starter`] that
/// gives each document a link of its own to the one engine of each kind, starting it for the
/// first, and starts everything else afresh with `start`.
pub fn shared_starter(shared: SharedEngines, mut start: impl FnMut(&str, &str, Option<Waker>) -> Result<Link, String> + Send + 'static) -> Starter {
    Box::new(move |engine: &str, mode: &str, waker: Option<Waker>| {
        if mode != "inproc" {
            return start(engine, mode, waker);
        }
        if let Some((conn, label)) = shared.get(engine) {
            return Link::start(conn, label, waker);
        }
        let link = start(engine, mode, waker)?;
        shared.put(engine, &link);
        Ok(link)
    })
}

/// The engines compiled in that a core's documents share, by engine id: held weakly, so an engine
/// stops once no document links to it.
#[derive(Clone, Default)]
pub struct SharedEngines(Arc<Mutex<Vec<SharedEngine>>>);

/// One shared engine: its id, the connection (held weakly) and its link's label.
type SharedEngine = (String, Weak<dyn plumcraft_host::Connection>, String);

impl SharedEngines {
    fn get(&self, engine: &str) -> Option<(Arc<dyn plumcraft_host::Connection>, String)> {
        let mut v = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        v.retain(|(_, w, _)| w.strong_count() > 0);
        v.iter().find(|(e, _, _)| e == engine).and_then(|(_, w, label)| w.upgrade().map(|c| (c, label.clone())))
    }

    fn put(&self, engine: &str, link: &Link) {
        let mut v = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        v.retain(|(e, w, _)| w.strong_count() > 0 && e != engine);
        v.push((engine.to_owned(), Arc::downgrade(&link.conn), link.label.clone()));
    }

    /// How many shared engines are running.
    pub fn running(&self) -> usize {
        let v = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        v.iter().filter(|(_, w, _)| w.strong_count() > 0).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    use plumcraft_proto::data::DataRef;
    use plumcraft_proto::error::ProtoError;
    use plumcraft_store::Blob;

    fn starter() -> Starter {
        Box::new(|e: &str, m: &str, w: Option<Waker>| crate::engines::Launcher::default().connect_ids(e, m, w))
    }

    fn wait(d: &mut Document, what: &str, done: impl Fn(&Document) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            turn(d);
            if done(d) {
                return;
            }
            assert!(Instant::now() < deadline, "{what}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn text_target() -> EngineTarget {
        EngineTarget::Choice { engine: "text".into(), mode: "inproc".into() }
    }

    #[test]
    fn view_theme_picks_the_scenes_theme_and_theme_get_follows() {
        let mut d = Document::new(Session::default());
        let paper = d.theme_tokens().unwrap();
        assert_eq!(paper["name"], "paper");
        d.session.views.theme_asked = Some("dusk".into());
        let dusk = d.theme_tokens().unwrap();
        assert_eq!(dusk["name"], "dusk");
        assert_ne!(dusk["tokens"]["canvas"], paper["tokens"]["canvas"], "Dusk's canvas isn't Paper's");
        assert_eq!(d.scene_theme().name, "dusk");
        // the old name reads as the theme that replaced it; one that is no theme is the default
        d.session.views.theme_asked = Some("dark".into());
        assert_eq!(d.scene_theme().name, "dusk");
        d.session.views.theme_asked = Some("chartreuse".into());
        assert_eq!(d.theme_tokens().unwrap()["name"], "paper");
    }

    #[test]
    fn documents_have_their_own_ids_and_the_session_knows_it() {
        let (a, b) = (Document::new(Session::default()), Document::new(Session::default()));
        assert_ne!(a.id, b.id);
        assert_eq!((a.session.doc_id(), b.session.doc_id()), (a.id, b.id));
        assert_eq!(plumcraft_session::commands::current_doc(), None);
        let seen = plumcraft_session::commands::with_doc(a.id, plumcraft_session::commands::current_doc);
        assert_eq!(seen, Some(a.id));
        assert_eq!(plumcraft_session::commands::current_doc(), None, "restored");
    }

    #[test]
    fn startup_waits_for_the_catalogue_then_opens_and_runs_what_was_held() {
        let mut d = Document::new(Session::default());
        d.starter = Some(starter());
        let demo = Graph::from_json(&crate::engines::EngineKind::Text.own_demo().unwrap().unwrap()).unwrap();
        let nodes = demo.nodes.len();
        d.start_engines(text_target(), Vec::new(), Opening::Graph(demo));
        assert!(d.starting() && d.engine_busy());
        // the turn begins the switch on the worker and returns at once
        let t0 = Instant::now();
        let t = turn(&mut d);
        assert!(t0.elapsed() < Duration::from_secs(1) && !t.ready);
        // a graph.new meanwhile is held, not refused
        assert_eq!(d.run("graph.new", &json!({"discard": true})).unwrap()["held"], true);
        assert!(d.session.graph().nodes.is_empty(), "nothing open yet");
        wait(&mut d, "the engine to connect", |d| !d.starting());
        assert!(d.session.connected && !d.session.catalog().nodes.is_empty());
        // the demo opened, then the held graph.new replaced it
        assert!(d.session.graph().nodes.is_empty(), "{nodes} demo nodes, then a new graph");
        assert_eq!(d.document, "Untitled");
        assert!(!d.session.is_dirty());
    }

    #[test]
    fn a_failed_first_engine_still_opens_the_graph_and_says_why() {
        let mut d = Document::new(Session::default());
        d.starter = Some(Box::new(|_: &str, _: &str, _: Option<Waker>| Err("no such program".into())));
        d.start_engines(text_target(), Vec::new(), Opening::Graph(Graph::new()));
        wait(&mut d, "startup to end", |d| !d.starting());
        assert!(d.last_error.as_deref().unwrap_or_default().contains("no such program"), "{:?}", d.last_error);
        assert!(!d.session.connected);
    }

    /// An engine whose calls hang until let go, counting them.
    struct Hung(Arc<AtomicUsize>, Arc<(Mutex<bool>, std::sync::Condvar)>);
    impl plumcraft_host::Connection for Hung {
        fn call(&self, method: &str, _: Value) -> Result<Value, ProtoError> {
            if method == "events.poll" {
                std::thread::sleep(Duration::from_millis(20));
                return Ok(json!({"events": [], "last_seq": 0}));
            }
            self.0.fetch_add(1, Ordering::SeqCst);
            let (m, cv) = &*self.1;
            let g = m.lock().unwrap();
            let _g = cv.wait_timeout_while(g, Duration::from_secs(20), |go| !*go).unwrap();
            Ok(json!({}))
        }
        fn state(&self) -> EngineState {
            EngineState::Off
        }
        fn resolve(&self, _: &DataRef) -> Option<Blob> {
            None
        }
    }

    #[test]
    fn closing_cancels_the_run_and_never_waits_for_the_engine() {
        let calls = Arc::new(AtomicUsize::new(0));
        let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
        let mut d = Document::new(Session::new(plumcraft_text::engine().schema()));
        let conn: Arc<dyn plumcraft_host::Connection> = Arc::new(Hung(Arc::clone(&calls), Arc::clone(&gate)));
        d.link = Some(Link::start(conn, "hung", None).unwrap());
        d.session.connected = true;
        d.run("node.add", &json!({"kind": "text.literal", "pos": [0, 0]})).unwrap();
        d.run("run.start", &json!({})).unwrap();
        turn(&mut d);
        assert!(d.session.run.running);
        // the run is with the worker, stuck in the engine; closing waits for none of it
        let t0 = Instant::now();
        d.close();
        drop(d);
        assert!(t0.elapsed() < Duration::from_millis(200), "closed in {:?}", t0.elapsed());
        // let the engine go: the cancel follows the run, on the worker
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        assert!(wait_closed(Duration::from_secs(10)), "the worker finished");
        assert!(calls.load(Ordering::SeqCst) >= 2, "the run, then its cancel: {}", calls.load(Ordering::SeqCst));
    }

    #[test]
    fn two_documents_share_one_engine_compiled_in_and_run_their_own_graphs() {
        let shared = SharedEngines::default();
        let mut docs: Vec<Document> = (0..2)
            .map(|_| {
                let mut d = Document::new(Session::default());
                d.starter = Some(shared_starter(shared.clone(), |e: &str, m: &str, w: Option<Waker>| crate::engines::Launcher::default().connect_ids(e, m, w)));
                d.start_engines(text_target(), Vec::new(), Opening::Nothing);
                d
            })
            .collect();
        for d in &mut docs {
            wait(d, "connected", |d| !d.starting() && d.session.connected);
        }
        assert_eq!(shared.running(), 1, "one engine for both");
        let a = Arc::as_ptr(&docs[0].link.as_ref().unwrap().conn).cast::<()>();
        let b = Arc::as_ptr(&docs[1].link.as_ref().unwrap().conn).cast::<()>();
        assert_eq!(a, b);
        for (i, d) in docs.iter_mut().enumerate() {
            let lit = d.run("node.add", &json!({"kind": "text.literal", "pos": [0, 0]})).unwrap()["id"].as_str().unwrap().to_owned();
            d.run("node.set", &json!({"node": lit, "port": "text", "value": format!("doc {i}")})).unwrap();
            let up = d.run("node.add", &json!({"kind": "text.upper", "pos": [200, 0]})).unwrap()["id"].as_str().unwrap().to_owned();
            d.run("wire.connect", &json!({"from": {"node": lit, "port": "text"}, "to": {"node": up, "port": "text"}})).unwrap();
            d.run("run.watch", &json!({"node": up, "port": "text"})).unwrap();
        }
        for d in &mut docs {
            wait(d, "the run", |d| !d.session.run.running && d.session.run.outbox_is_empty() && d.session.run.panel().and_then(|(_, v)| v).is_some());
        }
        for (i, d) in docs.iter().enumerate() {
            let shown: String = d
                .session
                .run
                .panel()
                .and_then(|(_, v)| v)
                .into_iter()
                .flat_map(|v| v.renditions.iter())
                .filter_map(|r| match r {
                    plumcraft_proto::display::Rendition::Excerpt { text, .. } => Some(text.clone()),
                    _ => None,
                })
                .collect();
            assert!(shown.contains(&format!("DOC {i}")), "doc {i} shows its own result: {shown}");
        }
        // closing one leaves the other its engine
        let mut gone = docs.remove(0);
        gone.close();
        drop(gone);
        let d = &mut docs[0];
        d.run("run.start", &json!({})).unwrap();
        wait(d, "a run after the other closed", |d| !d.session.run.running && d.session.run.outbox_is_empty());
        assert!(d.session.run.run_error.is_none(), "{:?}", d.session.run.run_error);
    }
}
