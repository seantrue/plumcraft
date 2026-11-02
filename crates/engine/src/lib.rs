//! The minimal engine SDK (plumcraft `docs/plan.md` milestone S4): write nodes as [`Node`]s,
//! collect them in an [`Engine`], and host it in-process with [`host::Host`]. The executor
//! ([`exec`]) runs research-shaped workflow JSON and emits every status record the interface
//! requires (spec 9.4): `started`, `preview` the moment a node finishes, a terminal state, and
//! `ms`.
//!
//! Values are [`Data`]: JSON (scalars, text, tables as columnar JSON) or typed arrays. The
//! executor decides how each crosses the interface ([`data`]): inline when small, otherwise a
//! blob in the commonkit store named by its handle (plan 10.2, D1).

#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

pub mod arrays;
pub mod bind;
pub mod color;
pub mod data;
pub mod exec;
pub mod files;
pub mod host;
pub mod http;
pub mod persist;
pub mod rpc;
pub mod seq;
pub mod serve;
pub mod vary;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use plumcraft_proto::catalogue::{NodeDef, SchemaReply, WireType};
use plumcraft_proto::error::{ErrorCode, NodeError, ProtoError};
use plumcraft_proto::event::{NodeState, StatusRecord};
use plumcraft_proto::run::{RunReply, RunRequest, SaveReply, SaveRequest};
use plumcraft_proto::session::{EngineInfo, Hello, HelloReply, Limits, ProtocolRange};
use plumcraft_proto::{Caps, PROTOCOL};
use serde_json::Value;

/// A value on a port.
#[derive(Debug, Clone, PartialEq)]
pub enum Data {
    /// Scalars, text, settings, and tables (`{"columns": [{name, type, values}]}`).
    Json(Value),
    Array(Array),
    Audio(Audio),
    /// Encoded bytes the engine names by content: an image, an SVG, a PDF, an application
    /// document (spec 4.4). See [`Blob`].
    Blob(Box<Blob>),
    /// A list (spec 7.6 rule 2): wired into an input that takes one item, the node runs once
    /// per item. See [`List`].
    List(List),
}

/// A list value and the sweeps it spans. `axes` is `((name, length), …)`, row-major (the
/// first varies slowest), its lengths multiplying to `items.len()`. A node may name its own
/// axes (outputs that share one zip downstream); one it leaves empty gets `<node>.<port>` as
/// it leaves the node. `labels`, when given, says what made each item (a sheet label).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct List {
    pub items: Vec<Data>,
    pub axes: Vec<(String, usize)>,
    pub labels: Vec<String>,
}

impl List {
    pub fn new(items: Vec<Data>) -> Self {
        List { items, ..Default::default() }
    }

    /// Its axes, or one anonymous axis when it has none.
    pub fn axes_or_plain(&self) -> Vec<(String, usize)> {
        if self.axes.is_empty() { vec![(String::new(), self.items.len())] } else { self.axes.clone() }
    }

    /// The label of item `i`, or `""`.
    pub fn label(&self, i: usize) -> &str {
        self.labels.get(i).map_or("", String::as_str)
    }
}

/// A blob value: its `md5:raw:` handle and metadata, where its bytes are, and an optional
/// thumbnail. Encoded images, SVG, PDF and application documents cross as these (the craft
/// adapter's values, plan 10.6).
#[derive(Debug, Clone, PartialEq)]
pub struct Blob {
    pub handle: plumcraft_proto::Handle,
    pub meta: plumcraft_proto::data::BlobMeta,
    pub source: BlobSource,
    /// Published for the UI (inline when small, else into the store, as text is), so the
    /// store's budget and TTL govern it. `false` leaves the value where it is and hands out
    /// [`BlobSource::File`]'s `path` locator only. A file too large for the store (over
    /// `plumcraft_store::MAX_READ`) crosses by `path` either way.
    pub publish: bool,
    /// A (h, w, 4) `|u1` thumbnail, sent as the node's `rgba8` thumb.
    pub thumb: Option<Array>,
}

impl Blob {
    /// An SVG value, the core SVG type (`core_types::SVG`): checked by parsing, its size in
    /// `Form::Vector`, and its node thumbnail drawn with `plumcraft-svg`. Bytes that aren't an
    /// SVG it can draw are a `config` error.
    pub fn svg(bytes: impl Into<Vec<u8>>) -> Result<Blob, NodeError> {
        let bytes: Vec<u8> = bytes.into();
        let tree = plumcraft_svg::parse(&bytes).map_err(|e| NodeError::new(NodeError::CONFIG, format!("not an SVG: {e}")))?;
        let (width, height) = (tree.size().width(), tree.size().height());
        let (w, h) = plumcraft_svg::fit((width, height), exec::THUMB_PX);
        let thumb = plumcraft_svg::render(&tree, w, h)
            .ok()
            .map(plumcraft_svg::Raster::on_paper)
            .map(|r| Array::new("|u1", vec![u64::from(r.h), u64::from(r.w), 4], r.pixels));
        Ok(Blob {
            handle: plumcraft_proto::Handle::md5_raw(&bytes),
            meta: plumcraft_proto::data::BlobMeta {
                media: plumcraft_proto::catalogue::core_types::SVG_MEDIA.into(),
                size: bytes.len() as u64,
                form: plumcraft_proto::data::Form::Vector { width: f64::from(width), height: f64::from(height) },
                aliases: Vec::new(),
            },
            source: BlobSource::Bytes(Arc::from(bytes)),
            publish: true,
            thumb,
        })
    }

    /// Its bytes, read from its file when it is one; a file over `max` bytes is a `config`
    /// error, one that can't be read `missing_source`.
    pub fn read(&self, max: u64) -> Result<Vec<u8>, NodeError> {
        match &self.source {
            BlobSource::Bytes(b) => Ok(b.to_vec()),
            BlobSource::File(p) => {
                let missing = |e: std::io::Error| NodeError::new(NodeError::MISSING_SOURCE, format!("{}: {e}", p.display()));
                if std::fs::metadata(p).map_err(missing)?.len() > max {
                    return Err(NodeError::new(NodeError::CONFIG, format!("{}: larger than {max} bytes", p.display())));
                }
                std::fs::read(p).map_err(missing)
            }
        }
    }

    /// Whether it is an SVG value.
    pub fn is_svg(&self) -> bool {
        self.meta.media == plumcraft_proto::catalogue::core_types::SVG_MEDIA
    }
}

/// Where a [`Blob`]'s bytes are.
#[derive(Debug, Clone, PartialEq)]
pub enum BlobSource {
    Bytes(Arc<[u8]>),
    /// A file under the engine's work root, holding exactly the bytes the handle names.
    File(std::path::PathBuf),
}

/// A C-order array with a numpy dtype string (`<u4`, `<f4`, `|u1`), little-endian: the core
/// ARRAY type (`plumcraft_proto::catalogue::core_types`). (H, W) is one channel, (H, W, C) a
/// stack of C, and an (N, k) `table` rows of k columns. `space` (a colour space such as `RGB`,
/// `HSI`, `Lab`) and `names` (one per channel or column) travel with it in `Form::Array`, so
/// another node, viewer or engine reads it the same way.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Array {
    pub dtype: String,
    pub shape: Vec<u64>,
    pub bytes: Arc<[u8]>,
    pub space: Option<String>,
    pub names: Vec<String>,
    pub table: bool,
}

impl Array {
    pub fn new(dtype: impl Into<String>, shape: Vec<u64>, bytes: impl Into<Arc<[u8]>>) -> Array {
        Array { dtype: dtype.into(), shape, bytes: bytes.into(), ..Default::default() }
    }

    /// An `<f4` array from values in C order.
    pub fn from_f32(shape: Vec<u64>, values: &[f32]) -> Array {
        Array::new("<f4", shape, values.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<u8>>())
    }

    /// The same array in colour space `space` with channel `names`.
    pub fn with_space(mut self, space: Option<String>, names: Vec<String>) -> Array {
        self.space = space;
        self.names = names;
        self
    }
}

/// PCM audio: interleaved `f32` samples, (frames, channels) in C order, at `sample_rate`. It
/// crosses as an `<f4` array of that shape with `Form::Audio` (plan 10.4, F1).
#[derive(Debug, Clone, PartialEq)]
pub struct Audio {
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Arc<[f32]>,
}

impl Audio {
    /// From per-channel buffers (SoundCraft's layout); channels of unequal length are cut to
    /// the shortest.
    pub fn from_channels(sample_rate: u32, ch: &[Vec<f32>]) -> Audio {
        let frames = ch.iter().map(Vec::len).min().unwrap_or(0);
        let mut samples = Vec::with_capacity(frames * ch.len());
        for f in 0..frames {
            samples.extend(ch.iter().map(|c| c.get(f).copied().unwrap_or(0.0)));
        }
        Audio { sample_rate, channels: u16::try_from(ch.len()).unwrap_or(u16::MAX), samples: Arc::from(samples) }
    }

    /// Per-channel buffers.
    pub fn to_channels(&self) -> Vec<Vec<f32>> {
        let n = usize::from(self.channels.max(1));
        let mut out = vec![Vec::with_capacity(self.samples.len() / n); n];
        for (i, s) in self.samples.iter().enumerate() {
            if let Some(c) = out.get_mut(i % n) {
                c.push(*s);
            }
        }
        out
    }

    pub fn frames(&self) -> u64 {
        (self.samples.len() / usize::from(self.channels.max(1))) as u64
    }

    pub fn duration_secs(&self) -> f64 {
        if self.sample_rate == 0 { 0.0 } else { self.frames() as f64 / f64::from(self.sample_rate) }
    }

    /// The samples as little-endian bytes, C order.
    pub fn bytes(&self) -> Vec<u8> {
        self.samples.iter().flat_map(|s| s.to_le_bytes()).collect()
    }
}

impl Data {
    pub fn json(&self) -> Option<&Value> {
        match self {
            Data::Json(v) => Some(v),
            Data::Array(_) | Data::Audio(_) | Data::Blob(_) | Data::List(_) => None,
        }
    }

    pub fn list(&self) -> Option<&List> {
        match self {
            Data::List(l) => Some(l),
            _ => None,
        }
    }

    pub fn audio(&self) -> Option<&Audio> {
        match self {
            Data::Audio(a) => Some(a),
            _ => None,
        }
    }

    pub fn blob(&self) -> Option<&Blob> {
        match self {
            Data::Blob(b) => Some(b),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        self.json().and_then(Value::as_str)
    }
}

impl From<Value> for Data {
    fn from(v: Value) -> Self {
        Data::Json(v)
    }
}

/// The MD5 of the SDK's own code (the executor, the protocol and the store) and of the
/// workspace's `Cargo.lock`, baked in by `build.rs`.
pub const SDK_DIGEST: &str = env!("PLUMCRAFT_SDK_DIGEST");

/// A memo salt (spec 14.2) that changes whenever code that can change a result does:
/// `prefix` (the engine's name and version), the engine's own code digest (its `build.rs`
/// bakes it, as `include!("../engine/digest.rs")` does for the text engine) and the SDK's.
/// Persisted records outlive the process, so a salt without the code would serve a fixed
/// node the result of the old code.
pub fn salt(prefix: &str, code_digest: &str) -> String {
    format!("{prefix}+code:{code_digest}+sdk:{SDK_DIGEST}")
}

/// A node's input values by port name, after wires, widget values and defaults are resolved.
pub type Inputs = BTreeMap<String, Data>;
/// A node's output values by port name.
pub type Outputs = BTreeMap<String, Data>;

/// One node type.
pub trait Node: Send + Sync {
    fn def(&self) -> NodeDef;
    /// Compute the outputs. Long nodes check [`Cx::cancelled`] and report [`Cx::progress`].
    fn run(&self, inputs: &Inputs, cx: &mut Cx) -> Result<Outputs, NodeError>;
    /// What the node's memo key must also cover that its inputs don't: the files a directory
    /// holds, a file's size and time. Called before the key is made (so it should be cheap);
    /// `None` adds nothing.
    fn volatile(&self, _inputs: &Inputs) -> Option<Value> {
        None
    }
}

/// The wire type of a port that takes or makes any value (Zip, Unzip, Load and Write Files).
/// An engine that declares it gets conversions both ways between it and every other type it
/// declares (see [`Engine::schema`]).
pub const ANY: &str = "ANY";

/// A run's cancel flag, shared between the host (which sets it at once, off the run path) and
/// the executor (which checks it between nodes and inside long nodes).
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }

    /// The flag itself, for an [`EngineCore`] that checks one of its own.
    pub fn flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.0)
    }
}

/// Where the executor sends what happens. The host assigns envelope `seq`s and wakes the UI.
pub trait Emit {
    fn status(&mut self, rec: StatusRecord);
}

/// An engine that runs whole graphs itself (spec 16.1), with an executor of its own. Built on the
/// worker thread, so it need not be `Send`. It publishes its own values and fills in each status
/// record's `seq`, `publisher` and `ts`.
pub trait EngineCore {
    fn run(&mut self, req: RunRequest, plane: &data::Plane<'_>, emit: &mut dyn Emit, cancel: &CancelToken) -> RunReply;

    /// Drop values held for a run (`values.held`, spec 7.6 rule 11): one count of each id.
    fn release(&mut self, _ids: &[String]) {}

    /// `graph.release`: a graph's life ended, so what the engine kept for it (scratch files) can
    /// go. `scope` is `RunRequest.scope`, from the other side: not to be trusted as a file name.
    fn release_scope(&mut self, _scope: &str) {}

    /// `output.save`: write a node's output, running the graph for it (spec 15). `cancel` is
    /// this save's own, not the last run's.
    fn save(&mut self, _req: SaveRequest, _plane: &data::Plane<'_>, _cancel: &CancelToken) -> SaveReply {
        SaveReply::unsupported()
    }
}

/// Builds an [`EngineCore`] on the worker thread; called again after a caught panic.
pub type CoreFactory = Box<dyn Fn() -> Result<Box<dyn EngineCore>, String> + Send + Sync>;

/// What a running node can do besides compute.
pub struct Cx<'a> {
    node_id: &'a str,
    cancel: &'a CancelToken,
    records: &'a mut dyn FnMut(StatusRecord),
    last_progress: Option<Instant>,
    setup: std::time::Duration,
}

/// Progress records are sent at most this often per node (spec 9.4).
const PROGRESS_EVERY_MS: u128 = 100;

impl<'a> Cx<'a> {
    pub fn node_id(&self) -> &str {
        self.node_id
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// Time this call spent getting ready rather than working: waiting for or starting a child
    /// process, say. The memo leaves it out when it weighs whether a result is slow enough to
    /// persist (spec 14.5), so a cold start doesn't make a trivial node look expensive.
    pub fn setup_took(&mut self, d: std::time::Duration) {
        self.setup += d;
    }

    /// Report progress (0..1) and an optional stage text, rate-limited to one record per 100 ms.
    pub fn progress(&mut self, fraction: f64, stage: Option<&str>) {
        let now = Instant::now();
        if self.last_progress.is_some_and(|t| now.duration_since(t).as_millis() < PROGRESS_EVERY_MS) {
            return;
        }
        self.last_progress = Some(now);
        let mut rec = StatusRecord::new(self.node_id, NodeState::Progress);
        rec.fraction = Some(fraction.clamp(0.0, 1.0));
        rec.stage = stage.map(str::to_owned);
        (self.records)(rec);
    }
}

/// An engine: its identity, wire types and nodes. Not tied to a binding.
pub struct Engine {
    info: EngineInfo,
    store: Option<plumcraft_store::Store>,
    wire_types: Vec<WireType>,
    nodes: BTreeMap<String, Box<dyn Node>>,
    caps: Caps,
    limits: Limits,
    work_root: Option<std::path::PathBuf>,
    persist_ms: Option<f64>,
    core: Option<CoreFactory>,
    catalogue: Option<SchemaReply>,
}

/// How slow a node must be (ms of its own work) before its result is persisted (spec 14.5),
/// when `PLUMCRAFT_MEMO_PERSIST_MS` doesn't say.
pub const PERSIST_MS: f64 = 1000.0;
/// `PLUMCRAFT_MEMO_PERSIST_MS`: the threshold in ms, or `off`.
pub const PERSIST_ENV: &str = "PLUMCRAFT_MEMO_PERSIST_MS";

/// The persist threshold from the environment: a number of ms (at least 0), `off` for none, or
/// [`PERSIST_MS`] when unset or unreadable.
pub fn persist_ms_from_env() -> Option<f64> {
    match std::env::var(PERSIST_ENV) {
        Ok(v) if v.trim().eq_ignore_ascii_case("off") => None,
        Ok(v) => Some(v.trim().parse::<f64>().ok().filter(|ms| ms.is_finite() && *ms >= 0.0).unwrap_or(PERSIST_MS)),
        Err(_) => Some(PERSIST_MS),
    }
}

impl Engine {
    pub fn new(info: EngineInfo) -> Self {
        Engine {
            info,
            store: Some(plumcraft_store::Store::new(plumcraft_store::default_root())),
            wire_types: Vec::new(),
            nodes: BTreeMap::new(),
            caps: Caps::new(["run.stats", "events.push", "events.poll", "events.resume", "status.progress", "display.stream", "codec.json"]),
            limits: Limits::default(),
            work_root: None,
            persist_ms: persist_ms_from_env(),
            core: None,
            catalogue: None,
        }
    }

    /// Run graphs with `core` instead of this crate's executor; `catalogue` is what it offers,
    /// answered as this engine's schema. Nodes given with [`Engine::with_node`] aren't offered.
    pub fn with_core(mut self, catalogue: SchemaReply, core: CoreFactory) -> Self {
        self.catalogue = Some(catalogue);
        self.core = Some(core);
        self
    }

    /// Declare a capability this engine has.
    pub fn with_cap(mut self, cap: &str) -> Self {
        self.caps.insert(cap);
        self
    }

    pub fn core(&self) -> Option<&CoreFactory> {
        self.core.as_ref()
    }

    /// Persist results whose own work took at least `ms` (spec 14.5); `None` persists nothing
    /// unless a node is marked `cache: always`. Persisting needs a store.
    pub fn with_persist_ms(mut self, ms: Option<f64>) -> Self {
        self.persist_ms = ms;
        self
    }

    pub fn persist_ms(&self) -> Option<f64> {
        self.persist_ms
    }

    /// Declare a work root (spec 4.4): `path` locators under it are the engine's own files,
    /// which the UI may read after the same-host proof. `hello` reports it as `data.work_root`.
    pub fn with_work_root(mut self, root: std::path::PathBuf) -> Self {
        self.work_root = Some(root);
        self
    }

    pub fn work_root(&self) -> Option<&std::path::Path> {
        self.work_root.as_deref()
    }

    /// Publish blobs under `root` instead of the shared commonkit store; `None` publishes
    /// nothing (every value must then fit inline).
    pub fn with_store(mut self, root: Option<std::path::PathBuf>) -> Self {
        self.store = root.map(plumcraft_store::Store::new);
        self
    }

    pub fn store(&self) -> Option<&plumcraft_store::Store> {
        self.store.as_ref()
    }

    pub fn limits(&self) -> &Limits {
        &self.limits
    }

    pub fn with_wire_type(mut self, w: WireType) -> Self {
        self.wire_types.push(w);
        self
    }

    /// Declare the core SVG type (`core_types::svg()`), once. Make SVG values with
    /// [`Blob::svg`].
    pub fn with_svg(mut self) -> Self {
        let svg = plumcraft_proto::catalogue::core_types::svg();
        if !self.wire_types.iter().any(|w| w.id == svg.id) {
            self.wire_types.push(svg);
        }
        self
    }

    /// Offer the Vary nodes ([`vary`]: Vary Float, Int, String, Bool, Choices, Range, Range
    /// 2D and 3D) and the scalar wire types they make, where the engine lacks them.
    pub fn with_vary(mut self) -> Self {
        for w in vary::wire_types() {
            if self.wire_type(&w.id).is_none() {
                self.wire_types.push(w);
            }
        }
        for n in vary::nodes() {
            self = self.with_node(n);
        }
        self
    }

    /// Offer every building block: the Vary nodes ([`Engine::with_vary`]), the sequence nodes
    /// ([`seq`]: Zip, Outer Join, Unzip) and the file nodes ([`files`]: Select, Find, Load and
    /// Write Files), with the `ANY` wire type they use.
    pub fn with_building_blocks(mut self) -> Self {
        if self.wire_type(ANY).is_none() {
            self.wire_types.push(WireType {
                id: ANY.into(),
                color: Some("#9ca3af".into()),
                media: plumcraft_proto::catalogue::Media::Bytes,
                ..Default::default()
            });
        }
        let mut e = self.with_vary();
        for n in seq::nodes().into_iter().chain(files::nodes()) {
            e = e.with_node(n);
        }
        e
    }

    pub fn with_node(mut self, n: Box<dyn Node>) -> Self {
        self.nodes.insert(n.def().id, n);
        self
    }

    pub fn node(&self, id: &str) -> Option<&dyn Node> {
        self.nodes.get(id).map(|n| n.as_ref())
    }

    pub fn wire_type(&self, id: &str) -> Option<&WireType> {
        self.wire_types.iter().find(|w| w.id == id)
    }

    pub fn info(&self) -> &EngineInfo {
        &self.info
    }

    pub fn caps(&self) -> &Caps {
        &self.caps
    }

    pub fn schema(&self) -> SchemaReply {
        if let Some(c) = &self.catalogue {
            return SchemaReply { engine: self.info.clone(), ..c.clone() };
        }
        // With ANY declared: the base conversions, plus every type to and from ANY.
        let compat = self.wire_type(ANY).map(|_| {
            let mut c = plumcraft_proto::catalogue::Compat::defaults();
            for w in self.wire_types.iter().filter(|w| w.id != ANY) {
                c.push(plumcraft_proto::catalogue::Compat { from: w.id.clone(), to: ANY.into() });
                c.push(plumcraft_proto::catalogue::Compat { from: ANY.into(), to: w.id.clone() });
            }
            c
        });
        SchemaReply {
            v: PROTOCOL,
            engine: self.info.clone(),
            wire_types: self.wire_types.clone(),
            compat,
            nodes: self.nodes.values().map(|n| n.def()).collect(),
            ..Default::default()
        }
    }

    /// Answer `hello`: choose the protocol and the session's capabilities.
    pub fn hello(&self, req: &Hello, session: &str) -> Result<HelloReply, ProtoError> {
        let protocol = ProtocolRange::choose(req.protocol, ProtocolRange::default())?;
        if req.op != "hello" {
            return Err(ProtoError::new(ErrorCode::InvalidRequest, "expected op hello"));
        }
        Ok(HelloReply {
            protocol,
            session: session.into(),
            engine: self.info.clone(),
            caps: Caps::negotiate(&req.caps, &self.caps),
            limits: self.limits,
            ..Default::default()
        })
    }
}

/// Seconds since the epoch, as status records carry it.
pub fn now_ts() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}
