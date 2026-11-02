# 06: Engine hosting modes and the example engines

*2026-10-27. Research note on branch `generic-base-research`; no code changed. It builds on `docs/research/flocraft_effort_20261017.md` (the 30–40 day estimate, the transport-trait recommendation, the Pillow example engine) and doesn't redo it. Base name below: `<base>` (the product name is settled in another note).*

## Summary

- **One facade, four hosts.** The UI keeps one handle on an engine: today's `EngineClient` (`crates/bridge/src/client.rs`). It is a channel pair carrying `EngineRequest` in and `EngineEvent` out, plus a `wake` callback. `RunState` (`crates/engine/src/run.rs`) stays sans-IO: it fills an outbox of `Outgoing::{Run, Cancel}` and is fed status records and replies. Each hosting mode is a different thread behind the same channels:
  - **(1) in-process:** a worker thread, as `research-local` does today;
  - **(2) separate-process Rust:** a supervised child over a local socket;
  - **(3) HTTP:** JSON-RPC over HTTP POST, with events over SSE or by polling;
  - **(4) pipekit/AMQP:** the existing bridge, unchanged on the wire.
- **One message set** (hello, schema, run, cancel, status, reply, op, blob) is framed four ways. Status records and run replies have the same shape in every mode, so `StatusReducer` and `RunState` don't change.
- **The data plane is chosen at handshake.** If the engine shares the host and the commonkit store, data travels as **shm**: mmap'd `.npy` and raw blobs, by `path`/`key`. Otherwise it travels as **CAS**: MD5 handles, with `has`/`put`/`get` for upload of sources and download of results. Every data reference carries its MD5 in both cases, so checkpoint reuse works the same way locally and remotely.
- **SDKs:**
  - **Rust:** an `<base>-engine-sdk` crate with a `#[node]` attribute macro. It provides the executor, memo, cancel, publish and display helpers, and one `serve()` that can host an engine in any of modes 1–3.
  - **Python:** `<base>-engine`, split out of `research_engine`. It has `schema.py`, `executor.py`, `nodes/io.py`, `publish.py` and `display.py`, plus a transport-neutral handler with **pipekit**, **socket** and **HTTP** front ends.
- **The example engines cover every mode, so they double as conformance fixtures:**
  - **text** (Rust): mode 1 and mode 2 from one crate. It is the reference SDK demo.
  - **audio** (Rust): mode 2, reusing SoundCraft's `dsp` and `audio-io` crates.
  - **OpenCV** (Python, `opencv-python-headless`): mode 3 locally and remotely, and mode 4 from the same handlers.
  - **VectorCraft MCP adapter** (Rust): mode 2, wrapping `vectorcraft-cli mcp --headless`.
  - **The research software supporting www.virtimp.com** keeps mode 1 (`research-local`) and mode 4 (the Python VI engine).

## 1. What the research software does today (the floor)

| Concern | Today | Where |
|---|---|---|
| UI handle | `EngineClient { requests: tokio UnboundedSender<EngineRequest>, events: std mpsc Receiver<EngineEvent> }`; `try_recv` each frame; `wake` = egui `request_repaint` | `bridge/src/client.rs:349-391` |
| Requests | `FetchSchema`, `Run(Value)`, `Cancel(run_id)`, `Export(Value)`, `Save(Value)` | `client.rs:133-145` |
| Events | `State(EngineStatus)`, `SchemaLoaded`, `RunReply`, `Status(record)`, `ExportReply`, `SaveReply`, `Error` | `client.rs:149-163` |
| Engine states | `Off`, `Connecting`, `BrokerUnreachable{retry_s}`, `EngineDown{queues}`, `LoadingSchema`, `Unavailable{reason}`, `Connected{version,nodes}`, each with a status-bar label and a banner with the fix | `client.rs:69-129` |
| In-process engine | `research_local::start(wake)` makes `EngineClient::detached()` and runs two threads: a dispatcher that reads requests and sets a run's `AtomicBool` cancel flag at once, and a single worker that owns the GPU device, the memo and the research software's commands. Status records are built with `research_schema::run::StatusRecord` (`seq`, `ts`, `generation`) and `preview` records carry thumbs as soon as a node finishes | `crates/local/src/engine.rs` |
| Sans-IO run state | `RunState::request()` → `Outgoing::Run`; `apply_status`, `apply_reply`; generations, supersede-cancel, debounced auto-run, stale reasons; the clock is a parameter | `engine/src/run.rs` |
| Status reducer | Per `(run_id,node_id)`; ignores older generations; orders by `(ts,seq)`; never moves a node back from terminal; the final reply reconciles | `bridge/src/status.rs` |
| Python engine | pipekit `Schema` and `Plan` nodes; `Plan.process_item` dispatches `op` = run/export/deliver/save; `Executor.run(on_status, on_result, cancelled)`; cancel via pipekit control broadcast `{system}.control`; arrays via commonkit `impc:v1:plumarr:<md5>:c`, with a path fallback | `python/src/research_engine/service.py`, `executor.py`, `publish.py` |
| Engine choice | `--engine local\|pipekit`; a broker or system implies pipekit | `apps/research/src/main.rs:31-101` |

**Must be preserved in every mode:**

- the `queued → started → progress* → preview → done|cached|error|skipped|cancelled` record stream, with `seq` and `generation`;
- previews pushed as soon as a node finishes (data in flight);
- cancel that reaches a *running* graph between nodes and list items;
- latest-wins supersede;
- errors as data per node;
- the export and save ops (or a capability flag that hides them, as `plugin_export: false` already does);
- the engine-down banner with a fix, while editing still works.

## 2. The common shape

### 2.1 The message set

These are the message names that the hosting modes frame. The interface note owns the field-level schema; the shapes below are today's protocol plus the additions this note needs.

| Message | Direction | Payload (summary) | Notes |
|---|---|---|---|
| `hello` | UI→E, reply | UI: `{protocol:[1,2], client, data:{shm:{host_id, store}, cas:true}}`; engine: `{protocol, engine:{id,version,components}, capabilities:{ops:[export,save,…], cancel, progress, partial_preview, multi_run, max_inline}, data:{shm?:{host_id, store}, cas?:{put,get,has,max_blob}}, viewers:[…]}` | Picks the data plane and the protocol version. New in modes 2 and 3; mode 4 synthesises it from `Schema` (`engine.*`) |
| `schema` | UI→E, reply | `SchemaReply` (`wire_types`, `nodes`, `expansions`, `engine`) | Unchanged |
| `run` | UI→E, ack + final reply | `RunRequest` (`run_id`, `generation`, `graph`, `targets`, `inspect`, `preview`) | Modes 2 and 3 ack at once (`{accepted:true}`); the `RunReply` follows as an event |
| `status` | E→UI event | `StatusRecord` (`run_id`, `generation`, `node_id`, `seq`, `ts`, `state`, `fraction?`, `thumb?`, `display?`, `error?`, `ms?`) | `display` (an inspect entry streamed as soon as its node is done) is new and optional; see 2.3 |
| `reply` | E→UI event | `RunReply` | Unchanged |
| `cancel` | UI→E | `{run_id}` | Out of band of the run in every mode |
| `op` | UI→E, reply | `{op:"export"\|"save"\|"deliver"\|<engine-defined>, …}` | Generalises `Export` and `Save`. Gated by `capabilities.ops` |
| `blob.has` / `blob.put` / `blob.get` | UI↔E | `{md5s:[…]}` → `{missing:[…]}`; bytes by md5 | CAS plane only (modes 3 and 4 when remote; 2 never needs it) |
| `ping` / `shutdown` | UI→E | — | Health checks and orderly stop |
| `log` | E→UI event | `{level, logger, message}` | Optional; feeds an engine log panel (MCP's `notifications/message` shape) |

**Recommended envelope: JSON-RPC 2.0.**

- Requests are `{"jsonrpc":"2.0","id",method,params}`; events are notifications (`run/status`, `run/reply`, `log`); cancel is a notification (`run/cancel`), as MCP's `notifications/cancelled` is.
- The craft apps' MCP servers speak exactly this. So do the control channel (near enough: id, method, params) and MCP's Streamable HTTP transport.
- One codec therefore serves modes 2 and 3, the MCP adapter engine, and any future "drive an engine from an agent" path.
- gRPC and Cap'n Proto were considered:
  - both need a schema compiler, and gRPC needs HTTP/2 and has no first-class Python-stdlib server;
  - bulk bytes never travel in messages here (shm or CAS), so neither one's binary efficiency buys anything;
  - so they stay out. The RPC-protocol note owns the final call; the per-mode framings below don't depend on it.

### 2.2 Data references and the two planes

Every value that isn't small JSON travels as a `DataRef`:

```json
{"md5": "<32 hex>", "media": "application/x-npy|text/plain;charset=utf-8|image/svg+xml|application/x-vectorcraft|…",
 "bytes": 1234567, "key": "impc:v1:plumarr:<md5>:c", "path": "/…/store/plumarr/ab/<md5>.c.npy",
 "meta": {"shape": [812, 808, 3], "dtype": "<f4", "space": "Lab"}}
```

| Plane | When chosen | Engine publishes | UI reads | Sources (Load nodes) |
|---|---|---|---|---|
| **shm** | `hello` shows the same `host_id` and a store root the UI can stat | commonkit segment (key and path) or the path fallback, as `publish.py` does today | mmap by `path` (the `shm` crate) | Plain local paths in widgets, as today |
| **CAS** | Different host, or a store that can't be shared | Blob in the engine's CAS, `md5` with `key`/`path` absent and optionally `url` | Previews (≤640 px RGBA8, peaks, short text) are fetched eagerly with `blob.get`; full arrays are fetched lazily when the viewer zooms or probes. Fetched blobs land in the UI's local CAS cache (same `ab/<md5>` sharding), so a second view is local | Before `run`, the host walks the graph for `path`-hinted widgets, hashes each file (cached by `(path, mtime, size)`), calls `blob.has`, `blob.put`s only what is missing, and rewrites the value to `{"cas": md5, "name": "test.jpg"}`. The engine never sees a UI path |

- **Checkpoint reuse:**
  - The SDK executors key their memo by a *derivation id*: `md5(kind, node_version, canonical params JSON, input md5s, slice spec)`. The Python executor's `_memo` already hashes kind, version, plain values and identity.
  - An engine with a persistent CAS keeps a `derivation → output DataRefs` table (alexandria-style MD5 store, sharded `ab/<md5>`). A re-run after an edit, after a restart, or from another UI computes nothing for an unchanged prefix of the graph.
  - Such nodes report `cached`, as nodes cut by commonkit do today.
- **The `save` op on a remote engine:** the engine can't write the user's path. The host sends `save` without `path`; the engine replies `{"ok":true, "files":[{"name","blob":DataRef}]}`; the host `blob.get`s and writes locally. Locally, `path` is passed through as today.
- **Raw blobs:** commonkit is `.npy`-only today. Text, SVG, WAV exports and vector documents need raw blobs under the same key and path scheme (`{root}/{form}/{md5[:2]}/{md5}.{variant}.<ext>`). That is a small commonkit change, proposed at source (golden rule 7) and owned by the CAS note.

### 2.3 Data in flight: what every mode must carry

1. **`preview` records** carrying a `thumb` DataRef the moment a node finishes (today).
2. **`progress`** with `fraction` at most every 100 ms per node (today, in the protocol).
3. **New: `display` on a status record.** The viewer's inspect entry is sent as soon as its node is done, not only in the final reply, so a probe on an early wire fills while the rest of the graph runs. The Python `Plan` and `research-local` both compute inspect entries after `executor.run` returns, and both can move that work into `on_result` with no change in results.
4. **New, optional: `partial` previews** (`state:"preview", partial:true`) from long nodes: VI the research software's commands stages, an audio STFT block by block, an OpenCV denoise per tile. Gated by `capabilities.partial_preview`.

## 3. The four hosting modes

### 3.1 Mode 1: compiled-in (in-process Rust)

**Today:** `research-local` (L2, beside the bridge). `start(wake) -> EngineClient` uses `EngineClient::detached()` and runs a dispatcher thread and a worker thread. The engine is `research_graph` from the research software's native engine.

**The generic design:**

- **Contract:** `trait EngineCore: Send` in the SDK. It has `hello() -> Hello`, `schema() -> SchemaReply`, `run(req, &mut dyn Emit, &CancelToken) -> RunReply` and `op(name, body, &CancelToken) -> Value`.
- **Host:** `<base>-host-inproc` wraps any `EngineCore` in today's two-thread pattern:
  - the dispatcher reads `EngineRequest`s and flips the run's cancel token at once;
  - the worker owns the core and runs one job at a time. Cores may declare `multi_run` for a small pool instead.
- **research-local** becomes `impl EngineCore for ResearchLocal` with no behaviour change. Its `wire_value` and `inspected` stay in it, and its `Sink` becomes the host's `Emit`.
- **Lifecycle:**
  - *discover:* compile time: an `inventory`-style registry, or an explicit list in the app crate (preferred: no link-time magic);
  - *spawn:* thread start;
  - *health:* the thread is alive; device failure is `Unavailable{reason}` (today);
  - *restart:* none, since a panic in an engine is a bug and `catch_unwind` around `run` turns it into a failed reply;
  - *shutdown:* drop the client, which ends the request channel, and the threads exit.
- **Handshake:** a direct call; the data plane is always shm. An optional `DataRef::Mem` (an `Arc<[u8]>` held in a host table) could skip the file for small previews, but the research software already maps previews from files cheaply, so don't add it until a profile asks for it.
- **Status:** a direct `Emit::status(record)`, which is today's `Sink::send` plus `wake`.
- **Cancel:** an `AtomicBool` per run (today).
- **Efficiency:** identical to today. The only change is the trait boundary: no serialisation is added. `EngineRequest::Run` stays a `serde_json::Value` until the typed-message refactor (interface note), then becomes `Box<RunRequest>` and saves the parse that `engine.rs:85` does now.

### 3.2 Mode 2: separate-process Rust (or any language) over a local socket

**Why:**

- **Crash isolation.** A decoder fed a hostile file (symphonia, image codecs), a GPU driver fault, or a C++ library (OpenCV via FFI) can't take the editor down.
- **Independent builds:** engine crates don't enter the app's dependency graph.
- **Restartable state:** a wedged device is cleared with a restart.

**Transport:**

- The UI creates a private directory (mode 0700) and a Unix domain socket, then spawns the child with `--connect unix:<path>` and `<BASE>_ENGINE_TOKEN` in its environment. The child connects back and authenticates in `hello`. On Windows this is a named pipe `\\.\pipe\<base>-<pid>-<rand>`.
- The UI listening and the child connecting avoids races on readiness and port files.
- **Stdout is never used.** the research software prints on import, and so do many libraries.
- **stderr** is captured into a 64 KiB ring and shown in the engine log panel. Its last lines go into the banner after a crash.
- **Framing:** newline-delimited JSON-RPC 2.0, a line of at most 16 MiB, as the crafts' MCP servers do.
- The child runs **two threads at minimum**: a reader, so `run/cancel` and `ping` are heard during a run (VectorCraft's MCP server learned this, `docs/mcp.md` › Long calls), and the core worker.

**Lifecycle:**

| Step | Behaviour |
|---|---|
| Discover | An engine manifest (TOML) in the app bundle, `~/.config/<base>/engines/*.toml`, or `--engine-manifest <file>`: `id`, `name`, `mode = "process"`, `command = ["…"]`, `cwd`, `env`, `restart = "on-crash"`, `max_restarts = 5` |
| Spawn | In its own process group; `PR_SET_PDEATHSIG` on Linux. On macOS and Windows the child exits on EOF of the socket, so a UI crash never leaves orphans |
| Handshake | `hello` within 10 s or the spawn counts as failed. Version mismatch: `Unavailable{reason:"engine speaks protocol 3, this app 1–2"}` |
| Health | Child alive (`try_wait`) plus `ping` every 3 s with a 2 s timeout. The `ping` is answered by the reader thread, so a long run doesn't look like a hang. A missed ping during a run only marks the engine "busy" |
| Crash | Every run in flight gets `failed_run(run_id, "the engine stopped (signal 11); restarting")` and the reducer marks its nodes `error`. State is `EngineDown{reason, stderr_tail}`. Restart uses today's `Backoff`; after the schema reloads, the `schema_digest` is compared and the UI keeps the graph. After `max_restarts` crashes inside 60 s it stops and says so |
| Shutdown | `shutdown` notification, 2 s grace, SIGTERM, 2 s, SIGKILL to the group |

**Data plane:** always shm. Same host by construction; the store is commonkit's shared store or a per-session directory.

**Status:** `run/status` notifications interleaved with other replies on the one socket, tagged by `run_id`.

**Cancel:** a `run/cancel` notification. The reader thread sets the token, and the executor checks it between nodes and list items.

**Remote?** No. A remote Rust engine uses mode 3: the SDK's `serve()` speaks HTTP too.

### 3.3 Mode 3: separate-process HTTP (any language; local or remote)

**Transport:** shaped like MCP's Streamable HTTP so that tooling and knowledge carry over.

| Endpoint | Use |
|---|---|
| `POST /v1/rpc` | JSON-RPC request → JSON reply (`hello`, `schema`, `run` → `{accepted, run_id}`, `op`, `blob.has`) |
| `GET /v1/runs/{run_id}/events` | `text/event-stream`: `status`, `reply` and `log` events; `id:` = record `seq`; resumable with `Last-Event-ID` |
| `GET /v1/runs/{run_id}/status?after=<seq>` | **Polling fallback:** the records since `seq`, plus `done:bool`. For proxies that buffer SSE, and for scripts |
| `GET /v1/runs/{run_id}` | The `RunReply` (200), or 202 with a progress snapshot |
| `POST /v1/runs/{run_id}/cancel` | Cancel (also `run/cancel` over `/v1/rpc`) |
| `HEAD`/`PUT`/`GET /v1/blobs/{md5}` | CAS. The server checks the MD5 of every `PUT` and rejects a mismatch; `Content-Type` carries `media` |
| `GET /v1/health` | `{ok, engine, busy, runs}`, cheap, no auth |

- **Status tracking: push and pull.**
  - The host opens the SSE stream before `run` returns. On a dropped connection it re-attaches with `Last-Event-ID`.
  - If SSE fails twice it polls every 100 ms while a run is live and every 1 s when idle.
  - Either way it feeds the same records into `EngineEvent::Status`.
  - Runs survive a UI disconnect for a TTL (default 10 min). A remote run outlives a laptop sleep, which today's AMQP status queue (exclusive, auto-delete) doesn't allow.
- **Lifecycle:**
  - *discover:* a manifest with `mode = "http"`, `url`, `token_file`. Optionally a `command` to launch a local HTTP engine with `--listen 127.0.0.1:0 --port-file <f>`; the host reads the port and owns the child as in mode 2.
  - *health:* `/v1/health` every 3 s.
  - *restart:* only a manifest-launched child is restarted.
  - *shutdown:* `shutdown` for a launched child; nothing for a shared remote service.
- **Data plane:**
  - shm when `hello` matches the host and the store (a local HTTP engine is as cheap as mode 2 except for the HTTP parse);
  - CAS otherwise, with upload of sources and download of results as in 2.2;
  - previews are pushed as DataRefs in `preview` records, and the host fetches each one (≤640 px RGBA8, about 1.6 MB; a peaks table or a text head is far less). A `thumb` may carry an inline `data` (base64) under `capabilities.max_inline` (default 64 KiB) to save a round trip on small thumbs.
- **Security:**
  - binds loopback by default;
  - non-loopback requires a bearer token (a token file, as the control channel does) and TLS (rustls in the Rust SDK; a reverse proxy for Python);
  - a remote engine refuses `path` sources (only `cas` sources);
  - Load and Save are confined to the engine's own roots (the crafts' `--automation-read-root` and `--automation-write-root` pattern);
  - blob size is capped (`max_blob`).

### 3.4 Mode 4: pipekit over AMQP (existing)

- **Unchanged on the wire:** R-1 reply-queue RPC; the `pk-*` envelope; `{system}.status` topic exchange with routing key `run.<correlation>`; cancel by `{system}.control` broadcast. The host is today's `client_loop`.
- **Adaptations, all inside the host:**
  - `hello` is synthesised from the `Schema` reply (`engine.plugin_export` → `capabilities.ops`);
  - the data plane is shm when the UI and the fleet share the commonkit store (the development machine today);
  - for a fleet on a remote server, the fleet publishes to its CAS (the rclone CAS alexandria already uses), and the host fetches through a small pipekit `Blob` service (`has`/`get`/`put` by md5) or through the HTTP blob endpoints of mode 3. AMQP bodies stay small.
- **Lifecycle:** today's: broker reachability with `Backoff`, consumer checks every 3 s (`EngineDown{queues}`), `pipectl up research.yml` as the hint. The hint and the system name move into the engine manifest (`mode = "pipekit"`, `broker`, `system`, `up = "cd python && uv run pipectl up research.yml"`).
- **Python side:** `service.py` splits into the transport-neutral `Engine` handler (`schema()`, `run(req, emit, cancelled)`, `op()`) and a pipekit front end. `Plan.process_item` becomes about ten lines that call the handler with `self.status` and `self.cancelled`. The other two front ends (`socket`, `http`) host the same handler, so the VI engine can also run in mode 2 or 3 (for example on a remote server with no broker).

### 3.5 Mode comparison

| | 1 in-process | 2 process (socket) | 3 HTTP | 4 pipekit |
|---|---|---|---|---|
| Languages | Rust | Any (Rust SDK, Python SDK) | Any | Python (pipekit) |
| Crash isolation | No (catch_unwind only) | Yes | Yes | Yes |
| Remote | No | No | Yes | Yes (broker) |
| Data plane | shm | shm | shm or CAS | shm or CAS |
| Status push | Channel | Socket notification | SSE, with polling fallback | AMQP topic |
| Cancel | AtomicBool | Notification → token | POST cancel → token | Control broadcast |
| Extra hop per record | 0 | 1 local write | 1 HTTP chunk | Broker |
| Fan-out (M9 scatter/join) | No | No | No (one server) | Yes |
| Typical use | research-local, text | audio, VectorCraft adapter, text, crash-prone native libs | OpenCV, remote GPU boxes | VI fleet, M9 distributed |

## 4. SDKs

### 4.1 Rust: `<base>-engine-sdk`

```rust
use base_engine_sdk::prelude::*;

/// Split text into a list.
#[node(id = "text.split", category = "Text/Structure", version = 1)]
fn split(
    text: Text,
    #[param(default = "\\n", doc = "separator; a regex when `regex` is on")] sep: String,
    #[param(default = false)] regex: bool,
    #[param(default = true)] trim: bool,
    cx: &mut Cx,
) -> Result<List<Text>, NodeError> { … }

fn main() -> std::process::ExitCode {
    Engine::new("text", env!("CARGO_PKG_VERSION"))
        .nodes(nodes![split, join, regex_replace, /* … */])
        .expansion(include_str!("expansions/slugify_title.json"))
        .serve()          // reads argv: --connect unix:… | --listen host:port | (none) → help
}
// in-process: `Engine::new(…).nodes(…).into_core()` → Box<dyn EngineCore>
```

- **`#[node]`** derives the `NodeDef`:
  - ports from the parameter types, through a `WireType` trait on `Text`, `Array<T>`, `Audio`, `List<_>` and so on;
  - widgets from `#[param(default, min, max, step, choices, multiline, path_hint)]`;
  - outputs from the return type (a tuple or `#[outputs(gray, color)]` names several);
  - the memo version from `version`.
  - A plain trait `impl Node for X { fn def(); fn run(); }` remains for dynamic catalogues; the MCP adapter needs it.
- **`Cx`** gives:
  - `cancelled()`;
  - `progress(f)`, rate-limited to 100 ms;
  - `preview(&value)` for a partial preview;
  - `publish(bytes|array, media) -> DataRef`, which chooses shm or CAS by the negotiated plane;
  - `fetch(&DataRef)`, which resolves a `cas` source through the engine's CAS;
  - `log`.
- **The executor is the SDK's, not the engine's.** It must reproduce the Python executor's semantics:
  - topological order;
  - digest memo with a byte budget;
  - list mapping and axes;
  - wire slices;
  - variadic inputs;
  - `Absent`/ExecutionBlocker;
  - errors as data;
  - provenance (`wire|bundle|widget|default`);
  - bundle precedence;
  - lineage.

  `research_graph`'s executor (the research software's native engine) already does this for VI. Lifting its generic half into the SDK is the main cost. A shared JSON conformance corpus (graph, expected `order`, states and provenance), run against both executors, keeps them equal.
- **Display helpers:** `display::image(rgba8|f32)`, `display::waveform(peaks)`, `display::text(head, lines)`, `display::table(cols, rows)`, `display::svg(blob, raster)`. Each returns today's `Preview` plus a `viewer` tag (section 6).

### 4.2 Python: `<base>-engine`

Split as the FloCraft study proposed (about 1,300 generic lines), plus:

- `<base>_engine.handler.Engine(registry, cache, store)`, which has `.hello()`, `.schema()`, `.run(req, emit, cancelled)` and `.op(name, body, cancelled)`. This is `Plan.process_item` minus pipekit.
- **Front ends:**
  - `fronts.pipekit`: today's `Schema`/`Plan` `PipelineNode`s, an optional extra `[pipekit]`;
  - `fronts.socket`: JSON-RPC over the Unix socket, with a reader thread and a worker thread;
  - `fronts.http`: stdlib `ThreadingHTTPServer` with SSE by chunked writes. No web framework dependency, in keeping with "no web tooling".
- `publish.Store` with three backends: `commonkit`, `path` and `cas`. The key scheme is a parameter (`IMPC` is the research software's).
- A `@node` decorator over today's `FuncNode`/`io.*` helpers:

```python
@node("cv.canny", category="OpenCV/Edges", version=1)
def canny(image: Image, low: Float(100, 0, 1000) = 100, high: Float(200, 0, 1000) = 200,
          aperture: Combo([3, 5, 7]) = 3, l2: bool = False) -> Image:
    return cv2.Canny(to_u8(image), low, high, apertureSize=aperture, L2gradient=l2)
```

- **Entry:** `python -m <base>_engine.serve opencv_engine:ENGINE --connect unix:… | --listen 127.0.0.1:0 | --pipekit research.yml`.

## 5. The example engines

### 5.1 Mode assignment (all four modes exercised)

| Engine | Language | Mode(s) | Role as conformance fixture |
|---|---|---|---|
| `research-local` (VI, the research software's native engine) | Rust | 1 | Existing default; must lose nothing |
| **text** | Rust (SDK reference) | **1 and 2**, from one crate (and 3 via `--listen`) | Proves an engine moves between in-process and process with no code change; tiny, deterministic, no GPU, so it is the CI fixture for the Rust host paths and replaces the 90 KB VI schema fixture in core tests |
| **audio** | Rust | **2** | Crash isolation (decoders), long ops with `progress` and partial previews, a non-image data type end to end |
| **vectorcraft-mcp** | Rust adapter | **2** | A dynamic catalogue, a stateful app made functional, MCP cancel and progress mapped onto ours |
| **opencv** | Python | **3** (local shm and remote CAS) and **4** (same handler under pipekit) | Proves the Python SDK is transport-neutral; exercises upload and download over CAS |
| The research software Python (VI) | Python | 4 (and 3 for a remote server) | Existing; exactness gate |

`cargo xtask conformance --engine <manifest>` runs one suite per engine. The suite checks:

- `hello` and version refusal;
- that the schema parses;
- a fixture graph's `order`, states and provenance;
- record invariants: `seq` strictly rising, a terminal state last, the generation echoed;
- cancel mid-run: unreached nodes are `cancelled` within 500 ms of the cancel;
- a second run gives `cached` everywhere;
- `display` streamed before the reply;
- the data plane: shm paths map, or CAS round trips byte-exact with a verified md5;
- mode 2 only: `kill -9` the child mid-run gives a failed reply, a restart and a reloaded schema;
- mode 3 only: drop the SSE stream mid-run gives a resume with no lost records, and polling alone gives the same final state;
- a `panic_hunt`-style fuzz of each node's params.

### 5.2 OpenCV engine (`<base>-engine-opencv`)

**Pick: Python, `opencv-python-headless`** (Apache-2.0). The Rust `opencv` crate was rejected:

- it needs a system OpenCV 4.x (brew opencv is about 1 GB with its dependencies) plus libclang for binding generation at build time;
- bindings take minutes to build and break across OpenCV and clang versions;
- Windows needs vcpkg;
- it would put a C++ toolchain in the base's CI.

The headless wheel installs in seconds on macOS arm64, Linux and Windows. Its arrays are already numpy, which is what `.npy` shm and CAS carry, and the Python SDK and the pipekit front end come for free.

**Determinism:**

- `cv2.ocl.setUseOpenCL(False)` and `cv2.setRNGSeed(seed)` per node that uses an RNG (kmeans, `seed` widget);
- `cv2.__version__` goes into every node's memo version.

**Images on the wire:** RGB float32 0..1 or uint8 HWC with `meta.space`. Nodes convert with `to_u8` and `to_f32` helpers, so OpenCV's BGR never leaks: conversion happens at Load and Save only.

| Category | Nodes |
|---|---|
| I/O | Load Image (`path` hint; `cas` remotely), Save Image (PNG/JPEG/TIFF via the `save` op), Video Frame (`VideoCapture`, frame at time t) |
| Colour | Convert Colour (Gray/HSV/Lab/YCrCb ↔ RGB), Split Channels, Merge Channels, Equalize Histogram, CLAHE, Normalize, Convert Scale (α, β) |
| Filter | Gaussian Blur, Median Blur, Bilateral Filter, Box Filter, Filter 2D (kernel text), Sobel, Scharr, Laplacian |
| Edges and features | Canny, Hough Lines P (→ `segments` table), Find Contours (→ contours table), Draw Contours, Good Features to Track (→ `points` table), ORB Keypoints (→ `points` table) |
| Threshold | Threshold (binary, inverse, truncate, to-zero, Otsu, triangle), Adaptive Threshold, In Range |
| Morphology | Structuring Element, Erode, Dilate, Morphology Ex (open, close, gradient, top hat, black hat), Distance Transform, Connected Components (→ labels, which uses the viewer's existing `labels` mode) |
| Geometry | Resize, Rotate, Flip, Crop, Warp Affine, Pyramid Down/Up |
| Photo (non-photorealistic) | Stylization, Pencil Sketch (two outputs: gray and colour), Detail Enhance, Edge-Preserving Filter, Inpaint (mask input: reuses core's mask painter), Fast NL-Means Denoise (reports `progress` per tile when tiled) |
| Clustering and segmentation | K-Means Colours (K, attempts, seed → quantised image + palette table), Watershed (markers), Grab Cut (rect, iterations) |
| Composite | Add Weighted, Abs Diff, Bitwise And/Or/Xor/Not (with mask) |
| Expansion | **Cartoon** = Pyramid Down ×n → Bilateral ×k → Pyramid Up ×n → (Gray → Median → Adaptive Threshold) → Bitwise And: the classic OpenCV cartoon. It is a command node with a stored expansion, showing VI's command → graph feature without VI |

Visualisation needs nothing new beyond what the research software has: RGB and grey previews, histograms, `labels`, and `points`/`segments` tables (already in `Display.table`). This makes OpenCV the low-risk first example.

### 5.3 Text engine (`<base>-engine-text`; Rust reference SDK demo)

**Types:**

- `TEXT`: inline under 64 KiB, a raw `text/plain` blob above it;
- `TEXT_LIST`;
- `TABLE`: columns and rows (an Arrow-like JSON, or `.npy` structured when large);
- `JSON`;
- `DIFF`.

**Libraries:** `regex`, `similar` (diff), `minijinja` (templates), `heck` (case), `unicode-normalization`, `unicode-segmentation`, `textwrap`, `csv`, `pulldown-cmark`, `md-5`/`sha2`. All are MIT or Apache, pure Rust, and fast to build.

| Category | Nodes |
|---|---|
| Sources | Text (multiline widget), Load Text (`path`, encoding), Lorem Ipsum (words, seed), Save Text (`save` op) |
| Structure | Split (sep or regex, trim, drop empty), Join, Lines, Sentences, Words (Unicode segmentation), Head/Tail, Slice (uses wire slices), Concat (variadic) |
| Regex | Match (bool), Find All (→ list, plus spans for highlighting), Replace (with `$1` groups), Filter Lines (invert), Extract Groups (→ table) |
| Case and form | Case (upper, lower, title, sentence, snake, kebab, camel), Trim/Collapse Whitespace, Unicode Normalize (NFC/NFD/NFKC/NFKD), Slugify, Wrap (width), Translate Characters |
| Analysis | Word Frequencies (→ table, top N, stop words), Counts (chars, words, lines, sentences), N-grams, Hash (md5, sha256) |
| Lists | Sort (natural, case-insensitive), Unique, Reverse, Shuffle (seed), Number Lines |
| Structured | CSV → Table, Table → CSV, JSON Pointer Select, Format JSON, Markdown → HTML (the text, not rendered) |
| Compare and compose | Diff (unified or side-by-side, context lines), Template (minijinja; variadic named inputs become variables) |
| Expansion | **Title Slug** = Trim → Case(title) → Slugify; **Word Cloud Table** = Words → Case(lower) → Filter(stop words) → Word Frequencies |

**Why it is the SDK reference:**

- every node is 5–20 lines;
- list mapping (Split → per-item Case → Join) exercises the executor's list semantics and wire slices;
- Template exercises variadic inputs;
- it builds in seconds, so it can be both a compiled-in engine (feature `engine-text` in the generic app) and a separate binary.

**Visualisation needs the generalised viewer:**

- a monospace text pane with line numbers, search and regex-span highlight;
- a diff pane (two columns, coloured);
- a table pane, sortable, with a bar chart for a numeric column;
- node thumbnails: the first lines rendered as text, or a sparkline of counts.

### 5.4 Audio engine (`<base>-engine-audio`; Rust, mode 2)

**Libraries:** SoundCraft's own crates, pinned by git rev as the crafts pin `craft-libs` (same family, MIT OR Apache-2.0):

- **`soundcraft-audio-io`:** symphonia decoding of WAV, AIFF, FLAC, MP3 and MP4/AAC, WAV writing, and the `peaks` multi-resolution min/max/RMS overview, which is exactly what a waveform viewer needs.
- **`soundcraft-dsp`:** RBJ biquads, 7-band EQ, dynamics and reverb plugins with `offline::apply_plugin_with_tail`, and from `offline`: windowed-sinc `resample`, WSOLA `time_stretch` and `pitch_shift`, `signal_generator`, `fade`, `normalize`, `detect_transients` and `non_silent_ranges`. Also `spectrum::magnitude_db` (rustfft), BS.1770 loudness meters and `pitch_detect`.

Both crates are pure Rust, so the engine has no system dependencies.

**The Python alternative was rejected** for the reference: soundfile, scipy and librosa pull in numba and llvmlite, are slow to import, and aren't needed when the DSP already exists in the family.

**Types:**

- `AUDIO`: planar f32 `.npy` `[channels, frames]` with `meta.sample_rate`. This matches commonkit's planned audio domain, form `wav16k` and variant `sr16000` (`commonkit/keys.py`).
- `SPECTRUM`: f32 `[bins, frames]` dB, with `meta.{hop, n_fft, sample_rate, scale}`.
- `ENVELOPE`: 1-D f32 with `meta.hop`.
- `EVENTS`: a times table.
- `TABLE`.

| Category | Nodes |
|---|---|
| I/O | Load Audio (`path`; decode, optional mono or rate on load), Save Audio (WAV 16/24/32f via the `save` op), Generator (sine, square, saw, triangle, white or pink noise; frequency, level, duration, seed), Silence |
| Edit | Trim (start, duration), Slice by Events, Concatenate (variadic, crossfade), Reverse, Fade In/Out (shape), Remove DC, Invert |
| Level | Gain (dB), Normalize (peak or LUFS target), Compressor, Limiter |
| Rate and time | Resample (target rate), Time Stretch (ratio), Pitch Shift (semitones) |
| Filter and effect | Biquad (low/high/band pass, notch, peak, low/high shelf; f, Q, gain), EQ 7-band, Reverb, Delay |
| Channels | To Mono, Split Channels, Merge Channels, Pan, Mix (variadic, per-input gain) |
| Analysis | FFT (window, size → spectrum of one frame), STFT Spectrogram (n_fft, hop, window, dB floor), Envelope (RMS or peak, window), Loudness (integrated LUFS, true peak, LRA → table), Transients (→ events), Non-silent Ranges (→ events), Pitch Track (→ envelope) |
| Expansion | **Clean Voice** = Remove DC → Biquad(high-pass 80 Hz) → Compressor → Normalize(−16 LUFS) |

**Data in flight:**

- Resample, Time Stretch, Pitch Shift, Reverb tails and STFT report `progress` per block.
- STFT emits `partial` spectrogram previews every about 250 ms, as columns fill.
- Thumbnails:
  - a waveform drawn by the engine into RGBA8, so the existing image thumbnail path shows it today;
  - *plus* a `peaks` DataRef that the generalised viewer draws crisply at any zoom.
- Spectrograms preview as heat-mapped RGBA (the viewer's existing `heat` mode applies).

**Viewer additions:**

- waveform (zoomable, from peaks, with a time ruler);
- spectrogram (image plus frequency and time axes);
- envelope or curve plot;
- **playback** of the inspected `AUDIO` (transport, loop, scrub). This needs an output-device crate in the UI layer: `cpal`, or SoundCraft's `playback` crate behind a cargo feature, so the base builds without audio.

### 5.5 VectorCraft MCP adapter (`<base>-engine-vectorcraft`; Rust, mode 2)

**Why VectorCraft** among the non-PhotoCraft crafts:

- its headless MCP server (`vectorcraft-cli mcp --headless`) is the most complete:
  - `command_list`, `command_run`, `command_batch` (atomic);
  - `open_file` takes PNG, JPEG, TIFF, SVG and PDF;
  - `save_file` writes SVG, PDF and `.vectorcraft`;
  - `render_preview` returns a PNG;
  - `vectorcraft://effect/{id}` describes each live effect's parameters and defaults;
- it already implements **MCP cancellation** (`notifications/cancelled`, which stops exports and renders at checkpoints) and **progress** (`notifications/progress`, at most every 200 ms) (`vectorcraft/docs/mcp.md` › Long calls);
- **Image Trace bridges raster graphs to vector.** The VI engine already uses potrace, so "VI render → Trace → Recolor → SVG" is a natural cross-engine story;
- its outputs preview as images, so the first version needs no new viewer.

**Runners-up:**

- WordCraft: Template → Markdown → document → PDF; a fine sink for the text engine.
- SoundCraft: its DAW session model is timeline-shaped, and the audio engine already reuses its DSP.
- EffectCraft: video, heavy.
- CadCraft and SolidCraft: need a 3D viewer.

**Adapter design:**

- **Making the app functional:**
  - The adapter is an SDK engine that spawns one (or N, `pool`) `vectorcraft-cli mcp --headless --automation-read-root <store> --automation-write-root <work>` child.
  - It speaks MCP JSON-RPC over the child's stdio. The MCP server's stdout *is* its protocol, which is acceptable here because the adapter, not the base, owns it.
  - Each node is a pure function `doc_in (CAS blob: .vectorcraft or .svg) + params → doc_out (CAS blob)`, implemented as `open_file(in)` → `command_batch{atomic:true, steps}` → `save_file(out)` → `render_preview(max_side: node_px)`.
  - No document state survives between nodes, so memo and derivation ids work unchanged: md5 of (command ids, params, input md5, `serverInfo.version` from `initialize`).
  - If repeated saves of the same input aren't byte-identical (object ids, timestamps), the adapter canonicalises the SVG, or declares `deterministic:false` so outputs are cached per session only. This is to be measured.
- **The catalogue:** a curated TOML that maps nodes to command ids with *typed* ports, because `list_commands` gives prose parameter docs, not types.
  - Generated nodes: Image Trace (preset, threshold, paths, corners, noise), Simplify, Offset Path, Outline Stroke, Pathfinder (unite, minus front, intersect, exclude), Recolor (palette), Apply Effect (one node per effect id, ports from the effect resource's parameters and defaults), Draw Shape, Text, Rasterize (→ `ARRAY` for VI or OpenCV), Export (SVG/PDF/PNG via the `save` op).
  - An escape-hatch **Run Command** node takes an id and a JSON params widget.
- **Status:**
  - `started` and `done` around each MCP call;
  - MCP `notifications/progress` → `progress` with `fraction = progress/total`;
  - `render_preview` → the `preview` thumb.
- **Cancel:** our cancel → `notifications/cancelled{requestId}` to the child. VectorCraft drops the reply and leaves the document untouched.
- **Busy:** `-32003` is avoided by the adapter serialising calls per child; a pool gives parallel branches.
- **Visualisation:**
  - a `vector` viewer tag: the PNG preview, a layer tree from `inspect_document` (collapsible), and the SVG source in the text pane.
- **Not verified live:** this session couldn't start the artcraft MCP servers (binaries not built under `/Volumes/src/artcraft/*/target/release`), so the design rests on `vectorcraft/docs/mcp.md`. Prototype step one: build `vectorcraft-cli` and record a trace → recolor → save round trip, checking byte determinism.

## 6. Visualisation beyond images (what the engines need)

The viewer must dispatch on a `viewer` tag carried by `Preview` and `Display`. The display note owns the schema. The kinds that these engines need:

| Kind | Payload | Used by | Thumbnail |
|---|---|---|---|
| `image` | RGBA8 preview, histograms, meta, full array lazily (today) | VI, OpenCV, audio spectrogram, VectorCraft | Today's |
| `labels`, `points`, `segments` | Today's modes and tables | OpenCV | Today's |
| `text` | head (≤64 KiB inline) + blob, line count, regex spans | Text, VectorCraft SVG source | Engine-rendered RGBA *and* `text` head |
| `diff` | hunks | Text | Coloured mini-diff |
| `table` | columns, rows (paged), optional chart hint | Text, OpenCV palette, audio loudness | Top rows |
| `waveform` | `peaks` DataRef, sample rate, channels, + `AUDIO` DataRef for playback | Audio | Engine-rendered RGBA |
| `spectrogram` | RGBA preview + axes | Audio | Engine-rendered RGBA |
| `curve` | x/y series | Audio envelope, pitch | Sparkline |
| `vector` | PNG preview, layer tree JSON, SVG blob | VectorCraft | PNG |
| `json` | Value | Any | First keys |

**Rule:** every engine also sends an RGBA `thumb`, so a UI that doesn't know a kind still shows something. This is how existing the research software keeps working while the viewer grows.

## 7. Effort (on top of the FloCraft study's phases)

| Item | Days |
|---|---|
| `EngineCore` trait, in-process host, `research-local` on it (no behaviour change) | 1.5–2 |
| JSON-RPC codec, `hello`/capabilities, typed messages in the bridge facade | 2 |
| Mode 2 host: spawn, socket, supervise, restart, stderr ring, Windows pipe | 3–4 |
| Mode 3 host: POST, SSE with resume, polling fallback, blob client, local CAS cache, source upload rewrite | 4–5 |
| Mode 4 adaptations (synthesised hello, manifest, CAS fetch path for remote fleets) | 1–2 |
| Rust SDK: `#[node]` macro, `Cx`, executor lifted from `research_graph` and held to the Python corpus, socket and HTTP fronts | 8–10 |
| Python SDK split + handler + socket and HTTP fronts | 5–6 (FloCraft's 4–6 plus 1 for the fronts) |
| Text engine (+ fixtures replacing the VI fixture in core tests) | 3 |
| OpenCV engine (about 50 nodes, mostly 10-line wrappers) | 4 |
| Audio engine + waveform, spectrogram and playback viewers | 6–8 |
| VectorCraft adapter (prototype, curated catalogue, determinism) | 4–5 |
| Conformance suite (`xtask conformance`) across all modes | 3 |
| **Total for this note's scope** | **about 49–60** |

**Suggested order:**

1. `EngineCore` and `research-local` on it.
2. The text engine in modes 1 and 2: it proves the SDK and the process host, and gives core its neutral fixture.
3. The Python split and OpenCV under pipekit, then under HTTP: this proves the Python fronts and CAS.
4. Audio: the new viewers.
5. The VectorCraft adapter.

The research software's gates (tests, exactness 12/12) run green at every step, because modes 1 and 4 keep their wire and their behaviour.

## 8. Open questions for the other notes

1. **The CAS digest** (CAS note): md5 of the *stored blob bytes* (alexandria's rule) or today's `publish.digest` (dtype + shape + raw bytes)? This note assumes blob bytes for CAS handles, with commonkit keys free to keep their own digest. One rule for both would be simpler.
2. **Raw (non-`.npy`) blobs in commonkit:** needed for text, SVG and documents. The change goes to commonkit at source.
3. **Multi-engine graphs** (OpenCV → VectorCraft → VI in one workflow): DataRefs with md5 make an engine boundary a CAS hand-off. Is this v1 (the graph is partitioned per engine by the UI's run state) or later? This note assumes one engine per workflow in v1, with a workflow `engine` field, as FloCraft proposed.
4. **The executor's source of truth:** with the Rust SDK executor, `research_graph`'s executor and the Python executor all implementing one semantics, the shared conformance corpus must be authoritative. Where should it live? This note proposes the base repo's `conformance/` directory.
