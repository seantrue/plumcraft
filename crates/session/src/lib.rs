//! The editor session and its command registry. Every user-visible action is a command
//! ([`commands::CommandSpec`]) run through [`Session::execute`], so the UI, the control channel
//! and tests drive the editor the same way, and every edit is journaled.
//!
//! Lifted from the research software's `research-engine` (2026-10-27, the research software `8d96021`; plan 10.3, E1.2)
//! without its bridge or plugin export; undo and redo, files
//! (`graph.open`, `graph.save`, `graph.saveAs`), copy and paste and relative paths came with E2;
//! groups (`group.*`) and inserting a workflow (`graph.insert`) with E3; switching the engine
//! (`engine.*`, the session records the request and the host connects) after E3.
//! The session never talks to an engine: run commands leave requests in [`RunState`]'s outbox,
//! and whatever hosts the session sends them over a `Connection` and feeds the events back
//! ([`RunState::apply_event`]).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod archive;
pub mod bundle_cmds;
pub mod commands;
pub mod convert;
pub mod doc_cmds;
pub mod doc_edits;
mod edit_cmds;
pub mod engine_cmds;
pub mod expand_cmds;
pub mod file_cmds;
mod frame_cmds;
mod graph_cmds;
mod group_cmds;
pub mod insert_cmds;
pub mod labels_cmds;
pub mod layout;
pub mod layout_cmds;
pub mod named_cmds;
mod net_cmds;
pub mod open_cmds;
pub mod paths;
pub mod run;
pub mod run_cmds;
pub mod save_cmds;
pub mod settings_cmds;
pub mod trust;
pub mod view_cmds;
pub mod viewer_cmds;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use plumcraft_graph::{Graph, GraphError, NodeId};
use plumcraft_proto::catalogue::SchemaReply;
use serde::Serialize;
use serde_json::Value;

pub use commands::{CommandSpec, command_specs, find};
pub use doc_cmds::{DocApp, DocApps};
pub use edit_cmds::PASTE_OFFSET;
pub use engine_cmds::{EngineChange, EngineChoice, EngineTarget, Engines};
pub use file_cmds::MAX_WORKFLOW_BYTES;
pub use run::RunState;
pub use run_cmds::default_output;

/// How many undo steps are kept.
pub const UNDO_LIMIT: usize = 200;

/// The title node `id` shows on the canvas (its id when the graph doesn't have it).
pub fn node_title(graph: &Graph, catalog: &SchemaReply, id: &NodeId) -> String {
    graph.node_title(id, |kind| display_name(catalog, kind)).unwrap_or_else(|| id.to_string())
}

/// Every node's title, as [`node_title`] gives each, in one pass ([`Graph::node_titles`]).
pub fn node_titles(graph: &Graph, catalog: &SchemaReply) -> std::collections::BTreeMap<plumcraft_graph::NodeId, String> {
    graph.node_titles(|kind| display_name(catalog, kind))
}

/// A node type's display name, or its id when the catalog has none: the stem of new nodes'
/// titles.
pub fn display_name(catalog: &SchemaReply, kind: &str) -> String {
    catalog.node(kind).map(|d| d.display.clone()).filter(|d| !d.is_empty()).unwrap_or_else(|| kind.to_owned())
}

/// Errors running a command. They are values the UI shows; nothing here panics.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("unknown command {0:?}")]
    UnknownCommand(String),
    #[error("{id} is not available: {reason}")]
    Disabled { id: String, reason: String },
    #[error("bad parameters: {0}")]
    Params(String),
    #[error(transparent)]
    Graph(#[from] GraphError),
    #[error("{0}")]
    File(String),
    /// `graph.new` or `graph.open` over unsaved changes, without `discard: true`.
    #[error("the graph has unsaved changes: save it first, or pass discard: true")]
    Unsaved,
}

pub type Result<T, E = SessionError> = std::result::Result<T, E>;

/// One executed command.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct JournalEntry {
    pub id: String,
    pub params: Value,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// The most journal entries kept.
pub const JOURNAL_LIMIT: usize = 10_000;

#[derive(Debug, Clone)]
struct Snapshot {
    label: String,
    graph: Graph,
    /// Which step this is: rises with every step pushed (a move that follows one may join it,
    /// [`MoveMark`]).
    serial: u64,
}

/// How far apart two moves of the same things may come, naming no drag, and still be one undo
/// step: a held arrow key, a script's run of `node.move`s (N3 slice 1).
pub const MOVE_COALESCE: std::time::Duration = std::time::Duration::from_secs(1);

/// The undo step a move made, for the next move to join ([`Session::edit_move`]).
#[derive(Debug, Clone, PartialEq)]
pub struct MoveMark {
    /// What was moved (`node:<id>`, `group:<id>`, `frame:<id>`, `size:<frame id>`), sorted.
    pub items: Vec<String>,
    /// The drag that moved them, when one was named.
    pub drag: Option<String>,
    /// When the last move joined it.
    pub at: std::time::Instant,
    /// The step's [`Snapshot::serial`].
    step: u64,
}

impl MoveMark {
    /// Does a move of `items` by `drag` at `now` join this step? The rule (N3 slice 1): the same
    /// things, and either the same drag named, or no drag named by either and at most
    /// [`MOVE_COALESCE`] since the last move joined. A move naming another drag, or a drag where
    /// the step named none (or none where it named one), is a step of its own.
    pub fn joins(&self, items: &[String], drag: Option<&str>, now: std::time::Instant) -> bool {
        self.items == items
            && match (self.drag.as_deref(), drag) {
                (Some(a), Some(b)) => a == b,
                (None, None) => now.saturating_duration_since(self.at) <= MOVE_COALESCE,
                _ => false,
            }
    }
}

/// What is wrong with `graph` against `catalog`: nodes of kinds it doesn't have, wires that
/// don't type-check, and a broken group. A workflow opened with problems keeps them (they are reported); an
/// edit may not add one.
pub fn graph_problems(graph: &Graph, catalog: &SchemaReply) -> BTreeSet<String> {
    let mut out: BTreeSet<String> =
        graph.nodes.iter().filter(|(_, n)| catalog.node(&n.kind).is_none()).map(|(id, n)| format!("node {id}: unknown kind {:?}", n.kind)).collect();
    out.extend(graph.wire_problems(catalog).into_iter().map(|(w, why)| format!("wire {} -> {}: {why}", w.from, w.to)));
    if let Err(e) = graph.check_groups(Some(catalog)) {
        out.insert(format!("groups: {e}"));
    }
    out
}

/// What an application says about itself: its About window and `--version`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct About {
    /// The title ("PlumImpress").
    pub title: String,
    pub version: String,
    /// What it was built from: a name and its version, in order.
    pub parts: Vec<(String, String)>,
}

/// What a graph needs that no connected engine has.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MissingEngines {
    /// The engines the file names that aren't connected, and those the host knows offer the
    /// missing kinds ([`Engines::offered`]). Empty when neither says.
    pub engines: Vec<String>,
    /// The node types nothing here offers.
    pub kinds: Vec<String>,
    /// All that, in a sentence.
    pub text: String,
}

/// How many node types [`MissingEngines::text`] lists.
const KINDS_SAID: usize = 4;

/// An open graph and everything about it.
#[derive(Debug, Clone)]
pub struct Session {
    /// The catalogue with core's nodes and the display labels ([`labels_cmds`]).
    catalog: SchemaReply,
    /// The same before the labels: what the engines said.
    unlabelled: SchemaReply,
    /// Display labels for ports ([`labels_cmds`]).
    pub labels: labels_cmds::LabelSources,
    graph: Graph,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// The serial the next undo step gets.
    next_step: u64,
    /// The move step on top of the undo stack, while it is there ([`Session::edit_move`]).
    last_move: Option<MoveMark>,
    journal: Vec<JournalEntry>,
    generation: u64,
    /// The workflow file this graph was opened from or last saved to.
    path: Option<PathBuf>,
    /// Changed since it was opened or saved.
    dirty: bool,
    /// Run state: per-node results, staleness, auto-run, requests for the engine.
    pub run: RunState,
    /// The host has a connected engine (runs are offered only then).
    pub connected: bool,
    /// The engines the host offers, the one it has, and a switch asked for
    /// ([`engine_cmds`]).
    pub engines: Engines,
    /// The apps that open document values, and an open waiting for its value ([`doc_cmds`]).
    pub docs: DocApps,
    /// Pop-out windows and the viewers in use ([`view_cmds`]).
    pub views: view_cmds::Views,
    /// The save in flight and the last one ([`save_cmds`]).
    pub save: save_cmds::SaveState,
    /// What the connected engine can do (`hello`'s caps): an application's commands' `enabled`
    /// reads them, never an engine's name (spec 5.3).
    pub caps: plumcraft_proto::caps::Caps,
    /// The application, for its About window.
    pub about: About,
    /// The trust file ([`trust`]); without one nothing is held for review.
    trust_file: Option<PathBuf>,
    /// The graph was read from a file, so whether it may run is a question.
    from_file: bool,
    /// The window's engine connection, as the editor last linked it ([`Session::connection`]).
    connection: SharedConnection,
    /// The document this session is, among the editor's open documents (0: none named). The
    /// application's `before` and `after` hooks read it ([`Session::doc_id`]).
    doc: u64,
}

/// The window's engine connection, held for an application's commands. Debug says only whether
/// there is one.
#[derive(Clone, Default)]
struct SharedConnection(Option<std::sync::Arc<dyn plumcraft_host::Connection>>);

impl std::fmt::Debug for SharedConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_some() { "SharedConnection(linked)" } else { "SharedConnection(none)" })
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new(SchemaReply::default())
    }
}

/// `catalog` with core's own nodes: Convert, offered beside the engines'
/// (docs/research/20-convert.md). An empty catalogue (no engine yet) stays empty.
fn with_core_nodes(mut catalog: SchemaReply) -> SchemaReply {
    if !catalog.nodes.is_empty() && !catalog.nodes.iter().any(|n| n.id == plumcraft_proto::convert::KIND) {
        catalog.nodes.push(plumcraft_proto::convert::def().clone());
    }
    catalog
}

impl Session {
    /// A session with an empty graph and the node types in `catalog`.
    pub fn new(catalog: SchemaReply) -> Self {
        let catalog = with_core_nodes(catalog);
        Session {
            catalog: catalog.clone(),
            unlabelled: catalog,
            labels: labels_cmds::LabelSources::default(),
            graph: Graph::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            next_step: 1,
            last_move: None,
            journal: Vec::new(),
            generation: 0,
            path: None,
            dirty: false,
            run: RunState::for_new_graph(),
            connected: false,
            engines: Engines::default(),
            docs: DocApps::default(),
            views: view_cmds::Views::default(),
            save: save_cmds::SaveState::default(),
            caps: Default::default(),
            about: About::default(),
            trust_file: None,
            from_file: false,
            connection: SharedConnection::default(),
            doc: 0,
        }
    }

    /// The window's engine connection (one engine, or the coordinator over several), for an
    /// application's commands: work of their own on the engines the window has, such as a
    /// background job or a runner. `None` while no engine is linked. The editor sets it whenever
    /// it links, relinks or unlinks ([`Session::set_connection`]).
    ///
    /// Hold the `Arc` no longer than the work needs: an engine the editor started stops only once
    /// the last holder lets its connection go, so a switch of engine waits on a clone kept here.
    pub fn connection(&self) -> Option<std::sync::Arc<dyn plumcraft_host::Connection>> {
        self.connection.0.clone()
    }

    /// Set (or, with `None`, drop) the connection [`Session::connection`] hands out. The editor
    /// calls it; an application doesn't need to.
    pub fn set_connection(&mut self, conn: Option<std::sync::Arc<dyn plumcraft_host::Connection>>) {
        self.connection.0 = conn;
    }

    /// Hold graphs from elsewhere until they are approved, with approvals in `path` ([`trust`]).
    /// A graph already opened from a file is held to it too.
    pub fn set_trust_file(&mut self, path: PathBuf) {
        self.trust_file = Some(path);
        if self.from_file {
            self.check_trust();
        }
    }

    /// Does this graph, just read from a file, need approving?
    fn check_trust(&mut self) {
        let Some(file) = &self.trust_file else { return };
        self.run.review_due = !trust::is_trusted(file, &trust::graph_hash(&self.graph));
    }

    /// `other`, from a file, has joined this graph: unless it is trusted, the whole graph needs
    /// approving.
    pub(crate) fn check_trust_of(&mut self, other: &Graph) {
        if let Some(file) = &self.trust_file
            && !trust::is_trusted(file, &trust::graph_hash(other))
        {
            self.run.review_due = true;
        }
    }

    /// What this graph touches ([`trust::review`]).
    pub fn review(&self) -> trust::Review {
        let mut r = trust::review(&self.graph, &self.catalog);
        r.pending = self.run.review_due;
        r
    }

    /// Approve this graph: what was held is dropped, and the next run goes out. Returns whether
    /// the approval was written down;
    /// without that it lasts as long as the session.
    pub fn trust_graph(&mut self) -> bool {
        self.run.release();
        self.trust_file.as_ref().is_some_and(|f| trust::remember(f, &trust::graph_hash(&self.graph)).is_ok())
    }

    /// A graph with no past: nothing held for the old one, and no way back to it past a review.
    pub(crate) fn start_afresh(&mut self) {
        if self.run.review_due {
            self.run.decline_review();
            self.run.review_due = false;
            self.undo.clear();
            self.redo.clear();
        }
    }

    /// A session on `graph` (a workflow the app opened), its serials assigned.
    pub fn with_graph(catalog: SchemaReply, mut graph: Graph) -> Self {
        graph.assign_serials(&BTreeSet::new(), |kind| display_name(&catalog, kind));
        Session { graph, ..Session::new(catalog) }
    }

    /// The document this session is (`docs/shell-interface.md` 4.3): what an application's hooks
    /// tell two documents apart by (decision 3). 0 when the editor named none.
    pub fn doc_id(&self) -> u64 {
        self.doc
    }

    /// Name the document this session is.
    pub fn set_doc_id(&mut self, doc: u64) {
        self.doc = doc;
    }

    /// Show `graph` as the one the session was made with ([`Session::with_graph`]), once the
    /// catalogue it is titled against has arrived: no undo step, nothing unsaved, no file. For a
    /// shell that opens its window before its engine has connected.
    pub fn start_graph(&mut self, mut graph: Graph) {
        let catalog = &self.catalog;
        graph.assign_serials(&BTreeSet::new(), |kind| display_name(catalog, kind));
        self.graph = graph;
        self.undo.clear();
        self.redo.clear();
        self.dirty = false;
        self.path = None;
    }

    pub fn graph(&self) -> &Graph {
        &self.graph
    }

    pub fn catalog(&self) -> &SchemaReply {
        &self.catalog
    }

    /// Replace the node catalog (a new `schema` reply, an engine switched). The graph is kept.
    pub fn set_catalog(&mut self, catalog: SchemaReply) {
        self.unlabelled = with_core_nodes(catalog);
        self.relabel();
    }

    /// The catalogue again from what the engines said, with the labels as they are now.
    pub(crate) fn relabel(&mut self) {
        let mut catalog = self.unlabelled.clone();
        self.labels.combined().apply(&mut catalog);
        self.catalog = catalog;
        self.generation = self.generation.wrapping_add(1);
    }

    /// Bumped by every command that succeeds, so a view knows when to refresh.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// Has the graph changed since it was opened or saved?
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// The workflow file this graph was opened from or last saved to.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Every graph undo and redo can still bring back.
    pub fn history(&self) -> impl Iterator<Item = &Graph> {
        self.undo.iter().chain(&self.redo).map(|s| &s.graph)
    }

    /// The label of the step `edit.undo` would undo.
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|s| s.label.as_str())
    }

    /// The label of the step `edit.redo` would redo.
    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|s| s.label.as_str())
    }

    /// Undo steps held (at most [`UNDO_LIMIT`]).
    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }

    /// What is wrong with the graph against the catalogue ([`graph_problems`]).
    pub fn problems(&self) -> BTreeSet<String> {
        graph_problems(&self.graph, &self.catalog)
    }

    /// What this graph needs that no connected engine has, naming the engines when the file does
    /// (a node's `engine`, the workflow's `engines`, spec 8.2; a recipe saved across engines holds
    /// them, spec 15) or when the host knows which engine offers a kind without starting it
    /// ([`Engines::offered`], [`engine_cmds::engines_used`]). `None` when every node's kind is
    /// offered.
    pub fn missing_engines(&self) -> Option<MissingEngines> {
        let unknown = self.graph.nodes.values().filter(|n| self.catalog.node(&n.kind).is_none());
        let (mut kinds, mut pinned) = (BTreeSet::new(), BTreeSet::new());
        for n in unknown {
            kinds.insert(n.kind.clone());
            pinned.extend(n.engine.clone());
        }
        if kinds.is_empty() {
            return None;
        }
        let here: BTreeSet<&str> = self
            .engines
            .engine_ids()
            .into_iter()
            .chain(std::iter::once(self.catalog.engine.id.as_str()))
            .chain(self.catalog.nodes.iter().flat_map(|d| d.engines.iter().map(String::as_str)))
            .collect();
        let named = pinned.iter().chain(&self.graph.engines);
        let mut engines: Vec<String> = Vec::new();
        for e in named {
            if !here.contains(e.as_str()) && !engines.contains(e) {
                engines.push(e.clone());
            }
        }
        // and the engines the host knows offer the kinds that are missing, though the file doesn't
        // name them (decision 8C: start only the engines the graph uses)
        let unknown = self.graph.nodes.values().filter(|n| self.catalog.node(&n.kind).is_none());
        let prefer: Vec<&str> = self.engines.engine_ids();
        let offered_here: engine_cmds::Offered =
            self.engines.offered.iter().filter(|(e, _)| !here.contains(e.as_str())).map(|(e, k)| (e.clone(), k.clone())).collect();
        for e in engine_cmds::engines_used(unknown.map(|n| (n.kind.as_str(), n.engine.as_deref())), &self.graph.engines, &offered_here, &prefer) {
            if !here.contains(e.as_str()) && !engines.contains(&e) {
                engines.push(e);
            }
        }
        let kinds: Vec<String> = kinds.into_iter().collect();
        let mut listed = kinds.iter().take(KINDS_SAID).cloned().collect::<Vec<_>>().join(", ");
        if kinds.len() > KINDS_SAID {
            listed.push_str(&format!(" and {} more", kinds.len() - KINDS_SAID));
        }
        let text = if engines.is_empty() {
            format!("No connected engine has {listed}, and the file doesn't say which engine does.")
        } else {
            let (startable, not): (Vec<&String>, Vec<&String>) = engines.iter().partition(|e| self.engines.choices.iter().any(|c| &c.engine == *e));
            let join = |l: &[&String]| l.iter().map(|e| e.as_str()).collect::<Vec<_>>().join(", ");
            let mut text = format!("This graph needs the engine {}, which isn't connected ({listed}).", engines.join(", "));
            if !startable.is_empty() {
                text.push_str(&format!(" The Engine menu offers {}.", join(&startable)));
            }
            if !not.is_empty() {
                text.push_str(&format!(" This application can't start {}.", join(&not)));
            }
            text
        };
        Some(MissingEngines { engines, kinds, text })
    }

    pub fn journal(&self) -> &[JournalEntry] {
        &self.journal
    }

    /// Run command `id` with `params`: check it exists and is enabled, run it, and journal it.
    pub fn execute(&mut self, id: &str, params: &Value) -> Result<Value> {
        let spec = find(id).ok_or_else(|| SessionError::UnknownCommand(id.chars().take(64).collect()))?;
        let hooks = commands::hooks();
        if let Some(before) = hooks.before {
            before(id, self);
        }
        // Journaled commands may change the graph: compare to see what they made stale.
        let before = spec.journal.then(|| self.graph.clone());
        let mut result = (spec.enabled)(self).map_err(|reason| SessionError::Disabled { id: id.to_owned(), reason }).and_then(|()| (spec.run)(self, params));
        // a graph that came in names its engines: start them when engines are picked automatically
        if self.engines.auto
            && matches!(id, "graph.open" | "graph.insert" | "graph.insertGroup")
            && let Ok(out) = &mut result
        {
            let started = engine_cmds::start_needed(self);
            if !started.is_empty()
                && let Some(o) = out.as_object_mut()
            {
                o.insert("startedEngines".into(), serde_json::json!(started));
            }
        }
        if result.is_ok() {
            self.generation = self.generation.wrapping_add(1);
            if let Some(before) = &before {
                let now = std::time::Instant::now();
                if id == "graph.new" || id == "graph.open" {
                    self.run.reset(now);
                    self.run.new_graph_life();
                    // a new graph: its pop-outs followed the old one's ports
                    self.views.popouts.clear();
                    self.views.editors.clear();
                    viewer_cmds::clear(self);
                    doc_edits::clear(self);
                } else {
                    self.run.on_edit(before, &self.graph, now);
                    view_cmds::prune(self);
                    viewer_cmds::prune(self);
                    doc_edits::prune(self);
                }
            }
        }
        if spec.journal {
            if self.journal.len() >= JOURNAL_LIMIT {
                self.journal.remove(0);
            }
            self.journal.push(JournalEntry {
                id: id.to_owned(),
                params: params.clone(),
                ok: result.is_ok(),
                error: result.as_ref().err().map(ToString::to_string),
            });
        }
        if let (Some(after), true) = (hooks.after, result.is_ok()) {
            after(id, self);
        }
        result
    }

    /// Apply an undoable edit to the graph, then check the graph against the catalog: the edit
    /// may not add a problem ([`graph_problems`]; one a workflow was opened with stays until
    /// it is fixed). On error the graph is left as it was. An application's commands make their
    /// undo steps here too, under their own `label`.
    pub fn edit<T>(&mut self, label: &str, f: impl FnOnce(&mut Graph, &SchemaReply) -> Result<T>) -> Result<T> {
        let before = self.graph.clone();
        let result = f(&mut self.graph, &self.catalog).and_then(|v| {
            let fresh: BTreeSet<NodeId> = self.graph.nodes.keys().filter(|id| !before.nodes.contains_key(*id)).cloned().collect();
            self.graph.assign_serials(&fresh, |kind| display_name(&self.catalog, kind));
            if self.graph.check(&self.catalog).is_err() {
                let had = graph_problems(&before, &self.catalog);
                if let Some(new) = graph_problems(&self.graph, &self.catalog).into_iter().find(|p| !had.contains(p)) {
                    return Err(GraphError::Broken(new).into());
                }
            }
            Ok(v)
        });
        match result {
            Ok(v) => {
                self.dirty = true;
                self.push_undo(label.to_owned(), before);
                self.redo.clear();
                Ok(v)
            }
            Err(e) => {
                self.graph = before;
                Err(e)
            }
        }
    }

    fn push_undo(&mut self, label: String, graph: Graph) {
        let serial = self.next_step;
        self.next_step = self.next_step.wrapping_add(1);
        self.undo.push(Snapshot { label, graph, serial });
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
    }

    /// A move (`node.move`, `group.move`, `frame.move`, `frame.resize`, `edit.move`; N3 slice 1):
    /// one undo step under `label`, or, when it follows a move of the same `items` that it
    /// [joins](MoveMark::joins), part of that step (its label kept). So a drag is one step however
    /// many moves it sends, and a held arrow key is one step. `f` must change the graph (the
    /// caller leaves out moves to where things already are) and must not fail once the caller has
    /// checked the ids: a step joined is not snapshotted again, so an error there leaves what `f`
    /// did so far, positions only.
    pub fn edit_move<T>(&mut self, label: &str, mut items: Vec<String>, drag: Option<&str>, f: impl FnOnce(&mut Graph) -> Result<T>) -> Result<T> {
        items.sort();
        items.dedup();
        let now = std::time::Instant::now();
        let top = self.undo.last().map(|s| s.serial);
        let joined = self.last_move.as_ref().is_some_and(|m| Some(m.step) == top && self.redo.is_empty() && m.joins(&items, drag, now));
        if joined {
            let v = f(&mut self.graph)?;
            self.dirty = true;
            if let Some(m) = &mut self.last_move {
                m.at = now;
            }
            return Ok(v);
        }
        let v = self.edit(label, |g, _| f(g))?;
        self.last_move = self.undo.last().map(|s| MoveMark { items, drag: drag.map(str::to_owned), at: now, step: s.serial });
        Ok(v)
    }

    /// The move step on top of the undo stack, if the last step was a move (tests, `ui.inspect`).
    pub fn last_move(&self) -> Option<&MoveMark> {
        let top = self.undo.last().map(|s| s.serial);
        self.last_move.as_ref().filter(|m| Some(m.step) == top && self.redo.is_empty())
    }

    /// A change that isn't an undo step (canvas positions while dragging). Call
    /// [`Session::mark_dirty`] when it succeeds, since positions are saved.
    pub(crate) fn graph_mut_untracked(&mut self) -> &mut Graph {
        &mut self.graph
    }

    pub(crate) fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// The graph was written to, or read from, `path`.
    pub(crate) fn mark_saved(&mut self, path: PathBuf) {
        self.path = Some(path);
        self.dirty = false;
    }

    /// Saved here, so a trusted graph stays trusted when it is opened again; an unapproved one
    /// doesn't become trusted by being saved.
    pub(crate) fn remember_saved(&self) {
        if let Some(file) = self.trust_file.as_ref().filter(|_| !self.run.review_due) {
            let _ = trust::remember(file, &trust::graph_hash(&self.graph));
        }
    }

    /// Replace the graph with one read from `path`. Opening isn't an undo step: the history
    /// belongs to the previous graph, so it is cleared.
    pub(crate) fn replace_opened(&mut self, mut graph: Graph, path: PathBuf) {
        // Workflows saved before serials existed get them (saved with the next save).
        graph.migrate_serial_titles(|kind| display_name(&self.catalog, kind));
        graph.assign_serials(&BTreeSet::new(), |kind| display_name(&self.catalog, kind));
        // values that only repeat a choice follow it again (so a wired choice varies them)
        graph.drop_filled_values(&self.catalog);
        self.graph = graph;
        self.undo.clear();
        self.redo.clear();
        self.mark_saved(path);
        self.from_file = true;
        self.run.decline_review();
        self.check_trust();
    }

    /// A fresh graph has no file and nothing to save.
    pub(crate) fn forget_path(&mut self) {
        self.path = None;
        self.dirty = false;
    }

    // ---------- undo and redo (crate::edit_cmds) ----------

    pub(crate) fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub(crate) fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Step back: the graph before the last edit. False when there is none.
    pub(crate) fn step_undo(&mut self) -> bool {
        let Some(prev) = self.undo.pop() else { return false };
        let current = std::mem::replace(&mut self.graph, prev.graph);
        self.redo.push(Snapshot { label: prev.label, graph: current, serial: prev.serial });
        self.dirty = true;
        true
    }

    /// Step forward again. False when there is nothing to redo.
    pub(crate) fn step_redo(&mut self) -> bool {
        let Some(next) = self.redo.pop() else { return false };
        let current = std::mem::replace(&mut self.graph, next.graph);
        // a step redone is a new step: a move after it doesn't join it
        self.push_undo(next.label, current);
        self.dirty = true;
        true
    }
}
