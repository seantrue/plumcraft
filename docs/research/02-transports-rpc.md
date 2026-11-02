# 02: Transports and RPC protocols for the UI ↔ engine interface

*2026-10-27, branch `generic-base-research`. Research only; no code changed. It builds on `docs/research/flocraft_effort_20261017.md` §3 (transport trait, subprocess backend). Crate versions were checked with `cargo search` on 2026-10-27 and against `~/.cargo/registry`.*

## Summary

- **One abstract interface, several bindings.**
  - The abstract interface is a small set of **operations** (hello, schema, run, cancel, inspect/peek, run snapshot, blob has/put/get) plus one **ordered event stream** (engine state, node status, in-flight previews, run reply, logs).
  - It is defined once as versioned serde types, and JSON Schema is generated from them.
  - Every binding carries the same messages. A binding only adds framing, a way to deliver events, and a way to move bytes.
- **Canonical encoding:** JSON, tagged unions, `"v"` on every message (as today). A msgpack codec is reserved in the handshake but not built. Arrays never travel inline; they travel as **DataRefs**: a CAS handle (`md5:<hex>`) plus locators (mmap path, HTTP URL, small inline).
- **Status delivery:** every binding must offer **poll** (`run.get` snapshot, `events.poll` with `since`). Bindings that can push also do. Events carry a per-session monotonic `seq`, which gives dedup, ordering and **resume after disconnect**. The existing rule "the final reply is authoritative" stays, so a lost event costs nothing.
- **Build order:**
  1. the proto crate and the **in-process** binding, a refactor of `crates/local`;
  2. the **AMQP/pipekit** binding, the existing `crates/bridge` behind the trait;
  3. **local socket JSON-RPC 2.0**, for a separate-process Rust engine and the Python stdio runner;
  4. **HTTP + SSE** with blob endpoints, for remote use;
  5. an **MCP adapter** engine (artcraft apps as node packs).
- **Kept out of the core, but designed for:** gRPC, WebSocket, and a msgpack codec. The design rules below keep each of them a mapping exercise.
- **Rejected:** Cap'n Proto, Arrow Flight, NATS, ZeroMQ and tarpc.
- **Framing convergence:** JSON-RPC 2.0 over newline-delimited JSON is already the framing of the craft apps' MCP servers (`soundcraft/crates/automation/src/mcp.rs`) and, minus the `jsonrpc` key, of the research software's `--control` channel. Binding 3, the MCP adapter and automation can share one framing module.

## 1. What the interface must carry (the floor)

Everything the research software supporting www.virtimp.com uses today, from `docs/protocol.md`, `crates/bridge/src/{client,status}.rs` and `crates/local/src/engine.rs`. No binding may offer less.

| Need | Today (AMQP) | Today (in-process `crates/local`) |
|---|---|---|
| Schema with digest caching | `research.Schema` RPC, `schema_digest` | `EngineEvent::SchemaLoaded` at start |
| Run a graph with targets, inspect requests and preview sizes | `research.Plan` RPC, priority 9 | `Job::Run` on the worker thread |
| Node status: queued, started, progress, preview, done, cached, error, skipped, cancelled | `{system}.status` topic, `run.<cid>`, non-persistent, `seq` + `ts` | `EngineEvent::Status(Value)` over mpsc, with a wake callback |
| Data in flight | `preview` records carry outputs and `Display` the moment a node finishes | Same records |
| Reconcile lost records | Final reply replaces reducer state (`StatusReducer`) | Same |
| Latest wins, by generation | Reducer drops older `generation` | Same |
| Cancel a running graph | `{system}.control` fanout, checked between nodes | Per-run `AtomicBool`, checked between nodes and list items |
| Engine health | Broker reachable, queues have consumers (`EngineStatus`) | `Unavailable{reason}` when there is no GPU device |
| Extra ops | export, deliver, save | save (export answered "needs the Python engine") |
| Bulk data | commonkit key or `.npy` path; the UI mmaps | `.npy` path; the UI mmaps |

**The flaws to fix while generalising:**

- **Request and response are untyped.** `EngineRequest::Run(Value)` and `EngineEvent::Status(Value)` carry raw JSON.
- **Event delivery is unbounded.** The event channel is an unbounded mpsc, with no conflation.
- **Status records cannot be replayed** after a reconnect. They are non-persistent AMQP messages.
- **Cancel is binding-specific.** It is a fanout exchange, because a request would queue behind the run it is meant to stop.

## 2. The abstract interface

### 2.1 Operations

| Op | Direction | Purpose | Notes |
|---|---|---|---|
| `hello` | UI → engine | Version and capability negotiation; session id; same-host proof | First message on any binding (§4) |
| `schema` | UI → engine | NodeDefs, wire types, expansions, display kinds, `schema_digest` | Reply may be `{"unchanged": true}` when the client sends its cached digest |
| `run` | UI → engine | Submit a graph run (`run_id`, `generation`, graph, targets, inspect, preview sizes, `priority`) | The ack is immediate (`accepted`); the result arrives as a `run.reply` event and is also fetchable with `run.get` |
| `run.cancel` | UI → engine | Cancel by `run_id` | Must never queue behind work (§3.3) |
| `run.get` | UI → engine | Snapshot of a run: per-node state, last display refs, `last_seq`, and the reply if finished | The polling path, and the resync after a gap |
| `run.peek` | UI → engine | Display for one `(node, port, index)` of a run **still in flight**, or of a cached result | Generalises today's `inspect`, which only rides on a new run |
| `events.poll` | UI → engine | `{since: seq, wait_ms}` → the events after `since` | Long-poll when `wait_ms > 0`, a plain poll at 0 |
| `events.subscribe` | UI → engine | Start push delivery from `since` | Only on bindings with push |
| `blob.has` | UI → engine | `[md5…]` → which digests the engine already holds | Upload dedup |
| `blob.put` / `blob.get` | either | Move bytes for one digest | Same host: never used (mmap). Remote: HTTP PUT/GET or binding chunks |
| `ping` | either | Liveness | Health without broker semantics |
| `x.<pack>.<op>` | UI → engine | Pack extensions: `x.vi.export`, `x.vi.deliver` | Declared in hello capabilities; core hides UI it lacks |

`save` is general (write an output to a path or a download), so it becomes a core op, `output.save`, with the format list declared in the schema.

### 2.2 Events (one stream per session)

Every event has the same envelope:

```json
{"v": 1, "seq": 1043, "ts": 1792170000.123, "kind": "node.status",
 "run_id": "…", "generation": 17, "body": { … }}
```

| Kind | Body | Conflatable? |
|---|---|---|
| `engine.state` | off, connecting, unreachable{reason, hint}, engineDown, loadingSchema, connected{version, nodes}, unavailable{reason} | No |
| `node.status` | `node_id`, `state`, `worker?`, `error?`, `ms?` (today's `StatusRecord` without outputs) | No: state transitions are never dropped |
| `node.progress` | `node_id`, `fraction`, `message?` | Yes: latest per node |
| `node.preview` | `node_id`, `outputs: {port → DataRef}`, `display: Display` | Yes: latest per (node, port) |
| `node.chunk` | `node_id`, `port`, `offset`, `DataRef` or small inline text | No within a port (append-only), for streaming text and audio |
| `run.reply` | Today's `RunReply` | No; authoritative |
| `log` | `level`, `node_id?`, `message` | Yes: drop oldest past a cap |
| `resync` | `run_id?`, `reason` | No: tells the client to call `run.get` |

**The rules:**

- `seq` is per session and strictly increasing, and `(run_id, generation)` scopes reducer state as today.
- The reducer stays idempotent: it applies `seq > last_seq` only.
- The final reply still overwrites.
- `node.chunk` is new. It is what makes text and audio "in flight" visible: an LLM node streams tokens, an audio node streams blocks. The visualisation side (the generalised Display) belongs to another note; the transport only has to carry ordered appends.

### 2.3 DataRef: the bulk-data handle

```json
{"cas": "md5:9e107d9d372bb6826bd81d3542a419d6", "bytes": 7873536,
 "media": "application/x-npy", "meta": {"shape": [812, 808, 3], "dtype": "<f4"},
 "loc": [{"shm": "/Users/…/commonkit/store/vimparr/9e/9e10….o0.npy", "key": "research:v1:vimparr:9e10…:o0"},
         {"url": "https://engine.example/v1/blobs/9e107d9d372bb6826bd81d3542a419d6"},
         {"inline": "<base64>"}]}
```

- **`cas`** is the identity. It is MD5 of the stored bytes, like alexandria's `CASStorage` (`<shard>/<digest>.<ext>`). It lets the UI cache across runs and engines, lets the engine skip uploads (`blob.has`), and lets checkpoints be reused across sessions and hosts.
- **`loc`** holds the ways to reach the bytes, in the engine's order of preference. The client takes the first one it can use.
  - `shm` exists only when hello proved the two sides share a host (§4.2). It is today's commonkit path.
  - `url` exists on remote bindings.
  - `inline` exists only for blobs at or under `limits.max_inline` (default 64 KiB): text, small JSON, a 160 px thumbnail.
- **Sources go up by reference.** A run request names its sources by DataRef. Remote: the client hashes a local file, calls `blob.has`, uploads what is missing, then sends `run`. Same host: it sends the path, and the engine hashes or maps it.
- **Results come down lazily.** Previews (≤640 px RGBA8, already small) are fetched at once. Full-resolution arrays are fetched only on zoom, probe, compare or download, as now (the viewer maps them on demand).
- MD5 is kept for compatibility with alexandria and commonkit, not for security. An engine with untrusted clients verifies uploads by rehashing (as `CASStorage.verify` does). The `cas` string carries its algorithm prefix, so `blake3:` (`blake3 1.8.7` is already in the registry) can be added later without a format change.

## 3. Cross-cutting semantics

### 3.1 Status delivery: push, long-poll, poll

| Mode | How | Who must support it |
|---|---|---|
| Push (callback) | Events are delivered as they occur: an in-process sink with a wake callback, JSON-RPC notifications, SSE, AMQP topic | Every binding that has a channel from engine to client |
| Long-poll | `events.poll{since, wait_ms}` returns as soon as there is anything after `since`, or at `wait_ms` | Every request/response binding |
| Poll | `run.get{run_id}` gives a full snapshot | **Every binding, mandatory.** It is also the resync path |

- The UI's reducer consumes all three identically.
- `run.get` makes "no push" engines possible: a trivial HTTP engine, or an MCP-wrapped app that only has request/response. Status tracking is never lost; it gets coarser.

### 3.2 Ordering and resume

- **Ordering:** the engine assigns `seq` at emit time, in one place per session. That is a single atomic counter in the Rust in-process engine, and a lock in the Python runner.
- **Ring buffer:** the engine keeps the last N events (default 4096, or 60 s, whichever is larger) per session.
- **Resume:**
  1. On reconnect the client sends `hello{session: <id>, since: <last seq>}`.
  2. If the buffer still covers `since`, the engine replays the missed events, then continues live.
  3. If it doesn't, the engine sends `resync` and the client calls `run.get` for each live run.
  4. Either way, nothing is shown wrongly. That matches the guarantee the AMQP path has today through the final reply.
- **SSE gives this almost free.** `id: <seq>` on each event, and the browser-standard `Last-Event-ID` header on reconnect. Alexandria's `events_gateway.py` already runs SSE with a polling fallback, which is the same pattern.
- **AMQP:**
  - Records stay non-persistent.
  - A per-session status queue with `x-message-ttl` and `x-max-length` (`overflow: drop-head`) bounds memory.
  - `Plan` answers `run.get`, which covers resume.
  - A RabbitMQ **stream** queue (`x-queue-type: stream`, replay by offset) is the upgrade if full replay over AMQP is ever needed. It needs no new broker, so NATS JetStream is not needed for this.

### 3.3 Cancellation

| Binding | Mechanism |
|---|---|
| In-process | Set the run's `AtomicBool`; the executor checks it between nodes and list items (as `crates/local/src/engine.rs`) |
| Socket JSON-RPC | `run.cancel` request, plus JSON-RPC `$/cancelRequest`-style notification for the `run` call. The engine's reader thread is never blocked by the worker, so cancel can't queue behind a run (the Python runner needs its reader on its own thread, as the FloCraft study says) |
| HTTP | `POST /v1/runs/{id}/cancel` on a separate connection; the server sets the flag |
| AMQP | Unchanged: `{system}.control` fanout (pipekit P3). `Plan` also learns `run.cancel` as an alias that forwards to the broadcast |
| MCP adapter | `notifications/cancelled` with the tools/call request id (effectcraft and filmcraft `long_job.rs` already honour it) |

**The rules for every engine:**

- Cancel is idempotent.
- Cancelling an unknown or finished run is not an error.
- A cancelled run ends with `node.status: cancelled` for the nodes it didn't reach, then `run.reply{ok: false}`.

### 3.4 Backpressure

- **The engine side is bounded per session.** Conflatable events (progress, preview, log) sit in "latest per key" slots. State transitions, chunks and replies queue in order. If the ordered queue overflows (a stalled client), the engine drops the subscription, keeps the ring buffer, and the client resyncs. The engine never blocks the executor on a slow UI.
- **The UI side:**
  - Replace the unbounded `mpsc` with a bounded channel plus a conflation map. The UI drains each frame, as today, with `request_repaint` from the wake callback.
  - Preview fetching is lazy: the UI fetches only the display refs of visible nodes, and the viewer's targets.
- **Bulk data never shares a channel with events.** Events carry only DataRefs (as today: "records carry no pixels"), so a 100 MB array can't stall a status update. The exception is `inline` under 64 KiB.

### 3.5 Streaming intermediates (data in flight)

There are three tiers. All bindings support all three, because each is just events plus DataRefs.

1. **Node finished:** `node.preview` with outputs and display, the moment a node is done. This is what the research software has today.
2. **Node partial:** `node.progress` may carry `display` for long ops: a tiled DoG, Perona–Malik every k iterations, an audio block. These are conflated, so the latest wins.
3. **Append streams:** `node.chunk`, for text tokens and audio buffers, carrying offsets.

Also, `run.peek` lets the viewer open any port of a running graph without starting a new run. That is new capability, not a loss.

## 4. Handshake, capabilities and versioning

### 4.1 hello

The client sends:

```json
{"v": 1, "op": "hello", "protocol": {"name": "<product>", "min": 1, "max": 1},
 "client": {"name": "research", "version": "0.9.0"},
 "session": null, "since": null,
 "caps": ["events.push", "events.poll", "data.shm", "data.url", "data.inline",
          "codec.json", "node.chunk", "run.peek"],
 "host": {"id": "<machine-id>", "boot": "<boot id>", "stores": ["~/Library/Caches/commonkit/store"]}}
```

The engine replies:

```json
{"v": 1, "protocol": 1, "session": "s-7f…", "engine": {"name": "research", "version": "0.1.0", "components": {"research": "…"}},
 "caps": ["events.push", "events.poll", "run.cancel", "run.peek", "data.shm", "x.vi.export", "x.vi.deliver"],
 "same_host": {"nonce_path": "<store>/.hello/<nonce>", "nonce": "…"},
 "limits": {"max_frame": 16777216, "max_inline": 65536, "max_runs": 4},
 "schema_digest": "md5…"}
```

- **Versioning.**
  - `protocol` is the **major** version. It changes only for breaking changes, and the two sides pick the highest major they share.
  - Everything additive is a **capability string**, never a minor number. This is MCP's model and photocraft's.
  - Unknown fields are ignored (serde without `deny_unknown_fields`, as the schema crate already does: "so a newer engine's types still parse"). Unknown event kinds are ignored, and unknown ops get JSON-RPC error `-32601`.
  - The per-message `"v": 1` stays, so the AMQP `Plan` and current fixtures keep working.
- **Extension ops** use the `x.<pack>.` prefix. Core shows a pack feature only when its cap is present. That covers export and deliver today, and the FloCraft study's item 2.
- **Codec.** `codec.json` is required. `codec.msgpack` is reserved: switching codec mid-connection is a framing change (length-prefixed), and it is negotiated here, but not built until a profile shows JSON is the bottleneck. Arrays never go inline, so it probably never will be.

### 4.2 Same-host proof (when to use shared memory)

Hostnames lie: containers, VMs, and a remote engine reached over an SSH tunnel on `localhost`. So the engine writes a nonce file into a store root the client listed, and the client reads it back.

- **The file is readable:** `data.shm` is on, the engine puts `shm` locators first, and the UI mmaps.
- **It isn't:** the session is remote, and locators are `url` or `inline`.

This decision is per session and automatic, so the same Python engine works locally over AMQP, over a socket, and over an SSH-tunnelled HTTP port.

## 5. Candidate evaluation

### 5.1 Comparison

| Option | Status delivery | Resume | Cancel | Backpressure | Bulk path | In-flight streaming | Schema / codegen | Auth / TLS (remote) | Rust crate (verified) | Python | Weight | Verdict |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| In-process trait calls | Push via sink + wake callback | N/A (same process); ring buffer still used for `run.get` | AtomicBool | Bounded channel + conflation | Memory or mmap path; zero copy | Yes, all tiers | Rust types are the model | N/A | std, tokio 1.54 (already used) | N/A | None | **Build first** (refactor of `crates/local`) |
| Unix socket / stdio subprocess, JSON-RPC 2.0, NDJSON | Push: JSON-RPC notifications | `hello{session, since}` + ring buffer | `run.cancel` / `$/cancelRequest` on a reader thread | Socket buffer + engine-side conflation | commonkit shm (same host by definition) | Yes | Same serde types; JSON Schema via schemars 1.2.3; Python pydantic or dataclasses checked by shared fixtures | Socket file mode 0600; no TLS needed | tokio `UnixStream`, or `interprocess 2.4.7` for portable named pipes; hand-rolled framing (~300 lines, as artcraft MCP) | stdlib `socket` + `json` | Tiny | **Build third**: default for separate-process Rust and Python engines and the examples |
| msgpack-RPC framing | Same as above | Same | Same | Same | Same | Same | rmp-serde 1.3.1 | Same | rmp-serde 1.3.1 | msgpack (already a pipekit extra) | Tiny | Reserve as `codec.msgpack`; don't build |
| HTTP/1.1 (+h2) REST + SSE | Push via SSE `text/event-stream`; long-poll and poll on REST | Native: SSE `id:` + `Last-Event-ID` | `POST /runs/{id}/cancel` | TCP + conflation; SSE is one-way, so simple | `GET/PUT/HEAD /v1/blobs/{md5}` with Range | Yes (SSE events; chunks) | Same JSON; OpenAPI can be generated from the JSON Schema | Bearer token (as the `--control` token); TLS via rustls 0.23 or an SSH tunnel / reverse proxy | axum 0.8.9 (server), hyper 1.12, reqwest 0.12/0.13 or ureq 3.5 (client; in registry) | Starlette/FastAPI (alexandria's gateway) or stdlib `ThreadingHTTPServer` (pipekit web) | Medium | **Build fourth**: the remote binding, and the easiest for third-party engines in any language |
| WebSocket | Push, bidirectional | Same `hello` resume as the socket binding | In-band | App-level | Side channel (HTTP blobs) | Yes | Same JSON-RPC frames as the socket binding | Same as HTTP | tokio-tungstenite 0.30.0 (tungstenite 0.30 in registry) | `websockets` | Small | Optional later: the socket binding's frames over WS, if SSE + POST proves awkward |
| gRPC (tonic + prost) | Server streaming | Manual: request carries `since` | Separate unary call, or stream cancel | HTTP/2 flow control | Inline bytes, or chunked streams | Yes | `.proto` is a **second schema source**; prost-build in build.rs | mTLS native | tonic 0.14.7, prost 0.14.4 (not in registry) | grpcio (heavy wheels) | Heavy (h2, tower, codegen) | Defer. Keep the model protobuf-mappable (§6) so a binding is possible |
| Cap'n Proto RPC | Callbacks as capabilities; promise pipelining | Manual | Drop a capability | Built-in flow limits | Zero-copy messages | Yes | `.capnp` schema; capnpc | Manual | capnp-rpc 0.27.2 | pycapnp (small community) | Medium | Reject: a second schema, a niche ecosystem, little gain over JSON when arrays are out of band |
| Arrow Flight | DoGet/DoPut streams; no status model | N/A | gRPC cancel | gRPC | Excellent for columnar or tensor bulk | Bulk only | Arrow schemas | gRPC TLS | arrow-flight 60.0.0 (pulls arrow + tonic) | pyarrow | Very heavy | Reject for now. A possible future `flight://` locator in DataRef, if remote bulk ever dominates |
| NATS (JetStream) | Push subjects; JetStream replay by sequence | Excellent (stream seq) | Subject publish | Server-side limits | Object store, or side channel | Yes | None (bring your own JSON) | Built-in TLS, NKeys | async-nats 0.51.0 | nats-py | Another broker to run | Reject: RabbitMQ is already installed, and RabbitMQ streams give replay if needed |
| AMQP via pipekit (existing) | Push: `{system}.status` topic, `run.<cid>`, inherited across child/forward/scatter/join | None today (non-persistent); add `run.get` | `{system}.control` fanout (P3) | Queue limits; non-persistent records | commonkit keys on the same host; a shared CAS store (rclone, as alexandria) for multi-host | Yes (preview records) | pipekit JSON container; the same serde types | Broker auth + TLS (amqps) | lapin 4.12.2 (in use) | pipekit (in use) | Heavy (broker) but already paid | **Build second** (wrap the existing bridge): required for the research software, fleets, scatter/join (M9) |
| MCP (as an engine protocol) | `notifications/progress` (with `progressToken`); no node-status vocabulary | None | `notifications/cancelled` | Client-paced | Resources, or inline base64 content | Progress only | Tool `inputSchema` JSON Schema → NodeDef ports | stdio local; streamable HTTP + OAuth remote | rmcp 3.8.0 (filmcraft, photocraft use 3.5); hand-rolled in soundcraft and others | `mcp` SDK | Small | **Adapter, not a binding**: wrap an artcraft app as an engine (§5.3). Also keep MCP as the automation face (exists) |
| ZeroMQ | PUB/SUB push | None (SUB drops on reconnect) | DEALER message | HWM drop | Multipart frames | Yes | None | CURVE only | zeromq 0.6.0 (pure Rust, young), zmq 0.10.0 (libzmq) | pyzmq | Medium, plus a native lib | Reject: no resume, weak auth, nothing the socket binding lacks |
| tarpc | Rust futures | Manual | Context deadline | — | Inline | Partial | Rust traits only | Manual | tarpc 0.38.0 | None | Small | Reject: no Python |
| jsonrpsee | Subscriptions over WS/HTTP | Manual | Unsubscribe | Bounded | Side channel | Yes | Rust macros | TLS via hyper | jsonrpsee 0.26.1 | None (plain JSON-RPC) | Medium | Not needed: the framing is ~300 lines, as the artcraft MCP servers show. Revisit if the hand-rolled code grows |

### 5.2 Notes on the chosen bindings

**In-process (binding 1).**

- `crates/local` already sits behind `EngineClient::detached()`. The change is to make the boundary an `Engine` trait taking **typed** messages (`Op` → `Reply`, events into an `EventSink`) instead of `serde_json::Value`, so there is no serialisation in the hot path.
- A conformance test still round-trips every message through JSON. That proves the in-process engine speaks the same protocol as the others.
- The DataRef may carry a `mem` locator: an `Arc` to the array in the engine's memo, so the viewer avoids even the `.npy` write. This is optional and only for in-process.
- **Efficiency is unchanged or better** for today's default `research` run.

**AMQP/pipekit (binding 2).**

- The existing lapin bridge maps one to one:
  - `schema` and `run` go to the `research.Schema` and `research.Plan` R-1 RPC;
  - events go to the `research.status` records (add `seq` per session and `kind`; today's records already have `seq` and `ts`);
  - `run.cancel` goes to the control fanout;
  - DataRefs go to commonkit keys plus paths.
- **New in `Plan`** (Python only, no pipekit change):
  - `hello`, answered by `Schema`;
  - `run.get` and `run.peek`, needed for resume and in-flight inspection;
  - `blob.has` against commonkit.
- **Multi-host CAS.** When workers run on a remote server, a shared CAS root (rclone, as alexandria's `CASStorage`) makes DataRefs resolvable everywhere. pipekit's scatter/join inherit the status header (P2), so the event stream already covers fan-out.

**Socket JSON-RPC (binding 3).**

- **Launch:** the UI starts the engine from a pack manifest (`command`, `args`, `env`) and passes `FLOW_ENGINE_SOCKET=<path>`. The engine connects back. Stdout is never the data channel, because the research software prints on import (AGENTS.md › Environment). Stderr goes to the UI's log panel.
- **Attach mode:** the same binding also attaches to an already-running engine by socket path.
- **Health:** "child alive + hello answered".
- **Framing:** one JSON-RPC 2.0 object per line. Requests carry ids; events are notifications `{"jsonrpc": "2.0", "method": "event", "params": <event>}`. The line limit comes from `limits.max_frame` (16 MiB default; the control channel uses 1 MiB).
- The Python side is a `stdio`/socket runner hosting the transport-neutral handler from the FloCraft study's Python split.

**HTTP + SSE (binding 4).**

| Method | Path | Purpose |
|---|---|---|
| POST | `/v1/hello` | Handshake |
| GET | `/v1/schema` | Schema, with the client's digest |
| POST | `/v1/runs` | Submit a run |
| GET | `/v1/runs/{id}` | `run.get` |
| POST | `/v1/runs/{id}/cancel` | Cancel |
| POST | `/v1/runs/{id}/peek` | `run.peek` |
| GET | `/v1/events?since=` | SSE, or a long-poll with `Accept: application/json` |
| HEAD, PUT, GET | `/v1/blobs/{md5}` | Blob has, upload, download |
| POST | `/v1/x/{pack}/{op}` | Extension ops |

- **Auth:** a bearer token, with the same 256-bit token file convention as `--control`.
- **TLS:** for v1, prefer an SSH tunnel (the development machine → a remote server is already routine) or a reverse proxy, over in-app certificate management. rustls is available when needed.
- **Server and client stacks:** a Python engine can serve this with Starlette, as alexandria's `events_gateway.py` does, or with the stdlib server, as pipekit's `web/api.py` does. A Rust separate-process engine uses axum. The UI side uses reqwest, already in the registry, or ureq for a smaller dependency tree. An SSE parser is about 60 lines.
- **Uploads:** large uploads stream with `Content-MD5` and `X-Content-Digest` headers, and the server verifies the digest before it accepts the bytes.

### 5.3 MCP: two roles

1. **Automation, which exists.** the research software's `--control` channel, and an `research-cli mcp` (M8) like the crafts'. This is the UI being driven, not an engine. It should reuse binding 3's JSON-RPC framing module.
2. **Engine adapter: an artcraft app as a node pack.**
   - **Nodes:** a small Rust or Python engine spawns `<app>-cli mcp` and calls `list_commands`; each command becomes a NodeDef, with ports from its params doc and the tool `inputSchema`.
   - **Runs:** a run executes `execute` and `batch` in topological order.
   - **Status:** `notifications/progress` maps to `node.progress`; start and end of each call map to `node.status`.
   - **Cancel:** `notifications/cancelled` maps to cancel. effectcraft and filmcraft already implement both, in `crates/automation/src/long_job.rs`.
   - **Outputs:** the app's `export` or `screenshot` tools write files, which the adapter hashes into DataRefs.
   - **Status floor:** MCP has no node-status stream of its own, but the adapter owns the graph walk and emits it, so the floor holds.
   - **Candidates:** soundcraft (audio in flight), effectcraft or filmcraft (long renders with progress and cancel already done), and wordcraft (text).
   - **Why not a core binding:** MCP's request/response tool model and `progressToken` are too thin for a general engine protocol: there are no per-node states, no replay, and blobs are base64 content.

## 6. Design rules for the canonical message model

These keep later bindings (gRPC, msgpack, WS) cheap.

1. **One source of truth.**
   - The Rust serde types live in a new L0 crate (working name `<product>-proto`). They absorb `research_schema::run` and the `research_graph-schema` types that are not VI-specific.
   - JSON Schema is generated from them with `schemars` 1.2.3, already in the registry, and committed under `docs/proto/`.
   - Python models are generated from that schema (datamodel-code-generator, or hand-written), and both sides are pinned by fixtures written by each side. That is today's `crates/schema/fixtures/python_*.json` pattern.
2. **Tagged unions only:** `#[serde(tag = "kind")]`. No `untagged`, so each union maps to a protobuf `oneof` or a msgpack map.
3. **Map keys are strings**, and ids are strings. Counters (`seq`, `generation`) are u64, not floats. `ts` is an f64 of seconds, kept for compatibility.
4. **Opaque extension payloads are explicit:** a `serde_json::Value` field named `ext` or `x`, never a struct flattened in. Unknown fields are ignored.
5. **No bytes in messages** except `inline` DataRefs. Bulk goes through `blob.*` or a locator.
6. **Errors are data,** as today: per-node `{kind, message}` in events and replies.
   - Binding-level failures use JSON-RPC error codes (`-32601` unknown op, `-32602` bad params, plus an app range for `cancelled`, `busy`, `version`).
   - HTTP maps them to status codes, and AMQP to `pk-error`.
7. **Every message stays valid JSON under 16 MiB.** A graph with an embedded recipe is the largest; it is well under.

## 7. Recommendation and order of work

| Step | Content | Why now | Rough days |
|---|---|---|---|
| 1 | `<product>-proto` crate: ops, events, DataRef, hello, caps; JSON Schema export; Python models; fixtures both ways | Everything else depends on it | 3–4 |
| 2 | `Engine` trait + in-process binding (refactor `crates/local`; typed messages; bounded conflating event queue; `run.get`, `run.peek`) | The research software's default path; must not regress | 2–3 |
| 3 | AMQP binding: wrap `crates/bridge`; add `hello`, `run.get`, `run.peek`, `blob.has`, session `seq` to the Python `Plan` | No loss of the research software functionality; fleets, M9 | 2–3 |
| 4 | Socket JSON-RPC binding: Rust client, Rust engine-side server (for the separate-process Rust engine), Python runner, launch from a pack manifest | The public default; example engines (OpenCV, text, audio) use it | 4–5 |
| 5 | HTTP + SSE binding with blob endpoints and upload dedup; token auth | Remote engines; any-language third-party engines | 5–6 |
| 6 | MCP adapter engine over one artcraft app | Shows non-image nodes and app-as-engine | 3–4 |
| 7 | Conformance suite: one test set that every binding passes (handshake, run, status order, cancel, resume after a forced disconnect, resync, peek in flight, blob dedup) | Guarantees "never less than the research software" for every binding | 2–3, then grows with each binding |

**Total transport work:** about 21–28 days. This replaces the FloCraft study's 4–6 days for "subprocess transport", which covered only one binding and no resume, CAS or remote.

**Decisions to confirm with the maintainer:**

1. Capability strings rather than minor versions.
2. MD5 as the CAS digest, with an algorithm prefix so it can change later.
3. SSE rather than WebSocket for the remote event stream.
4. Bearer token plus SSH tunnel for v1 remote, rather than built-in TLS.
5. gRPC deferred, with the message rules in §6 keeping it possible.
