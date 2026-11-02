//! Run state: what the last runs said about each node, what is stale since, and the requests
//! waiting to go to the engine.
//!
//! Lifted from the research software's `run.rs` and rebuilt on protocol 2: the normative [`Reducer`] orders
//! status records, requests are [`RunRequest`]s with [`WatchRequest`]s, and events arrive as
//! [`Envelope`]s from a `Connection`. The session owns a [`RunState`]; `run.start` builds a
//! request and leaves it in the outbox; the host sends it and feeds every event back with
//! [`RunState::apply_event`]. Each request has a fresh `run_id` and a higher `generation`, and
//! only the latest request's records and reply are taken: anything older is ignored.
//!
//! Pure apart from the clock, which every time-dependent method takes as a parameter, so the
//! debounce and the timings are tested without sleeping.

use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

use plumcraft_graph::{Graph, NodeId, PortRef, Wire};
use plumcraft_proto::display::{Display, Rendition, WatchRequest};
use plumcraft_proto::error::NodeError;
use plumcraft_proto::event::{Envelope, Event, NodeState};
use plumcraft_proto::reducer::Reducer;
use plumcraft_proto::run::{GraphRef, RunReply, RunRequest, RunStats};
use serde::Serialize;
use serde_json::{Value, json};

/// How long the graph must be quiet after an edit before auto-run starts a run.
pub const AUTO_RUN_DELAY: Duration = Duration::from_millis(150);

/// The most "why stale" reasons a node keeps (the newest).
pub const MAX_STALE_REASONS: usize = 8;

/// One node's run state.
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct NodeRun {
    /// `None` when the node hasn't run (or wasn't part of the latest run).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<NodeState>,
    /// The generation of the request this state belongs to.
    pub generation: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fraction: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<NodeError>,
    /// The latest thumbnail (kept while a new run computes, so previews don't blink).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumb: Option<Rendition>,
    /// The edit count when an edit made this node stale (it, or something upstream, changed
    /// since its result was computed). `None`: fresh.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale_since: Option<u64>,
    /// Where the latest run took each input from: wire, bundle, widget or default.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub provenance: BTreeMap<String, String>,
    /// Why it is stale: the edits since its last run that changed it or something upstream,
    /// oldest first. Empty when fresh.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stale_why: Vec<String>,
    /// Where a cached result came from: `memory`, `store` or `content`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// What computing its result took (for a cached node, `ms` is only the fetch).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub computed_ms: Option<f64>,
    /// What the engine has to say on the node (a summary, a warning about an unused override).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// What its editor needs from a run, by `editor.kind` (`NodeReport.editor`): the last run that
    /// sent any, so a failed run leaves an editor something to repair the node with. Opaque here.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub editor: BTreeMap<String, Value>,
}

/// What the caches did: the latest run, and every run since the session began (spec 14.7).
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct CacheStats {
    /// The latest run that reported.
    pub last: Option<RunStats>,
    /// Runs that reported.
    pub runs: u64,
    /// Every one of them.
    pub session: RunStats,
    /// The same by engine, with several connected.
    pub by_engine: BTreeMap<String, RunStats>,
}

impl CacheStats {
    fn add(&mut self, reply: &RunReply) {
        let Some(s) = &reply.stats else { return };
        self.runs += 1;
        self.session.add(s);
        self.last = Some(s.clone());
        for (engine, part) in &reply.engine_stats {
            self.by_engine.entry(engine.clone()).or_default().add(part);
        }
    }
}

impl NodeRun {
    pub fn is_stale(&self) -> bool {
        self.stale_since.is_some()
    }

    /// Queued or running.
    pub fn is_active(&self) -> bool {
        matches!(self.state, Some(NodeState::Queued | NodeState::Started | NodeState::Progress | NodeState::Preview))
    }

    /// Done, cached, failed, skipped or cancelled.
    pub fn is_finished(&self) -> bool {
        self.state.is_some_and(NodeState::is_terminal)
    }
}

/// Who asked for a watch entry (docs/research/16-viewers.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "role", content = "id", rename_all = "snake_case")]
pub enum WatchRole {
    /// The Watched panel.
    Panel,
    /// A pop-out window, by its id.
    Popout(u64),
    /// A wire's hover: for one run.
    Peek,
    /// The value pinned as A or B for comparing ([`crate::viewer_cmds`]).
    PinA,
    PinB,
}

/// Something for the host to send to the engine.
#[derive(Debug, Clone, PartialEq)]
pub enum Outgoing {
    /// A graph's life ended: what the engine kept for it can go (`graph.release`).
    Release(String),
    Run(Box<RunRequest>),
    /// Cancel a run (`run.cancel`): the engine stops it between nodes or inside a long one.
    Cancel(String),
}

/// The run state of a session.
#[derive(Debug, Clone, Default)]
pub struct RunState {
    /// The open graph's life, from its opening or making to the next, sent with every run: what
    /// the engine keeps for it goes when it ends. Empty: unscoped.
    pub scope: String,
    pub nodes: BTreeMap<String, NodeRun>,
    /// The generation of the latest request (0: none yet).
    pub generation: u64,
    /// The latest request's run id.
    pub run_id: Option<String>,
    /// A run is in flight (requested, no reply yet).
    pub running: bool,
    /// Run after every edit (debounced by [`AUTO_RUN_DELAY`]).
    pub auto: bool,
    /// This graph came from elsewhere and nobody has approved it ([`crate::trust`]): what
    /// [`RunState::request`] builds waits in `held` and isn't sent.
    pub(crate) review_due: bool,
    /// The run waiting for that approval.
    held: Option<Box<RunRequest>>,
    /// Something other than a run (a save) asked for the approval.
    review_asked: bool,
    /// What the Watched panel shows; sent with every run.
    pub watch: Vec<WatchRequest>,
    /// Who asked for each watch entry ([`WatchRole`]): the Watched panel, a pop-out window, a
    /// wire's peek. Missing entries read as the panel for the first, a peek after.
    pub roles: Vec<WatchRole>,
    /// The latest display for each watch entry, as it streams in (`preview` records) and then
    /// from the reply.
    pub watched: Vec<Option<Display>>,
    /// The latest accepted reply.
    pub last_reply: Option<RunReply>,
    /// What the caches did.
    pub cache: CacheStats,
    /// A graph-level failure of the latest run (a cycle, an unknown kind, the engine gone).
    pub run_error: Option<String>,
    /// Wall time of the latest finished run, request to reply.
    pub last_wall: Option<Duration>,
    /// Nodes in the latest request (for "n of m").
    pub total: usize,
    started: Option<Instant>,
    first_record: Option<Duration>,
    /// Edits so far.
    edits: u64,
    /// `edits` when the latest request was made.
    edits_at_request: u64,
    /// When the last edit happened, while an auto-run is pending.
    edited_at: Option<Instant>,
    /// Requests at or below this generation were cancelled.
    cancelled_through: u64,
    /// Runs cancelled because a newer one replaced them while in flight.
    pub superseded: u64,
    outbox: Vec<Outgoing>,
    reducer: Reducer,
}

/// The nodes whose results an edit from `before` to `after` invalidates: nodes added, or whose
/// kind, values, inherited set or incoming wires changed, and everything downstream of them.
/// Positions don't count.
pub fn changed_nodes(before: &Graph, after: &Graph) -> BTreeSet<NodeId> {
    stale_reasons(before, after).into_keys().collect()
}

/// A short form of a widget value for a reason ("2.5", "\"Lab\"", "…" for long ones).
fn short_value(v: Option<&Value>) -> String {
    let text = v.map_or_else(|| "unset".to_owned(), Value::to_string);
    if text.chars().count() > 24 { format!("{}…", text.chars().take(23).collect::<String>()) } else { text }
}

/// [`changed_nodes`] with the reasons: for each node an edit invalidates, what changed on it (a
/// widget, an input's wire, its type) or which edited nodes upstream reach it.
pub fn stale_reasons(before: &Graph, after: &Graph) -> BTreeMap<NodeId, Vec<String>> {
    let incoming =
        |g: &Graph, id: &NodeId| -> BTreeMap<String, Wire> { g.wires.iter().filter(|w| &w.to.node == id).map(|w| (w.to.port.clone(), w.clone())).collect() };
    let mut out: BTreeMap<NodeId, Vec<String>> = BTreeMap::new();
    for (id, n) in &after.nodes {
        let Some(b) = before.nodes.get(id) else {
            out.insert(id.clone(), vec!["added since the last run".into()]);
            continue;
        };
        let mut why = Vec::new();
        if b.kind != n.kind {
            why.push(format!("its type changed ({} → {})", b.kind, n.kind));
        }
        if b.engine != n.engine {
            let name = |e: &Option<String>| e.clone().unwrap_or_else(|| "any".into());
            why.push(format!("its engine changed ({} → {})", name(&b.engine), name(&n.engine)));
        }
        let keys: BTreeSet<&String> = b.values.keys().chain(n.values.keys()).collect();
        for k in keys {
            let (old, new) = (b.values.get(k), n.values.get(k));
            if old != new {
                why.push(format!("widget {k} changed: {} → {}", short_value(old), short_value(new)));
            }
        }
        let (wb, wa) = (incoming(before, id), incoming(after, id));
        let ports: BTreeSet<&String> = wb.keys().chain(wa.keys()).collect();
        for port in ports {
            match (wb.get(port), wa.get(port)) {
                (None, Some(w)) => why.push(format!("input {port} wired from {}", w.from)),
                (Some(w), None) => why.push(format!("input {port} disconnected (was {})", w.from)),
                (Some(x), Some(y)) if x.from != y.from => why.push(format!("input {port} now from {} (was {})", y.from, x.from)),
                (Some(x), Some(y)) if x.slice != y.slice => {
                    let s = |w: &Wire| w.slice.as_deref().map_or_else(|| "none".to_owned(), |t| format!("[{t}]"));
                    why.push(format!("input {port} slice changed: {} → {}", s(x), s(y)));
                }
                _ => {}
            }
        }
        if why.is_empty() && b.inherited != n.inherited {
            why.push("which inputs are wired changed".into());
        }
        if !why.is_empty() {
            out.insert(id.clone(), why);
        }
    }
    // Everything downstream, naming the edited nodes that reach it.
    let roots: Vec<NodeId> = out.keys().cloned().collect();
    let mut reached: BTreeMap<NodeId, BTreeSet<NodeId>> = BTreeMap::new();
    for root in &roots {
        let mut set = BTreeSet::from([root.clone()]);
        downstream(after, &mut set);
        set.remove(root);
        for n in set {
            reached.entry(n).or_default().insert(root.clone());
        }
    }
    for (n, from) in reached {
        let names: Vec<String> = from
            .iter()
            .map(|r| match after.node(r).map(|x| x.kind.rsplit('.').next().unwrap_or(&x.kind).to_owned()) {
                Some(kind) => format!("{r} ({kind})"),
                None => r.to_string(),
            })
            .collect();
        out.entry(n).or_default().push(format!("upstream {} changed", names.join(", ")));
    }
    out
}

/// Add everything downstream of `set` (in `graph`) to it.
pub fn downstream(graph: &Graph, set: &mut BTreeSet<NodeId>) {
    let mut frontier: Vec<NodeId> = set.iter().cloned().collect();
    while let Some(id) = frontier.pop() {
        for w in graph.wires.iter().filter(|w| w.from.node == id) {
            if set.insert(w.to.node.clone()) {
                frontier.push(w.to.node.clone());
            }
        }
    }
}

/// A run id unique per client: an engine may outlive this editor and remember its runs.
fn new_run_id(generation: u64) -> String {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    format!("ed-{}-{generation}-{nanos:x}", std::process::id())
}

impl RunState {
    /// The run state of a session's first graph, its life begun.
    pub fn for_new_graph() -> Self {
        RunState { scope: new_run_id(0), ..RunState::default() }
    }

    /// A new graph's life begins (one was opened or made): the last one's is released.
    pub fn new_graph_life(&mut self) {
        let old = std::mem::replace(&mut self.scope, new_run_id(self.generation));
        if !old.is_empty() {
            self.outbox.push(Outgoing::Release(old));
        }
    }

    /// Take the requests waiting to go to the engine.
    pub fn take_outbox(&mut self) -> Vec<Outgoing> {
        if crate::commands::hooks().hold_runs.is_some_and(|hold| hold()) {
            return Vec::new();
        }
        std::mem::take(&mut self.outbox)
    }

    pub fn outbox_is_empty(&self) -> bool {
        self.outbox.is_empty()
    }

    pub fn node(&self, id: &str) -> Option<&NodeRun> {
        self.nodes.get(id)
    }

    /// Edits so far (each journaled change to the graph counts one).
    pub fn edits(&self) -> u64 {
        self.edits
    }

    /// Build the next request for `graph` and queue it. Every node of the graph is marked
    /// queued; thumbnails stay until new ones arrive.
    ///
    /// **Latest wins:** a run still in flight is superseded, so a [`Outgoing::Cancel`] for it
    /// goes out first; its late records and reply are ignored by generation either way.
    pub fn request(&mut self, graph: &Graph, targets: Vec<String>, watch: Vec<WatchRequest>, now: Instant) -> RunRequest {
        if self.running
            && self.held.is_none()
            && self.generation > self.cancelled_through
            && let Some(old) = &self.run_id
        {
            self.outbox.push(Outgoing::Cancel(old.clone()));
            self.superseded = self.superseded.saturating_add(1);
        }
        self.generation = self.generation.saturating_add(1);
        let run_id = new_run_id(self.generation);
        let req = RunRequest {
            run_id: run_id.clone(),
            generation: self.generation,
            graph: GraphRef::Inline { graph: serde_json::to_value(graph).unwrap_or(Value::Null) },
            targets,
            watch: watch.clone(),
            scope: self.scope.clone(),
            ..Default::default()
        };
        self.reducer = Reducer::new(&run_id, self.generation);
        self.run_id = Some(run_id);
        if watch != self.watch {
            // A watch list from elsewhere (run.start's `watch`): the panel first, peeks after.
            self.roles = (0..watch.len()).map(|i| if i == 0 { WatchRole::Panel } else { WatchRole::Peek }).collect();
        }
        self.watch = watch;
        self.watched = vec![None; self.watch.len()];
        self.running = true;
        self.started = Some(now);
        self.first_record = None;
        self.edits_at_request = self.edits;
        self.edited_at = None;
        self.run_error = None;
        self.total = graph.nodes.len();
        self.nodes.retain(|id, _| graph.nodes.contains_key(&NodeId(id.clone())));
        for id in graph.nodes.keys() {
            let n = self.nodes.entry(id.0.clone()).or_default();
            n.state = Some(NodeState::Queued);
            n.generation = self.generation;
            n.fraction = None;
            n.stage = None;
        }
        if self.review_due {
            self.held = Some(Box::new(req.clone()));
        } else {
            self.outbox.push(Outgoing::Run(Box::new(req.clone())));
        }
        req
    }

    /// Send `req` again: it waited for something the host had to ask about.
    pub fn resend(&mut self, req: Box<RunRequest>) {
        self.outbox.push(Outgoing::Run(req));
    }

    /// Is something waiting for this graph to be approved?
    pub fn awaiting_review(&self) -> bool {
        self.review_due && (self.held.is_some() || self.review_asked)
    }

    /// A save wants to go: ask for the approval, as a held run does.
    pub(crate) fn ask_review(&mut self) {
        self.review_asked = true;
    }

    /// The graph is approved: what was held goes out.
    pub(crate) fn release(&mut self) {
        self.review_due = false;
        self.review_asked = false;
        if let Some(req) = self.held.take() {
            self.outbox.push(Outgoing::Run(req));
        }
    }

    /// Nobody approved it: what was held is dropped, and the next run asks again.
    pub fn decline_review(&mut self) {
        self.review_asked = false;
        if self.held.is_some() {
            self.cancel();
        }
    }

    /// Is `run_id`/`generation` the latest live request?
    fn is_current(&self, run_id: &str, generation: u64) -> bool {
        self.run_id.as_deref() == Some(run_id) && generation == self.generation && self.generation > self.cancelled_through
    }

    /// Apply one event from the engine. Events of older or cancelled requests are ignored.
    /// Returns whether anything changed.
    pub fn apply_event(&mut self, e: &Envelope, now: Instant) -> bool {
        let (Some(run_id), Some(generation)) = (e.run_id.as_deref(), e.generation) else { return false };
        match &e.event {
            Event::Status(rec) => {
                if !self.is_current(run_id, generation) || rec.node_id.is_empty() {
                    return false;
                }
                if self.first_record.is_none() {
                    self.first_record = self.started.map(|s| now.saturating_duration_since(s));
                }
                self.reducer.apply(run_id, generation, rec);
                for d in &rec.display {
                    self.take_display(d);
                }
                self.sync_node(&rec.node_id)
            }
            Event::RunReply(reply) => self.apply_reply(reply, now),
            _ => false,
        }
    }

    /// Who asked for watch entry `i`.
    pub fn role(&self, i: usize) -> WatchRole {
        self.roles.get(i).copied().unwrap_or(if i == 0 { WatchRole::Panel } else { WatchRole::Peek })
    }

    /// The index of the entry `role` asked for.
    pub fn index_of(&self, role: WatchRole) -> Option<usize> {
        (0..self.watch.len()).find(|i| self.role(*i) == role)
    }

    /// What the Watched panel shows, and its latest display.
    pub fn panel(&self) -> Option<(&WatchRequest, Option<&Display>)> {
        let i = self.index_of(WatchRole::Panel)?;
        Some((self.watch.get(i)?, self.watched.get(i).and_then(Option::as_ref)))
    }

    /// The entry `role` asked for, and its latest display.
    pub fn entry(&self, role: WatchRole) -> Option<(&WatchRequest, Option<&Display>)> {
        let i = self.index_of(role)?;
        Some((self.watch.get(i)?, self.watched.get(i).and_then(Option::as_ref)))
    }

    /// The entry `role` asked for, to change it.
    pub fn entry_mut(&mut self, role: WatchRole) -> Option<&mut WatchRequest> {
        let i = self.index_of(role)?;
        self.watch.get_mut(i)
    }

    /// Set (or, with `None`, drop) the entry for `role`, keeping the others and their displays.
    /// The panel's entry stays first.
    pub fn set_watch(&mut self, role: WatchRole, w: Option<WatchRequest>) {
        // Roles for every entry (a list set before roles: the panel first, peeks after).
        self.roles = (0..self.watch.len()).map(|i| self.role(i)).collect();
        self.watched.resize(self.watch.len(), None);
        match (self.index_of(role), w) {
            (Some(i), Some(w)) => {
                if self.watch.get(i) != Some(&w) {
                    self.watch[i] = w;
                    self.watched[i] = None;
                }
            }
            (Some(i), None) => {
                self.watch.remove(i);
                self.watched.remove(i);
                self.roles.remove(i);
            }
            (None, Some(w)) => {
                let at = if role == WatchRole::Panel { 0 } else { self.watch.len() };
                self.watch.insert(at, w);
                self.watched.insert(at, None);
                self.roles.insert(at, role);
            }
            (None, None) => {}
        }
    }

    /// Give the entry for `role` a display it already has (a pin of what the panel shows).
    pub fn set_display(&mut self, role: WatchRole, d: Option<Display>) {
        if let Some(slot) = self.index_of(role).and_then(|i| self.watched.get_mut(i)) {
            *slot = d;
        }
    }

    /// Drop the peeks (after the run that answered them).
    pub fn drop_peeks(&mut self) {
        while let Some(i) = self.index_of(WatchRole::Peek) {
            self.watch.remove(i);
            self.watched.remove(i);
            self.roles.remove(i);
        }
    }

    /// A display for one of the watch entries (matched by node and port, and item: A and B can
    /// be two items of one list).
    fn take_display(&mut self, d: &Display) {
        for (w, slot) in self.watch.iter().zip(self.watched.iter_mut()) {
            if w.node == d.node && w.port == d.port && (d.items <= 1 || w.index < 0 || w.index == d.index) {
                *slot = Some(d.clone());
            }
        }
    }

    /// Copy the reducer's view of `id` into the node's run state.
    fn sync_node(&mut self, id: &str) -> bool {
        let Some(view) = self.reducer.node(id).cloned() else { return false };
        let generation = self.generation;
        let n = self.nodes.entry(id.to_owned()).or_default();
        let before = n.clone();
        n.generation = generation;
        n.state = Some(view.state);
        n.fraction = view.fraction;
        n.stage = view.stage;
        if let Some(ms) = view.ms.filter(|m| m.is_finite()) {
            n.ms = Some(ms);
        }
        n.error = view.error;
        if view.thumb.is_some() {
            n.thumb = view.thumb;
        }
        *n != before
    }

    /// Apply a run's reply. Returns false (and changes nothing) for a reply to an older or
    /// cancelled request.
    pub fn apply_reply(&mut self, reply: &RunReply, now: Instant) -> bool {
        if !self.is_current(&reply.run_id, reply.generation) {
            return false;
        }
        self.running = false;
        self.last_wall = self.started.map(|s| now.saturating_duration_since(s));
        self.run_error = reply.error.as_ref().map(|e| e.message.clone());
        self.reducer.finish(reply);
        let ids: Vec<String> = self.nodes.keys().cloned().collect();
        for id in ids {
            if reply.nodes.contains_key(&id) {
                self.sync_node(&id);
                if let Some(n) = self.nodes.get_mut(&id) {
                    n.fraction = None;
                    let memo = reply.nodes.get(&id).and_then(|r| r.memo.as_ref());
                    n.source = memo.and_then(|m| m.source.clone());
                    // an engine that doesn't report it: the node's own last compute
                    let computed = match n.state {
                        Some(NodeState::Done) => n.ms,
                        Some(NodeState::Cached) => n.computed_ms,
                        _ => None,
                    };
                    n.computed_ms = memo.and_then(|m| m.computed_ms).or(computed);
                    n.provenance = reply.nodes.get(&id).map(|r| r.provenance.clone()).unwrap_or_default();
                    n.text = reply.nodes.get(&id).and_then(|r| r.text.clone());
                    if let Some(payload) = reply.nodes.get(&id).map(|r| &r.editor).filter(|e| !e.is_empty()) {
                        n.editor = payload.clone();
                    }
                    if !matches!(n.state, Some(NodeState::Done | NodeState::Cached)) && reply.nodes.get(&id).is_some_and(|r| r.thumb.is_none()) {
                        n.thumb = None;
                    }
                }
            } else if let Some(n) = self.nodes.get_mut(&id).filter(|n| n.is_active()) {
                // Not part of the run (the engine ran only what the targets needed), or the run
                // failed as a whole.
                n.state = None;
            }
            if let Some(n) = self.nodes.get_mut(&id)
                && n.stale_since.is_some_and(|since| since < self.edits_at_request)
                && n.is_finished()
            {
                n.stale_since = None;
                n.stale_why.clear();
            }
        }
        // The reply's displays are in request order: by position, so two entries on one port
        // (the panel's heat view, a pop-out's colour view) each get their own.
        if reply.watch.len() == self.watched.len() {
            for (slot, d) in self.watched.iter_mut().zip(&reply.watch) {
                *slot = Some(d.clone());
            }
        } else {
            for d in &reply.watch {
                self.take_display(d);
            }
        }
        self.last_reply = Some(reply.clone());
        self.cache.add(reply);
        true
    }

    /// The graph changed from `before` to `after` (a journaled edit). Marks what it invalidates
    /// stale and, with auto-run on, schedules a run. Returns whether anything changed.
    pub fn on_edit(&mut self, before: &Graph, after: &Graph, now: Instant) -> bool {
        let changed = stale_reasons(before, after);
        let wired = |g: &Graph| g.wires.iter().map(|w| (w.from.clone(), w.to.clone(), w.slice.clone())).collect::<Vec<_>>();
        let removed = before.nodes.keys().any(|id| !after.nodes.contains_key(id)) || wired(before) != wired(after);
        if changed.is_empty() && !removed {
            return false;
        }
        self.nodes.retain(|id, _| after.nodes.contains_key(&NodeId(id.clone())));
        self.forget_watches(after);
        for (id, why) in changed {
            if let Some(n) = self.nodes.get_mut(&id.0) {
                if n.stale_since.is_none() {
                    n.stale_since = Some(self.edits);
                }
                for line in why {
                    if !n.stale_why.contains(&line) {
                        n.stale_why.push(line);
                    }
                }
                // Keep the newest few: a long session of edits shouldn't grow a tooltip forever.
                let excess = n.stale_why.len().saturating_sub(MAX_STALE_REASONS);
                n.stale_why.drain(..excess);
            }
        }
        self.edits = self.edits.saturating_add(1);
        if self.auto {
            self.edited_at = Some(now);
        }
        true
    }

    /// A watch on a node that has gone goes too: an engine refuses a run watching what isn't there.
    fn forget_watches(&mut self, graph: &Graph) {
        let mut i = 0;
        while i < self.watch.len() {
            if self.watch.get(i).is_some_and(|w| graph.nodes.contains_key(&NodeId(w.node.clone()))) {
                i += 1;
                continue;
            }
            self.watch.remove(i);
            if i < self.watched.len() {
                self.watched.remove(i);
            }
            if i < self.roles.len() {
                self.roles.remove(i);
            }
        }
    }

    /// The watches on `port` of a node about to go read `to` from now on, its display gone until
    /// the next run.
    pub fn move_watches(&mut self, port: &PortRef, to: &PortRef) {
        for (i, w) in self.watch.iter_mut().enumerate() {
            if w.node == port.node.0 && w.port == port.port {
                w.node.clone_from(&to.node.0);
                w.port.clone_from(&to.port);
                if let Some(d) = self.watched.get_mut(i) {
                    *d = None;
                }
            }
        }
    }

    /// A different graph (new): forget every node's state.
    pub fn reset(&mut self, now: Instant) {
        self.watch.clear();
        self.watched.clear();
        self.roles.clear();
        self.nodes.clear();
        self.last_reply = None;
        self.run_error = None;
        self.edits = self.edits.saturating_add(1);
        if self.auto {
            self.edited_at = Some(now);
        }
    }

    /// Switch auto-run; switching it on schedules a run at once.
    pub fn set_auto(&mut self, on: bool, now: Instant) {
        self.auto = on;
        self.edited_at = if on { Some(now.checked_sub(AUTO_RUN_DELAY).unwrap_or(now)) } else { None };
    }

    /// With auto-run on and an edit pending: how long until the run is due (zero: now).
    pub fn auto_due_in(&self, now: Instant) -> Option<Duration> {
        let at = self.edited_at?;
        if !self.auto {
            return None;
        }
        Some(AUTO_RUN_DELAY.saturating_sub(now.saturating_duration_since(at)))
    }

    /// Cancel the latest run: its records and reply will be ignored.
    pub fn cancel(&mut self) {
        self.cancelled_through = self.generation;
        self.running = false;
        for n in self.nodes.values_mut().filter(|n| n.is_active()) {
            n.state = Some(NodeState::Cancelled);
        }
        // a held run never went out, so there is nothing to tell the engine
        if self.held.take().is_none()
            && let Some(id) = &self.run_id
        {
            self.outbox.push(Outgoing::Cancel(id.clone()));
        }
    }

    /// The engine went away mid-run: the run ends with this error (the host has already been
    /// told; nothing more will arrive for it).
    pub fn fail(&mut self, why: &str) {
        if self.running {
            self.running = false;
            self.run_error = Some(why.to_owned());
            for n in self.nodes.values_mut().filter(|n| n.is_active()) {
                n.state = None;
            }
        }
    }

    /// (finished, total) nodes of the latest request.
    pub fn progress(&self) -> (usize, usize) {
        let finished = self.nodes.values().filter(|n| n.generation == self.generation && n.is_finished()).count();
        (finished, self.total.max(finished))
    }

    /// Time since the latest request was made.
    pub fn elapsed(&self, now: Instant) -> Option<Duration> {
        self.started.map(|s| now.saturating_duration_since(s))
    }

    /// Nodes in the error state.
    pub fn errors(&self) -> usize {
        self.nodes.values().filter(|n| n.state == Some(NodeState::Error)).count()
    }

    /// Is a run in flight or about to start?
    pub fn is_busy(&self, now: Instant) -> bool {
        self.running || !self.outbox.is_empty() || self.auto_due_in(now).is_some()
    }

    /// The state as JSON (`ui.inspect`, `ui.waitRun`).
    pub fn summary(&self, now: Instant) -> Value {
        let (done, total) = self.progress();
        let nodes: serde_json::Map<String, Value> = self
            .nodes
            .iter()
            .map(|(id, n)| {
                let mut v = json!({"state": n.state, "ms": n.ms, "thumb": n.thumb.is_some(), "stale": n.is_stale()});
                if !n.stale_why.is_empty() {
                    v["staleWhy"] = json!(n.stale_why);
                }
                if let Some(e) = &n.error {
                    v["error"] = json!(e.message);
                }
                (id.clone(), v)
            })
            .collect();
        json!({
            "generation": self.generation,
            "runId": self.run_id,
            "running": self.running,
            "auto": self.auto,
            "autoPending": self.auto_due_in(now).is_some(),
            "awaitingReview": self.awaiting_review(),
            "progress": [done, total],
            "elapsedMs": self.elapsed(now).map(|d| d.as_secs_f64() * 1e3),
            "lastRunMs": self.last_wall.map(|d| d.as_secs_f64() * 1e3),
            "engineMs": self.last_reply.as_ref().map(|r| r.elapsed_ms),
            "firstStatusMs": self.first_record.map(|d| d.as_secs_f64() * 1e3),
            "errors": self.errors(),
            "runError": self.run_error,
            "superseded": self.superseded,
            "watch": self.watch.iter().zip(&self.watched).map(|(w, d)| json!({"node": w.node, "port": w.port, "summary": d.as_ref().map(|d| d.summary.clone())})).collect::<Vec<_>>(),
            "nodes": nodes,
        })
    }
}

#[cfg(test)]
mod editor_payload_tests {
    use std::time::Instant;

    use plumcraft_proto::event::NodeState;
    use plumcraft_proto::run::{NodeReport, RunReply};
    use serde_json::json;

    use super::*;

    fn finish(run: &mut RunState, report: NodeReport) {
        let now = Instant::now();
        let graph = Graph::from_json(r#"{"version": 1, "nodes": {"n1": {"kind": "k"}}, "wires": []}"#).unwrap();
        let req = run.request(&graph, Vec::new(), Vec::new(), now);
        let reply = RunReply { run_id: req.run_id.clone(), generation: req.generation, nodes: [("n1".to_owned(), report)].into(), ..Default::default() };
        assert!(run.apply_reply(&reply, now));
    }

    /// A failed run sends no editor data: an editor keeps what it has, to repair the node with.
    #[test]
    fn a_nodes_editor_data_is_the_last_run_that_sent_any() {
        let mut run = RunState::default();
        let mut sent = NodeReport { state: NodeState::Done, ..Default::default() };
        sent.editor.insert("kind".into(), json!({"bins": [1]}));
        finish(&mut run, sent);
        assert_eq!(run.node("n1").unwrap().editor["kind"], json!({"bins": [1]}));
        finish(&mut run, NodeReport { state: NodeState::Error, ..Default::default() });
        assert_eq!(run.node("n1").unwrap().editor["kind"], json!({"bins": [1]}), "kept");
        let mut newer = NodeReport { state: NodeState::Done, ..Default::default() };
        newer.editor.insert("kind".into(), json!({"bins": [2]}));
        finish(&mut run, newer);
        assert_eq!(run.node("n1").unwrap().editor["kind"], json!({"bins": [2]}), "replaced");
    }
}
