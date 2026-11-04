# The UI ↔ engine interface, protocol 2 (normative draft)

*2026-10-27; written in the research software's repository and moved to this one the same day. Status: **draft for the maintainer's decision**, not yet implemented. Companion to `docs/plan.md`, which explains the choices; this file states the rules. It consolidates the research notes in `docs/research/` (01 floor, 02 transports, 03 data plane, 04 visualisation, 05 artcraft MCP, 06 hosting modes and examples). Where those notes disagree, this file records the decision and says so. **Revision 2** applies the review `docs/research/08-review.md`; the plan's "Review dispositions" section says where a fix chose among options.*

The words MUST, MUST NOT, SHOULD and MAY are used as in RFC 2119. The product name is **PlumCraft** (decision D1 in the plan, decided 2026-10-27).

## 1. Scope

This interface is everything that crosses between the editor (the "UI": canvas, viewer, session, commands) and an engine (anything that publishes a node catalogue and runs graphs). It covers:

- the session handshake, capabilities and versioning (section 5);
- the operations a UI calls (section 6);
- the node catalogue (section 7);
- runs: request, plan, watch, reply (section 8);
- the event stream, including node status (section 9);
- displays, renditions and data in flight (section 10);
- cancellation (section 11);
- errors and failure (section 12);
- bulk data: handles, locators, upload, download (sections 4 and 13);
- checkpoint reuse (section 14);
- engine-specific operations (section 15);
- how each binding (in-process, AMQP/pipekit, local socket, HTTP) carries all of the above (section 16).

It does not cover the control channel (`docs/control-protocol.md`), which drives the UI rather than an engine, or the workflow file format beyond the fields an engine reads.

**The floor.** A binding or engine host that cannot carry every item F1–F85 (F85, local model management, added after review) of the floor inventory is non-conforming. The plan's section 9 maps each item to the section here that provides it.

## 2. Terms

| Term | Meaning |
|---|---|
| UI | The editor process: canvas, viewer, session, commands, control channel |
| Engine | A process or library that answers `hello` and `schema` and runs graphs |
| Binding | A transport carrying this interface: in-process, AMQP/pipekit, socket JSON-RPC, HTTP+SSE |
| Host | The UI-side code that owns a binding: spawns, supervises, reconnects, synthesises failures |
| Session | One handshake between one UI and one engine. Has an id and one ordered event stream |
| Run | One `run` request: a `run_id` (uuid) and a `generation` (u64, monotonic per UI) |
| Value | What a node port produces: JSON (scalars, settings, curves) or a blob named by a handle |
| Handle | The content identity of an immutable blob: `md5:nd1:<hex>` (section 4.1) |
| DataRef | A handle plus typed metadata plus zero or more locators. The only way a message names bytes |
| Locator | One way to reach a blob's bytes: commonkit key, path, in-process memory slot, URL, inline |
| Rendition | An engine-made derivative the UI can draw without understanding the value (rgba8, peaks, excerpt…) |
| Display | What the viewer shows for one (node, port, index): summary, renditions, stats, full DataRef |
| Memo key | The identity of a computation (node kind, version, salt, params, inputs). Section 14 |

## 3. Encoding rules

1. **Canonical encoding is JSON** (UTF-8). Every message is valid JSON of at most `limits.max_frame` bytes (default 16 MiB). A msgpack codec is reserved (`codec.msgpack`) and not built.
2. **Every top-level message carries `"v"`**, the protocol major version. Protocol 2 messages carry `"v": 2`. Today's messages are protocol 1 (section 17).
3. **Unions are internally tagged** with a `kind` field (`#[serde(tag = "kind")]`). No untagged unions, so every union maps to a protobuf `oneof` if a gRPC binding is ever added.
4. **Map keys and ids are strings.** Counters (`seq`, `generation`, sizes) are unsigned 64-bit integers. `ts` is f64 seconds since the Unix epoch (kept from protocol 1).
5. **Readers ignore unknown fields and unknown event kinds.** Every field is optional on read unless this file says MUST; absent and `null` mean the default. Writers MUST NOT rely on a reader rejecting anything.
6. **Opaque payloads are explicit**: a field named `x` (an object keyed by pack id, for example `x.vi`) or `ext`. Pack data is never flattened into core structs.
7. **No bytes in messages**, except an `inline` locator at or under `limits.max_inline` (default 64 KiB; 4 KiB inside status events, except a `display` on a `preview` record, which has its own `limits.max_display_inline`, default 64 KiB on local bindings and 16 KiB remotely). Arrays never travel in messages, however small (section 4.4). Encoded remote thumbs of 4 KiB or less are the one image payload allowed inline (section 13.3).
8. **Every message from the other side is hostile input** (section 18). Parsing never panics; it yields a typed error or a default.

## 4. Handles and data references

### 4.1 Handle grammar

```text
handle   = algo ":" recipe ":" digest
algo     = "md5" / "b3"
recipe   = "nd1" / "raw"
digest   = 32LOWHEX   ; when algo = "md5"
         / 64LOWHEX   ; when algo = "b3"
```

- The sets of algorithms and recipes are closed. An unknown tag, wrong length or uppercase hex is a parse error, and the reference is treated as absent.
- **Decision (reconciles notes 02, 03, 04 and 06):** handles carry a recipe. Note 02 proposed `md5:<hex>` of stored bytes and note 06 assumed the same; that would rename every array the research software supporting www.virtimp.com publishes today, so `nd1` keeps today's digest and `raw` covers files.

### 4.2 Digest recipes

Both engines MUST produce the same handle for the same value. Shared test vectors live in `crates/proto/fixtures/handles.json`, checked by Rust and pytest.

**`nd1`** (exactly today's `publish.digest` in `python/src/research_engine/publish.py:26` and `research_graph/src/publish.rs:44`, so existing `impc` commonkit keys stay valid). The digest is computed over the concatenation of:

1. `dtype.str` as ASCII: byte-order character, kind, item size (`<f4`, `|u1`, `<i2`). Big-endian arrays are byte-swapped to little-endian before hashing **and the hashed string is the normalised one** (`>f4` hashes as `<f4`; new, pinned by a test vector). Object and structured dtypes are refused (their bytes are pointers or layout-dependent); the value is then `opaque`, never a blob.
2. Python's tuple repr of the shape as ASCII: `()`, `(5,)`, `(812, 808, 3)` (`", "` separator; trailing comma only for one element).
3. The C-order element bytes.

**`raw`**: the bytes as stored, nothing else. This is alexandria's item digest (`alexandria/storage/cas.py`). Text is its UTF-8 bytes. JSON meant to dedupe is written in the typed canonical form of section 14.1.

A `.npy` file's own bytes are never an array's identity (numpy pads headers differently across versions).

### 4.3 Algorithm choice

- **MD5 is the default** for every handle a session creates, for compatibility with commonkit's key slot, the research software's checkpoint store, the research software's published keys and alexandria's CAS.
- **BLAKE3 (`b3`) is REQUIRED for uploads into a namespace shared by more than one client** (a multi-tenant remote store), because MD5 chosen-prefix collisions are cheap. A single-tenant remote (the maintainer's a remote server) MAY accept MD5 by configuration.
- A server keeps an alias index `md5 → b3` built only from bytes it hashed itself, so a client can ask for an alexandria item by its MD5 and get a `b3` handle back (`blob.has` reply `aliases`).
- An engine declares the algorithms it accepts in `hello` (`data.algos`, first preferred). The UI uses the first it also supports.

### 4.4 DataRef

```json
{"handle": "md5:nd1:9e107d9d372bb6826bd81d3542a419d6",
 "meta": {"media": "application/x-npy", "size": 7873536,
          "form": {"kind": "array", "dtype": "<f4", "shape": [812, 808, 3],
                   "space": "Lab", "names": ["L", "a", "b"], "table": false}},
 "at": [{"kind": "commonkit", "key": "impc:v1:plumarr:9e107d9d372bb6826bd81d3542a419d6:c"},
        {"kind": "path", "path": "/Users/me/Library/Caches/commonkit/store/plumarr/9e/9e107d9d372bb6826bd81d3542a419d6.c.npy"},
        {"kind": "url", "url": "https://engine.example:7311/v1/blobs/md5/nd1/9e107d9d372bb6826bd81d3542a419d6"}]}
```

```rust
pub struct DataRef { pub handle: Handle, pub meta: BlobMeta, pub at: Vec<Locator> }

pub struct BlobMeta { pub media: String, pub size: u64, pub form: Form, pub aliases: Vec<Handle> }

#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Form {
    Array  { dtype: String, shape: Vec<u64>, space: Option<String>, names: Vec<String>, table: bool },
    Text   { lines: u64, encoding: String },
    Json,
    Audio  { sample_rate: u32, channels: u16, frames: u64, dtype: String },   // stored (frames, channels)
    Vector { width: f64, height: f64 },                                       // SVG and similar
    Image  { width: u32, height: u32 },                                       // an encoded source file
    Mesh   { vertices: u64, faces: u64 },
    Doc    { app: String },                                                   // an artcraft document
    Table  { rows: u64, columns: Vec<Column> },                               // JSON {"columns": [{name, type, values}]}
    Bytes,
}

#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Locator {
    Commonkit { key: String },                   // resolve under the READER's store root
    Path      { path: PathBuf, offset: u64 },    // same host only (section 5.4); offset into a .npy payload
    Mem       { slot: u64 },                     // in-process binding only: Arc<[u8]> in the host's table
    Url       { url: String, expires: Option<f64> },
    Inline    { b64: String },                   // size <= limits.max_inline
}
```

Rules:

- **The handle is identity; locators are hints.** Two DataRefs with one handle are the same value whatever their locators say. The UI caches textures, compares and pins by handle.
- **Locators are in the engine's order of preference.** The reader takes the first it can use, in this resolution order of cost: `mem`, `commonkit`, `path`, the reader's own durable CAS by handle, `url`, then a derivative request (section 10.5).
- `commonkit` and `path` locators are honoured only when the session's same-host proof passed (section 5.4), and only under the store root, the durable CAS root or the engine's declared work root (canonicalised, no symlink escape). Otherwise they are ignored.
- A `path` locator is today's `ArrayRef.path`; a `commonkit` locator is today's `ArrayRef.key`. Protocol 1 `{key, path}` pairs map one to one (section 17).
- Commonkit stores only `.npy` today. Non-array blobs are stored as 1-d `|u1` arrays, with `Path.offset` pointing at the payload, until commonkit gains a raw segment kind (plan, source proposals). **Such a wrapper segment is keyed by the `raw` digest of its payload**, and its DataRef handle is `md5:raw:…`, never `md5:nd1:…`, so a blob has one identity.
- **Compressed raw blobs.** A raw blob of at least 64 KiB that deflate (zlib format) makes at least 10% smaller, judged on its first 64 KiB before the whole is compressed, is stored compressed under form `plumz` (`plum:v1:plumz:<raw digest>:c`), keyed by the digest of the **uncompressed** bytes: the handle is unchanged, and a reader inflates (at most 128 MiB) and checks the bytes against it, so a damaged stream is a miss (section 4.6). Its DataRef carries a `commonkit` locator only, never a `path`, because a path reader expects the bytes as they are; a reader resolves it through its store module. Arrays (`plumarr`), which a reader may map in place, are never compressed. A blob already stored (either form) is only touched when published again. Both SDKs read what the other writes: Rust with `flate2` on `miniz_oxide`, Python with the standard library's `zlib`.
- **Only engines write commonkit.** The UI writes what it downloads or derives into its own durable CAS (section 13.2), which it maps by path exactly as it maps commonkit.

### 4.5 Values on ports

A port value in a reply, snapshot or event is one of:

```json
{"kind": "blob", "ref": <DataRef>}
{"kind": "list", "items": [<Value>, …]}
{"kind": "json", "value": <any JSON: number, bool, string, settings, curves>}
{"kind": "opaque", "type": "STYLE", "summary": "style 'pencil', 14 settings"}
```

Protocol 1's `{array: ArrayRef, meta}`, `{list: […]}` and `{type}` map to `blob`, `list` and `opaque`.

### 4.6 A missing blob is a state

A locator that no longer resolves (reaped commonkit segment, expired URL, 404) is **never an error** on either side. The reader reports a miss (`Ok(None)`), the UI greys the value, marks the node stale and asks for a rerun **once**, as `viewer.rs` does today (F58). An engine whose memo record points at a missing blob treats it as a memo miss and recomputes.

## 5. Sessions

### 5.1 Lifecycle

```text
connect (binding-specific)
  → hello            (UI → engine)
  → hello reply      (engine → UI)
  → session.confirm  (UI → engine; carries the same-host verdicts)
  → schema           (UI → engine; may answer {"unchanged": true})
  → events flow; runs, peeks, blobs, ops …
  → shutdown | disconnect → (resume with hello{session, since})
```

A UI MUST NOT send `run` before `hello` succeeded and the catalogue is loaded. Editing never waits for an engine (F1).

### 5.2 hello

UI → engine:

```json
{"v": 2, "op": "hello",
 "protocol": {"min": 2, "max": 2},
 "client": {"name": "research", "version": "0.9.0", "base": "plumcraft 0.1.0"},
 "session": null, "since": null,
 "caps": ["events.push", "events.poll", "events.resume", "status.queued", "status.progress",
          "live.snapshot", "live.append", "display.stream", "run.peek", "run.watch",
          "data.shm", "data.path", "data.cas", "data.mem", "hash.b3", "codec.json"],
 "host": {"id": "<IOPlatformUUID or /etc/machine-id>", "boot": "<boot id>",
          "stores": ["/Users/me/Library/Caches/commonkit/store"]},
 "proof": {"path": "/Users/me/Library/Caches/commonkit/store/.hello/c-5f1e….nonce", "nonce": "5f1e…"},
 "token": null}
```

Engine → UI:

```json
{"v": 2, "protocol": 2, "session": "s-7f3a…",
 "engine": {"id": "research", "name": "research", "version": "0.1.0",
            "components": {"research": "1a2b…", "yapocis": "2909778"},
            "salt": "py:research+1a2b3c…", "schema_digest": "3b8c…"},
 "caps": ["events.push", "events.poll", "events.resume", "run.peek", "run.watch", "run.plan",
          "display.stream", "status.progress", "data.shm", "data.path", "memo.persist",
          "output.save", "editor.curves", "editor.mask", "x.vi.export", "x.vi.deliver"],
 "data": {"algos": ["md5", "b3"], "stores": ["/Users/me/Library/Caches/commonkit/store"],
          "work_root": "/Users/me/Library/Caches/research/arrays"},
 "proof": {"path": "/Users/me/Library/Caches/commonkit/store/.hello/e-91c0….nonce", "nonce": "91c0…",
           "client_proof_read": true},
 "limits": {"max_frame": 16777216, "max_inline": 65536, "max_status_inline": 4096,
            "max_display_inline": 65536, "max_runs": 1, "max_blob": 4294967296,
            "ring_events": 4096, "ring_seconds": 60, "run_ttl_s": 600, "snapshot_runs": 16,
            "idle_cancel_s": 30, "inactivity_s": 30},
 "replay": null}
```

- `protocol` is the major version both sides chose: the highest in both ranges. No overlap → the engine replies error `version` (section 12.2) and the host shows `Unavailable{reason: "engine speaks protocol 3, this app 2"}`.
- **Capability strings, never minor versions** (section 5.3). Both sides list what they support; a feature is on when both list it.
- `engine.salt` is the engine's code digest (section 14). `schema_digest` lets the UI skip a catalogue it has cached.
- `token`: required on HTTP and socket bindings, loopback included (section 16). Never logged.
- **A session owns its runs.** Events are scoped per session: a session never sees another session's run events. Only the owning session, or a session whose token carries the `admin` capability, may `run.cancel`, `run.watch` or `run.peek` a run. A run whose session has had no subscriber and no `events.poll` or `run.get` for `limits.idle_cancel_s` (default 30 s) is cancelled, unless it was sent with `detach: true`; a detached run lives for `run_ttl_s`. Generations are per session.
- **Resume:** a client reconnecting sends its `session` and the last envelope `seq` it applied as `since`. If the ring buffer still covers `since`, the reply has `"replay": "ok"` and the engine replays the missed events, then continues live. Otherwise `"replay": "gap"`, the engine emits `resync` (section 9.2) and the client calls `run.get` for each live run.

### 5.3 Capabilities

| Capability | Who lists it | Meaning when both list it | If absent |
|---|---|---|---|
| `events.push` | both | Engine pushes events (callback, notification, SSE, AMQP topic) | Client long-polls `events.poll` |
| `events.poll` | both | `events.poll{since, wait_ms}` works | — (MUST for every engine) |
| `events.resume` | both | Ring buffer replay by `since` | Gap → `resync` → `run.get` |
| `run.plan` | engine | `run.plan` answers order, plan keys and cached states before running | UI marks all `queued` optimistically, as today |
| `run.peek` | engine | `run.peek` returns a display without a new run | UI reruns with the memo, as today (F49) |
| `run.watch` | engine | The watch list can change mid-run | Applied at the next run |
| `run.multi` | engine | Several runs may execute concurrently (`limits.max_runs`) | Runs execute serially; UI supersedes anyway |
| `graph.put` | engine | Graphs may be sent by handle | Graph sent inline on every run, as today |
| `status.queued` | both | Engine emits `queued` | UI marks `queued` locally |
| `status.progress` | both | Engine emits `progress` with `fraction` and optional `stage` | Bar stays indeterminate |
| `display.stream` | both | Watched displays ride `preview` records the moment their node finishes | Viewer updates from the reply only (today) |
| `live.snapshot` | both | `node.live` snapshot partials for watched values | — |
| `live.append` | both | `node.live` append streams (text tokens, audio blocks, table rows) | Value appears at `done` |
| `data.shm` | both | Engine `commonkit`/`path` locators are readable by the UI (proof passed) | `url`/`inline` locators only |
| `data.path` | both | The engine can read paths the UI names (proof passed) | Sources are uploaded and named by handle |
| `data.cas` | engine | `blob.has`, `blob.put`, `blob.get` work | Remote use impossible; same-host only |
| `data.mem` | both | In-process `mem` locators | Files, as today |
| `blob.derive` | engine | `blob.derive` makes thumbs, previews, tiles, regions, peaks… on request | UI derives locally from the full blob, or shows the summary |
| `blob.range` | engine | Range reads of blobs | Whole-blob download |
| `memo.lookup` | engine | `memo.lookup` by key | — |
| `memo.persist` | engine | Records of slow `machine`-scope nodes (and `cache: always` ones) survive restarts in the engine's store (section 14.5). The SDKs persist but don't advertise it yet; no UI behaviour depends on it | In-memory memo only |
| `run.stats` | engine | `RunReply.stats`: what the run's caches did (section 14.6) | Client counts node states |
| `memo.audit` | engine | `memo.audit{sample}` recomputes a sample of memo hits and reports mismatches | — |
| `admin` | engine (per token) | The token may cancel and watch other sessions' runs | Owner only |
| `hash.b3` | both | BLAKE3 handles accepted | MD5 only |
| `output.save` | engine | `output.save` writes or returns rendered outputs (formats in schema) | Save hidden |
| `graph.release` | engine | `RunRequest.scope` names the open graph's life, and `graph.release{scope}` frees what the engine kept for it (scratch files) when that life ends. A run goes on being able to use a scope released, as a new one | Kept until the engine stops |
| `editor.curves` / `editor.mask` | engine | `NodeReport.editor.curves` / `.mask` payloads | The editor opens without engine data |
| `x.<pack>.<op>` | engine | A pack operation exists (`x.vi.export`, `x.vi.deliver`) | The pack's UI is hidden with a reason |
| `codec.msgpack` | both | Reserved | JSON |

Unknown capability strings are ignored. Commands' `enabled` checks read capabilities, never engine names (replaces `plugin_export`, F6, F80).

### 5.4 Same-host proof

Hostnames lie (containers, VMs, SSH tunnels to `localhost`). Each direction is proved separately; **decision** (notes 02 and 03 each proposed one direction):

1. **UI reads engine files (`data.shm`).** The engine writes a nonce file under a store root the client listed and returns its path and nonce. The client reads it. A match turns `data.shm` on for the session.
2. **Engine reads UI files (`data.path`).** The client writes a nonce file under its store root and sends its path and nonce in `hello`. The engine reads it and reports `client_proof_read`. True turns `data.path` on.
3. The client sends `session.confirm {"shm": true|false, "path": true|false}`. Thereafter the engine emits `commonkit`/`path` locators first when `shm` is true, and `url`/`inline` otherwise; it accepts path sources only when `path` is true.

Nonce files are 32 random bytes in hex, mode 0600, in `<store>/.hello/`, deleted after 60 s. The in-process binding sets both true without files. A VM or Docker engine on the same Mac fails the proof and runs in remote mode, which is correct.

### 5.5 Session limits and liveness

- `ping` (either direction) answers `{"v": 2, "pong": <ts>}` within 2 s. The engine answers it on a thread that a running graph never blocks; a missed ping during a run marks the engine `busy`, not down.
- **Off the run path (normative).** `ping`, `run.cancel`, `run.get`, `events.poll`, `run.peek`, `run.watch` and `blob.has` MUST be answered by a thread a running graph never blocks, reading a lock-protected run table (snapshots, append buffers, tombstones, ownership). `run.get` MUST answer within 200 ms during a run. Section 16 says how each binding meets this.
- The host checks liveness every 3 s: AMQP by consumer counts (today, F3); socket by child alive plus `ping`; HTTP by `GET /v1/health`.
- `shutdown` (UI → engine) asks the engine to stop: 2 s grace, then the host escalates (section 16).

## 6. Operations

### 6.1 Summary

| Op | Params | Result | Required | Timeout |
|---|---|---|---|---|
| `hello` | section 5.2 | section 5.2 | MUST | 10 s |
| `session.confirm` | `{shm, path}` | `{}` | MUST | 10 s |
| `ping` | `{}` | `{pong}` | MUST | 2 s |
| `shutdown` | `{}` | — (notification) | SHOULD | — |
| `schema` | `{digest?}` | `SchemaReply` or `{unchanged: true}` | MUST | 30 s |
| `graph.put` | `{graph}` | `{handle}` | `graph.put` | 30 s |
| `run.plan` | `RunRequest` | `PlanReply` | `run.plan` | 30 s |
| `run` | `RunRequest` | `{accepted: true, run_id, queued_behind}` immediately; the result is the `run.reply` event | MUST | ack 10 s; then inactivity `limits.inactivity_s` (section 12.3); 300 s total only for protocol 1 |
| `run.cancel` | `{run_id}` | `{}` (idempotent) | MUST | out of band |
| `run.watch` | `{run_id, watch: [WatchRequest]}` | `{}` | `run.watch` | 10 s |
| `run.get` | `{run_id}` | `RunSnapshot` | MUST | 10 s |
| `run.peek` | `PeekRequest` | `PeekReply` | `run.peek` | 30 s |
| `events.poll` | `{since, wait_ms, run_id?}` | `{events: [Event], last_seq, gap: bool}` | MUST | `wait_ms` + 5 s |
| `events.subscribe` | `{since, run_id?}` | stream (binding-specific) | `events.push` | — |
| `blob.has` | `{handles: [≤10000]}` | `{have, missing, aliases, lease_s}` | `data.cas` | 30 s |
| `blob.put` | handle + bytes (binding-specific) | `{ref: DataRef}` | `data.cas` | size-based |
| `blob.get` | handle (+ range) | bytes (binding-specific) | `data.cas` | size-based |
| `blob.get_many` | `{handles: [≤256]}` | multipart bytes (HTTP) | `data.cas` | size-based |
| `memo.audit` | `{sample: u32}` | `{checked, mismatches: [memo key]}` | `memo.audit` | 300 s |
| `blob.derive` | `{handle, spec}` | `{ref: DataRef}` or inline readout | `blob.derive` | 30 s |
| `memo.lookup` | `{keys: [memo key]}` | `{records: [MemoRecord]}` | `memo.lookup` | 30 s |
| `output.save` | `SaveRequest` | `SaveReply` | `output.save` | 300 s |
| `graph.release` | `{scope}` | `{}` (idempotent) | `graph.release` | 10 s |
| `x.<pack>.<op>` | pack-defined | pack-defined | the cap | 300 s |

On a request/response binding every op is a JSON-RPC 2.0 method with these names. On AMQP the op is the `op` field of the body, as today (section 16.2).

### 6.2 Correlation

Every request has an id (JSON-RPC `id`, AMQP `correlation_id`, in-process a host counter). Every reply echoes it. Run-scoped events carry `run_id` and `generation`. Op-scoped `log` events MAY carry `op_id` (the request id), so a long `x.vi.deliver` can report what it is doing.

## 7. The catalogue

### 7.1 SchemaReply

Protocol 2's catalogue is protocol 1's `SchemaReply` (`research_graph-schema` `lib.rs:334`) with the generic fields promoted and the VI-shaped fields moved under `x`. Fields:

| Field | Type | Notes |
|---|---|---|
| `v` | 2 | |
| `engine` | `EngineInfo` | `id, name, version, components{}, salt, schema_digest`. Replaces the `research` field and `plugin_export` (now caps) |
| `wire_types` | `[WireType]` | Section 7.2 |
| `compat` | `[{from, to}]` | Extra type conversions. Defaults below |
| `families` | `[{id, display, description}]` | Palette order, as today |
| `nodes` | `[NodeDef]` | Section 7.3 |
| `expansions` | `{command_id: Expansion}` | JSON the catalogue carries as it came; its form is section 7.5 (F24) |
| `templates` | `{name: Graph}` | Today's `style_graphs` (F25). Protocol 1 `style_graphs` is read as `templates` |
| `save_formats` | `[{id, label, extensions, options, media}]` | For `output.save` (PNG, JPEG, TIFF, WAV, SVG…). `options` are ports: what `SaveRequest.options` may hold, checked by the UI. `media`: the kinds of value it writes, empty for any; the viewer offers Save only for those. An extension is given without its dot; the first format's first extension is what a bare name gets. An engine with the `output.save` capability lists at least one |
| `texts` | `{key: string}` | Engine-supplied UI strings that core used to hard-code: `blank_value` ("use the research software's fallback"), `result_prefix` ("Result:"), `engine_notice` (something the person should know while the engine is connected, such as having no GPU: the editor's banner shows it), `export_mark` ("checkpoint") and its hover text |
| `x` | `{pack: any}` | Pack data: `x.vi` holds anything VI-only |

### 7.2 Wire types and compatibility

```json
{"id": "ARRAY", "color": "#8a5cc2", "media": "ndarray",
 "views": ["auto", "color", "stretch", "clip", "heat", "labels", "angle"],
 "view_params": {"angle": [{"name": "turn", "type": "FLOAT", "default": 1.0}]},
 "default_view": "auto", "thumb": true}
{"id": "LAYER", "color": "#c47ad6", "media": "composite",
 "fields": {"array": "ARRAY", "blend": "ENUM", "opacity": "FLOAT", "label": "STRING"}}
{"id": "AUDIO", "media": "audio"}
{"id": "SETTINGS", "media": "tree", "bundle": true}
```

- **ARRAY is a core type** (maintainer, 2026-10-29: "Make arrays first class"; `plumcraft_proto::catalogue::core_types::array()`, the Python SDK's `Array`): every engine that has arrays declares exactly the wire type above (colour `#8a5cc2`, the seven views, `angle`'s `turn`), so an array crosses from one engine to another unchanged. An array value is a `Form::Array` blob: dtype, shape, an optional colour space (`space`, absent for plain numbers: no space is ever assumed but by the `auto` view, which shows an unnamed three-channel stack as RGB), channel or column `names`, and `table` for an (N, k) table. Engines make its node thumbnail (the `auto` view, 160 px) and, for a watched array, the renditions of the view asked for (role `view`, captioned per channel, at most 4, `WatchRequest.px` a side) and per-channel `stats` (exact finite min and max, NaN count, a 256-bin histogram over each channel's own range): `crates/engine/src/arrays.rs`, `python/src/plumcraft_engine/arrays.py`. The editor's `run.view {view, params}` asks for another view.
- **SVG is a core type** (maintainer, 2026-10-30: "Make SVG first class"; `plumcraft_proto::catalogue::core_types::svg()`, the Python SDK's `Svg`): every engine that has SVG values declares exactly `{"id": "SVG", "media": "vector", "color": "#16a34a", "thumb": true}`, so an SVG crosses from one engine to another unchanged (the coordinator lets a core type cross any border). An SVG value is an SVG document, UTF-8, as an `md5:raw:` blob of media `image/svg+xml` with `Form::Vector {width, height}` (its size in user units, 0 when unknown). **The editor draws SVG itself** (`plumcraft-svg`: usvg and resvg, no external files, text with the system's fonts), in the Watched panel, pop-outs and every preview, so an engine needn't send a picture of it; an engine may still send a node thumbnail, and the Rust SDK's `Blob::svg` makes one (160 px). The SDKs make SVG values with `Blob::svg` (Rust; `Engine::with_svg` declares the type) and `svg()` (Python); both check that the bytes are an SVG document (`config` otherwise).
- **TEXT, TABLE, IMAGE, AUDIO and JSON are core types** (maintainer, 2026-10-30, decision G3 in `docs/research/17-engine-generator.md`: "yes core data types", "Core audio, please", and "PDF no; JSON yes"), like ARRAY and SVG: every engine that has one declares exactly `core_types::text()`, `table()`, `image()`, `audio()` or `json()` (the Python SDK's `Text`, `Table`, `Image`, `Audio`, `Json`), and a value crosses from one engine to another unchanged. **The core types and their bytes:**

| Type | Wire type | A value | Bytes when it is a blob |
|---|---|---|---|
| `ARRAY` | `ndarray`, `#8a5cc2`, thumb, views | numbers of a numpy dtype, any shape | `md5:nd1:`, `Form::Array {dtype, shape, space?, names?, table?}`, media `application/x-npy` |
| `SVG` | `vector`, `#16a34a`, thumb | an SVG document | `md5:raw:` of UTF-8, `Form::Vector {width, height}`, media `image/svg+xml` |
| `TEXT` | `text`, `#c9a227`, thumb | Unicode text: a JSON string inline | `md5:raw:` of UTF-8, `Form::Text {lines, encoding: "utf-8"}`, media `text/plain; charset=utf-8` |
| `TABLE` | `table`, `#4f86c6` | columnar JSON `{"columns": [{"name", "type", "values": [...]}]}`, every column as long as the first: inline | `md5:raw:` of compact JSON, `Form::Table {rows, columns: [{name, type}]}`, media `application/json` |
| `IMAGE` | `image`, `#2563eb`, thumb | an encoded picture, always a blob | `md5:raw:` of the file's bytes, `Form::Image {width, height}`, media naming the format (`image/png`, `image/jpeg`, `image/tiff`, …) |
| `AUDIO` | `audio`, `#2fa39a`, thumb | PCM samples, always a blob | `md5:nd1:` of `<f4` (frames, channels), interleaved, `Form::Audio {sample_rate, channels, frames, dtype: "<f4"}`, media `application/x-npy` |
| `JSON` | `tree`, `#64748b` | any JSON value: inline | `md5:raw:` of compact JSON, `Form::Json`, media `application/json` |

  - A value is inline when it is small (`limits.max_inline`, 64 KiB by default) and the type allows it (TEXT, TABLE and JSON); otherwise a blob, published to the store or carried inline as `b64`.
  - **IMAGE:** every engine that reads IMAGE reads PNG, and an engine makes PNG unless a node is asked for another format; one that can't read the format it is given fails the node (`config`), never the run.
  - **JSON on screen:** a value stays JSON on the wire; the editor shows it as YAML by default, with the tree beside it (the maintainer: "format as Yaml for display"). A list of lists shows as a table first.
  - **PDF is not a core type** (the maintainer, 2026-10-30: "PDF no"): an engine declares its own, and it crosses only to an engine that declares it with the same media.
  - **Thumbnails:** TEXT an `excerpt`, AUDIO `peaks` (`(1, channels, bins, 2)` `<f4` min and max, inline, at most 4096 bytes), IMAGE and ARRAY `rgba8`; the editor draws SVG itself.
  - **The SDKs:** Rust `Data::Json` (TEXT, TABLE), `Data::Blob` (IMAGE, SVG), `Data::Array`, `Data::Audio`; Python `str`, `dict`, `Blob`, `ndarray` or `ArrayValue`, and `AudioValue` (`samples` (frames, channels) float32, `sample_rate`), new with AUDIO: a bound or wired AUDIO input arrives as one.
  - **Conformance:** `crates/coordinator/tests/merge.rs` holds every engine here to the exact declarations; `python/tests/test_audio.py` the Python SDK's.
- `media` is one of the closed set `image ndarray table scalar text audio vector mesh tree bytes composite sequence doc` (section 10.1). Unknown → `bytes` (the hex viewer plus the rgba8 fallback).
- `views` replaces the Rust `MODES` list (F51). The viewer offers exactly what the engine lists.
- `bundle: true` marks the parameter-bundle type (today `SETTINGS`, F43).
- **Compatibility:** a port accepts a wire of the same type, plus each `{from, to}` in `compat`. When an engine sends no `compat`, the base defaults are today's rules from `crates/graph/src/ports.rs:114`: `INT → FLOAT`, `STRING → ENUM`, `ENUM → STRING`. List typing is section 7.6.
- **A type for any value** is a convention, not a core rule: an engine built on the SDK that declares `ANY` (Zip, Unzip, Load and Write Files) sends `compat` with the base defaults plus `T → ANY` and `ANY → T` for every other type it declares, so the UI needs no wildcard. A display of an `ANY` port carries the media of its value.
- **Path modes** (`path.mode`): `open` (or unset), a file the node reads: keyed by its content and, for a remote engine, uploaded; `save`, a file it writes; `dir`, a folder; `files`, several files chosen together, one path per line (a dialog picks many; relative lines resolve against the workflow's folder one by one). Only `open` is a source.

### 7.3 NodeDef

Every protocol 1 field stays with its meaning (F8–F23): `id, display, category[], description, resource, inputs[], outputs[], flags, editor, family, icon`. Changes:

| Field | Protocol 2 | Protocol 1 mapping |
|---|---|---|
| `version` | u32; part of the memo key | absent → 1 |
| `memo` | `{scope, device_sensitive: bool, content: bool, persist: bool}`, scope one of `none`, `session`, `machine`, `global` (section 14.4); `content` asks for a content key (section 14.3); `persist` marks a node eligible for persistence (section 14.5) | absent → `machine`; `content` false; `persist` true for checkpoint-markable nodes |
| `editor` | `{kind, port, interpolant?}`, kind `curves`, `mask` or a pack kind; for `curves`, `interpolant` names the curve maths (VI: `monotone_cubic`) and the engine ships its sample fixture | `"curves"` → `{kind:"curves", port: <the CURVES input>, interpolant: "monotone_cubic"}`; `"mask"` → `{kind:"mask", port:"mask"}` |
| `export` | `{markable: bool, mark: "checkpoint", x: {…}}` opaque to core | `codegen{expr, imports, checkpointable}` → `export{markable: checkpointable, x.vi.codegen: {expr, imports}}` |
| `flags` | as today, plus `bundle_port: "<name>"` and `result: true` | `bundle_port` defaults to `"settings"` on VI nodes with a SETTINGS input; `result` set on `vi.op.result` |
| `x` | pack fields: `x.vi.code`, `x.vi.helpers`, `x.vi.picture` | Python's `code`, `helpers`, `picture` move here and pass through unchanged |
| `engines` | Optional. In a catalogue merged from several engines by a coordinator (`docs/research/15-multi-engine.md`), the ids of the engines that offer the kind, the one that runs it by default first; a kind several engines offer appears once, its definition from the first. A single engine leaves it out | absent |
| `converts` | Optional. One mark or a list of them (a craft's Open takes a picture, an SVG or a PDF), each `{from, to, input?, output?, lossless?, from_shape?, to_shape?}`: the node turns a value of type `from` into one of type `to` (`docs/research/20-convert.md`). `input` names the wire-only input that takes the `from` (default: the first of that type), `output` the output that gives the `to` (default: the first of that type); `lossless` says the inverse gives the value back. `from_shape` and `to_shape` say what an ARRAY end is, `image` (H, W[, C]) or `series` (1-D, or frames × channels): a path never gives an array of one shape to a step that wants the other, so Audio to Array never leads to Encode Image; an array whose shape nobody says goes either way. A mark that names a port the node lacks, a port of another type, `ANY`, or the same type twice is ignored, as is one that doesn't read; it never fails the catalogue. An optional field like `editor`: a UI that doesn't know it ignores it, so it needs no capability. Craft packs mark every export (document → file type) and every Open (picture, SVG or PDF → document), none lossless. The SDKs: `@node(converts=True, lossless=…)` or `converts=("WAV", "AUDIO")` in Python; `NodeDef.converts = vec![Converts::new(from, to, lossless)]` in Rust, with `.input(…)`, `.from_shape(…)` and `.to_shape(…)` | absent |

Ports keep `name, type, label, tooltip, default, min, max, step, decimals, choices, optional, advanced, list, socket, setting, variadic, options, path, external, disabled_choices, active_when, fills, shows, widget, soft_min, soft_max` (F11–F18). `fills` is `{choice: {port: value}}`: choosing sets those ports, which take no value of their own until set away from it; `shows` is `{choice: [port]}`, what an editor keeps in view for that choice. `widget` hints how to edit a value its type doesn't say: `color` (`r,g,b` in 0..1), `hexcolor` (`#rrggbb`), `multichoice` (comma-separated picks, from `choices` or the input the node feeds), `multiline` (a STRING of several lines, such as a query or a script: a monospace text box where Enter is a new line, committed by a click elsewhere or by a shortcut, which runs after the commit); unknown hints are ignored. `soft_min` and `soft_max` are the span a slider covers; a typed value may pass them, up to `min` and `max`. `setting{section, name}` is read as `param_ref{group, name}` (F13).

### 7.4 Rules on the catalogue

- The UI MUST NOT hard-code node ids, port names or wire types beyond the scalar types `FLOAT INT BOOL STRING ENUM`. What core used to hard-code (`vi.op.result`, "Result:", `settings`, `mask`) comes from `flags.result`, `texts.result_prefix`, `flags.bundle_port` and `editor.port`.
- **Conversions** (`docs/research/20-convert.md`). The editor reads every connected engine's `converts` marks as one graph of types, and finds paths through it: fewest steps, then every step lossless, then fewest changes of engine, at most 4 steps, never through a type twice. It uses them two ways, neither of which an engine sees:
  - **A wire the types refuse** offers the best path beside the error ("Insert WAV to Audio"). `wire.convert {from, to, kinds?}` places the path's nodes between the two ends, wired, as one undo step; `wire.convertOffers {from, to}` lists the paths. `plumcraft-editor check` names the fix for such a wire. A path with a step that isn't lossless is placed with a warning.
  - **The Convert node** (`core.convert`) is core's own: one input and one output, both `value`, of type `CONVERT`, which no engine declares. Each end takes its type from its wire: its input accepts what some path takes to every input its output feeds (any converter's input while the output is unwired), and the other way round; a Convert never feeds a Convert, and its wires take no slice. Wired at both ends, it keeps the best path's kinds in the workflow's node, `"convert": ["sc.export_wav", "audio.from_wav"]`, and keeps it until the types it is wired between no longer fit it, or `node.convert {node}` picks again; the engine of each step is left to the usual choice. A kept path whose kinds have gone is a wire problem, and the node runs the best path there is until it is picked again. Before a run or a save, the session replaces each Convert by its path's nodes (the last with the Convert's id, the others `<id>_convert<k>`) and puts what comes back on the Convert: the last step's output is its `value`, a failing step fails it with that step's error, and the other steps' records are dropped. A Convert with an end unwired, or no path, is left out with its wires. Its steps run with their options' defaults.
- **Value editors.** Core has no curve or mask editor, and no editor for any value it has no widget for. `editor` on a NodeDef names one: `kind` is a free string and `port` the input it edits. An application registers what draws a kind (`plumcraft_ui::value_editors::register`, or `AppConfig.editors`): its title, its button's label and help, a short summary of a value, and a factory for a `ValueEditor`. Then the parameter editor gives that port a button (a port of a type core doesn't know gets only the button and the summary; one it knows keeps its widget beside it), and the node's footer and menu offer it, unless the port is on a wire. `node.editor {node, open?}` opens and closes the window; a node that names no editor, or a port on a wire, is refused. With no registered kind nothing is offered and the port stays read-only, as for any type the UI doesn't know, so an engine that names a kind no application draws still works. An editor reads the port's value and `NodeReport.editor[kind]`, the last one a run sent, and makes its changes as commands by id (`node.set` or its own, `AppConfig.commands`, undoable through `Session::edit`), one command a gesture, so one gesture is one undo step. `editor.interpolant` and everything else about a kind belong to whoever draws it.
- **Pack UI.** A result window or command that core cannot provide generically is registered by the app's pack crate (result windows by `x.<pack>.<op>`, commands, texts).
- A catalogue reply replaces the catalogue and keeps the graph (F4). The UI MAY rewrite a catalogue before use (F7: `--demo` merges testkit nodes).
- An engine MAY emit `schema.changed` (section 9.2) when its catalogue changes (a model installed, a craft rebuilt); the UI refetches with its digest.

### 7.5 Expansions and templates

Unchanged from protocol 1: JSON the UI turns into graph fragments (`node.expand`, F24, including the delivered-plugin `group` block, F67) and graphs it opens (`templates`, F25, F77).

An expansion (`plumcraft_proto::expansion`, read by the UI, so hostile like any message):

| Field | Type | Notes |
|---|---|---|
| `command` | string, optional | The node type it is for; when present it must be the key |
| `nodes` | `{local id: {kind, values?, pos?, title?}}` | At least one, at most 4096. `pos` is relative to the command, within ±1,000,000 |
| `wires` | `[{from: {node, port}, to: {node, port}, slice?}]` | Between local ids, at most 65536 |
| `inputs` | `{command input: [[local id, port]]}` | What fed the command feeds these |
| `settings` | `{command input: [[local id, port]]}` | The command's value goes on each widget; a wire into it goes to each. A value from its bundle is copied in |
| `outputs` | `{command output: [local id, port]}` | The command's consumers read this. A wire from an output not listed is dropped and reported |
| `note` | string, optional | What it unrolls into, for people |
| `group` | `{promoted?: [{port, label, doc?, min?, max?}], outputs?: [{from, name}], checkpoints?: [local id]}`, optional | The group it came from, restored when expanded into a group (F67). Without it, a group gets the `settings` as parameters and the `outputs` as results. `checkpoints` are kept on the group as a file keeps them |

An expansion that doesn't fit the catalogue is refused whole; the graph doesn't change.

**A stored expansion** (`plumcraft_proto::expansion::StoredExpansion`, F67) is a group kept as a file, `<slug>.expansion.json`, so that a pack can offer it as a command. It isn't a message: which folder holds such files, and what runs them, are the pack's, and the catalogue carries the result as any other node and `expansions` entry.

| Field | Type | Notes |
|---|---|---|
| `format` | `"plumcraft.expansion"` | Anything else is refused |
| `version` | `1` | A later one is refused, saying so |
| `display` | string | What the palette calls it, 1 to 200 characters |
| `description` | string, optional | At most 8192 characters |
| `expansion` | an expansion, above | `command` is required: letters, digits, `.`, `_`, `-`, at most 128. So is one `outputs` entry at least |

The file is hostile input, in the sweep (`crates/proto/tests/hostile.rs`). Beyond an expansion's checks it is refused for: more than 16 MiB; a cycle; a fragment input fed twice (a wire and a mapping, or two of either); a port both an input and a setting; a port name empty, over 128 bytes or with a control character. The file declares no ports: a pack reads them off the fragment, so a file can't say what the fragment doesn't do.

### 7.6 Graph semantics every engine MUST match

The UI types graphs exactly as engines run them. These rules are normative for every executor (Python, `research_graph`, the Rust SDK); a shared corpus in `conformance/graphs/` (graph, expected `order`, states, provenance) checks them.

1. Topological order; cycles are a graph error (`kind: graph`).
2. **Lists** (F20, F61): a `list: true` port takes or makes whole lists. A list into a single-item port maps the node over the list. Lists zip along a shared axis and form a product over different axes; a length mismatch on one axis is a node error.
3. **Wire slices** (F21): Python slice or index syntax (`2`, `-1`, `1:4`, `::2`), parsed identically on both sides (`crates/proto/fixtures/slices.json`); part of the memo key and of stale reasons. A range keeps a list a list, an index takes one item; slicing a single value, an empty result or an index out of range is the node's `config` error. A wire between engines is sliced by the coordinator, since a binding replaces the wire.
4. **Variadic inputs** (F14) arrive as `{slot: value | absent}`: each wired slot (`c1`, `c2`, … from the template `c{n}`, or one of `names`) is an input under its own name; a variadic port with `min` ≥ 1 and no slot wired is a `config` error. (Rust executor; the Python one has no variadic nodes yet.)
5. **Parameter precedence** (F43): a wire into the port, then the incoming bundle on `bundle_port`, then the node's widget, then blank. Memo keys use resolved values, never provenance.
6. **Absent values** (F34): an absent input (an `ExecutionBlocker`, a missing channel) skips downstream nodes without an error.
7. **Errors as data**: a failing node is `error` with `{kind: config|op, message, type}`; downstream nodes are `skipped`.
8. **UI-only fields are ignored** (F76): positions, frames, nets, `via` bends, views, titles. Engines MUST NOT fail on them.
8a. **A node's `cache`** (`"auto"`, the default when absent; `"always"`; `"never"`; anything else is `auto`) says whether the engine persists its result (section 14.5). It never changes what the node computes and is not part of its memo key.
9. **Lineage** (F63): every output carries, per item, what made it. Engines that declare it fill `Display.recipe`, sheet labels and save labels from it.
10. **Bindings** (`RunRequest.inputs`, decision D14): `{"node.port": PortValue}` replaces the wire or widget of that port, and a wire into a bound port is dropped, so what fed it is neither waited for nor computed. A variadic slot (`c2`) can be bound. A key naming a node the graph lacks is ignored, and so is a port the node doesn't have. A `json` value arrives as is; a `blob` is read by its handle from the engine's store, else from its own locators: `inline` always, and an engine MUST read a `path` inside its store or inside a run's `trusted_roots`, since another engine may publish under keys of its own or, for an array too large for the store, only in its work root. `trusted_roots` comes with the request: a coordinator names the store and work roots of the same-host engines whose values cross into this run, and an engine never takes a folder from a value. Whatever is read is checked against the handle, so a blob without a handle is a `config` error and one the engine can't find is `missing_source`; by `form`, an array arrives as an array (with its space, names and table flag), audio as audio (in an SDK without audio values, its (frames, channels) float32 array), text as a string, json and tables parsed, anything else as encoded bytes; a `list` item by item. An `opaque` value is a `config` error. A bound input's memo identity is its content (section 14.2). How a coordinator uses this to join engines: `docs/research/15-multi-engine.md`.
11. **Held values** (capability `values.held`): a value that has no form to leave its engine in (a pack value, say hundreds of MB of signals) can still reach a later fragment of the same engine. `RunRequest.hold` lists `node.port`s; an engine with the capability keeps those values and reports each as `opaque` with `held: <id>`, and a bound input carrying that value back is the kept value, with `id` as its memo identity, so the same id MUST mean the same value. A coordinator asks only for wires between two fragments of one engine. When the run ends it sends `values.release {ids}`, which drops one count per id. Holds don't outlive the engine process: a held id the engine no longer has is `missing_source`. To any other engine an `opaque` value, held or not, can't be delivered, and the node is skipped saying so. An engine without the capability ignores `hold`.

## 8. Runs

### 8.1 RunRequest

```json
{"v": 2, "op": "run",
 "run_id": "0b5e…", "generation": 17, "priority": 9,
 "graph": {"kind": "inline", "graph": { …workflow JSON, as today… }},
 "targets": [],
 "watch": [{"node": "n7", "port": "value", "index": 0, "sheet": false,
            "view": {"id": "heat", "params": {}},
            "want": [{"kind": "rgba8"}, {"kind": "stats"}, {"kind": "full"}],
            "px": 1024}],
 "preview": {"node_px": 160, "inspect_px": 1024},
 "memo": "use",
 "inputs": {},
 "detach": false}
```

| Field | Meaning |
|---|---|
| `run_id`, `generation` | F28. Fresh uuid per request; generation monotonic per UI. Records and replies of older generations are ignored |
| `priority` | 0–9; runs 9, saves and ops 5 (today's AMQP priorities) |
| `graph` | `{"kind": "inline", "graph": …}` (today, F26, F76) or `{"kind": "handle", "handle": "md5:raw:…"}` when `graph.put` is on. An unknown handle → error `not_found`; the host resends inline |
| `targets` | F27. Empty means the graph's sinks. Watched nodes are added by the engine |
| `watch` | Replaces protocol 1 `inspect` (section 10.4). Protocol 1 `{node, port, index, mode, turn, sheet}` maps to `view.id = mode`, `view.params.turn = turn` and the media kind's default `want` |
| `preview` | Thumb and viewer sizes, as today |
| `memo` | `use` (default), `refresh` (recompute targets, keep upstream), `off`. The exactness and conformance gates always send `off` against a fresh store |
| `trusted_roots` | Folders besides its store where `path` locators may be read (section 7.6 rule 10); usually absent |
| `hold` | `["node.port"]`: values to keep for this run if they can't be sent (section 7.6 rule 11); usually absent |
| `inputs` | `{"node.port": PortValue}`: external bindings that replace a port's wire or widget (section 7.6 rule 10). For graphs that span engines (one engine's DataRef fed to another) and for clients that feed a node a value they have; usually `{}` |
| `detach` | When true, the run survives its session going idle, for `run_ttl_s` (section 5.2) |

Path-hinted widget values (F16, F75) are absolute paths when `data.path` is on, as today (`with_absolute_paths`). When it is off, the host rewrites each to a **source** before sending (section 13.2):

```json
{"source": {"handle": "md5:raw:d41d8cd98f00b204e9800998ecf8427e", "name": "test.jpg", "media": "image/jpeg"}}
```

The engine never sees a client path on a remote session.

### 8.2 The ack and supersede

`run` is acknowledged at once (`{accepted: true, run_id, queued_behind: n}`); the result arrives as a `run.reply` event and is also available from `run.get`. The UI supersedes (F29): starting a run while one is in flight sends `run.cancel` for the old one first. The host drops a superseded run that it has not yet sent instead of sending and cancelling it. With `run.multi` absent, the engine executes runs serially; the off-run-path ops (section 5.5) are still served at once (F36, F84).

A workflow node MAY carry an `engine` field naming an engine id, and a workflow MAY carry `engines: [id]`, the engines it prefers, in order. Engines ignore both (rule 8). A coordinator (`docs/research/15-multi-engine.md`), which is one engine to its client and runs several, reads them: a node runs on its `engine` when that engine offers its kind, else on the only engine that does, else on the first of `engines` that does, else on the first connected; it splits the graph into fragments, one request each, joined through `inputs` (section 7.6 rule 10).

### 8.3 run.plan (optional)

`run.plan` takes a RunRequest and returns, without running anything:

```json
{"v": 2, "run_id": "0b5e…", "order": ["n1", "n2", "n7"],
 "nodes": {"n1": {"plan_key": "md5:…", "cached": true}, "n2": {"plan_key": "md5:…", "cached": false}},
 "missing_sources": ["md5:raw:…"]}
```

An engine that plans as part of `run` SHOULD emit `queued` and `cached` records at plan time instead. Either way the UI's own stale tracking (F45) stays UI-side and advisory; the engine's memo decides (F72).

### 8.4 RunReply

`run.reply` is protocol 1's `RunReply` (`run.rs:273`) as a superset (F32, F33):

```json
{"v": 2, "run_id": "0b5e…", "generation": 17, "ok": true, "error": null,
 "order": ["n1", "n2", "n7"], "elapsed_ms": 812.4,
 "nodes": {"n7": {"state": "done", "ms": 412.0, "worker": "Plan@gpu-host:4182",
                  "error": null,
                  "provenance": {"sigma": "wire", "iterations": "bundle", "mode": "widget"},
                  "thumb": <Rendition>, "text": null,
                  "outputs": {"value": {"kind": "blob", "ref": <DataRef>}},
                  "editor": {"curves": { …CurveData… }},
                  "memo": {"plan_key": "md5:…", "content_key": "md5:…"}}},
 "watch": [<Display> | {"error": {"kind": "op", "message": "…"}}]}
```

- `ok` means every node is `done` or `cached`. `error` is a graph-level failure (`kind: graph|engine|cancelled`).
- `outputs` are filled for targets only (cost, F27).
- `editor` replaces protocol 1's `curve` and `mask` fields (F59, F60), keyed by editor kind. Its contents are between the engine and the kind's editor, and core keeps them without reading them: a run that sends none for a node (it failed, say) leaves the last. A picture in one is a `Rendition` holding a `DataRef`, not a file path, so an editor reads it through the connection as a thumbnail is read, from any engine however hosted.
- `watch` replaces protocol 1's `inspect`, in request order.
- **The reply is authoritative** (F40): the reducer fills every node without a record from it, so a lost event costs nothing.

### 8.5 run.get

`run.get{run_id}` returns a snapshot, mandatory on every engine. It is the polling path and the resync path.

```json
{"v": 2, "run_id": "0b5e…", "generation": 17, "phase": "running", "last_seq": 1043,
 "nodes": {"n2": {"state": "done", "ms": 31.0, "thumb": <Rendition>},
           "n7": {"state": "progress", "fraction": 0.42, "stage": "dabs 3/7"}},
 "watch": [<Display or null>],
 "partials": {"n9.text": {"offset": 5120, "partial": <DataRef>}},
 "reply": null}
```

`partials` gives, per (node, port) that is appending, the bytes so far and the next offset; engines keep append buffers until `done`, so a gap or resync is recovered from here before the value is complete. `run.get` is the resync path, not the polling path: a polling client calls `events.poll{since}` (section 9.6), which returns only new events, and calls `run.get` after `resync` or a gap.

`phase` is `accepted|running|finished|cancelled|failed`. `reply` is the RunReply once finished. Engines keep snapshots of the last `limits.snapshot_runs` runs, and every run for `limits.run_ttl_s` after it finishes, so an HTTP run survives a laptop sleep. An unknown run → `not_found`.

### 8.6 run.peek

`run.peek` opens any (node, port, index) of a run in flight or a finished run without a new run (new; generalises F49):

```json
{"v": 2, "op": "run.peek", "run_id": "0b5e…", "watch": <WatchRequest>}
```

Reply: `{"display": <Display>}` when the value exists (computed in this run or a memo hit); `{"pending": true}` when it will exist later (the engine adds it to the run's watch list, so its display arrives on the `preview` record); `{"missing": true}` when it is not in this run's targets or ancestors. Without `run.peek` the host falls back to today's behaviour: rerun with the viewer's requests and rely on the memo.

### 8.7 run.watch

`run.watch{run_id, watch}` replaces the watch list of a run in flight, so clicking a running node shows it at once. For each newly watched entry whose node has **already finished**, the engine MUST emit its Display at once (on a `preview` record for that node, re-sent with the display); entries for nodes not yet run get theirs when the node finishes. Without the capability the host keeps the new list for the next run.

## 9. Events and node status

### 9.1 Envelope

```json
{"v": 2, "seq": 1043, "ts": 1792170000.123, "kind": "node.status",
 "run_id": "0b5e…", "generation": 17, "body": { … }}
```

- `seq` is per session, strictly increasing, assigned in one place by the engine's emitter. It gives ordering of delivery and resume. On a binding where events originate from several publishers (AMQP fan-out under pipekit scatter/join, M9), the binding assigns `seq` on receipt and resume falls back to `run.get`. **Envelope `seq` is never used for dedup or for ordering within a node**; those use the record's `(publisher, publisher_seq)` (section 9.3), which survives redelivery.
- `run_id` and `generation` are present on run-scoped events.

### 9.2 Event kinds

| Kind | Body | Delivery class |
|---|---|---|
| `engine.state` | `EngineState` (section 12.4) | Ordered, never dropped |
| `schema.changed` | `{schema_digest}` | Ordered |
| `node.status` | `StatusRecord` (section 9.3) | `progress`: latest per node. `preview`: latest per node. All other states: ordered, never dropped |
| `node.live` | `LiveUpdate` (section 10.6) | `snapshot`: latest per (node, port). `append`: ordered, never dropped. `log`: drop oldest past 256 per run |
| `run.reply` | `RunReply` | Ordered, never dropped |
| `log` | `{level, logger, message, node_id?, op_id?}` | Drop oldest past 1024 |
| `resync` | `{run_id?, reason}` | Ordered; the client calls `run.get` |

Note 02's `node.progress`, `node.preview` and `node.chunk` are folded into these: progress and preview remain `node.status` states (so the reducer is unchanged) and chunks are `node.live` appends. **Decision**: one status record type keeps `StatusReducer` and both existing engines as they are.

### 9.3 StatusRecord

```json
{"node_id": "n7", "seq": 41, "publisher": "Step.gpu@gpu-host:4182#a1", "ts": 1792170000.123, "state": "progress",
 "fraction": 0.42, "stage": "dabs 3/7", "ms": null, "worker": "Step.gpu@gpu-host:4182",
 "error": null, "thumb": null, "outputs": null, "display": null}
```

| Field | When | Notes |
|---|---|---|
| `node_id`, `state`, `ts`, `seq` | always | `seq` here is the record's own counter, per publisher and run (`publisher_seq`; protocol 1's `seq`) |
| `publisher` | always in protocol 2 | A stable id of the emitting process instance (worker name, host, pid, start nonce). Protocol 1 records get the AMQP `pk-worker` header or `"engine"` |
| `fraction` | `progress` | 0..1 |
| `stage` | `progress` (optional) | Short text under the bar (new) |
| `ms` | `done`, `cached`, `error` | MUST on `done`; per-node timing (F42) |
| `worker` | optional | Who ran it |
| `error` | `error` | `{kind, message, type}` |
| `thumb` | `preview` | A `Rendition` (rgba8 at ≤ `node_px`) |
| `outputs` | `preview`, targets only | `{port: Value}` |
| `display` | `preview`, watched nodes, `display.stream` on | `[Display]`, one per watch entry for this node |

### 9.4 States

| State | Emitted by | Meaning | Terminal |
|---|---|---|---|
| `queued` | optional (`status.queued`) | Planned, waiting | no |
| `started` | MUST | About to compute | no |
| `progress` | optional (`status.progress`), at most every 100 ms per node | Fraction and stage | no |
| `preview` | MUST, the moment a node finishes | Thumb, outputs, watched displays | no (adds data, never changes a terminal state) |
| `done` | MUST | Computed; `ms` | yes |
| `cached` | MUST on a memo hit | Not computed | yes |
| `error` | MUST | Node failed | yes |
| `skipped` | MUST | Not run: an upstream error or an absent value | yes |
| `cancelled` | MUST | Not reached or interrupted by cancel | yes |

Every engine MUST emit `started`, `preview`, a terminal state and `ms` as they happen, which is what the research software gets from both engines today (F39). Every UI MUST accept all nine. The preview record for a node is sent **as soon as that node finishes**, before downstream nodes run (F47).

### 9.5 Reducer rules (normative, as `crates/bridge/src/status.rs` today)

1. State is keyed by `(run_id, node_id)`. Records of an older generation are ignored; a newer generation resets.
2. Within a node, records from one publisher are ordered by `publisher_seq`; records from different publishers are ordered by **state rank** (`queued` < `started` < `progress` < terminal), never by `ts`, because publishers' clocks differ. A node never moves back from a terminal state. (Revision 1 ordered by `(ts, seq)`, which dropped a lagging worker's `started` under fan-out.)
3. `preview` adds thumb, outputs and display whatever the order, without changing a terminal state.
4. The final reply is authoritative: it overwrites every node and fills nodes that had no record.
5. Duplicates (same `(publisher, publisher_seq)`, as after AMQP redelivery) are dropped.
6. The canvas maps states onto `queued|running|done|cached|error|skipped|cancelled` (`crates/engine/src/run.rs:41`).

Any binding MAY lose, duplicate or reorder `node.status` and `node.live` events; these rules make the display correct anyway once the reply arrives.

### 9.6 Delivery: push, long-poll, poll

| Mode | How | Who |
|---|---|---|
| Push | In-process callback with a wake; JSON-RPC notification `event`; SSE `event:`; AMQP topic `{system}.status`, `run.<correlation>` | Every binding that can push |
| Long-poll | `events.poll{since, wait_ms > 0}` returns at the first event after `since` or at `wait_ms` | Every request/response binding |
| Poll | `events.poll{since, wait_ms: 0}`; `run.get{run_id}` on resync | **Every engine, MUST** |

The UI feeds all three into the same reducer. The host switches to polling when push fails twice: `events.poll{since}` every 100 ms while a run is live, every 1 s when idle. It calls `run.get` only after `resync`, a gap, or an inactivity timeout, because a snapshot of a 300-node graph is 100 KB or more.

### 9.7 Backpressure

- **Engine side, per session:** conflatable events (the "latest per" classes in section 9.2) sit in slots keyed as stated; ordered events queue. If the ordered queue exceeds the ring (a stalled client), the engine drops the subscription, keeps the ring, and the client resyncs. **The executor never blocks on a slow UI.**
- **UI side:** the host's event queue keeps the same conflation map (replacing today's unbounded `mpsc`). **Only the conflated slots are bounded** (one per key). The ordered classes are never dropped and the producer never blocks: if the ordered queue passes a high-water mark (default 65,536 events), the host discards it, emits one local `resync` per live run and calls `run.get`, whose snapshot plus the authoritative reply restore the state. The UI drains the queue once per frame, as today, and the wake callback repaints.
- **Bulk never shares a channel with events.** Events carry DataRefs only, so a 100 MB array can't delay a status record.

## 10. Displays, renditions and data in flight

### 10.1 Media kinds

| Kind | Holds | Native viewer (plan section 7) |
|---|---|---|
| `image` | (H, W[, C]) for display | Image (today's viewer) |
| `ndarray` | Any dtype and shape; `space`, `names`, `table` | Dispatch: 2-D/3-D → Image; (N, k) table → Table/plot; 1-D → Plot; N-D → slice picker |
| `table` | Named typed columns | Table grid and plot |
| `scalar` | Number, bool, enum, short string | Scalar |
| `text` | UTF-8 text, optional spans | Text |
| `audio` | (frames, channels) PCM + rate | Audio |
| `vector` | SVG | SVG |
| `mesh` | Vertices, faces, or GLB | Mesh |
| `tree` | JSON-like structure | Tree |
| `bytes` | Opaque | Hex |
| `composite` | Named fields of other kinds (LAYER, STYLE) | Composite |
| `sequence` | Ordered items of one kind with a time base | Sheet + scrubber |
| `doc` | An application document (artcraft) | Engine renditions + tree |

Lists are orthogonal: any kind can arrive as a list; the viewer host gives every kind the item stepper and the sheet.

**Every value can be looked at.** For every wire type an engine declares, a value of it MUST reach a viewer other than the hex dump: a `media` other than `bytes`, or an `rgba8` rendition. A type with no useful picture says so as text or a tree. `bytes` is the fallback for a value an engine can't describe, never a type's only view.

### 10.2 Display

```json
{"media": "ndarray", "type": "ARRAY", "node": "n7", "port": "value",
 "items": 7, "index": 2,
 "summary": "array Lab 812×808×3 float32",
 "full": <DataRef>,
 "view": {"id": "heat", "params": {}},
 "renditions": [{"kind": "rgba8", "role": "view", "caption": "L", "w": 1024, "h": 1019, "ref": <DataRef>},
                {"kind": "rgba8", "role": "view", "caption": "a", "w": 1024, "h": 1019, "ref": <DataRef>}],
 "stats": {"channels": [{"name": "L", "min": 0.0, "max": 100.0, "nan": 0,
                         "histogram": {"bins": 256, "lo": 0.0, "hi": 100.0, "counts": [ …256 u64… ]}}]},
 "value": null,
 "recipe": {"style": "pencil", "params": {…}, "settings": {…}, "graph": {…}},
 "sheet": [{"label": "dim 150", "summary": "…", "thumb": <Rendition>}],
 "live": {"provisional": false, "seq": 0},
 "error": null}
```

- `full` is the value's DataRef: the probe, |A−B|, tables, playback and 1:1 zoom read it lazily (F52–F54, F74).
- A viewer reads numbers from `full`, never from a rendition. When `full` can't be read where the UI is, it says so; it doesn't guess from 8-bit pixels. A reader MAY map a same-host array too large to read rather than copy it; it then checks the file's shape and size against the DataRef, since checking the handle would read the whole of it.
- `renditions` (≤ 4 views for arrays, as `MAX_PREVIEWS`) are drawn directly. Every engine MAY fall back to one `rgba8` rendition for any kind, which gets zoom, sheet and pixel compare for free.
- `stats` carries histograms inline, as today (F50). `value` carries a scalar.
- `recipe` is REQUIRED from engines that track lineage (F56; the local engine must add it, plan G2).
- `sheet` (≤ 64 items at 256 px) when the watch request asked for it (F55).
- Inline budgets: a Display on a `preview` record may be up to `limits.max_display_inline` (64 KiB local, 16 KiB remote), which holds a Lab display with four 256-bin histograms and four renditions (about 6–10 KB, to be measured with the Lab fixture in G6); other status-event payloads ≤ 4 KiB; ≤ 64 KiB in replies. Above the budget, the engine moves `stats` into a JSON blob DataRef and keeps the rest inline.

### 10.3 Renditions

| Kind | Payload | Used by |
|---|---|---|
| `rgba8` | DataRef to (h, w, 4) `u1`, ≤ requested px | Image, every fallback, page rasters, spectrogram pictures, mesh turntables |
| `peaks` | Inline or DataRef: (levels, channels, bins, 2) min/max | Audio waveform, 1-D plots, thumbs |
| `spectrogram` | DataRef to (frames, bins) f32 dB + `hop`, `fft`, `rate` | Audio |
| `excerpt` | Inline text ≤ 4 KiB + `offset`, `total_chars`, `lines` | Text thumbs, paging |
| `svg` | DataRef to SVG bytes | Vector, engine-drawn plots |
| `mesh_lod` | DataRef to a decimated mesh | Mesh thumbs, remote meshes |
| `plot` | Inline series ≤ 4 KiB | Scalars over a sweep, small tables |
| `tree` | Inline JSON, depth ≤ 32, ≤ 64 KiB | Tree, composite fields |

### 10.4 WatchRequest

```json
{"node": "n3", "port": "audio", "index": 0, "sheet": false,
 "view": {"id": "auto", "params": {}},
 "want": [{"kind": "peaks", "px": 1600, "t0": 0.0, "t1": 12.5},
          {"kind": "spectrogram", "fft": 2048, "hop": 512},
          {"kind": "full"}],
 "px": 1024}
```

`px` is a request the viewer raises for 1:1 work: an engine sends view renditions at most that on their long side, up to what it can (the SDK, 4096), so a pixel of the picture can be a pixel of the array. A rerun for it is all memo hits.

`want` defaults per media kind; an engine that cannot make a derivative omits it, and the UI computes it from `full` if it can or shows the summary. The `ndarray` default is today's request: one `rgba8` per channel view, histograms, `full`.

### 10.5 Derivatives on request

`blob.derive{handle, spec}` (and HTTP `GET /v1/blobs/derived/{handle}?spec=`) returns a derivative's DataRef or an inline readout. Specs: `thumb:<px>`, `preview:<px>:<view>`, `tile:<z>/<x>/<y>`, `region:<x>,<y>,<w>,<h>` (raw values, for a remote probe and curves), `probe:<locus>`, `diff:<handle>` (rendition plus stats, remote |A−B|), `peaks:<n>`, `frames:<a>-<b>`, `lines:<a>-<b>`, `raster:<px>`. A derivative's identity is `hash(source handle, spec, deriver version)`, so it is cacheable forever by URL. **`region` and `probe` readouts of floating-point data are returned as `.npy` DataRefs** (an `inline` b64 locator when small), never as JSON numbers, so NaN, ±Inf and −0.0 survive exactly; JSON readouts are allowed only for integer dtypes.

### 10.6 Live data in flight (`node.live`)

```json
{"node_id": "n7", "port": "result", "stream": "snapshot", "seq": 3, "provisional": true, "display": <Display>}
{"node_id": "n9", "port": "text", "stream": "append", "seq": 118, "offset": 5120, "inline": "the next tokens…"}
{"node_id": "n4", "port": "audio", "stream": "append", "seq": 22, "offset": 960000, "frames": 48000, "ref": <DataRef of the chunk>}
{"node_id": "n7", "port": null, "stream": "log", "level": "info", "text": "pyramid level 2"}
```

| Stream | Meaning | UI behaviour |
|---|---|---|
| `snapshot` | A whole provisional value (the research software's commands stage, a Perona–Malik iteration, a coarse-to-fine render) | Replace; keep the newest per (node, port); draw with a provisional marker |
| `append` | A growing value (text tokens, audio blocks, table rows, frames) with a contiguous `offset` | Append in order. A gap or out-of-order `seq` makes the viewer fetch the partial value so far from `run.get` `partials` (section 8.5) and continue from its `offset`; after `done`, the full value |
| `log` | Node-scoped messages | Status tooltip and run log, never the value |

Rules:

- The engine throttles `snapshot` to one per node per 100 ms locally and 250 ms remotely, and batches `append` to ≥ 50 ms or ≥ 64 KiB.
- Live updates are produced for **watched** values only; unwatched nodes get thumbs only.
- `provisional: true` never survives `done`. The final reply replaces every provisional value; a cancelled run keeps the last good non-provisional display.
- **Data in flight has three tiers, all on every binding:** a node finished (`preview` with thumb and watched display, today plus `display.stream`), a node partial (`snapshot`), a growing value (`append`). Plus `run.peek` opens any port of a running graph.

## 11. Cancellation

1. `run.cancel{run_id}` MUST NOT queue behind the run it cancels (F36). Each binding carries it out of band (section 16).
2. Cancel is idempotent; cancelling an unknown or finished run is not an error. **A cancel for an unknown run leaves a tombstone**: the engine keeps cancelled `run_id`s for `run_ttl_s`, and a run that arrives after its own cancel (HTTP cancel on another connection, an AMQP fanout ahead of the queued run) ends at once with `cancelled` records and `run.reply{ok: false, error: {kind: "cancelled"}}`, without computing anything. Only the owning session (or an `admin` token) may cancel a run (section 5.2). A cancel from another session is answered `unauthorized` and leaves no tombstone.
3. The executor checks the run's cancel token between nodes and between list items. Long nodes SHOULD check it inside (`Cx::cancelled()`).
4. A cancelled run ends with `cancelled` records for nodes it didn't reach, then `run.reply{ok: false, error: {kind: "cancelled"}}`.
5. The UI marks active nodes `cancelled` at once and ignores later records and the reply of that run (as `run.rs:481` today).

## 12. Errors and failure

### 12.1 Node errors (data, in records and replies)

| `kind` | Meaning |
|---|---|
| `config` | Bad parameters or settings (ValueError and friends) |
| `op` | The node's computation failed |
| `skipped` | Downstream of an error (the reason names the upstream node) |
| `missing_source` | A source handle the engine cannot resolve and the client didn't upload |
| `graph` | Run-level: a cycle, an unknown node kind, a malformed graph |
| `engine` | Run-level: the engine stopped, timed out or is unreachable (usually synthesised by the host). Also node-level, while the engine itself stays up, when a process the engine drives for that node crashes, times out or breaks its framing (an MCP adapter's craft, section 16.5; D46): that node is `engine`, its downstream `skipped`, independent branches run, and the run's own `error` stays unset |
| `cancelled` | Run-level: cancelled |

Messages are plain text. The UI truncates at 180 characters with the rest on hover (F34).

### 12.2 Protocol errors

| Code | Name | HTTP | AMQP |
|---|---|---|---|
| -32700 | parse | 400 | `pk-error: parse` |
| -32600 | invalid_request | 400 | `pk-error: invalid` |
| -32601 | unknown_op | 404 | `pk-error: unknown_op` |
| -32602 | bad_params | 422 | `pk-error: bad_params` |
| -32603 | internal | 500 | `pk-error: internal` |
| -32000 | version | 409 | `pk-error: version` |
| -32001 | busy | 503 + `Retry-After` | `pk-error: busy` |
| -32002 | cancelled | 409 | `pk-error: cancelled` |
| -32003 | not_found (run, blob, graph handle) | 404 | `pk-error: not_found` |
| -32004 | too_large | 413 | `pk-error: too_large` |
| -32005 | unauthorized | 401 | `pk-error: unauthorized` |
| -32006 | unavailable (no device, no model) | 503 | `pk-error: unavailable` |
| -32007 | integrity (digest mismatch) | 422 | `pk-error: integrity` |
| -32008 | unsupported (capability off) | 501 | `pk-error: unsupported` |

Error objects carry `{code, message, data?: {kind, hint?}}`.

### 12.3 The host never lets the UI wait forever (normative)

For **every** request, the host guarantees exactly one outcome: the engine's reply, or a synthesised failure of the same type, when the request cannot be sent, times out (section 6.1), the connection is lost or the engine stops. Today's `failed_run`, `failed_export` and `failed_save` (`client.rs:252-264`, F35) generalise to every op: a synthesised `RunReply{ok: false, error: {kind: "engine", message}}`, `PeekReply{error}`, `SaveReply{ok: false}`, and so on. While the engine is down, requests fail at once (F2).

**Run timeouts are inactivity timeouts.** After the `run` ack, the host fails a run only when it has seen no event for that run and no `ping` answer for `limits.inactivity_s` (30 s in-process and socket, 60 s HTTP and AMQP; the binding may raise it). A long run that keeps emitting events or answering pings never times out. Before synthesising the failure the host calls `run.get` once; a live snapshot resets the timer. **A real reply of the current generation that arrives after a synthesised failure replaces it** (the reducer treats the real reply as authoritative). Protocol 1 engines, which send no ack, keep today's 300 s ceiling on the whole run.

### 12.4 Engine state

The host reports one `EngineState`, today's `EngineStatus` (`client.rs:69`) with two additions (F1):

| State | Fields | Label / banner |
|---|---|---|
| `off` | | No engine |
| `connecting` | `url` | |
| `unreachable` | `url, reason, retry_s, hint` | Broker or server unreachable, with the fix (today `BrokerUnreachable`) |
| `engine_down` | `reason, queues?, stderr_tail?, hint` | The engine is not running or crashed, with the fix (`pipectl up research.yml`, the manifest's command) |
| `loading_schema` | | |
| `unavailable` | `reason` | Engine up but unusable (no GPU device; protocol mismatch) |
| `busy` | `since` | **New:** missed pings during a run |
| `connected` | `version, nodes, caps` | `caps` is new |

Engines MAY push `engine.state` for states only they know (`unavailable`). Every other state is the host's. The banner hint comes from the engine manifest (section 16), not core.

**Control channel.** `docs/control-protocol.md` replies (`ui.inspect` and others) serialise a **v1 projection** of these types: `unreachable` is shown as `BrokerUnreachable`, `connected` without `caps`, `WatchRequest` as `InspectRequest`, Display v2 as Display v1. New fields appear only under a `v2` key or when the request sets `control.v2`, so recorded session journals replay unchanged (golden rule 8).

## 13. Remote data: upload and download

### 13.1 Endpoints

On the HTTP binding (and mirrored as ops on others):

| Method | Path | Purpose |
|---|---|---|
| `POST` | `/v1/blobs/has` | `{"handles": [≤10000]}` → `{"have", "missing", "aliases": {md5: b3}, "lease_s": 600}`. Every "have" holds a lease so cleanup can't remove it before the run |
| `PUT` | `/v1/blobs/{algo}/{recipe}/{hex}` | Upload. `Content-Length` REQUIRED, `Content-Type`, `X-Plumcraft-Meta` (BlobMeta JSON). The server hashes while writing a temp file and renames only on a match (422 `integrity` otherwise). A store that declines a matching upload (its budget, free-space floor or size limit) answers 503 `unavailable`. Existing blob → 200 without reading the body (`Expect: 100-continue`) |
| `PATCH` | `/v1/uploads/{id}` | Resumable chunks for large sources, finalised against the declared handle |
| `GET` | `/v1/blobs/{algo}/{recipe}/{hex}` | Download. `Range` supported; `ETag` = handle; `Cache-Control: immutable` |
| `POST` | `/v1/blobs/get_many` | `{"handles": [≤256]}` → `multipart/mixed`, one part per blob with its handle in `X-Plumcraft-Handle`; for many small renditions on HTTP/1.1 (HTTP/2 multiplexing makes it optional) |
| `GET` | `/v1/blobs/derived/{handle}?spec=` | Section 10.5. The reply's `X-Plumcraft-Handle` names the derivative, so a repeat is a plain GET |
| `POST` | `/v1/memo/lookup` | Section 14.5 |

### 13.2 A remote run

1. **Gather sources.** The host walks the graph for path-hinted values (the same walk `graph.bundle` does, F78) and the mask editor's edit files (F60). It hashes each file (cached by `(path, mtime, size)`).
2. **Choose.** A value that is only the catalogue's default is not a source: the engine wrote the catalogue, so it is never how an engine asks for a file. A UI asks before it sends files to an engine it didn't start.
3. **Negotiate.** `blob.has` with all handles; upload only `missing`, in parallel, smallest first so planning starts sooner.
4. **Rewrite and run.** Values become `{"source": {handle, name, media}}` (section 8.1); the run names sources by handle only.
5. **Plan and stream.** Memo hits report `cached` before anything runs. As nodes finish, `preview` records carry the thumb **inline** (an encoded rendition of 4 KiB or less, section 13.3) and DataRefs with `url` locators for watched displays.
6. **Fetch lazily by handle.** Inline thumbs need no fetch; they stay in UI memory and are never written to a store. Then the watched display's renditions (batched with `blob.get_many`), then nothing. The full value is fetched only for 1:1 zoom (tiles), probe (region), playback (frames), compare (or `diff` derivative), save or pin.
7. **Verify and store locally.** Every fetched byte is hashed while streaming; a match is renamed into the UI's **durable CAS** (never commonkit, which only engines write; no Rust commonkit writer is needed), so the rest of the UI maps it by path exactly as it maps commonkit locally. A mismatch is discarded and the remote marked suspect.
8. **Remote save** (F64): `output.save` without `path` returns `{"files": [{"name", "ref": DataRef}]}`; the host downloads and writes locally.

### 13.3 Keeping a remote run interactive

- On sessions where `data.shm` is off, node thumbs MUST ride the `preview` record inline as an encoded rendition of 4 KiB or less (QOI, or WebP at 80 px when `preview_codec` is negotiated), so a 100-node graph costs no thumb round trips. A thumb that doesn't fit falls back to a `url` locator.
- Previews on the wire MAY be encoded (PNG or QOI lossless for previews and tiles; WebP or JPEG for thumbs when `preview_codec` is negotiated). Locally they stay rgba8 `.npy`, so the mask editor's exact 8-bit round trip (F73) holds.
- The client states `preview_budget_kbps`; the engine drops superseded previews rather than queueing them.

### 13.4 Leases and pins

- A **lease** is a time-bound hold (default 600 s, renewed while a run is live) granted by `blob.has`, held by an upload until finalised and by a running job on its inputs.
- A **pin** is an indefinite hold: sources of open or recently saved workflows, user-frozen values (Freeze pins, plan section 7), mask edits (user work, not recomputable) and blobs a delivered plugin's tests reference.
- Cleanup removes only blobs that are neither pinned, leased nor referenced by a live memo record, after a grace period.

## 14. Checkpoint reuse

### 14.1 The typed canonical form

Memo inputs are serialised in a **custom typed canonical form** (deliberately not RFC 8785, which writes `1.0` as `1` and so would merge an INT `1` and a FLOAT `1.0`, values numpy treats differently; and Rust `ryu`, Python `repr` and ECMAScript disagree on floats such as `1e16`):

1. **Coerce first.** Each resolved parameter is coerced to its declared port type before encoding (an INT port's `1.0` becomes `1`; a FLOAT port's `1` becomes `1.0`).
2. **Numbers are tagged.** Integers are `{"i": "<decimal>"}` (any size, no exponent). Floats are `{"f": "0x<16 lowercase hex>"}`, the IEEE-754 binary64 bit pattern, so NaN, ±Inf, −0.0 and subnormals (`5e-324`) are exact. Booleans and `null` are JSON literals.
3. **Other values:** strings as JSON strings (NFC is not applied; bytes as given); tuples as lists; numpy scalars as their Python value (then rule 2); dicts with keys sorted by UTF-8 bytes; no whitespace.
4. **Refused, never `repr`'d:** values with no defined encoding (arbitrary objects). Refusal is a node `config` error, but every value today's `json.dumps(default=repr)` accepts in practice (numpy scalars, tuples, NaN, Inf) has an encoding above, so nothing that works today is refused.
5. **Shared test vectors** (`crates/proto/fixtures/memo_canon.json`, checked by Rust and pytest) include `1` against `1.0`, `1e16`, `5e-324`, NaN, −0.0, `2**64 + 1`, nested tuples and numpy scalars.
6. **Migration:** Python keeps today's SHA-1 of `json.dumps(default=repr)` (`executor.py:670`) until the vectors pass in both languages; then both switch.

### 14.2 Memo key

```text
memo key = <algo>(canon(["plumcraft-memo-v1", kind, node_version, engine_salt,
                          device_class_or_null, params, inputs]))
written   memo:<algo>:<hex>
```

- `engine_salt`: the engine's code digest from `hello` **plus a digest of the resolved dependency set**. **Required since records persist (section 14.5)**: a salt without the code would serve a fixed node the old code's result. The SDKs build it as `<prefix>+code:<engine code digest>+sdk:<SDK code digest>`, and a Python engine that names `dependencies` adds `+deps:<digest of those packages' versions>`: in Rust, `build.rs` bakes the MD5 of the engine crate's `src/` and of the SDK's own sources (`crates/engine`, `crates/proto`, `crates/store`), each with the workspace `Cargo.lock` (`crates/engine/digest.rs`), so the salt is the same in every hosting mode and changes only when code that can change a result does; in Python, the MD5 of the engine's package `.py` files and of the SDK's, plus Python's minor version and the numpy and pyuncommonkit versions, computed when the engine starts. The craft adapter adds the craft's version, executable digest and manifest digest (as listed below) (Python: the research software's `code_version()` over package, yapocis and kernels, plus `uv.lock`, `numpy.__version__`, `cv2.__version__`; Rust: `rust:research-<version>+<build digest>` plus `Cargo.lock`; GPU engines add the Metal or MLX framework and driver version; MCP adapter: app id, `serverInfo.version` and build hash). A dependency upgrade therefore misses every persisted record.
- `device_class`: present only for nodes that declare `memo.device_sensitive`.
- `params`: resolved non-wired values; provenance excluded (F43).
- `inputs`: `{port: identity}` including any wire slice. A plan-key identity is the string `<upstream plan key>#<upstream port>`, with `[<slice>]` appended in canonical text for a sliced wire. A bound input (section 7.6 rule 10) is identified by its content instead, since another engine's plan key means nothing here: a blob by its handle string (`md5:nd1:…`), a json value as `{"json": value}`, a list as the list of its items' identities. So when an upstream engine recomputes the same bytes, the downstream memo still hits.
- A node with `memo.scope: none` passes `memo:none:<run_id>:<generation>:<node>` downstream instead of its plan key, so nothing below a fresh value is reused.
- The text engines (Rust and Python) implement the plan key with an in-memory memo; `crates/proto/src/memo.rs` and `python/src/plumcraft_engine/memo.py` are held to `crates/proto/fixtures/memo_canon.json`. Coercion applies to ports typed `INT` and `FLOAT` until wire types declare their numeric kind.
- **Algorithm: MD5 when the engine accepts a single client token** (the maintainer's own UI chooses every parameter and template). **`b3` MUST be used when an engine accepts more than one client token**, because parameters and templates are then client-controlled by several parties and MD5 chosen-prefix collisions apply. **Decision**; note 03 proposed BLAKE3 always.

### 14.3 Two lookups

| Key | Input identity | Known | Use |
|---|---|---|---|
| Plan key | upstream plan key + port (+ slice); leaves use the source's content handle | Before running | `cached` at plan time; skip whole subgraphs; today's Merkle chain (F71) made portable |
| Content key | the input's content handle | After the upstream node ran | Reuse when another graph, or another engine, produced identical bytes upstream |

**Content keys cost hashing**, about 10–13 ms per 7.9 MB intermediate with MD5, so an engine computes one only for a node whose inputs are already published (and so already hashed: targets, watched values, previews) or that declares `memo.content: true` (expensive nodes, group boundaries). Every other node uses the plan key alone, which needs no hashing beyond today's. The executor tries the plan key, then the content key where there is one; a hit on either writes the other as an alias. Leaves are keyed by content, never by path, so `test.jpg` from two folders is one source.

### 14.4 Scopes

Declared per node in `NodeDef.memo.scope`: `none` (time, network, unseeded random, an MCP call whose app state isn't captured), `session`, `machine` (default), `global` (needs the engine's cross-device bit-exactness attestation). STATUS shows host numerics differ between the development machine (arm64) and a remote server (x86-64), so VI nodes stay `machine` until attested.

### 14.5 Memo records

```json
{"memo": "memo:md5:…", "kind": "vi.cp.pencil", "version": 3, "salt": "py:research+1a2b…",
 "outputs": {"value": {"kind": "blob", "ref": <DataRef without locators>}},
 "editor": {}, "ms": 412.0, "created": 1792170000.1, "scope": "machine"}
```

Records whose outputs are gone are dropped, never trusted. Records from elsewhere are checked: salt and version must match exactly, outputs must exist and their meta must agree with the stored header.

The record above is the design; **what the SDKs write today** (`crates/engine/src/persist.rs`, `python/src/plumcraft_engine/persist.py`, the same format in both) is a compressed JSON segment under form `plummemo`, `plum:v1:plummemo:<plan key digest>:c`, in the engine's own store:

```json
{"v": 1, "plan_key": "memo:md5:…", "ms": 2140.5,
 "outputs": {"doc":   {"blob":  {"handle": "md5:raw:…", "meta": <BlobMeta>, "publish": true,
                                 "thumb": {"handle": "md5:nd1:…", "dtype": "|u1", "shape": [120, 96, 4]}}},
             "paths": {"json": 1097},
             "codes": {"array": {"handle": "md5:nd1:…", "dtype": "<u4", "shape": [812]}},
             "tone":  {"audio": {"handle": "md5:nd1:…", "sample_rate": 48000, "channels": 2}},
             "items": {"list":  {"items": [ … ], "axes": [["v.values", 3]], "labels": ["1", "2", "3"]}}}}
```

**What persists (maintainer, 2026-10-29: "auto caching for documents which take a thresholded time to compute"), replacing the opt-in design below.** After a `machine`-scope node computes, its record is written when its **own work** took at least the engine's threshold (1 s by default; `PLUMCRAFT_MEMO_PERSIST_MS`, a number of ms or `off`), measured without the setup it reports (`Cx::setup_took`, `Cx.setup_took`: waiting for or starting a child process), **and** writing and reading its outputs back would cost well under computing them (their size at ~200 MB/s, twice, under the work time). A workflow node's `cache` (section 7.6 rule 8a) overrides: `always` persists regardless, `never` never. `session` and `none` scopes, `memo: "off"`, and an engine without a store never persist. Every value a record names is published first (raw ones compressed, section 4.4) and the record written last, so a record never names a value that isn't stored; a recomputed node rewrites its record. A value over the store's 128 MiB read cap can't be persisted, so its node isn't.

**Lookup.** A node not in the in-memory memo whose plan key has a record reports `cached` from it: every value is read back and checked against its handle, and the record must hold every declared output. A record is untrusted input (any process on the machine can write the store): one that doesn't parse, names another plan key or version, nests too deep, or names a value that is gone or wrong is a miss and the node runs again, never an error. Records and their values are reaped with everything else by the store's TTL and budget. A value is read back whole when the record is used (not lazily). Persisting is synchronous, after the node's `done` and before the next node starts; the size test keeps its cost under half the node's own time.

**The original design, not built:** persistence opt-in with `memo.persist` offered and the user's setting on, for nodes with `memo.persist: true` (checkpoint-marked nodes and group boundaries) in the durable tier, copied on a **background thread that never delays a `preview` record**, by `clonefile` on APFS when source and tier share a volume. `memo.audit` recomputes a sample of hits and reports mismatches (not built). **The exactness gate and the conformance suite always run with `memo: "off"` and a fresh store**, so a persisted result can never mask an exactness failure.

### 14.6 What the caches did

An engine with capability `run.stats` puts `stats` on every `RunReply`, so a client needs no polling, and an engine without it simply reports nothing:

```json
{"computed": 3, "from_memory": 8, "from_store": 3, "from_content": 1,
 "computed_ms": 412.0, "saved_ms": 3200.5, "kept": 2, "store_missed": 1, "evicted": 0,
 "store": {"shm_hit": 41, "shm_publish": 6, "shm_declined_space": 2},
 "tree": {"bytes": 8123456, "files": 42, "max_bytes": 17179869184,
          "free_bytes": 14173392076, "min_free_bytes": 8589934592}}
```

- `from_memory`, `from_store`, `from_content`: nodes answered by the in-memory memo, a persisted record, and a content key (section 14.3). They stay apart: a store hit is what skips work across restarts and processes; a content hit found the same bytes made another way.
- `saved_ms`: what those results took to compute, from the memo entry or the record. `computed_ms`: what the nodes that ran took, without setup.
- `kept`: records written. `store_missed`: persistable nodes with no usable record, which is not the same as evicted: only nodes worth keeping have records. `evicted`: entries the in-memory memo dropped.
- `store`: the commonkit counters of its contract section 11, drained over the run, summable across languages. A hit there is a read of a record or of a value, so one skipped node can be several; count skipped nodes with the `from_` fields. `shm_declined_*` are writes the store refused.
- `tree`: the store. `bytes` and `files` come from its reaper's last walk and are absent when that is stale or missing. `free_bytes` is the disk's free space, read each run, and `min_free_bytes` the floor under which the store refuses writes (`shm_declined_space`), so a client can warn before the first refusal.
- `NodeReport.memo` adds `source` (`memory`, `store`, `content`; absent when the node ran), and `computed_ms`.
- A coordinator sums its fragments into `stats` and keeps each engine's share in `RunReply.engine_stats`.

The editor shows a summary in the status line, with refused writes as a warning, and the whole in the Cache Report (View › Cache Report, `run.cacheReport`); `run.stats` returns the same numbers.

## 15. Engine-specific operations

- **`output.save`** (core, `output.save` cap; F64):

```json
{"v": 2, "op": "output.save", "graph": {…}, "node": "n7", "port": "value", "index": null,
 "path": "/Users/me/Pictures/out.png", "format": "png", "options": {"quality": 92}, "sheet": false}
```

Reply `{"ok", "paths": [], "files": [{"name", "ref"}], "recipe", "items", "labels", "error"}`. With `data.path` off, `path` is omitted and `files` is returned (section 13.2). The engine runs the graph for the node (a memo hit after a run). `recipe` is true when every file carries what made it. `index` picks one item of a list; without it a list saves as numbered files; `sheet` puts every item on one labelled picture and leaves `index` out (`items` and `labels` then say what is on it). A failure is data: `ok: false` and `error`.

It is the one operation a caller waits for: it answers when the files are written, or after 300 s, and takes its turn behind any run. A binding's call timeout MUST allow for that (section 6.1). So a UI sends it off its own thread.

With several engines the coordinator sends a save to the engine that computes the node, with the fragment a run would send. What that fragment needs from other engines is computed first, in the same way, and comes as `inputs`, read as in a run (section 7.6 rules 10 and 11), with `recipe_graph` the whole workflow, its `engines` (section 8.2) filled in with the engine of each node. **The saved recipe holds that whole graph**, not the fragment, since that is what remakes the file: remaking it needs the same engines, and a node they lack is an error that names it. Opening such a recipe where one is missing says which, from that list (a file that names none can only say which node types are missing). An engine that takes `output.save` MUST honour `inputs` and `recipe_graph`. Saving a node whose engine has no `save_formats` is refused with `config`.

- **Pack operations** `x.<pack>.<op>`: request/response with pack-defined bodies. The research software's are `x.vi.export` (F65) and `x.vi.deliver` (F66), bodies exactly protocol 1's `export` and `deliver`. Core shows a pack's UI only when its capability is present; commands' `enabled` says why otherwise. Long ops MAY emit `log` events with `op_id`.

## 16. Bindings

Every binding carries sections 5–15 unchanged; it adds only framing, event delivery, cancel transport and byte movement.

### 16.1 In-process (mode 1)

```rust
/// Built ON the worker thread by the host, so the core need not be Send (research_graph's Context and
/// the research software's commands aren't). Fn: called again to rebuild after a caught panic.
pub type CoreFactory = Box<dyn Fn() -> Result<Box<dyn EngineCore>, String> + Send + Sync>;

/// An engine with an executor of its own, compiled in. Not Send. Identity, schema, caps and limits
/// stay on `Engine` (`Engine::with_core(catalogue, factory)`), which is Send + Sync, so the host
/// answers them while a graph runs.
pub trait EngineCore {
    fn run(&mut self, req: RunRequest, plane: &Plane<'_>, emit: &mut dyn Emit, cancel: &CancelToken) -> RunReply;
    /// `values.release`: one count of each id (section 7.6 rule 11).
    fn release(&mut self, ids: &[String]) {}
    /// `output.save` (section 15), on the worker, between runs, with a token of its own. The
    /// default says it isn't supported, as does a hosted engine with no core.
    fn save(&mut self, req: SaveRequest, plane: &Plane<'_>, cancel: &CancelToken) -> SaveReply { SaveReply::unsupported() }
}

/// The host assigns envelope seq, conflates, keeps the ring for run.get, and wakes the UI.
pub trait Emit { fn status(&mut self, rec: StatusRecord); fn live(&mut self, u: LiveUpdate); fn log(&mut self, l: LogRecord); }

/// What the UI holds, whatever the binding (today's EngineClient, typed).
pub struct EngineClient { requests: Sender<Request>, events: EventQueue, caps: Caps, state: EngineState }

pub enum Request {
    Schema { digest: Option<String> }, Run(Box<RunRequest>), Cancel { run_id: String },
    Watch { run_id: String, watch: Vec<WatchRequest> }, Get { run_id: String },
    Peek(Box<PeekRequest>), Save(Box<SaveRequest>), BlobHas(Vec<Handle>),
    BlobPut { handle: Handle, source: BlobSource }, BlobGet { handle: Handle, range: Option<Range<u64>> },
    Ext { op: String, body: serde_json::Value }, Ping, Shutdown,
}

pub enum Event {
    State(EngineState), Schema(Box<SchemaReply>), Status(Box<StatusEvent>), Live(Box<LiveUpdate>),
    RunReply(Box<RunReply>), Snapshot(Box<RunSnapshot>), Peek(Box<PeekReply>), Save(Box<SaveReply>),
    Blob(BlobOutcome), Ext { op: String, id: u64, result: Result<serde_json::Value, ProtoError> },
    Log(LogRecord), Resync { run_id: Option<String>, reason: String },
}
```

- The host runs today's two-thread pattern (`crates/local/src/engine.rs`): a dispatcher that reads requests, flips a run's `AtomicBool` at once, records tombstones, and answers `run.get`, `events.poll` and `run.peek` from the host's run table; and a worker that calls the factory and owns the core.
- **Panics.** `catch_unwind` wraps every core call. After a caught panic the host fails the run or save in flight with `kind: engine`, drops the core (its memo and device state may be inconsistent), emits `engine_down{reason}`, calls the factory again and emits `connected` when it succeeds. Five panics in 60 s stop the rebuilds, as section 16.3. GPU engines install a non-panicking wgpu uncaptured-error handler that turns device errors into node `op` errors. A regression test panics inside a node.
- **No serialisation on this path.** Requests, events and replies are typed Rust values moved through channels; protocol 1's `Run(Value)` parse in `engine.rs:85` disappears. A conformance test still round-trips every message through JSON to prove the in-process engine speaks the protocol.
- Data: `mem` locators (an `Arc<[u8]>` table in the host) are allowed; the default stays commonkit files, which the UI maps zero-copy, until a profile shows the write matters.
- Same-host proof: both true, no files.

### 16.2 AMQP / pipekit (mode 4)

| Interface | Carrier (today's wire, unchanged) |
|---|---|
| `hello` | Synthesised by the host from the `research.Schema` reply (protocol 1 engine) or answered by `Schema` (protocol 2 engine) |
| `schema`, `run`, `output.save`, `x.vi.*` | R-1 reply-queue RPC to `{system}.Schema` and `{system}.Plan`, `pk-*` headers, priorities 9 and 5, `op` field as today |
| `run.get`, `events.poll` | **Emulated by the host** from the records and replies it has received, for protocol 1 and protocol 2 engines alike: `Plan` runs a graph synchronously on its only consumer (`replicas: 1`, default concurrency), so anything queued to it waits behind the run |
| `run.peek`, `run.watch`, `blob.has`, `hello` | A second consumer in the `Plan` process on a `{system}.Session` queue, with its own thread, reading a lock-protected run table that the executor updates (latest displays, append buffers, tombstones). Never queued behind a run (Python only; no pipekit change: a second node class in `research.yml`) |
| `run.cancel` | `{system}.control` fanout broadcast, as cancel today (pipekit P3); never queued behind the run. `Plan` records a tombstone when the run is not yet known |
| Events | `{system}.status` topic, routing key `run.<correlation>`, exclusive auto-delete queue bound before the run is published (F38). Records stay non-persistent and pixel-free and carry `(publisher, publisher_seq)`. The host assigns envelope `seq` on receipt |
| Resume | `run.get` (records are not replayable). RabbitMQ stream queues are the upgrade if full replay is ever needed |
| Liveness | Passive declares counting consumers every 3 s; backoff 1–30 s (F2, F3) |
| Bytes | commonkit keys and paths on the same host. A remote fleet runs a **blob daemon** (`{system}.Blobs`, Python stdlib HTTP, the same code as the mode 3 front's section 13.1 endpoints) as a `research.yml` service over its CAS root and commonkit store; its URL base is in `hello.data.blob_url` |

The Python SDK's socket and HTTP fronts meet section 5.5 the same way: a reader thread answers off-run-path ops from the thread-safe run table while a worker thread runs the graph.

### 16.3 Local socket, JSON-RPC 2.0 (mode 2)

- **Launch:** the host creates a private directory (0700), listens on a Unix socket in it (macOS and Linux; Windows is not a target), and spawns the engine from its manifest with `PLUMCRAFT_ENGINE_SOCKET=<path>` and `PLUMCRAFT_ENGINE_TOKEN=<256-bit hex>` in its environment (also `--connect unix:<path>`). The engine connects back and presents the token in `hello`. **Stdout is never the data channel** (the research software prints on import); stderr goes to a 64 KiB ring shown in the engine log panel and, after a crash, in the banner.
- **Attach:** the same binding attaches to a running engine by socket path.
- **Framing:** one JSON-RPC 2.0 object per line, at most `limits.max_frame`. Requests carry `id`; events are notifications `{"jsonrpc": "2.0", "method": "event", "params": <Event>}`; cancel is a notification `run.cancel` read by the engine's reader thread, which a running graph never blocks. The framing module is shared with the control channel and the MCP adapter.
- **Supervision:** process group; exit on socket EOF so a UI crash leaves no orphan; `hello` within 10 s; a crash fails every run in flight with `kind: engine` ("the engine stopped (signal 11); restarting"), shows `engine_down{stderr_tail}`, restarts with backoff, compares `schema_digest` and keeps the graph; after 5 crashes in 60 s it stops and says so. Shutdown: `shutdown`, 2 s, SIGTERM, 2 s, SIGKILL.
- **Data: same host is REQUIRED.** The binding has no byte framing, so `blob.put` and `blob.get` are not offered on it. The proof still runs; if either direction fails (an engine in a container behind a forwarded socket), the host fails closed with `unavailable{reason: "socket engines must share the host's filesystem"}` instead of inventing byte frames. A remote engine uses section 16.4.

### 16.4 HTTP + SSE (mode 3)

| Method | Path | Purpose |
|---|---|---|
| `POST` | `/v1/rpc` | Every op as JSON-RPC 2.0 (one dispatcher, the same codec as section 16.3) |
| `GET` | `/v1/events?since=<seq>&run_id=` | SSE: `id: <seq>`, `event: <kind>`, `data: <Event>`; resumes with `Last-Event-ID`. With `Accept: application/json` and `wait_ms`, a long-poll |
| `GET` | `/v1/runs/{run_id}` | `run.get` convenience |
| `POST` | `/v1/runs/{run_id}/cancel` | Cancel on its own connection |
| `GET` | `/v1/health` | `{ok, engine, busy, runs}`; cheap |
| | `/v1/blobs/…`, `/v1/uploads/…`, `/v1/memo/lookup` | Section 13.1 |

- **Security (v1):** every request needs a bearer token, on any address, and a server refuses to start without one: a page in a browser can reach loopback, so a loopback address protects nothing. Non-loopback also needs an SSH tunnel or a TLS reverse proxy; built-in rustls TLS is later. A remote engine refuses path sources. It does not confine Load and Save: file nodes reach whatever the engine's user can, so a graph runs with that user's permissions (section 18). Blob size is capped by `limits.max_blob`.
- **Runs outlive a dropped connection** while the owning session resubscribes or polls within `limits.idle_cancel_s` (30 s), so a laptop that drops Wi-Fi briefly loses nothing; a run sent with `detach: true` lives for `limits.run_ttl_s` (10 min). Snapshots of finished runs are kept for `run_ttl_s` either way.
- The server is threaded (`ThreadingHTTPServer` in the Python SDK): cancel, `run.get`, `events.poll` and SSE are served on their own connections from the thread-safe run table (section 5.5).
- A manifest MAY name a `command` that launches a local HTTP engine with `--listen 127.0.0.1:0 --port-file <f>`; the host then supervises it as in section 16.3. It also passes `--lifeline` with a pipe on stdin: the engine exits when stdin closes, so a UI that crashes leaves no orphan (an HTTP engine has no connection whose end would tell it).
- **v1 as built:** `POST /v1/rpc` carries every operation, `events.poll` (long-poll) included; SSE comes later. Blob `url` locators are relative (`/v1/blobs/md5/raw/<hex>`), resolved against the base the client used, so a tunnel or proxy needs no rewriting.

### 16.5 MCP adapter (an engine, not a binding)

An artcraft app's MCP server is wrapped by an **adapter engine** that speaks this interface to the UI (over section 16.3 or 16.4) and MCP to the app. The adapter owns the graph walk, so it emits every `node.status` state itself; MCP `notifications/progress` become `progress`, our cancel becomes `notifications/cancelled` (or a child restart). Documents travel as `doc` DataRefs; nothing crosses JSON-RPC as bytes. MCP is not a binding because it has no node states, no replay and no blob model.

### 16.6 Engine manifest

```toml
id = "research"
name = "research"
mode = "pipekit"          # inproc | process | http | pipekit
# process / http:
command = ["uv", "run", "--project", "python", "python", "-m", "research_engine.serve"]
cwd = "."
env = { YAPOCIS_PROJECT = "~/workspace/yapocis_external" }
restart = "on-crash"
max_restarts = 5
# http:
url = "https://127.0.0.1:7311"
token_file = "~/.config/plumcraft/tokens/remote-engine"
# pipekit:
broker = "amqp://localhost"
system = "research"
hint = "cd python && uv run pipectl up research.yml"
# any:
demo = "examples/demo.json"   # opened when this engine is chosen with no workflow

[conformance]
slow = "research.slow"            # a node that takes `seconds`, for the cancel and off-the-run-path cases
```

Found in the app bundle, `~/.config/plumcraft/engines/*.toml`, or `--engine-manifest`. In-process engines are listed by the app crate (no link-time registry).

- **Parsing** (`plumcraft_proto::manifest`): at most 64 KiB; hostile input, never a panic; unknown keys are ignored. `id` is a lower-case letter, then lower-case letters, digits and `_`, at most 32. An empty `command`, a `mode` outside the closed set, or a bad `id` is an error.
- **Resolving** (`plumcraft_host::manifest`):
  - `~` is expanded. `cwd`, `demo`, `token_file` and a `command` path are relative to the manifest's real directory, so a link in the config folder points back at the project. `cwd` defaults to `.`.
  - A bare program in `command` is looked up in `$PLUMCRAFT_UV` (for `uv`), PATH, then `~/.local/bin`, `~/.cargo/bin` and `/opt/homebrew/bin`, because an app started from the desktop doesn't have the shell's PATH. A miss is an error with `hint`.
  - `url` without a `command` attaches to a running engine; `token_file` holds its token.
  - **Installed engines write their own.** `register` writes `~/.config/plumcraft/engines/<id>.toml`. `unregister` removes it, but only a manifest `register` wrote. A checkout links its own manifest there instead (`scripts/install`), which `register` leaves alone unless `--force`.
    - **Python:** an SDK engine's console script has the two commands. The manifest runs `command = [<its python>, "-m", <package>]`, and its `demo` is the package's `examples/demo.json`.
    - **Rust:** an engine binary whose `main` is `plumcraft_engine::serve::main(program, factory, &registration)` has them. With `Registration::Own {id, name, args, demo}`, the manifest runs that executable with `args`, and a demo is written beside the manifest as `<id>.demo.json`. With `Registration::BuiltIn` (the text, array and audio engines, and built-in craft packs), they say every editor already offers the engine.
    - **A craft pack from a file:** `plumcraft-craft-engine --craft PACK.toml register` offers the pack in every editor.
  - `pipekit` isn't supported yet, and `inproc` is never named by a file: both are errors.

## 17. Versioning and protocol 1 compatibility

- Protocol 1 is today's the research software protocol (`docs/protocol.md`, `research_graph-schema` at `4009eac`). Protocol 2 is this file.
- **A protocol 1 engine works unchanged.** The AMQP host detects it (no `hello` answer, `"v": 1` replies), synthesises `hello` from the `Schema` reply (caps `events.push`, `data.shm`, `data.path`, `output.save`, `editor.curves`, `editor.mask`, plus `x.vi.export` and `x.vi.deliver` when `plugin_export` is true), emulates the mandatory `run.get` and `events.poll` from the records it has already received (the reply stays authoritative, so nothing is lost), and parses every protocol 1 message into protocol 2 types:

| Protocol 1 | Protocol 2 |
|---|---|
| `ArrayRef{key, path}` | `DataRef{handle: md5:nd1:<digest from key>, at: [commonkit, path]}` (handle absent when `key` is null: the path's file is hashed lazily if needed) |
| `Preview{key, path, w, h, caption, summary, items}` | `Rendition{kind: rgba8, ref, w, h, caption}` |
| `Display{previews, histograms, meta, array, text, value, recipe, sheet}` | `Display{renditions, stats.channels[].histogram, full.meta.form, full, summary, value, recipe, sheet}`; media `ndarray` (or `table` when `meta.table`) |
| `InspectRequest{node, port, index, mode, turn, sheet}` | `WatchRequest{node, port, index, view{id: mode, params{turn}}, sheet}` |
| `NodeReport.curve` / `.mask` | `NodeReport.editor.curves` / `.mask` |
| `StatusRecord` | `node.status` body, unchanged fields; `publisher` from the `pk-worker` header (or `"engine"`), `seq` read as `publisher_seq` |
| Control-channel replies | Unchanged: the control channel serialises a v1 projection (section 12.4) |
| `RunReply.inspect` | `RunReply.watch` |
| `style_graphs` | `templates` |
| `EngineInfo.research` | `engine.components.research` |

- Fixtures pin the mapping both ways (`crates/proto/fixtures/v1_*.json` from today's `crates/schema/fixtures/python_*.json`).
- A future major (3) changes only when a protocol 2 reader would misread a message. Everything additive is a capability.

## 18. Security and hostile input

| Input | Defence |
|---|---|
| Handle text | Closed algorithm and recipe sets; exact lowercase hex length; parsed before use. Paths are composed only from a parsed handle or commonkit key, never from a message string |
| `path` locators | Honoured only after the same-host proof and only under declared roots, canonicalised, no symlink escape |
| `.npy` headers | `research-shm` rules: magic, version, header ≤ 64 KiB, dtype whitelist, shape product by `checked_mul`, exact file length; torn → miss |
| Sizes | Every `BlobMeta.size` checked against bytes; per-blob caps (128 MiB read in full and 2 GiB published in commonkit, an array past the first mapped or checked whole; `max_blob` elsewhere); per-session totals; `Content-Length` required on upload |
| Downloads and uploads | Hash while streaming into a temp file; rename only on a match; nothing unverified is ever mapped |
| Memo records | Salt, version, outputs and meta checked; optional audit recomputes a sample |
| Images, SVG, audio, text, JSON | Decode limits before allocation (≤ 16,384 px a side, ≤ 256 Mpx); SVG without external resources; audio rate 1–768,000 and channels 1–64; UTF-8 validated; JSON depth ≤ 64 |
| Frames | `max_frame` enforced before parse; a longer line is an error and the connection is reset |
| Tokens | 256-bit, from a 0600 file or the environment; never logged; compared in constant time |

Every new parser gets `panic_hunt` fixtures (golden rule 1).

## 19. Conformance

`plumcraft conform --engine-manifest FILE [--modes process,http]` (`crates/conform`) runs one suite against any engine on any binding, with graphs built from its catalogue and its `[conformance] slow` node (`<id>.slow` by default). `plumcraft-engine check` is the Python half, for engines still in development. It MUST pass for every engine before it ships, and it encodes the floor:

1. `hello`: version negotiation and refusal; capability intersection; both same-host proofs, including a forced failure.
2. `schema`: parses; `unchanged` with the cached digest; protocol 1 mapping where applicable.
3. The shared graph corpus (section 7.6): `order`, states, provenance, list mapping, slices, variadics, absent values, errors as data.
4. Record invariants: envelope `seq` strictly rising; every node ends in a terminal state; the generation is echoed; `preview` precedes downstream `started`; `ms` on `done`.
5. Status delivery: push, long-poll and `run.get` alone give the same final reducer state. **During a 5 s run, `run.get`, `events.poll` and `ping` each answer within 200 ms** (off the run path, section 5.5). Records from two publishers delivered out of `ts` order still reduce correctly; a redelivered record is dropped.
6. Cancel mid-run: unreached nodes `cancelled` within 500 ms; cancel never queues behind a run; idempotent. **Cancel sent before its run**: the run ends `cancelled` without computing. A cancel from a non-owning session is `unauthorized`.
7. A second identical run gives `cached` everywhere; with persistence on, a restart gives `cached` for `memo.persist` nodes, and a changed dependency digest gives a miss. Memo canonical-form vectors pass. Every other conformance case runs with `memo: "off"` and a fresh store.
8. `display.stream`: the watched display arrives before the reply, inline within `max_display_inline` for the Lab fixture; `run.peek` in flight returns `pending` then the display; `run.watch` on an already-finished node emits its display at once.
9. Long runs: a run that emits events for longer than 300 s is not failed; a run silent past `inactivity_s` gets one synthesised failure, and a later real reply replaces it.
10. Append recovery: drop an `append` event mid-stream; the viewer recovers from `run.get` `partials` before `done`.
11. Sessions: a second session sees none of the first's events; a run whose session goes idle for `idle_cancel_s` is cancelled unless `detach: true`.
12. Mode 1: a node that panics gives a failed reply, `engine_down`, then `connected` with a rebuilt core.
13. Data plane: shm paths map, or CAS round trips byte-exact with verified handles; `blob.has` dedup uploads nothing the second time; a reaped blob is a miss, not an error.
14. Mode 2: `kill -9` mid-run → failed reply, restart, schema reloaded, graph kept. A failed same-host proof gives `unavailable`. `research-local` runs this suite in mode 2 as well as mode 1.
15. Mode 3: drop the SSE stream mid-run → resume with no lost records; polling alone gives the same final state.
16. Synthesised failure for every op on timeout and on disconnect.
17. A `panic_hunt` fuzz of every node's parameters and of malformed events, displays and DataRefs, and of every new UI command (Freeze pin, View as, peek, engine select, connect, restart and attach, upload cancel, token entry).
18. Remote thumbs arrive inline on `preview` records; no per-thumb GET is made for a 100-node graph.
