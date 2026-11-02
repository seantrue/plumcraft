# Plan: a general-purpose node-flow base under the research software supporting www.virtimp.com

*2026-10-27. Researched in the research software's repository at `main` 7e1b733 (rebased to 8d96021, with T16 in it), then moved to this repository, `plumcraft`, the same day (D17). Paths such as `crates/bridge/…`, `crates/local/…`, `python/…` and `docs/research/flocraft_effort_20261017.md` refer to the research software (the research software's repository); `crates/proto` and `docs/` are this repository's. **Decided by the maintainer the same day:** the name (PlumCraft), a thin slice first (milestone S, section 10.1), and the decisions in section 11 that the slice needs. The rest are deferred to the re-plan after the slice.*

The maintainer asked: "Replan [the base refactor] with particular attention to defining the interface between the UI and the engine. Assume shared memory and commonkit protocol when single system. Support remote system via upload of source and download of result. Support CAS (MD5 hash like alexandria) handles to reduce data movement and to reuse checkpoint results. Consider other RPC protocols now rather than shoe horning later." Engines to support: the compiled-in engine, a separate-process Rust engine, a separate-process HTTP engine and the existing Python/pipekit framework. Example engines for OpenCV, text and audio, and one around an artcraft app's MCP server. "Under no circumstance support less functionality than the research software uses", including node status tracking and visualising data in flight.

This plan builds on `docs/research/flocraft_effort_20261017.md` (the FloCraft study: 30–40 days, extract in-repo first, then a public repo) and does not redo it. **Revision 2** folds in the adversarial review `docs/research/08-review.md` (findings H1–H8, M1–M12, L1–L10); dispositions are listed at the end. The evidence is in seven notes in `docs/research/`:

| Note | Subject |
|---|---|
| `02-transports-rpc.md` | 14 transport and RPC options compared; the abstract interface |
| `03-cas-data-plane.md` | Handles, MD5 versus BLAKE3, remote upload and download, memo keys, storage tiers |
| `04-visualisation.md` | Display v2, the viewer registry, live data in flight |
| `05-artcraft-mcp-engine.md` | The 14 artcraft apps as engines through their MCP servers |
| `06-engines-and-examples.md` | The four hosting modes, the SDKs and the example engines |
| `07-name.md` | 37 names screened |
| `08-review.md` | Adversarial review of revision 1: "no loss" and "efficient" not yet shown; contained fixes |

The normative interface is a separate file: **`docs/interface.md`** (cited below as "the spec").

## 1. Summary

- **Name: PlumCraft** (decided by the maintainer, 2026-10-27). It plays on plumbing: piping data between nodes, with a nod to pipekit. On 2026-10-27 no PLUMCRAFT product or company was found. crates.io, PyPI (`plumcraft` and `pyplumcraft`) and npm were free. `.dev`, `.app` and `.io` don't resolve, and `.com` is parked. The GitHub user name `plumcraft` belongs to an inactive personal account, so the repository lives under an existing owner, as the crafts do. PlumCraft, the first recommendation, and the other names screened are recorded in `docs/research/07-name.md`.
- **One interface, four bindings.** The UI talks to every engine through one set of operations and one ordered event stream per session, defined once as versioned serde types in a new L0 crate (`plumcraft-proto`) with JSON Schema generated from them. Bindings add only framing, event delivery, cancel transport and byte movement: **in-process** (typed calls, no serialisation), **AMQP/pipekit** (today's wire), **local socket JSON-RPC 2.0** (separate-process engines), **HTTP + SSE** (remote and any language).
- **Today's protocol becomes protocol 1 and keeps working.** The Python engine runs unchanged on day one: the AMQP host synthesises the handshake and parses every protocol 1 message into protocol 2 types.
- **Status tracking is never weaker than today.** The nine node states, the generation filter, "never back from terminal" and "the final reply is authoritative" are kept as normative rules; ordering within a node is by `(publisher, publisher_seq)` then state rank, which fixes multi-publisher fan-out. Every engine adds a mandatory `run.get` snapshot and `events.poll`, so push, long-poll and poll all feed the same reducer. These are served **off the run path** on every binding (on pipekit the host emulates them from received records, and `Plan` gets a second, session consumer), so polling works while a run is live. A per-session sequence number adds resume after a disconnect. Runs time out on **inactivity**, not a fixed 300 s; a cancel that overtakes its run is honoured by **tombstones**.
- **Data in flight gets stronger, in the UI and in the engines.** Today thumbnails arrive as nodes finish but the viewer waits for the reply. Protocol 2 adds watched displays on the `preview` record (with their own 64 KiB inline budget), provisional `snapshot` partials, `append` streams (text tokens, audio blocks) recoverable after a gap, and `run.peek` to open any port of a running graph. G6 makes both VI executors emit per-item `progress`, and the research software's commands stage snapshots through the research software hooks fixed at source.
- **Data plane.** Bytes never travel in messages. A `DataRef` is a content handle (`md5:nd1:<hex>` for arrays, exactly today's digest; `md5:raw:<hex>` for files, alexandria's digest) plus locators. On one machine the UI maps commonkit `.npy` segments, as now, proved by a two-way nonce check rather than hostnames. Remotely, sources go up only when `blob.has` says the engine lacks them, and results come down lazily by handle. MD5 by default; BLAKE3 required for uploads into a store shared by several clients.
- **Checkpoint reuse.** Memo keys move to a **typed canonical form** (each parameter coerced to its declared port type, integers and floats tagged, floats as IEEE-754 bit patterns), so Python and Rust compute the same keys without merging `1` and `1.0`. Today's SHA-1 key stays in Python until the shared vectors pass. A content key is added only where outputs are already published or a node declares `memo.content`. Persistence is **opt-in** for checkpoint-marked and group-boundary nodes, written off the preview path, salted with the resolved dependency set; the exactness and conformance gates run with the memo off.
- **Example engines exercise every mode:** text (Rust; modes 1 and 2), audio (Rust on SoundCraft's DSP crates; mode 2), OpenCV (Python; modes 3 and 4), and a VectorCraft MCP adapter (Rust; mode 2). The research software keeps mode 1 (`research-local`) and mode 4 (Python VI), and `research-local` is also run in mode 2 to prove a separate-process Rust engine is efficient on 8 MB arrays.
- **Effort (re-costed after review):** about **130–160 working days** for everything here, of which **65–80 days** put the research software on the new base with nothing lost (G0–G6). The FloCraft estimate covered one transport, no CAS, no remote and image-only viewing.

## 2. Architecture

### 2.1 Layers

The base keeps the research software's layer rules (`cargo xtask layers`): a crate depends only on lower layers, and nothing below the UI uses egui. New crates register in `xtask/src/layers.rs`. Following the FloCraft study's option D, the extraction happens **in this repository**, under `core/` and `packs/vi/`, with a layers rule that nothing in `core/` names `vi.`, `research`, `research commands` or `yapocis`. The public repository is created later with fresh history.

| Layer | Generic base crate (`core/`) | From today | Notes |
|---|---|---|---|
| L0 | `plumcraft-proto` | `crates/schema` (generic part) and the generic types of the research software's native engine `research_graph-schema` | Ops, events, `DataRef`, `Handle`, `Display` v2, `SchemaReply` v2, protocol 1 parsing, JSON Schema export, fixtures both ways |
| L0 | `plumcraft-shm` | `crates/shm` | mmap reader; resolves a locator rather than only a commonkit key. Still the only crate allowed `unsafe` |
| L1 | `plumcraft-graph` | `crates/graph` | Compatibility rules become engine-declared (spec 7.2); no VI ids |
| L1 | `plumcraft-layout` | `crates/layout` | Unchanged |
| L1 | `plumcraft-store` | new | `BlobStore` resolver (mem, commonkit, path, durable CAS, URL), the durable local CAS with pins and leases, same-host proof, hashing. The UI writes downloads only into this durable tier; it never writes commonkit (no Rust commonkit writer is needed) |
| L1 | `plumcraft-exec` | the generic half of `research_graph`'s executor, lifted once in G4 | Graph semantics (spec 7.6), list mapping, slices, cancel checks, typed canonical memo keys. Used by `research_graph` and the Rust SDK, so there are two executors to hold to the corpus (Python and this), not three |
| L2 | `plumcraft-host` | `crates/bridge` (`client.rs` facade, `status.rs` reducer) and the thread pattern of `crates/local` | `EngineClient` (typed), `EngineCore` trait plus factory, in-process host, conflating event queue, synthesised failures, inactivity timeouts, engine manifest |
| L2 | `plumcraft-rpc` | new; shared with `crates/automation` | JSON-RPC 2.0 newline framing, used by the socket binding, the HTTP `/v1/rpc` body, the control channel and the MCP adapter |
| L2 | `plumcraft-bind-amqp` | `crates/bridge` (lapin) | Behind cargo feature `amqp`. Emulates `run.get` and `events.poll` from received records for every engine |
| L2 | `plumcraft-bind-socket` | new | Spawn, supervise, Unix socket (macOS and Linux only) |
| L2 | `plumcraft-bind-http` | new | reqwest or ureq client, SSE parser, blob client |
| L3 | `plumcraft-session` | `crates/engine` | Session, command registry, `RunState` (sans-IO, unchanged), stale tracking. Reads capabilities, not `plugin_export` |
| L4 | `plumcraft-ui` | `crates/ui-egui` minus VI | Canvas, palette, widgets, params, groups, generic mask painter, generic monotone-spline editor (used only when an engine declares the interpolant). Exposes the **`PackUi` registration API**: editors by `editor.kind`, result windows by `x.<pack>.<op>`, extra commands and texts |
| L4 | `plumcraft-view` | `viewer*.rs` (with the T16 window from the `inspector-window` checkout) | Viewer host, `MediaViewer` registry, core viewers |
| L4 | `plumcraft-view-audio`, `-svg`, `-mesh` | new | Feature-gated: cpal and rustfft; usvg and tiny-skia; wgpu callback |
| L4 | `plumcraft-automation` | `crates/automation` | Control channel and MCP face of the UI |
| — | `plumcraft-testkit` | `crates/testkit` | Fake engine, recorded event replays; fixtures from the text engine, not VI |
| app | `plumcraft`, `plumcraft-cli` | `apps/research`, `apps/research-cli` | `plumcraft_app::run(AppConfig)`; loads engine manifests |
| SDK | `plumcraft-engine-sdk` (+ `-macros`) | builds on `plumcraft-exec` | `Node` trait and `Cx` (minimal form in G4), then `#[node]`, publish, `serve()` for modes 1–3 in G7 |
| SDK | Python `plumcraft-engine` | about 1,300 generic lines of `research_engine` (FloCraft study section 4) | Transport-neutral handler with a thread-safe run table; pipekit, socket and HTTP front ends; `@node` |

### 2.2 The the research software pack (`packs/vi/`)

| Piece | From today | Role |
|---|---|---|
| `research` app | `apps/research` | A thin wrapper: `AppConfig {title, app_id, engines, demo_workflow}`, the research software branding |
| `research-local` | `crates/local` | `impl EngineCore for ResearchLocal` over `research_graph`, built on the worker thread by a factory (its `Rc<Devices>` state is not `Send`); no behaviour change |
| `research-ui` (`packs/vi/ui`) | `export_view.rs`, `mask_view.rs` (VI semantics: `label`, `lo`, `hi`, `<workflow>.edits/` sidecar, digest check), `curves.rs` VI glue, `bundle_view.rs`, `groups_view.rs` checkpoint and codegen gating | Registers with `PackUi` at startup: the `x.vi.export` result window, the VI mask and curves editors, checkpoint and codegen commands |
| Python `research_engine` | `python/` minus the generic part | VI nodes, settings, expansions, exactness, M7 plugin export, `research.yml`; depends on `plumcraft-engine` |
| Pack ops | `export_cmds.rs`, `deliver.py` | `x.vi.export`, `x.vi.deliver` |
| VI fixtures, exactness, acceptance sessions | `crates/schema/fixtures`, `xtask exactness`, `docs/acceptance/` | Stay with the pack; the exactness gate stays the research software's |

### 2.3 What changes in the coupling list

The FloCraft study's coupling items 1–9 stand. The interface work settles several of them:

| FloCraft item | Settled by |
|---|---|
| 1 Settings bundles | `wire_types[].bundle`, `flags.bundle_port`, `param_ref{group, name}`, `texts.blank_value` (spec 7) |
| 2 Plugin export | `NodeDef.export{markable, mark, x}` and capabilities `x.vi.export`, `x.vi.deliver` |
| 3 Custom editors | `editor{kind, port}`, `NodeReport.editor{kind → payload}`, capabilities `editor.*` |
| 4 Wire-type constants | `wire_types[].media`, `views`; compatibility in `compat` |
| 5 Engine identity | `hello.engine{id, name, version, components, salt}`; manifest for system name and hint |
| 6 Transport | Four bindings (section 5) instead of one subprocess transport |
| 7 commonkit store | `plumcraft-store` resolver; keys optional |
| 8 Exactness gate | Stays in the pack; core gets `xtask conformance` |
| 9 Tests and fixtures | Text engine fixtures replace the 90 KB VI schema fixture in core tests |

The floor note found three more hard-coded VI ids (`RESULT_KIND = "vi.op.result"`, the "Result:" title prefix, `SETTINGS_PORT`) and duplicated curve maths in `curves.rs`. The first three become `flags.result`, `texts.result_prefix` and `flags.bundle_port`. **The curve maths stays in the research software** (golden rule 6): the engine declares the interpolant in its schema (`x.<pack>.curves.interpolant`, for VI `monotone_cubic`) and ships the sample fixture (`python_curve_samples.json`). Core's generic monotone-spline editor is enabled only when the declared interpolant is one it implements and its output matches the fixture; otherwise the pack registers its own editor through `PackUi`. The PNG recipe chunk becomes `<app_id>.recipe`, and readers keep accepting `research.recipe` (F77).

## 3. The UI ↔ engine interface

The full rules are in the spec. This section is its summary.

### 3.1 Shape

One **session** per UI–engine connection. Within it, the UI calls **operations** and receives one ordered **event stream**. Bulk data is named by **DataRefs** and moved outside messages.

| Group | Operations | Notes |
|---|---|---|
| Session | `hello`, `session.confirm`, `ping`, `shutdown` | Protocol major version plus capability strings; two-way same-host proof; resume with `{session, since}` |
| Catalogue | `schema` | Cached by digest (`{unchanged: true}`); `schema.changed` event |
| Graph | `graph.put` (optional) | Send a graph once by handle; runs then name it |
| Runs | `run.plan` (optional), `run`, `run.cancel`, `run.watch`, `run.get`, `run.peek` | `run` acks at once; the reply is an event and is also in `run.get` |
| Events | `events.subscribe` (push), `events.poll` (long-poll) | Every event carries a per-session `seq` |
| Data | `blob.has`, `blob.put`, `blob.get`, `blob.derive`, `memo.lookup` | Remote upload dedup, lazy download, derivatives (tiles, regions, peaks) |
| Outputs | `output.save` | Writes locally, or returns files by handle for the UI to write |
| Packs | `x.<pack>.<op>` | `x.vi.export`, `x.vi.deliver`; shown only when the capability is present |

Mandatory for every engine: `hello`, `session.confirm`, `ping`, `schema`, `run`, `run.cancel`, `run.get`, `events.poll`, and the record states `started`, `preview`, `done`, `cached`, `error`, `skipped`, `cancelled` with `ms`. Everything else is a capability (spec 5.3), and commands' `enabled` checks read capabilities, never engine names.

### 3.2 Types (sketch)

```rust
pub struct EngineClient { requests: Sender<Request>, events: EventQueue, caps: Caps, state: EngineState }

pub enum Request { Schema { digest: Option<String> }, Run(Box<RunRequest>), Cancel { run_id: String },
    Watch { run_id: String, watch: Vec<WatchRequest> }, Get { run_id: String }, Peek(Box<PeekRequest>),
    Save(Box<SaveRequest>), BlobHas(Vec<Handle>), BlobPut { handle: Handle, source: BlobSource },
    BlobGet { handle: Handle, range: Option<Range<u64>> }, Ext { op: String, body: Value }, Ping, Shutdown }

pub enum Event { State(EngineState), Schema(Box<SchemaReply>), Status(Box<StatusEvent>), Live(Box<LiveUpdate>),
    RunReply(Box<RunReply>), Snapshot(Box<RunSnapshot>), Peek(Box<PeekReply>), Save(Box<SaveReply>),
    Blob(BlobOutcome), Ext { op: String, id: u64, result: Result<Value, ProtoError> },
    Log(LogRecord), Resync { run_id: Option<String>, reason: String } }

// The host takes a factory and calls it ON the worker thread, so the core need not be Send
// (research-local holds Rc<Devices>). After a caught panic the host drops and rebuilds the core.
pub type CoreFactory = Box<dyn FnMut() -> Result<Box<dyn EngineCore>, String> + Send>;

pub trait EngineCore {                     // what a compiled-in engine implements; not Send
    fn hello(&self, req: &Hello) -> HelloReply;
    fn schema(&self) -> Result<SchemaReply, ProtoError>;
    fn run(&mut self, req: RunRequest, emit: &mut dyn Emit, cancel: &CancelToken) -> RunReply;
    fn peek(&mut self, req: PeekRequest) -> PeekReply;
    fn save(&mut self, req: SaveRequest, cancel: &CancelToken) -> SaveReply;
    fn op(&mut self, name: &str, body: Value, emit: &mut dyn Emit, cancel: &CancelToken) -> Result<Value, ProtoError>;
}
```

This is today's `EngineRequest`/`EngineEvent` facade (`crates/bridge/src/client.rs:133,149`) made typed and complete. `RunState` stays sans-IO and `StatusReducer` keeps its rules.

**Reserved now for multi-engine graphs** (so they are not a shoehorn later, review M8): `RunRequest.inputs: {"node.port": Value}` binds external values (DataRefs from another engine) to ports; a workflow node MAY carry an `engine` field; `Session` holds `BTreeMap<EngineId, EngineClient>` and v1 puts exactly one entry in it.

**After a caught panic** (`catch_unwind` around every core call) the host fails the run in flight with `kind: engine`, drops the core, emits `engine_down` then `connected` after the factory rebuilds it, and the memo starts empty. `research-local` installs a non-panicking wgpu uncaptured-error handler that turns device errors into node errors.

### 3.3 One event, on any binding

```json
{"v": 2, "seq": 1043, "ts": 1792170000.123, "kind": "node.status", "run_id": "0b5e…", "generation": 17,
 "body": {"node_id": "n7", "seq": 41, "state": "preview",
          "thumb": {"kind": "rgba8", "w": 160, "h": 159, "ref": {"handle": "md5:nd1:3f2a…", "at": [{"kind": "commonkit", "key": "impc:v1:impcprev:3f2a…:c"}]}},
          "display": [ … the watched Display, when display.stream is on … ]}}
```

| Event kind | Carries | Delivery class |
|---|---|---|
| `engine.state` | Today's `EngineStatus` plus `busy` and `caps` | Never dropped |
| `node.status` | Today's `StatusRecord` plus `stage` and `display` | `progress` and `preview` conflate to latest per node; other states never dropped |
| `node.live` | `snapshot` (provisional value), `append` (ordered chunk with offset), `log` | Snapshot latest per (node, port); append never dropped |
| `run.reply` | Today's `RunReply` plus `editor`, `memo` keys; `inspect` renamed `watch` | Never dropped; authoritative |
| `log`, `resync`, `schema.changed` | | |

**Decision** (where notes 02 and 04 differed): progress and preview stay `node.status` states rather than new kinds, so the reducer and both existing engines are untouched; streaming chunks get their own `node.live` kind.

### 3.4 Node status: push with poll fallback

| Mode | Carrier | Requirement |
|---|---|---|
| Push | In-process callback and wake; JSON-RPC notification; SSE; AMQP topic `{system}.status` | Every binding that can push |
| Long-poll | `events.poll{since, wait_ms}` | Every request/response binding |
| Poll | `events.poll{since, wait_ms: 0}` every 100 ms while a run is live; `run.get` snapshot only on resync | **Every engine**, served off the run path (on AMQP emulated by the host from received records, for protocol 1 and 2 engines alike) |

The reducer rules are normative (spec 9.5): keyed by `(run_id, node_id)`, older generations ignored, ordered within a node by `(publisher, publisher_seq)` and then by state rank (so a lagging worker's `started` is not dropped as "older" under fan-out), never back from terminal, `preview` adds data without regressing, the reply fills everything. Duplicates (AMQP redelivery) are dropped by `(publisher, publisher_seq)`. Any binding may lose, duplicate or reorder status events without showing anything wrong. Resume: `hello{session, since}` replays from a ring buffer (4,096 events or 60 s); a gap produces `resync` and `run.get` per live run. The engine never blocks its executor on a slow UI: conflatable events sit in latest-per-key slots, and a stalled subscriber is dropped and resyncs. On the UI side only the conflated slots are bounded; when the ordered queue overflows, the host discards it and resyncs rather than blocking the engine thread or dropping a terminal record.

**Timeouts and cancel (review H7, H8).** After the `run` ack the host times out on **inactivity** (no event and no `ping` answer for N seconds; N per binding: 30 s in-process and socket, 60 s HTTP and AMQP), never on total duration. Before synthesising a failure it tries `run.get`; a real reply of the current generation that arrives later replaces a synthesised failure. The fixed 300 s ceiling survives only for protocol 1 engines, which send no ack. Engines keep **tombstones** of cancelled `run_id`s for `run_ttl_s`, so a cancel that overtakes its run (HTTP on its own connection, an AMQP fanout ahead of the queued run) ends that run at once with `cancelled`. On supersede the host also drops queued, not-yet-sent runs instead of sending and cancelling them.

**Sessions own runs (review M3).** Events are scoped per session. Only the owning session (or a token holding the `admin` capability) may cancel or watch a run. A run whose session has had no subscriber and no poll for 30 s is cancelled unless it was sent with `detach: true`, so a crashed UI can't block a `max_runs: 1` engine for ten minutes.

### 3.5 Data in flight

| Tier | Today | Protocol 2 |
|---|---|---|
| Node finished | Thumb on the `preview` record; the viewer waits for the reply | Same, plus the watched Display on the same record (`display.stream`), under its own `max_display_inline` budget (64 KiB on local bindings) so a Lab display with four histograms stays inline |
| Node partial | `progress` fraction, never emitted | `progress` with `stage`, emitted per list item by both VI executors (G6); `node.live` `snapshot` with a provisional Display (the research software's commands stages through the research software hooks, `Cx::snapshot` in `research_graph`, Perona–Malik iterations, STFT columns) |
| Growing value | none | `node.live` `append` with offsets (text tokens, audio blocks, table rows); engines keep append buffers until `done`, and `run.get` returns `{offset, partial: DataRef}` per appending port, so a gap or resync recovers without waiting for `done` |
| Open a running port | Rerun the graph with the viewer's requests | `run.peek` (display now, or `pending` then the display); `run.watch` changes the watch list mid-run and MUST emit displays at once for watched nodes that already finished |

Snapshots are throttled to 100 ms per node locally and 250 ms remotely, and only for watched values. Provisional values never survive `done`. **Exit criterion (G6):** watching a pencil-style node on either VI engine shows its the research software's commands stages before `done`.

### 3.6 Errors

Node errors stay data (`config`, `op`, `skipped`, plus `missing_source`); run-level errors are `graph`, `engine`, `cancelled`. Protocol errors are JSON-RPC codes with an application range (`version`, `busy`, `cancelled`, `not_found`, `too_large`, `unauthorized`, `unavailable`, `integrity`, `unsupported`) mapped to HTTP statuses and pipekit `pk-error`. **The host guarantees exactly one outcome per request**: the engine's reply or a synthesised failure of the same type, generalising today's `failed_run`, `failed_export` and `failed_save` to every op (F35).

### 3.7 Versioning

- Protocol major version in `hello`, and `"v"` on every message (today's messages are protocol 1, the new ones 2).
- Everything additive is a capability string, as MCP and photocraft do.
- Unknown fields and event kinds are ignored.
- Protocol 1 maps to protocol 2 field by field (spec 17), pinned by fixtures written from today's `crates/schema/fixtures/python_*.json`.
- **The control channel keeps its v1 shapes (review M9).** `ui.inspect` and every other control reply serialise a **v1 projection** of the new types (`BrokerUnreachable`, `InspectRequest`, Display v1), so the maintainer's journals replay unchanged. New fields appear only under a `v2` key or behind a `control.v2` request flag. G0 adds a journal-replay test that runs every kept session journal against the base.

## 4. Data plane

| Question | Decision |
|---|---|
| What names bytes | A `DataRef`: handle, typed `BlobMeta` (`form`: array, text, json, audio, vector, image, mesh, doc, bytes) and locators (`mem`, `commonkit`, `path`, `url`, `inline`) |
| Handle format | `<algo>:<recipe>:<hex>`. `nd1` = today's array digest (dtype string, shape repr, C-order bytes), so every existing `impc` key stays valid. `raw` = the file's own bytes, alexandria's item digest. Shared test vectors for Rust and pytest |
| Which hash | **MD5 by default** (commonkit, the research software checkpoints, the research software keys, alexandria all use it). **BLAKE3 required for uploads into a store shared by several clients**, because MD5 chosen-prefix collisions are cheap. The server keeps an MD5 → BLAKE3 alias index from bytes it hashed itself |
| Single system | The engine publishes to commonkit; the UI maps by path, zero-copy (`research-shm` unchanged). Proved per session by two nonce files, one each way, so a Docker or VM engine on the same Mac falls back to remote mode |
| Compiled-in | Typed values through channels; optional `mem` locators (an `Arc` table) when a profile shows the commonkit write matters |
| Remote upload | The host gathers sources by the same walk as `graph.bundle`, hashes each (cached by path, mtime, size), asks `blob.has`, uploads only what is missing, and rewrites path values to `{source: {handle, name}}`. The engine never sees a client path. Uploads verify while streaming; resumable for large files |
| Remote download | Lazy by handle. On remote sessions thumbs of 4 KiB or less ride **inline**, encoded (QOI or WebP at 80 px), in `preview` records, so a 100-node graph costs no thumb round trips; they stay in UI memory and are never published to commonkit. Then the watched renditions (batched with `blob.get_many`, or multiplexed over HTTP/2), then nothing. Full values only for 1:1 zoom (tiles), probe (regions), playback (frames), compare, save or pin. Verified, then written into the UI's **durable tier**, which the UI maps exactly as it maps commonkit today |
| Previews on a slow link | Encoded on the wire (PNG or QOI lossless; WebP/JPEG for thumbs if negotiated), superseded previews dropped, a client bandwidth budget |
| Checkpoint reuse | Memo key = hash of the **typed canonical form** (spec 14.1) of `[kind, node version, engine salt, device class if sensitive, resolved params coerced to their port types, input identities]`. Two lookups: the **plan key** (today's Merkle chain, portable) and, only for nodes whose outputs are already published or that declare `memo.content: true` (expensive nodes, group boundaries), the **content key** (input handles). Scopes per node: `none`, `session`, `machine` (default), `global` (needs cross-device bit-exactness attestation) |
| Engine salt | Code digest **plus** a digest of the resolved dependency set (`uv.lock` or `Cargo.lock`, `numpy.__version__`, `cv2.__version__`, Metal or MLX and driver identity), so an upgrade never serves stale bytes from a persisted memo |
| Persistence | **Opt-in**, and only for checkpoint-marked or group-boundary nodes. Written on a background thread that never delays a `preview` record, by `clonefile` on APFS when on the same volume, by copy otherwise (commonkit is on `/dev/shm` on Linux, so no hard link). `memo.audit` recomputes a sample of hits |
| Tiers | T0 executor RAM (1.5 GB LRU, today). T1 commonkit (TTL 900 s, unchanged; written only by engines). T2 durable local CAS, `~/Library/Caches/<app>/cas/<shard>/<digest>.<ext>`, pins and leases, 20 GB budget; the UI's only write target. T3 remote CAS at an alexandria-style URL base |
| Missing data | Never an error: a miss greys the value, marks it stale and reruns once (F58) |

**Note on memo keys:** note 03 proposed BLAKE3 for memo keys. The plan uses MD5 when an engine accepts a single client token: memo keys are then computed from inputs one trusted client chose. **When an engine accepts more than one client token**, parameters and templates are chosen by several parties and chosen-prefix collisions apply, so memo keys MUST use `b3` there.

**Gates run with the memo off.** `xtask exactness` and `xtask conformance` run with `memo: "off"` and a fresh store, so a persisted result can never mask an exactness failure.

**Cross-machine sharing stays off by default.** STATUS records that host numerics differ between the development machine (arm64) and a remote server (x86-64) for OpenCV and numpy functions, so VI nodes are `machine` scope until an attestation exists.

## 5. Transport bindings

| Order | Binding | Hosting mode | Why | Days |
|---|---|---|---|---|
| 1 | In-process: typed `EngineCore` calls through a factory, conflating queue | 1 | The research software's default path; must not regress; removes today's `Value` parse | 2–3 |
| 2 | AMQP/pipekit: today's bridge behind the facade; the host emulates `run.get` and `events.poll` from received records; `Plan` gains a second consumer on a `{system}.Session` queue (own thread, lock-protected run table) answering `hello`, `run.peek`, `run.watch`, `blob.has` off the run path | 4 | No the research software loss; fleets; M9 scatter/join | 5–6 |
| 3 | Local socket, JSON-RPC 2.0 over newline-delimited JSON; engine connects back; never stdout; Unix sockets only; same host REQUIRED (no byte frames: a failed proof fails closed with `unavailable`) | 2 | Default for separate-process Rust and Python engines and the examples; the same framing as the artcraft MCP servers and `--control` | 4–5 |
| 4 | HTTP + SSE: `POST /v1/rpc`, SSE events with `Last-Event-ID`, long-poll, blob endpoints | 3 | Remote engines; any language; a run survives a dropped connection while its session polls or resubscribes within 30 s, or for 10 min when sent with `detach: true` | 5–6 |
| — | MCP adapter: an engine (not a binding) wrapping an artcraft app | 2 (or 3) | MCP lacks node states, replay and blobs; the adapter supplies them | section 8 |

**Deferred, with the message rules keeping them a mapping exercise** (tagged unions, string keys, u64 counters, no bytes in messages, one schema source):

- **gRPC** (tonic 0.14 + prost): a second schema source (`.proto`), heavy dependency tree, grpcio wheels on the Python side. Bulk never travels in messages here, so its binary efficiency buys nothing. Revisit if a third party demands it.
- **WebSocket** (tokio-tungstenite 0.30): the socket binding's frames over WS, if SSE plus POST ever proves awkward (for example frequent mid-run watch changes).
- **msgpack codec** (rmp-serde): reserved as `codec.msgpack`; built only if a profile shows JSON is the bottleneck.

**Rejected:**

| Option | Reason |
|---|---|
| Cap'n Proto RPC | A second schema, a small Python community, little gain when arrays are out of band |
| Arrow Flight | Very heavy (arrow + tonic), bulk only, no status model. A possible future `flight://` locator if remote bulk ever dominates |
| NATS / JetStream | Another broker to run; RabbitMQ is installed and stream queues give replay if needed |
| ZeroMQ | No resume (SUB drops on reconnect), weak auth, nothing the socket binding lacks |
| tarpc | Rust only; no Python |
| jsonrpsee | Unneeded: the framing is about 300 lines, as the artcraft MCP servers show |

Every binding passes one conformance suite (spec 19) before it ships.

## 6. Engine hosting modes and SDKs

| | 1 Compiled-in | 2 Separate-process Rust (any language) | 3 HTTP | 4 Python / pipekit |
|---|---|---|---|---|
| Today | `research-local` | — | — | `research_engine` over RabbitMQ |
| Binding | In-process | Socket JSON-RPC | HTTP + SSE | AMQP |
| Start | Thread start | Host spawns from a manifest; engine connects back with a token | Manifest URL, or a manifest command with `--listen 127.0.0.1:0 --port-file` | `pipectl up research.yml` (hint in the manifest) |
| Health | Thread alive; `Unavailable` on no device | Child alive + `ping` every 3 s (answered by the reader thread) | `/v1/health` every 3 s | Consumer counts every 3 s; backoff 1–30 s |
| Crash | `catch_unwind` → failed reply, core dropped and rebuilt by the factory, `engine_down` then `connected` | Failed replies, `engine_down{stderr_tail}`, restart with backoff, graph kept; stop after 5 in 60 s | Reconnect and resume; owned runs survive 30 s idle, detached runs 10 min | `EngineDown{queues}` with the fix |
| Status push | Channel + wake | Notifications | SSE; polling fallback | `{system}.status` topic |
| Cancel | `AtomicBool` per run | Notification read by the reader thread | `POST /v1/runs/{id}/cancel` | `{system}.control` broadcast |
| Data plane | shm (optionally `mem`) | shm; same host REQUIRED, proof failure → `unavailable` | shm when proved, else CAS | shm when proved, else CAS through the `research.Blobs` daemon (Python stdlib, shared with the mode 3 front; a `research.yml` service, G8) |
| Remote | No | No | Yes | Yes (broker) |
| Fan-out | No | No | No | Yes (M9) |
| Extra hop per record | 0 | One local write | One HTTP chunk | Broker |

**Rust SDK (`plumcraft-engine-sdk`).** A `#[node]` attribute derives the NodeDef from the function signature (ports from types implementing `WireType`, widgets from `#[param(...)]`, outputs from the return type, the memo version from `version`). A plain `Node` trait remains for dynamic catalogues (the MCP adapter). `Cx` gives `cancelled()`, `progress()` (rate-limited), `snapshot()`, `append()`, `publish()`, `fetch()` and `log()`. The executor is `plumcraft-exec`, the generic half of `research_graph`'s executor lifted once in G4 and used by both `research_graph` and the SDK, held to the Python executor by the shared graph corpus (spec 7.6), which runs against both executors and all three engine paths. A minimal SDK (`Node` trait, `Cx`, executor; no macro) lands in G4 so the text engine is written once against it. One `serve()` hosts an engine in modes 1–3: `--connect unix:…`, `--listen host:port`, or `.into_core()` for in-process.

**Python SDK (`plumcraft-engine`).** The FloCraft split (schema, executor, io, publish, display, curve maths) plus a transport-neutral `Engine` handler (`hello`, `schema`, `run(req, emit, cancelled)`, `peek`, `op`) over a **thread-safe run table** (snapshots, append buffers, cancel tombstones, session ownership) so `run.get`, `run.peek`, `run.watch`, `events.poll` and cancel are answered on a thread a running graph never blocks, with three front ends: `pipekit` (today's `Schema` and `Plan`, an optional extra), `socket` (reader and worker threads) and `http` (stdlib `ThreadingHTTPServer`, SSE by chunked writes; no web framework). `@node` decorator over today's `FuncNode`. `publish.Store` with commonkit, path and CAS backends. Entry: `python -m plumcraft_engine.serve <module>:ENGINE --connect … | --listen … | --pipekit research.yml`.

**Python VI can then run in mode 3** on a remote server without a broker, from the same handler.

## 7. Generalised visualisation

The current `Viewer` splits into a **host** and a **registry of media viewers** (note 04).

- **The host keeps** everything generic in `viewer.rs` today: Current/A/B slots, pins, compare on and off, list stepper and sheet, the T16 own window, the recipe panel, missing-file rerun-once, rerun dedup and the `viewer.*` commands. It batches viewers' wants into the next watch request (or a `run.peek`).
- **Each viewer** (`MediaViewer` trait) declares what it accepts (a score per Display), its capabilities (zoom, pan, probe, channels, side-by-side, split, overlay, diff, sheet, play, stream, window, save, thumb), its wants, its `ui`, `thumb`, `probe(locus)`, `compare` and `live` methods. The toolbar is built from capabilities, not hard-coded. "View as" lists every viewer that accepts a value, always including Hex and the rgba8 fallback.
- **Engines describe, the UI decides.** Each wire type names a media kind and its view modes in the schema; the Rust `MODES` list goes. Any engine can show anything through an `rgba8` rendition.

| Viewer | Crate | Notes |
|---|---|---|
| Image | `plumcraft-view` | Today's code behind the trait: exact probe from the mapped full array, abs(A−B) max-of-block with NaN magenta, split, side by side, channel grid or tabs, histograms, sheet, 1:1, own window. Adds blink |
| ndarray dispatch, Table, Scalar, Tree, Hex, Composite, Sequence | `plumcraft-view` | Table grid with egui_extras; scalar history; JSON tree; virtualised hex |
| Text | `plumcraft-view` | Virtual scroll over a mapped buffer, search, spans; `similar` line/word diff; streams appends |
| Audio | `plumcraft-view-audio` | Waveform from peaks, spectrogram (rustfft), cpal 0.18 playback with sample-aligned A/B switching and a null test, following SoundCraft |
| SVG | `plumcraft-view-svg` | usvg 0.48 + tiny-skia 0.12, external resources off |
| Mesh | `plumcraft-view-mesh` | wgpu through `egui_wgpu::Callback`, as SolidCraft |

**Compare** returns `{exact, summary, stats, views}`, with `exact` first: equal handles prove bit equality without reading data, which makes the exactness rule a viewer feature for every type. A new **Freeze** pin holds a value by handle, so run-versus-run, before-versus-after an edit and Python-versus-Rust comparisons become possible. **Probe** generalises to a locus per kind (pixel, sample, text position, cell, vertex, JSON path, byte offset, SVG element). Canvas thumbnails come from each viewer's `thumb()`, so an audio node shows its waveform and a text node its first lines.

## 8. Example engines

| Engine | Language | Mode(s) exercised | What it proves | Days |
|---|---|---|---|---|
| **Text** (`plumcraft-engine-text`) | Rust, SDK reference | **1 and 2** from one crate (3 via `--listen`) | An engine moves between in-process and separate process with no code change; list mapping, slices and variadics; the neutral fixture that replaces the VI schema in core tests. About 45 nodes: split, join, regex, case, Unicode, frequencies, CSV, JSON pointer, diff, minijinja template; expansions Title Slug and Word Cloud Table | 3 |
| **Audio** (`plumcraft-engine-audio`) | Rust on SoundCraft's `audio-io` and `dsp` crates (pinned by git rev) | **2** | Crash isolation for decoders; long ops with progress and STFT `snapshot` partials; the audio viewer end to end. About 40 nodes: load, save, generators, trim, fades, gain, LUFS normalize, compressor, resample, stretch, pitch, biquads, EQ, reverb, channels, FFT, STFT, envelope, loudness, transients, pitch track; expansion Clean Voice | 5–6 (+ viewer in G6) |
| **OpenCV** (`plumcraft-engine-opencv`) | Python, `opencv-python-headless` | **3** (local shm and remote CAS) and **4** (same handler under pipekit) | The Python SDK is transport-neutral; upload and download over CAS. About 50 nodes in colour, filter, edges and features, threshold, morphology, geometry, photo (stylization, pencil sketch, inpaint with core's mask painter), clustering, composite; expansion Cartoon. Determinism: OpenCL off, seeds as widgets, `cv2.__version__` in the memo version | 4 |
| **VectorCraft MCP** (`plumcraft-engine-vectorcraft`) | Rust adapter over `vectorcraft-cli mcp --headless` | **2** (3 remotely over SSH) | A dynamic catalogue; a stateful app made functional by threading a document handle; MCP progress and cancel mapped onto ours. Nodes: Image Trace, Simplify, Offset Path, Outline Stroke, Pathfinder, Recolor, effects, Rasterize (to ARRAY for VI or OpenCV), Export, Run Command | 8–10 with the generic adapter |
| The research software local | Rust (`research_graph`) | **1** | Existing default; loses nothing | — |
| The research software Python | Python | **4** (and 3 on a remote server) | Existing; exactness gate | — |

**Why VectorCraft for the MCP engine, and SoundCraft second.** Note 05 ranked SoundCraft first (typed plugin catalogue, audio) and note 06 ranked VectorCraft first. The audio example already comes from SoundCraft's DSP crates in pure Rust, so an MCP SoundCraft engine would duplicate it. VectorCraft's MCP server already implements progress and cancellation, its Image Trace connects raster graphs (VI, OpenCV) to vectors, and its outputs preview as images, so the first version needs no new viewer. The adapter is generic over every craft (they share one `CommandSpec` model), so a SoundCraft pack afterwards is about 4–5 days. Both depend on proposals at source: a JSON Schema `params_schema` and an `effect` class (query, mutate, view, ui, io) on `CommandSpec` in craft-libs, and for SoundCraft a pure `dsp.process` plus `.npy` I/O.

**Housekeeping found on the way:** Claude Code's MCP configuration points the craft servers at `/Volumes/src/artcraft/…`, which doesn't exist; the binaries are under `/Users/me/src/…`. Every craft server except figcraft failed to connect, so the artcraft findings come from source and CLI runs, not live MCP calls. SolidCraft has no release build.

## 9. Functionality-floor conformance

Every item of the floor inventory maps to the spec. "Spec n" is a section of the interface file. Anything without a row would be a bug in this plan.

| F | Item | Provided by | Change |
|---|---|---|---|
| F1 | Engine connection state | Spec 12.4 `EngineState`, `engine.state` event | Same states, plus `busy` and `caps` |
| F2 | Reconnect with backoff | Spec 16.2, 16.3, 16.4 per-binding reconnect; spec 12.3 | Same backoff; requests fail at once while down |
| F3 | Liveness / health | Spec 5.5 `ping`, consumer counts, `/v1/health` | Same for AMQP; new for others |
| F4 | Catalog fetch | Spec 6.1 `schema`, 7.1, 7.4 | Adds `{unchanged: true}` by digest |
| F5 | Version gate | Spec 3, 5.2, 17 | Protocol major plus capabilities; unknown fields ignored as today |
| F6 | Engine identity, capabilities | Spec 5.2 `engine`, 5.3 caps | `plugin_export` becomes `x.vi.export`, `x.vi.deliver`; `schema_digest` kept |
| F7 | Catalog hook | Spec 7.4 | Unchanged (UI side) |
| F8 | NodeDef core | Spec 7.3 | Same fields; `code`, `helpers`, `picture` pass through under `x.vi` |
| F9 | Wire types | Spec 7.2 | Adds `media`, `views`; compatibility declared in `compat`, today's rules as defaults |
| F10 | Families and icons | Spec 7.1 | Unchanged |
| F11 | Port widget fields | Spec 7.3 | Unchanged |
| F12 | Socket modes | Spec 7.3 | Unchanged |
| F13 | Setting binding | Spec 7.3 `param_ref` | `setting{section, name}` read as `param_ref{group, name}` |
| F14 | Variadic inputs | Spec 7.3, 7.6 rule 4 | Unchanged |
| F15 | Choice-dependent sub-ports | Spec 7.3 | Unchanged |
| F16 | Path hints | Spec 7.3, 8.1 | Absolute paths when `data.path`; sources by handle otherwise |
| F17 | External ports | Spec 7.3 | Unchanged |
| F18 | Machine-dependent choices | Spec 7.3 | Unchanged |
| F19 | Node flags | Spec 7.3 | Adds `bundle_port`, `result` (replacing hard-coded ids) |
| F20 | List typing | Spec 7.6 rule 2 | Now normative for every executor |
| F21 | Wire slices | Spec 7.6 rule 3, 14.2 | Normative grammar; part of memo keys |
| F22 | Custom editors | Spec 7.3 `editor{kind, port}`, caps `editor.*` | Port name declared, not assumed |
| F23 | Codegen metadata | Spec 7.3 `export` | `checkpointable` → `export.markable`; codegen under `x.vi` |
| F24 | Expansions | Spec 7.5 | Unchanged |
| F25 | Style graphs | Spec 7.1 `templates` | Renamed; `style_graphs` still read |
| F26 | Run request | Spec 8.1 | Ack plus `run.reply`; graph inline or by handle |
| F27 | Targets | Spec 8.1 | Unchanged |
| F28 | Run identity, generation | Spec 8.1, 9.5 | Unchanged |
| F29 | Latest wins | Spec 8.2 | Unchanged (UI side) |
| F30 | Auto-run | Spec 14 (memo), `graph.put` | Unchanged; cheaper remotely |
| F31 | Optimistic queued | Spec 8.3, 9.4 | Unchanged; engines may now emit `queued` |
| F32 | Run reply | Spec 8.4, 8.5 | Superset; also in `run.get` |
| F33 | Per-node report | Spec 8.4, 4.5 | Outputs as typed Values; `editor` map; memo keys |
| F34 | Errors as data | Spec 12.1, 7.6 rules 6–7 | Adds `missing_source` |
| F35 | Failure stands in for a reply | Spec 12.3 | Generalised to every op; inactivity timeout after the ack; a late real reply replaces a synthesised failure |
| F36 | Cancel | Spec 11, 16 | Out of band on every binding; tombstones honour a cancel that overtakes its run |
| F37 | Run summary for automation | Spec 9.3 `ms`, 8.4 `elapsed_ms`, envelope `ts` | Unchanged; `ms` now MUST |
| F38 | Status record stream | Spec 9 | Same record; envelope adds session `seq` |
| F39 | States emitted | Spec 9.4 | All nine kept; `queued`, `progress` optional to emit, mandatory to accept |
| F40 | Reducer rules | Spec 9.5 | Normative; ordering within a node by `(publisher, publisher_seq)` then state rank, terminal rule unchanged |
| F41 | Canvas status display | Spec 9.3 `stage` | Unchanged; adds stage text |
| F42 | Per-node timing | Spec 9.3 | `ms` MUST on `done` |
| F43 | Parameter wires, bundles | Spec 7.6 rule 5 | Normative |
| F44 | Provenance | Spec 8.4 | Unchanged |
| F45 | Stale tracking | UI side; spec 8.3 plan keys | Unchanged; `run.plan` can confirm |
| F46 | Text on nodes | Spec 8.4 `text` | Unchanged |
| F47 | Thumbnails in flight | Spec 9.3, 9.4 | Same timing, MUST; thumb is a Rendition |
| F48 | Texture cache | Spec 4.4 (handle is identity) | Keyed by handle instead of path: fewer re-uploads |
| F49 | Inspect piggybacks on runs | Spec 8.1 `watch`, 8.6 `run.peek`, 8.7 `run.watch` | Kept as the fallback; cheap peek added |
| F50 | Display payload | Spec 10.2, 17 | Display v2; v1 parses; displays on records (`display.stream`) |
| F51 | View modes | Spec 7.2 `views` | Listed by the engine, not Rust |
| F52 | Pixel probe | Spec 4.4 (`path`, `commonkit`), 10.5 `region`, `probe` | Same zero-copy mmap locally; regions remotely |
| F53 | Compare | Spec 4.4, 10.5 `diff` | Same full-array diff locally; equal handles short-circuit |
| F54 | Tables and scalars | Spec 10.1, 10.2 | Unchanged data; Table viewer added |
| F55 | Sheet | Spec 10.2, 10.4 | Unchanged |
| F56 | Recipe display | Spec 10.2 `recipe` | REQUIRED from lineage-tracking engines: closes the local-engine gap (G2) |
| F57 | Picture window (T16) | Viewer host (section 7) | Unchanged |
| F58 | Missing data is a state | Spec 4.6 | Normative |
| F59 | Curve editor | Spec 8.4 `editor.curves`, `PackUi` | Maths stays in the research software; engine declares the interpolant and ships the fixture; VI editor in `packs/vi/ui` |
| F60 | Mask editor | Spec 8.4 `editor.mask`, 13.2, `PackUi` | Same files locally; uploaded by handle remotely; VI semantics (sidecar, digest check) in `packs/vi/ui` |
| F61 | List mapping with axes | Spec 7.6 rule 2 | Normative |
| F62 | Vary nodes | Spec 7.3 flags | Unchanged |
| F63 | Lineage | Spec 7.6 rule 9, 10.2 | Normative for engines that declare it |
| F64 | Save picture with recipe | Spec 15 `output.save`, 13.2 step 7 | Remote returns files by handle |
| F65 | Export group as plugin | Spec 15 `x.vi.export` | Capability-gated as today |
| F66 | Deliver plugin | Spec 15 `x.vi.deliver` | May log progress with `op_id` |
| F67 | Plugin round trip | Spec 7.5 | Unchanged |
| F68 | Arrays never in messages | Spec 3 rule 7, 4.4 | Normative for every kind |
| F69 | Content addressing | Spec 4.1, 4.2 `nd1` | Same digest; algorithm and recipe tags added |
| F70 | Hostile `.npy` and keys | Spec 18 | Unchanged rules, extended to every parser |
| F71 | Engine memo | Spec 14 | Typed canonical keys (no `1`/`1.0` merge; numpy scalars, tuples, NaN, Inf accepted as today); content key where already published; opt-in persistence |
| F72 | Partial re-run | Spec 8.3, 14.3 | Unchanged |
| F73 | Preview latency path | Spec 16.1 (typed, no JSON), 16.2 (unchanged) | In-process faster; AMQP same |
| F74 | Full resolution on demand | Spec 13.2 step 5 | Unchanged locally; lazy remotely |
| F75 | Pictures as engine-side files | Spec 8.1 sources, 13.2 | Same locally; uploaded by handle remotely |
| F76 | Workflow JSON is the payload | Spec 8.1, 7.6 rule 8 | Unchanged; optionally by handle |
| F77 | Recipes in pictures | Spec 7.1 `templates` | Chunk written as `<app_id>.recipe`; `research.recipe` still read |
| F78 | Bundles | Spec 13.2 step 1 | Its walk gathers sources for upload |
| F79 | Insert graph or group | UI side | Unchanged |
| F80 | Engine-touching commands | Spec 5.3 | `enabled` reads capabilities |
| F81 | Control channel | `docs/control-protocol.md` | v1 projection keeps every reply shape (`BrokerUnreachable`, `InspectRequest`, Display v1); new fields only under `v2`; journal-replay test in G0 |
| F82 | Headless CLI catalog | Spec 6.1 `schema` on any binding | Same output |
| F83 | Config by environment | Spec 16.6 manifest | Env vars kept as aliases; VI variables move to the VI manifest |
| F84 | Single-threaded engines | Spec 5.3 `run.multi`, 8.2 | Serial by default; cancel, ping, `run.get`, `events.poll`, `run.peek` bypass the run path |
| F85 | Local model management (`research-cli models install`) | Pack op `x.vi.models` or CLI-local | Engine-specific; stays a VI CLI command (added to the floor after review L10) |

**The gaps in the floor note's table are closed as follows:** `queued` and `progress` stay in the interface (optional to emit), and both VI executors emit `progress` from G6; `display` on status records becomes `display.stream`; the local engine's missing recipe is a G2 task; export, deliver and round trip stay capability-gated; cancel granularity is normative (between nodes and list items); the curve maths gets one definition; the memo gains persistence; type compatibility and the hard-coded ids become declared.

### 9.1 Performance of the research software on the base

| Path | Today | On the base | Effect |
|---|---|---|---|
| Run request, in-process | `serde_json::Value` built by the session, parsed by `engine.rs:85` | Typed `Box<RunRequest>` moved through the channel | One parse fewer per run |
| Status and reply, in-process | `Value` trees through an unbounded mpsc | Typed events through a bounded conflating queue | Less allocation; no backlog under a burst of progress |
| Arrays and previews | Published as `.npy` (commonkit or fallback), mmap'd by path | Same; DataRef names the same files | Zero-copy, unchanged |
| Hashing | MD5 `nd1` on every published array (both engines) | Same digest, now also the handle; content keys only for already-published outputs or `memo.content` nodes | No new hashing on the default path (an unrestricted content key would cost about 10–13 ms per 7.9 MB intermediate, about 0.5 s on a 50-node graph) |
| Memo keys | SHA-1 of `json.dumps(default=repr)` | Typed canonical form then MD5; SHA-1 kept in Python until shared vectors pass | Same order of cost; small JSON |
| Memo persistence | none (RAM) | Opt-in, checkpoint and group-boundary nodes only, background thread, `clonefile` where possible | Zero on the default path |
| Texture cache | Keyed by path | Keyed by handle | A cached node republished under a new path no longer re-uploads |
| Viewer reruns (mode, item, sheet) | Whole-graph rerun, memo hits | `run.peek` when offered | Fewer reruns |
| AMQP | Broker hops | Unchanged wire; envelope `seq` assigned on receipt; `run.get` and `events.poll` answered by the host from records it already has | Same |
| Live displays on records | Display only in the reply | Up to 64 KiB inline on `preview` records (local); no extra fetch | Faster first view |
| Remote thumbs | — | Inline encoded thumbs ≤ 4 KiB | No per-thumb round trip |
| Per-frame UI cost | Drain mpsc | Drain queue + one viewer lookup by id | Negligible |

**Gate:** a benchmark recorded before G1 (local engine: time to first thumbnail, time to reply, frame time during a run, on the Draught and Pencil style graphs with `test.jpg`) must not regress by more than 5% at any later milestone. The gate also records **hashes per run** and **bytes written per run**, which must not grow on the default path (memo persistence off). From G7 the same benchmark runs with `research-local` in mode 2 (`--connect unix:…`) to show a separate-process Rust engine is efficient on the research software's arrays. Hashing speeds in note 03 are rough and should be measured on the development machine (`claude-debug/hash_bench_*`) before choosing BLAKE3 for large arrays.

## 10. Milestones

Every milestone keeps the research software's gates green: `cargo test`, clippy with `-D warnings`, `cargo xtask layers`, pytest and ruff, **the exactness gate (currently 92 of 92) and the local engine's exact workflows, run with the memo off and a fresh store**, the control-journal replay, plus the benchmark in section 9.1. Every new user action (Freeze pin, View as, peek, engine select, connect, restart and attach, upload cancel, token entry) is an engine command with `panic_hunt` coverage in the milestone that adds it (golden rule 3). Each milestone that changes something visible ends with a `docs/acceptance/` checklist for the maintainer.

| | Milestone | Content | Exit criteria | Days |
|---|---|---|---|---|
| G0 | Prerequisites and the floor as tests | **Gate: T16 (`inspector-window`) merged first.** Decide D1–D3. Branch `generic-base` from this one. "No VI in core" layers rule in report-only mode. Turn F1–F85 into tests: fixtures, recorded event replays through a fake engine (testkit), live checks for both engines. Control-journal replay test. Record the performance baseline, including hashes and bytes written per run | Every F-item has a test that passes on today's the research software with both engines; journals replay; baseline recorded | 8–10 |
| G1 | `plumcraft-proto` and typed messages | Protocol 2 types (ops, events, DataRef, Handle, Display v2, SchemaReply v2); protocol 1 parsing; control-channel v1 projection; JSON Schema export; fixtures both ways; `EngineClient` carries typed requests and events | Floor tests green; exactness green; protocol 1 fixtures round-trip; journals replay unchanged | 4–5 |
| G2 | In-process host; `research-local` on `EngineCore` | Two-thread host built by a factory on the worker thread; `catch_unwind` with core rebuild; non-panicking wgpu error handler; conflating queue with ordered-overflow resync; inactivity timeouts; cancel tombstones; `run.get`, `run.peek`, `display.stream`; recipe in the local engine's displays (F56) | A node that panics gives a failed reply and a rebuilt engine (regression test); T06 and T07 re-run by the maintainer (local engine, local save); benchmark no worse | 4–5 |
| G3 | AMQP host and the Python handler | Bridge behind the facade; host emulation of `run.get` and `events.poll` for every engine; synthesised hello for protocol 1; Python split into `plumcraft-engine` (handler with thread-safe run table, pipekit front) and `research_engine`; `{system}.Session` consumer in `Plan` for `hello`, `run.peek`, `run.watch`, `blob.has`; tombstones; session ownership; `(publisher, publisher_seq)` on records | During a 5 s run `run.get` answers within 200 ms; a cancel sent before its run cancels it; S03 (run) and T15 (round trip) re-run; pytest and exactness green; a protocol 1 engine still works | 8–10 |
| G4 | Extract the generic core | FloCraft items 1–5 and 7–9 as settled in section 2.3; `core/` and `packs/vi/` including `packs/vi/ui` on the `PackUi` API (export window, VI mask and curves editors, checkpoint and codegen gating); `<app_id>.recipe`; `plumcraft-exec` lifted from `research_graph` and a minimal SDK (`Node`, `Cx`, executor; no macro); layers rule enforced; text engine in mode 1 on the minimal SDK as the neutral fixture; core tests ported to it | The no-VI-in-core search over `core/` is empty; S04 (editing) and T16 re-run; the corpus passes on Python and `plumcraft-exec`; all gates green | 17–21 |
| G5 | Data plane | `plumcraft-store`: resolver, two-way same-host proof, durable CAS with pins and leases (the UI's only write target); handle test vectors (dtype normalisation, object dtype refused); typed canonical memo keys with shared vectors (`1` against `1.0`, `1e16`, `5e-324`, NaN, −0.0, large ints) in Python and `plumcraft-exec`, SHA-1 kept in Python until they pass; salt with the dependency digest; opt-in `memo.persist` on a background thread; `memo.audit`; source rewrite | Handles identical from both engines; with persistence on for checkpoint nodes a restart gives `cached` for them and an upgraded dependency gives a miss; a reaped blob is a miss; exactness green with the memo off | 9–11 |
| G6 | Viewer host, registry and live data | Image viewer behind the trait; ndarray, table, scalar, text, tree, hex, composite, sequence; caps toolbar; handle-keyed textures; watch list, `max_display_inline`, `node.live` snapshot and append with `run.get` partials; `run.watch` displays for finished nodes; Freeze pins; generic compare. **Engine side:** per-list-item `progress` from the Python executor and `research_graph`; the research software's commands stage snapshots through the research software hooks (fixed at source); `Cx::snapshot` in `research_graph` | A new acceptance session repeats T16 and the M3 inspection checklist on the base; watching a pencil-style node shows its stages before `done` on both engines; nothing lost | 15–18 |
| | **The research software on the base, nothing lost** | G0–G6 | | **65–80** |
| G7 | Separate process and the Rust SDK | Socket binding and supervisor (Unix sockets; same host required); `plumcraft-rpc` shared with automation; full Rust SDK with `#[node]` on `plumcraft-exec`; text engine in mode 2; **`research-local` also in mode 2** | Conformance suite passes for text in modes 1 and 2 and for `research-local` in mode 2, including `kill -9` recovery; section 9.1 benchmark recorded for `research-local` in mode 2 | 14–17 |
| G8 | HTTP, remote CAS and OpenCV | HTTP+SSE binding and server (Rust SDK and Python stdlib); blob endpoints, upload dedup, lazy verified download into the durable tier, inline remote thumbs, `blob.get_many`, derivatives; `research.Blobs` daemon in `research.yml` for mode 4 remote; OpenCV engine in modes 3 and 4; VI Python in mode 3 on a remote server | Conformance for OpenCV in modes 3 and 4 and for VI in modes 3 and 4 remote; a remote the research software run from one machine to another uploads `test.jpg` once and shows thumbs in flight | 14–17 |
| G9 | Audio | Audio engine on SoundCraft's crates; audio viewer (waveform, spectrogram, cpal playback, A/B, null test); SVG viewer | Conformance for audio in mode 2; an acceptance session on audio nodes | 9–12 |
| G10 | Artcraft MCP adapter | Generic craft adapter (rmcp client, pool, status emission, cancel, fused batches); VectorCraft pack; source proposals sent to craft-libs | Conformance for VectorCraft in mode 2; a VI render → Trace → Recolor → SVG graph works | 9–12 |
| G11 | Public repository | FloCraft phases 3–4: docs, author guides, CI on macOS and Linux, licenses (MIT/Apache dual), fresh-history `plumcraft` repo; the research software pins it by git rev | The research software builds against the pinned base; all gates green in both repos | 10–14 |
| | **Total** | | Sum 121–152; budget with contingency | **about 130–160** |

The mesh viewer (3–4 days) and the SoundCraft MCP pack (4–5 days) are outside the total.

### 10.1 Milestone S: the thin slice (decided 2026-10-27)

The maintainer chose to prove the interface on a short path before committing to G0–G11: **G0–G2, plus the text engine in mode 1 (in-process) and mode 2 (separate process)**, then re-plan. Everything after S is re-decided then.

**Order (revised by the maintainer, 2026-10-27): PlumCraft code first.** The floor is not written as tests on today's the research software. It becomes PlumCraft's engine conformance suite, which every engine must pass; the research software's engines are simply the first to run it. Only what must be captured before anything changes (journal replay and the performance baseline) runs on the old code.

| | Step | Content | Exit criteria | Days |
|---|---|---|---|---|
| S0 | Prerequisite | **Met:** T16 (`inspector-window`) merged to the research software's `main` as PR #58 on 2026-10-27. This repository holds the base; the research software changes (the `research-proto1` mapping, the in-process host) go on the research software branch that pins `plumcraft` by git rev | `plumcraft` exists with these docs | — |
| S1 | `plumcraft-proto` (G1) | Protocol 2 types with the D14 multi-engine fields reserved and D23 session ownership; protocol 1 parsing; control-channel v1 projection (D21); JSON Schema export. **Placement (2026-10-27):** protocol 1 is the research software's protocol, so its mapping lives in an research-side crate (`research-proto1`, on `research_graph-schema`), never in `plumcraft-proto`, which depends on no engine | Protocol 1 fixtures round-trip; the research software's gates green | 4–5 |
| S2 | Pre-move capture (the part of G0 that must precede change) | Control-journal replay test and the performance baseline (hashes, bytes written, preview latency) on today's the research software | Journals replay; baseline recorded | 2 |
| S3 | Conformance suite and in-process host (rest of G0, and G2) | The floor F1–F85 written as **PlumCraft's engine conformance suite** in `plumcraft-testkit`, phrased in protocol 2 and binding-agnostic. In-process host; `research-local` on `EngineCore`; the suite runs against it, and against the Python engine through the protocol 1 adapter | Every F-item is a conformance test or is marked UI-only with its own test; `research-local` passes in mode 1; benchmark no worse than S2's baseline | 8–10 |
| S4 | Minimal SDK and text engine, mode 1 | Brought forward from G4: `Node` trait, `Cx`, a small executor (no `#[node]` macro); text engine (load, split, regex, case, counts, diff, template) in-process; text viewer from G6 at its minimum (plain text, diff between runs) | Text engine passes the conformance suite in mode 1; status events and live appends shown in the inspector | 5–7 |
| S5 | Socket binding, mode 2 | Brought forward from G7: Unix-socket JSON-RPC 2.0 binding (D4), supervisor (spawn, `hello`, health, restart, `kill -9` recovery), same-host proof by two nonce files (D8), the shm data plane; the text engine as a separate process | Conformance in modes 1 and 2, including `kill -9` recovery; `run.get` polling answers during a run | 6–8 |
| | **Total** | | | **25–32** |

**Risk to watch:** S4's small executor would be a third executor beside Python's and `research_graph`'s, which the review (M-findings) warned drifts. Keep it minimal and replace it with `plumcraft-exec`, lifted from `research_graph`, in G4. The shared graph corpus runs against it from S4 on.

The Python/pipekit engine keeps working through today's bridge during S: the AMQP host (G3) is not in the slice, so its protocol 1 messages pass through the S2 parser unchanged.

### 10.2 Milestone D: the data plane on the text engine (decided 2026-10-27)

After S (modes 1 and 2, the memo), the maintainer chose the data plane next: **local, then remote**, with blobs in **commonkit**, on the text engine in both languages. The engine gains three kinds of data: large text as blobs, a `table` (`text.letters`: named typed columns, letter STRING and count INT, which the maintainer pointed out is the spec's `table` kind rather than an array), a true 1-D array (`text.codes`: code points as `<u4`, `nd1` handles), and a file source (`text.load`).

| | Step | Content | Exit criteria |
|---|---|---|---|
| D1 | Local data plane | **Values:** the SDKs gain typed values (JSON, text, array, table); a value over `limits.max_inline` crosses as `{"kind": "blob", "ref": DataRef}`, never inline. **Store:** commonkit key scheme `plum:v1:{plumarr,plumraw}:<md5>:c`; the Python engine publishes with the `commonkit` package; the Rust engine through a small writer (`plumcraft-store`: `.npy` write, temp file and rename, commonkit's layout) and both read through it; non-array blobs as 1-d `|u1` arrays keyed by their `raw` digest (spec 4.4). **Same-host proof** (spec 5.4) both ways, in the host and both engines; `data.shm` and `data.path` only when proved (fixes the socket engine's unproven claim). **Memo** entries hold handles; a reaped blob is a memo miss. **File source:** `text.load` keyed by content, never path. **UI:** the CLI and viewer resolve DataRefs (proof required for `commonkit`/`path`); Watched shows full text, a table grid, a 1-D plot; a missing blob greys the value and reruns once (F58). The Python engine now runs under `uv run` (it needs numpy and commonkit) | Conformance in all three modes: large text and arrays cross as blobs with identical handles from Rust and Python; a failed proof disables `commonkit`/`path` locators (a socket engine then fails closed for values over the inline limit, spec 16.3); a deleted segment is a miss, not an error; P02 checklist |
| D2 | Remote: HTTP (mode 3) (built 2026-10-27) | HTTP+SSE binding (spec 16.4) in both SDKs (Python stdlib `ThreadingHTTPServer`; Rust on std); blob endpoints (13.1: `has`, `PUT` with hash-while-writing, `GET` with `Range`); the UI's durable CAS (`~/Library/Caches/plumcraft/cas`, the UI's only write target) with leases and pins (13.4); source upload with `blob.has` dedup (13.2); inline thumbs on remote sessions (13.3); bearer token for non-loopback | Conformance in mode 3 with separate store roots (forced remote): `text.load` uploads once, a rerun uploads nothing, watched values download lazily and verify; the same run from one machine to another; P03 checklist |

**D1 as built (2026-10-27):** the engine memo keeps values in RAM and republishes a blob whose segment was reaped (publishing checks for the file first), so "a reaped blob is a memo miss" applies once the memo persists. `text.letters` counts Latin a–z in both engines, because Rust's `is_alphabetic` and Python's `isalpha` disagree on parts of Unicode. The Rust writer applies none of commonkit's policy gates (budget, free-space floor, TTL); the Python engine goes through commonkit and does. Engines started through a wrapper (`uv run`) are supervised as a process group.

**D2 as built (2026-10-27):** `/v1/rpc` carries every operation, the `events.poll` long-poll included, so SSE is later; uploads are verified before they are stored and dedupe by `blob.has`; downloads go to the UI's durable CAS, verified; a UI-spawned HTTP engine gets `--lifeline` (exits when its stdin closes), found when a killed viewer left HTTP engines running; run ids are unique per client, found when a second CLI run against a long-lived engine on a remote server was refused (`run_id already used`). The the development machine → a remote server run used the Python engine (a remote server has no Rust toolchain) on loopback behind an SSH tunnel.

**Not in D:** persistence of the memo across restarts, content keys, `blob.get_many`, derivatives (10.5), resumable uploads, BLAKE3 namespaces. **To propose to commonkit (golden rule 7):** the Rust writer belongs in commonkit eventually; D1 keeps a minimal one here, which writes but never reaps (Python publishers reap the shared tree).

### 10.3 Milestone E: the editor (decided 2026-10-27)

After D, the maintainer chose: **grow the editor, then an audio engine** (F, after E).

| Decision | Choice |
|---|---|
| How | **Lift the research software's generic parts** into PlumCraft and generalise them (plan G4 brought forward): the graph model, the command registry, the `egui-snarl` canvas. The research software is only read; it moves onto the base later |
| Theme | **`craft-ui`**, pinned by git rev as the research software pins it: its tokens and widget palette; node states and engine families mapped onto them. Closes the ROADMAP item "Externalize colours and theme materials" |
| First slice (E1) | **Edit the graph** (palette from the catalogue; add, delete, move, wire with type checks; inline widgets; run and watch) and **commands and control** (every action a command with id, label, shortcut, `enabled`; a control channel that drives the editor and takes screenshots) |
| Later in E | Files (open and save workflow JSON with positions) and undo; groups (collapse and expand, the plugin boundary) |

**E1, from the map of the research software's editor** (2026-10-27; the research software at `8d96021`). Each step builds and tests on its own; the research software is only read.

| | Step | From the research software | Strip | ~LOC |
|---|---|---|---|---|
| E1.1 | `plumcraft-graph` (L1): the model, add, remove, move, set, connect, disconnect, check, workflow JSON; port typing | `crates/graph` `lib.rs`, `ports.rs` | groups, frames, nets, insert; hard-coded type rules (use the catalogue's `accepts`); always-written `groups`/`next_id` | 1,000 |
| E1.2 | `plumcraft-session` (L3): `Session`, the command registry (`CommandSpec`), graph and run commands, run state (staleness, auto-run) over `Connection` and the proto `Reducer`; `panic_hunt` | `crates/engine` `lib.rs`, `commands.rs`, `graph_cmds.rs`, `run.rs`, `run_cmds.rs` | bridge, export, save, groups, expansions, undo, wire routes, curves and masks | 1,400 |
| E1.3 | `plumcraft-ui` (L4): shell (menus and shortcuts from the registry), canvas over `egui-snarl`, selection and move, widgets, palette, run colours, theme from `craft-ui`, icons, fonts; the Watched panel from `plumcraft-view` | `crates/ui-egui` `lib.rs`, `canvas.rs`, `edit_view.rs`, `widgets.rs`, `palette.rs`, `run_view.rs`, `theme.rs`, `icons.rs`, `fonts.rs`, pin mapping from `groups_view.rs` | the VI viewer, groups, nets, frames, routes, slices, bundles, curves, masks, export, rename, prefs | 3,300 |
| E1.4 | `plumcraft-automation` (L4) and `ui.*` control; the `plumcraft-editor` app with `--control` | `crates/automation`, `ui-egui/src/control.rs`, `apps/research` shell | groups and save in `ui.inspect` | 700 |
| E1.5 | Gates: `xtask layers`; kittests (shell, widgets, palette); a control-driven end-to-end test; P04 checklist | `xtask/src/layers.rs`, `tests/kittest_*` | VI fixtures (a text-engine catalogue instead) | 600 |

**E2: files and undo** (started 2026-10-27, after F). Lift from the research software `crates/engine`: the undo and redo snapshots in `Session::edit` (labels, a 200-step limit), `edit_cmds.rs` (undo, redo, copy, cut, paste with fresh ids; not align, distribute or `wire.slice`), `file_cmds.rs` (`graph.open`, `graph.save`, `graph.saveAs`, the workflow path, dirty tracking, a confirm before discarding unsaved work), `paths.rs` (relative path inputs resolve against the workflow's folder; saved relative when inside it); the UI's file dialogs and the window title with the file name and an unsaved mark. Exit: open, edit, undo, redo, save, reopen round-trips positions and values; a kittest per command; `panic_hunt` covers the new commands; P04 gains items.

**E3: groups** (started 2026-10-27, after E2). Lift the research software's generic group overlay: `crates/graph` `groups.rs` (membership, nesting, promoted ports with labels and ranges, group outputs; not checkpoints), `crates/engine` `group_cmds.rs` (create, ungroup, rename, promote, unpromote, add output, collapse and expand, move), `crates/ui-egui` `groups_view.rs` (collapsed group nodes with proxies for promoted ports and outputs, wires through them, enter and leave a group), and `insert` (insert a workflow file as a group). Groups stay an editing overlay saved in the workflow file; engines run the flat graph and ignore them (spec 7.6 rule 8). **Left in the research software's VI pack** (plan section 2): checkpoints, codegen gating, plugin export. **Deferred decision touched:** D10 recommends opt-in memo persistence at group boundaries; not needed for E3, left deferred. Exit: create, collapse, expand, ungroup, promote and wire through a collapsed group in the editor; groups round-trip through save and open; a run gives the same results grouped and ungrouped; kittests and `panic_hunt` cover the commands; P04 gains items.

**Risks (from the map):** groups run through the canvas core (pin mapping, overlays); the run plumbing is a rewrite on `Connection`; type rules move from hard-coded to the catalogue; proto field drift cascades (`editor`, `param_ref`, `flags.result`); egui-snarl keeps its own selection by index (the research software's `Picked` workaround comes along). `plumcraft-view` stays until the editor covers P01–P03, then retires.

### 10.4 Milestone F: the audio engine (planned 2026-10-27)

The maintainer: "Plan the audio engine, go." This is G9 of the original plan, on the base as it now stands (protocol 2, the data plane, the memo, the editor). Audio is the first engine whose values aren't text: it proves the base's visualisation beyond images (waveform, spectrogram, playback).

| Decision | Choice (AI's, stated for review) |
|---|---|
| DSP and I/O | **SoundCraft's `soundcraft-dsp` and `soundcraft-audio-io`**, pinned by git rev (`github.com/storytold/soundcraft`, MIT OR Apache-2.0, fetchable over https as craft-libs is). Pure Rust: offline ops on channel buffers, a plugin registry with parameter metadata, symphonia decoding, WAV encoding, peaks |
| Engine | `crates/audio` (`plumcraft-audio`), Rust on the SDK, in every mode the SDK already serves: 1 (in-process), 2 (`plumcraft-audio-engine`, crash isolation for decoders, as the plan said), 3 (HTTP). No Python audio engine |
| Values | A new `Data::Audio` in the SDK: (frames, channels) `<f4` with its sample rate. It crosses as a blob (`md5:nd1:` over the samples, `Form::Audio`); its display is media `audio` with an inline `peaks` rendition (min/max per bin per channel, ≤ 4 KiB) and the full value by reference; its node thumbnail is the same peaks, so the canvas shows waveforms |
| Viewer | In the editor's Watched panel: waveform (from peaks, then from the full value), spectrogram (rustfft in the UI over the full value), playback with cpal (play, stop, loop) |

| | Step | Content | Exit |
|---|---|---|---|
| F1 | Audio values | `Data::Audio` and `Plane::audio_ref`; peaks renditions for displays and thumbs; `WireType` `AUDIO` (media `audio`); conformance helpers | A test engine node returning audio crosses inline and by blob with the right handle and `Form::Audio` in all three modes |
| F2 | `plumcraft-audio` | About 25 nodes: load (any format symphonia reads; keyed by content), save (WAV; an output node), tone and noise generators, gain, normalize, fade, reverse, trim, mix, resample, time stretch, pitch shift, biquad filter, the SoundCraft effect plugins as nodes generated from its registry (parameters become widgets), peak and loudness (FLOAT), spectrum (ARRAY), transients and silence (TABLE). Long ops report progress and honour cancel | Conformance for the audio engine in modes 1, 2, 3 (the shared cases plus audio cases: handles stable, a cached rerun, cancel mid-stretch); deterministic output (the same graph gives the same handles twice) |
| F3 | Audio in the editor | Waveform thumbs on nodes; the Watched panel's audio view (waveform, spectrogram, playback via cpal); the editor chooses its engine (`--engine text|audio`) | P05 checklist with the maintainer: build a chain, hear it, see it |

**Not in F:** MIDI, plugin hosting (CLAP/VST3/AU), streaming `node.live` appends, a SoundCraft MCP pack, A/B and null-test compare (they come with Freeze pins).

**SoundCraft over MCP is still open (D15, deferred).** The maintainer asked, 2026-10-27, "What happened to accessing SoundCraft as an MCP server?" The AI had planned F on the crates (the earlier plan's G9 choice) without raising D15 again. The maintainer: "Continue on this plan for now." The generic MCP craft adapter (G10; VectorCraft or SoundCraft as its first pack) stays a candidate for after F.

### 10.5 Milestone Q: streaming in queued chunks (accepted 2026-10-27)

The maintainer asked to "plan processing data in queued chunks, to deal with continuously streaming data", then had the plan reviewed adversarially twice and revised twice (`docs/research/09`–`13`; the accepted text is revision 3, `13-streaming-r3.md`, and its decisions D25–D39). The maintainer: "Add Q." Not yet scheduled against the other milestones.

**The model.** A wire type may declare that it streams, as one of two kinds:

- **sample streams** (audio frames, characters), whose chunks are slices the engine concatenates and splits at declared boundary units;
- **item streams** (frames, lines, files), whose chunks are whole values. They follow spec 7.6's list rules over time.

**Nodes.** There are three contracts: `process` (state, resumable output, flush, reset, live parameters), `source` (pull, or push through a device ring) and `whole` (today's nodes, behind an implicit accumulate).

**Order.** Data, `gap`, `flush` (seek), `format`, `barrier` and `eos` travel in-band. Live parameters go on each node's control channel, stamped with an offset and applied in offset order, so every data edge keeps one writer. Edges carry frontiers. A `time` join on one clock reads only its lagging input and ends when all inputs end. A `latest` join is marked nondeterministic.

**Queues and executors.**

- Edges are bounded crossbeam queues with credit: `block`, `drop_oldest` or `conflate`.
- Deadlock is found on the wait-for graph and resolved by doubling every full edge in the cycle up to a cap, then failing by name.
- Four memory caps: queues, join buffers, accumulate, retention.
- Text and files run a thread per node. Audio runs a fused block render that reproduces the batch loop call for call.

**Protocol.** A stream run is a `run` with `stream`, plus `stream.control`, `stream.set`, `stream.push` and `stream.stats`. Stats carry each node's state header, so state heals without a reply. Taps are lossy and outside the event ring. HTTP push returns credit in long-polled responses. A stream panic fails the batch run in flight and the worker rebuilds; the shared memo refuses stale inserts.

| Decision | Choice (AI's, for review; note 13 §4) |
|---|---|
| Model | Two stream kinds; three contracts; in-band items; frontiers; push sources lossy at their ring (D25) |
| Start | `run` with `stream`; `stream.inputs` (D26) |
| Queues | `block` 8 between nodes; `drop_oldest` at rings and taps; `conflate` for `latest` secondaries and meters; an `EdgeQueue` type (D27) |
| Executors | Threaded for text and files; cooperative fused render for audio and non-`Send` cores; resumable calls (D28) |
| Tap bytes | Typed in mode 1; inline ≤ 32 KiB per append and ≤ 4 MiB/s per session on the socket; npy endpoints on HTTP (D29, D38) |
| Distribution | An engine; dora-rs and pipekit with credit, both tested for lossless delivery in Q6 (D30) |
| Memo | Graph-property eligibility; accumulated identity; shared and incarnation-guarded (D31) |
| Live parameters | Control channel, offset-ordered, `applied_at` recorded (D33) |
| Joins | Frontier `time` join plus marked `latest` (D35) |
| Status | Stats carry the state header; previews repaired by watch or get (D36) |
| Deadlock | Wait-for detection, growth of every full edge, a named error at the cap (D37) |
| HTTP push | Credit in the response, long-polled; WebSocket only if measured necessary (D39) |

| | Step | Content | Exit criteria | Days |
|---|---|---|---|---|
| Q0 | Spec and types | Stream sections in `docs/interface.md`; proto types incl. `EdgeQueue`; corpus with D1–D5; hostile sweep | Schema, fixtures, sweep green; spec reviewed | 4–5 |
| Q1a | Executor, Rust text | Threaded executor on credit; in-band items; frontiers; joins; detector; control-channel params; text nodes | Stream ≡ batch at four chunk sizes; split-boundary; D1–D5; seek under load; single writer; resumable flush; cancel | 9–11 |
| Q1b | Host, modes 1 and 2 | Stream slot; shared memo; panic path; stats; taps; timeouts and idle cancel | Panic with a batch in flight; memo eligibility; lost record and preview healed; `kill -9`; tap numbers | 8–10 |
| Q2 | Editor | Stream typing, badges, transport, live views, session state, commands, P06 | P06 with the maintainer; kittests and control green | 6–8 |
| Q3 | Python and HTTP | Python stream SDK; HTTP tap and push (long-poll credit) | Both engines in modes 2 and 3; corpus parity incl. surrogates; lossless remote push; remote tap resumes | 6–8 |
| Q4a | Audio, offline | Islands; `fixed_block: 1024`; extended multi-size test; load and save | Bit-exact chain; reverb tail through the join; 2-hour file bounded | 8–10 |
| Q4b | Audio, live | Mic; Listen; meters; P07 | Monitoring under 150 ms; four inline taps within caps; P07 | 7–9 |
| Q5 | Cooperative parity | Whole graphs cooperative; a non-`Send` test engine | Corpus identical on both executors | 3–4 |
| Q6 | Distribution evaluation | dora-rs and pipekit with credit; barrier prototype | Lossless test on both; D30 recommendation; barrier restore exact | 5–7 |
| | **Total** | | | **56–72** |

**Not in Q:** the distribution build (10–14 days, after D30, gated on G3 and D22), a pool scheduler, per-chunk memo, `zip` and `merge`, hard real-time, video, MIDI, a shared-memory ring (a separate `unsafe` decision, only if inline taps fail), WebSocket (D5, only if D39's measurement fails).

**Source changes proposed (golden rule 6):**

- **SoundCraft:** packet decode; a public block render with tail; `InputRing` outside `Player`; frame-major noise in `dither`.
- **pipekit:** credit queues and barriers, only if D30 chooses it; `CONCURRENCY` pinned for stateful nodes.
- **commonkit:** none.
- **This repository:** correct plan 2.1's `plumcraft-shm` row.

### 10.6 Milestone M: the craft MCP adapter, VectorCraft first (decided 2026-10-27)

The maintainer chose D15 next, then **VectorCraft first** and **no craft-libs changes now**. This is the original plan's G10 on the base as it stands (protocol 2, the data plane, the memo, the editor, groups). Research note 05 surveyed the crafts (one command model across fourteen apps; commands mutate one open document and act on a selection; MCP has no node states, no typed parameter schema and no blob model, so the adapter supplies them).

| | Step | Content | Exit |
|---|---|---|---|
| M1 | Map and design | VectorCraft's MCP server and CLI as built today (commands, params, documents, selection, progress, cancel, export); the adapter's design: an engine (spec 16.5) that owns the graph walk, threads a document handle (a CAS key) through the wires, makes selection an explicit parameter, emits every node state itself, maps progress and cancel, pools craft processes; which commands become nodes and how their ports are inferred without `params_schema` | `docs/research/14-craft-mcp-adapter.md`, reviewed by the maintainer |
| M2 | `plumcraft-craft` adapter | Generic over any craft (the shared `CommandSpec` model): an MCP client, a process pool, a node per pure-able command, document and export wire types (`doc` DataRefs, SVG and raster outputs), status, progress, cancel; serves modes 1–3 like any engine | Conformance for a VectorCraft graph in mode 2 (states, cancel, a cached rerun, handles stable); a raster → Trace → Recolor → SVG graph runs |
| M3 | In the editor | VectorCraft nodes in the palette (`--engine vectorcraft` and an Engine menu choice), SVG and image previews in the Watched panel (an SVG view), a thumbnail on document nodes, and **Open in VectorCraft** on any document value: the editor opens the node's saved native file (already in the adapter CAS, named by its handle) in the desktop app with `vectorcraft <file>`, read-only by convention, so runs stay headless and reproducible (maintainer, 2026-10-27, option 1); the P08 checklist | The maintainer builds a trace-and-recolor graph by hand and opens an intermediate document in VectorCraft |
| M4 | Live mode (planned, below) | An opt-in hosting in which the adapter drives a VectorCraft window it started itself, so each node is seen running | P09: the maintainer watches a trace-and-recolor run in the window, cancels one, and the headless engine is unaffected |

**Not in M:** craft-libs source changes (deferred), SoundCraft as a second pack (after M), streaming through MCP (MCP can't stream; note 13).

**M1 done (2026-10-27):** `docs/research/14-craft-mcp-adapter.md` maps VectorCraft's MCP server as built (32 tools, 811 commands, three typed catalogues, byte-stable saves) and designs the adapter. The maintainer, after seeing the corgi traces: "Looks pretty good. Continue" — **D40–D50 adopted as recommended** (note 14 section 5.3):

| Decision | Topic | Adopted |
|---|---|---|
| D40 | MCP client | **Hand-rolled newline JSON-RPC over the child's stdio on std threads**, sharing `plumcraft-engine::rpc` framing; not rmcp (tokio, nothing gained on stdio) |
| D41 | Cancel | **Send `notifications/cancelled`, then SIGKILL after 200 ms and respawn** (under 50 ms); never wait on cooperative cancel, since trace ignores it and cancelled calls never reply |
| D42 | Materialising documents | **v1 saves and hashes every node's document; warm processes skip reopening; lazy (unsaved) intermediates only after an M2 measurement on the 218 MB case**, with a `Cx::demand()` SDK addition then. Touches D10/D11 (deferred): doc records are not persisted by default |
| D43 | Document identity | **`md5:raw:` of the uncompressed native save**, stored with its real extension in an adapter CAS that is the craft's read root; never craft-side gzip (unstable); PDF never content-keyed; `file.new` always with `created: null` |
| D44 | Selection | **A `select` STRING parameter on every art node** (`all`, `none`, `ids:`, `color:`, `kind:`, `json:`) plus an `ids` JSON socket, compiled to `select.*` before the command; no selection-producing nodes in v1 |
| D45 | Which commands are nodes | **24 curated nodes (section 2.3), 116 generated effect nodes from `effect.list`, and a `vc.run` escape hatch over a manifest allowlist**; not all ~190 inferred transforms |
| D46 | A craft crash, timeout or overlong reply | **Fail that node with kind `engine` and let independent branches continue**; amend spec 12.1, which calls `engine` run-level only |
| D47 | Memo salt and scope | **Salt `craft:<id>:<serverInfo.version>:<md5 of the binary>`** (no build hash is exposed); scope `machine` for manifest-pure nodes, `none` otherwise; never `global` (fonts, host numerics). Applies D9's typed canonical form to the params unchanged |
| D48 | Wire types | **Pack-namespaced `vectorcraft.doc`**; shared `IMAGE`, `SVG`, `PDF`, `JSON` so later multi-engine graphs (D14) can join on them |
| D49 | Modes and process | **Mode 2 by default** (`plumcraft-craft-engine --craft vectorcraft.toml`), mode 1 for tests, mode 3 later; one craft process per run in v1 (the executor is serial), pool of up to 4 for concurrent runs and sessions |
| D50 | Source proposals (golden rule 6; none made now) | **Record, don't send:** VectorCraft could expose `journal` in `CommandInfo`, put a build id in `serverInfo`, make trace and pathops cancellable with progress, answer a cancelled request with an error instead of silence, and default `file.new` `created` to null under automation roots; craft-libs `params_schema` and `effect` stay deferred (D15) |

M2 (the adapter and the VectorCraft pack, about 15 days) is next.

**M2 done (2026-10-27, `3ad257f`; P07).** See STATUS. M3 is in progress.

#### M4, live mode: a craft engine with its window (planned 2026-10-27, not decided)

The maintainer asked whether the craft engines can show their GUI ("Is there anyway to get GUI on the craft MCP engines yet?"), chose "Open in VectorCraft" for M3, and asked for this mode to be planned. VectorCraft already supports it: `vectorcraft --control PORT --automation-read-root DIR --automation-write-root DIR` starts the app with a control port and its own roots, and `vectorcraft-cli mcp --connect 127.0.0.1:PORT` serves MCP against it (the CLI refuses roots together with `--connect`; the app enforces its own). **This revisits note 14's R8** ("the adapter always passes roots, which imply headless"), so it is opt-in and never touches an app the maintainer opened.

| Step | Content | Exit | Days |
|---|---|---|---|
| M4.1 | Measure: run the M2 conformance chain against a `--control` app; compare every reply and the SVG with headless; time a node; check cancel (`notifications/cancelled`), what the window shows per command, and which stores the app attaches under roots | A table in note 14: equal or which keys differ; a decision on D52 | 1 |
| M4.2 | Hosting: `plumcraft-craft-engine --craft vectorcraft --live`; the adapter launches its own app on a free port with the adapter's CAS and work roots, waits for the port, and connects one `vectorcraft-cli mcp --connect` to it; the pool is one process; the watchdog covers the app too; an app the maintainer quits is a node `engine` error, and Reconnect relaunches it | Conformance's state, cancel and crash cases pass live | 2 |
| M4.3 | Editor: Engine menu "VectorCraft · live (window)"; the status bar and title say live; results are marked live in the Watched panel | The maintainer switches between headless and live with the graph kept | 1 |
| M4.4 | P09 | Accepted | 0.5 |

About 4.5 days, after M3.

| Decision | Topic | Recommendation |
|---|---|---|
| D51 | Whose app | **Always an instance the adapter starts** with its own roots and a free port; attaching to an app already open (the default `mcp` behaviour at 7979) only through Engine › Attach…, with a warning that it edits that app's documents |
| D52 | Memo in live mode | **Results are computed, hashed and shown, but neither read from nor written to the memo** (salt `live`), unless M4.1 shows replies and saves equal to headless; then live may read the memo but still never writes it |
| D53 | Cancel in live mode | **`notifications/cancelled`, then wait up to 5 s, then ask before killing the app** (a kill closes the window the maintainer is watching); headless keeps D41 |
| D54 | Documents in the window | **One window tab per run, showing each node's document as it is produced, left open at the end**; edits made in the window are not read back (one-way in v1; pulling a window document back as a node value is a later step) |
| D55 | Scope | **VectorCraft only in M4**; the hosting is generic over a manifest field `gui = "vectorcraft"`, so another craft adds a line |

**Not in M4:** reading edits back from the window, several live windows at once, live mode over HTTP (mode 3).

**Source changes proposed (golden rule 7),** each to its own repository:

| Repo | Change | Needed by |
|---|---|---|
| commonkit | A `keep` list or `pinned/` subtree the reaper skips | G5 (until then pins copy to the durable tier) |
| commonkit | Optional: a Rust writer crate following commonkit's layout, index, TTL and reaper rules (2–3 days) | Only if UI-side publishing into commonkit is ever wanted; v1 writes the durable tier instead |
| The research software | Stage hooks in the research software's commands so an executor can emit stage snapshots; per-item progress callback | G6 |
| commonkit | A raw segment kind (`.bin` plus header sidecar) | G6 text, G9 audio exports, G10 documents (until then 1-d `u1` arrays) |
| commonkit | A BLAKE3 key helper (128-bit truncation in the digest slot) | Only if D3's measurement favours BLAKE3 for large arrays |
| The research software | `checkpoint_store` digests on the typed canonical form instead of `repr` | G5, so the research software's native engine and Python share checkpoints |
| The research software's native engine | Lift the generic half of `research_graph`'s executor into `plumcraft-exec`; `nd1` byte-order normalisation; `Cx::snapshot` | G4 (lift), G5 (normalisation), G6 (snapshot) |
| pipekit | None required for G3 (the `Session` consumer is a second node in `research.yml`) | — |
| pipekit | Stream queues only if full AMQP replay is ever wanted | later |
| craft-libs and crafts | `params_schema` (JSON Schema) and `effect` on `CommandSpec` | G10 |
| soundcraft | Pure `dsp.process` and `.npy` I/O | SoundCraft MCP pack (later) |
| soundcraft | Progress and cancel callbacks for `time_stretch`, `pitch_shift`, `resample`, `detect_transients`, `signal_generator`, `decode`; a public block render with a progress hook (PlumCraft copies `apply_plugin_with_tail`'s private render); `decode` with a caller's sample limit (a headerless stream can allocate up to its 4 GiB cap first); a seed for the generator's noise; a bound on `pitch_shift`'s 16× intermediate | F2 (found 2026-10-27; worked around in `crates/audio`, worker threads and guards) |

## 11. Decisions for the maintainer

| | Question | Status and recommendation |
|---|---|---|
| D1 | Product name | **Decided (the maintainer, 2026-10-27): PlumCraft.** Reserve crates.io `plumcraft`, PyPI `plumcraft` (or the py-prefixed form) and `plumcraft.dev` now; the GitHub user name is taken, so use an existing owner |
| D2 | Versioning | **Decided 2026-10-27: as recommended.** **Protocol major 2 plus capability strings**, never minor versions; today's messages are protocol 1 and keep working through the AMQP host |
| D3 | CAS digest | **Decided 2026-10-27: as recommended.** **Algorithm- and recipe-tagged handles; MD5 by default** (`nd1` = today's digest, `raw` = alexandria's); BLAKE3 required for uploads into multi-client stores. Measure hashing on the development machine before considering BLAKE3 for large local arrays |
| D4 | Canonical encoding and envelope | **Decided 2026-10-27: as recommended.** **JSON with tagged unions; JSON-RPC 2.0** framing for the socket binding and HTTP `/v1/rpc`; msgpack reserved, not built |
| D5 | Bindings and order | *Deferred to the re-plan after S; this recommendation is the default.* **In-process, AMQP, socket, HTTP+SSE**; gRPC and WebSocket deferred; Cap'n Proto, Arrow Flight, NATS, ZeroMQ, tarpc, jsonrpsee rejected |
| D6 | Remote event stream | *Deferred to the re-plan after S; this recommendation is the default.* **SSE** with `Last-Event-ID` resume and a polling fallback, rather than WebSocket |
| D7 | Remote security for v1 | *Deferred to the re-plan after S; this recommendation is the default.* **Bearer token plus an SSH tunnel** (or a TLS reverse proxy); built-in rustls later |
| D8 | Same-host detection | **Decided 2026-10-27: as recommended.** **Two nonce files, one each way**, never hostnames |
| D9 | Memo keys | *Deferred to the re-plan after S; this recommendation is the default.* **Typed canonical form** (a custom form, not RFC 8785): params coerced to port types, `{"i": "<decimal>"}` for integers, `{"f": "0x<IEEE-754 bits>"}` for floats, tuples as lists, numpy scalars as Python values; shared test vectors; SHA-1 kept in Python until they pass. Plan key always; content key only for published outputs or `memo.content` nodes. MD5 for single-client engines, `b3` when an engine accepts several client tokens |
| D10 | Memo persistence and scope | *Deferred to the re-plan after S; this recommendation is the default.* **Opt-in persistence** for checkpoint-marked and group-boundary nodes (durable tier, 20 GB), background writes; salt includes the dependency digest; gates run with the memo off; scope `machine` by default; no cross-machine sharing of VI results until attested (host numerics differ between the development machine and a remote server) |
| D11 | Leases and budgets | *Deferred to the re-plan after S; this recommendation is the default.* Lease **10 min**, durable tier **20 GB**, commonkit unchanged (2 GB on macOS, 15-min TTL) |
| D12 | Remote store on a remote server | *Deferred to the re-plan after S; this recommendation is the default.* **Its own root in alexandria's layout**, reading alexandria's rclone CAS for sources by alias (read-only), so alexandria's cleanup stays simple |
| D13 | Upload policy | *Deferred to the re-plan after S; this recommendation is the default.* **Hash first, then ask `has`** (one extra local read; no wasted bandwidth) |
| D14 | Engines per workflow | **Decided 2026-10-27: as recommended.** **One engine per workflow in v1**, with the multi-engine shape reserved now: `RunRequest.inputs` external bindings, an optional per-node `engine` field, `Session` holding a map of engine clients. Several engines in one graph later, joined by CAS handles |
| D15 | MCP engine | **Decided 2026-10-27 by the maintainer: VectorCraft first** through a generic craft adapter; SoundCraft second. **No craft-libs source changes now** ("Not now"): the adapter infers parameters and purity from what the crafts expose (`params` prose, command metadata, `describe`/`info` CLIs), and the `params_schema` and `effect` proposals wait |
| D16 | OpenCV binding | *Deferred to the re-plan after S; this recommendation is the default.* **Python `opencv-python-headless`**, not the Rust `opencv` crate (system OpenCV, libclang, slow and fragile builds) |
| D17 | Repository strategy and license | **Decided 2026-10-27: its own repository, `plumcraft`;** **licence MIT or Apache-2.0, at the user's choice** (first MIT; the maintainer chose the dual licence on 2026-11-01: `LICENSE-MIT`, `LICENSE-APACHE`, every crate and the Python package). (First decided as "extract inside the research software"; the maintainer moved it to its own repository the same day, once `plumcraft-proto` existed.) the research software will depend on it by git rev, as the crafts pin `craft-libs`. Was: **Extract in this repository first** (`core/` and `packs/vi/`), then a public repo with fresh history; **MIT/Apache dual** as the crafts |
| D18 | Mid-run watch on pipekit | *Deferred to the re-plan after S; this recommendation is the default.* **Over the `{system}.Session` consumer** (D22), which updates the run table the executor reads; applied at the next run when unsupported |
| D19 | `mem` locators in-process | **Decided 2026-10-27: as recommended.** **Defined now, built only when a profile shows the commonkit write matters** |
| D20 | Craft MCP configuration | *Deferred (the maintainer, 2026-10-27): leave the config until the adapter work starts.* **Fix the paths** from `/Volumes/src/artcraft/…` to `/Users/me/src/artcraft/…` and build SolidCraft, so the adapter can be prototyped live |
| D21 | Control channel compatibility | **Decided 2026-10-27: as recommended.** **Keep a v1 projection** for `ui.inspect` and every control reply; new fields only under `v2`; rather than versioning the control protocol and migrating journals |
| D22 | Off-run-path service on pipekit | *Deferred to the re-plan after S; this recommendation is the default.* **Host emulation of `run.get`/`events.poll` plus a `{system}.Session` consumer in `Plan`** (second thread, lock-protected run table), rather than reply-to over the control fanout |
| D23 | Multi-client sessions | **Decided 2026-10-27: as recommended.** **A session owns its runs**; events scoped per session; cancel and watch by the owner or an `admin` token; idle runs cancelled after 30 s unless `detach: true` |
| D24 | Estimate | **Decided 2026-10-27: thin slice first** (milestone S, section 10.1, 25–32 days), then re-plan. Was: **Budget 65–80 days to "nothing lost" and 130–160 in total**; accept or cut scope (G9–G10 are the separable tail) |

## 12. Review dispositions

Review `docs/research/08-review.md` raised 30 findings. All high and medium findings are accepted and fixed in this plan and the spec. The table records where a fix chose among the review's options, narrowed it, or declined part of it.

| Finding | Disposition | Reason |
|---|---|---|
| H1 | Accepted; chose the `{system}.Session` consumer over control-fanout reply-to | A queue with its own consumer gives request/reply semantics and correlation for free; the fanout is a broadcast and would make every Plan replica answer |
| H2 | Accepted in full | False cache hits break golden rule 5 |
| H3 | Accepted in full | |
| H4 | Accepted, except device or driver identity goes in the salt only for nodes declaring `memo.device_sensitive` (via `device_class`) plus a Metal/MLX driver digest for GPU engines | Putting the device identity in every key would split the memo by machine for nodes that are bit-exact everywhere; `machine` scope already isolates the rest |
| H5 | Accepted in full | |
| H6 | Accepted; factory is `FnMut` rather than `FnOnce` | The host must call it again to rebuild the core after a caught panic |
| H7 | Accepted; N set to 30 s (in-process, socket) and 60 s (HTTP, AMQP) | The review left N open |
| H8 | Accepted in full | |
| M1 | Accepted; chose the 64 KiB `max_display_inline` budget over a `stats` blob | Keeps the display one message; a `stats` blob would add a fetch on the path meant to be faster |
| M2 | Accepted in full | |
| M3 | Accepted in full | |
| M4 | Accepted in full | |
| M5 | Accepted; `blob.get_many` and HTTP/2 multiplexing both allowed | |
| M6 | Accepted; chose "the UI writes only its durable tier"; the Rust commonkit writer is listed as optional at source | Avoids duplicating commonkit's layout, index and reaper rules in Rust |
| M7 | Accepted in full | |
| M8 | Accepted in full | |
| M9 | Accepted; chose the v1 projection over versioning the control protocol | Journals are regression tests (golden rule 8); keeping them byte-compatible is cheaper than migrating |
| M10 | Accepted; adopted the review's figures | |
| M11 | Accepted in full (`plumcraft-exec`) | |
| M12 | Accepted in full | |
| L1, L2, L3, L5, L6, L8, L9, L10 | Accepted in full | |
| L4 | Accepted | |
| L7 | **Not built now, not dropped (the maintainer, 2026-10-27).** First recorded as "Accepted: Windows named pipes dropped", on the AI's own assumption that only the development machine and a remote server matter; the maintainer: "Who told you to drop Windows named pipes? We have a separate machine doing dev work on Windows", then "Just don't delete support from shared infrastructure like commonkit". PlumCraft builds no Windows binding now, and nothing shared (commonkit) loses its Windows support | Platform scope is the maintainer's decision |
| "What held up" | No change | The interface shape stands |

No finding was rejected outright.
