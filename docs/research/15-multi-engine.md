# 15. Several engines at once

The maintainer, 2026-10-29: "Long term goal is to be able to load multiple engines, and move across engine borders", then "prepare to support multiple simultaneous engines".

This note records what PlumCraft has today, what is missing, a design and the order to build it in. Decision D14 (plan.md) chose one engine per workflow for v1 and reserved three things for later: `RunRequest.inputs`, an optional per-node `engine` field, and a `Session` that holds a map of engine clients. This note turns those reservations into a plan.

**Revised the same day.** The first draft coordinated runs inside the editor's session. The maintainer asked "Why not a separate coordinator?". A coordinator that speaks the engine interface keeps the goal that the editor never knows which engine it has, works for every client (CLI, viewer, MCP, pipekit) rather than only the editor, and can outlive the editor. The design below is the coordinator; the session keeps one catalogue and one run state.

## 1. What exists today

| Piece | State | Where |
|---|---|---|
| `RunRequest.inputs` (`{"node.port": PortValue}`) | In the protocol and the spec ("engines MUST accept blob and json values here"). **Neither SDK reads it**: the Rust and Python executors ignore it, which already breaks the spec | `crates/proto/src/run.rs:88`, spec 7.6 table |
| Per-node `engine` field | In the spec (section 7.6, "MAY carry"). Not in `plumcraft_graph` | `docs/interface.md:436` |
| Clients of an engine | The editor, `plumcraft run`, `plumcraft-view` and the conformance suite each hold one `Connection` (`Arc<dyn Connection>`); switching drops it and connects another | `crates/host/src/lib.rs`, `crates/ui/src/link.rs` |
| Session | One `catalog: SchemaReply`, one `connected: bool`, one `RunState`; `Engines` records one `current` target | `crates/session/src/lib.rs:115`, `engine_cmds.rs` |
| Node ids | Prefixed by convention (`text.`, `array.`, `vectorcraft.`), not enforced | each engine's catalogue |
| Shared wire types | Core: `ARRAY` (rule 3). Shared by convention (D48): `IMAGE` (array, crafts), `TABLE` (text, audio), `JSON`, `SVG`, `PDF` | `core_types`, D48 |
| Data between local engines | Every local engine uses the same commonkit tree (`$COMMONKIT_STORE_ROOT` or commonkit's default), so a DataRef from one resolves in another with no copy. Arrays use one layout (`Form::Array`) in both SDKs | `crates/store/src/lib.rs:41` |
| Data to and from a remote engine | `blob.get` and `blob.put` exist (spec 4.4); a `Connection` resolves a DataRef to bytes | `crates/host/src/lib.rs` |
| Memo identity of an input | `<upstream plan key>#<port>`: an engine-local plan key | spec 14.2 |

## 2. What a graph across engines needs

1. **One catalogue from several.** Merge the node types and wire types of each engine, and remember which engine owns each node type.
2. **Type checking across engines.** A wire from engine A to engine B is allowed only for a type both declare with the same meaning: a core type (`ARRAY`, the scalars) or a shared type on an agreed list. A pack type (`vectorcraft.doc`) never crosses.
3. **Running.** Split the graph into one fragment per engine, run the fragments in dependency order, and give each cross-engine wire to the downstream fragment as an `inputs` binding holding the upstream DataRef.
4. **Data crossing.** Local: pass the DataRef unchanged (one store tree). Remote: fetch the bytes from A and upload them to B, then bind B's DataRef.
5. **Status, previews and cancelling.** Merge each fragment's status records into one stream. Route watch requests and views to the owning engine. Cancel cancels every fragment in flight.
6. **The workflow file.** It records which engines it uses. A node names its engine only when two engines offer the same kind.
7. **Managing engines.** Add, remove and reconnect each engine, and show each one's state, in the editor, the CLI and the control channel.

## 3. Design: a coordinator that is an engine

### 3.1 Shape

A new crate, `crates/coordinator`, provides `Coordinator`, which implements `plumcraft_host::Connection`. It holds `BTreeMap<EngineId, Box<dyn Connection>>`, one per sub-engine, connected by the existing hosts (in-process, process, HTTP). To its client it is one engine:

- `schema` returns the merged catalogue;
- `run` takes one request for the whole graph and returns one reply;
- status records, previews and the reply arrive as from any engine.

**Hosting.** By default the editor hosts the coordinator in-process, as mode 1, so there is no extra hop. A small binary, `plumcraft-coordinator`, serves it over a socket or HTTP like any engine, for headless runs, for runs that outlive the editor (`run_ttl_s`), and to sit next to remote engines.

**With one sub-engine** the coordinator passes every call straight through: the same catalogue, the same requests and the same events. A single-engine user sees no change, and the conformance suite can run against a coordinator with one engine to prove it.

### 3.2 The merged catalogue

- **Engine id:** the coordinator's own (`plumcraft`), with `engines: [{id, name, version, state}]` listing its sub-engines (a new capability, `engines`).
- **Node kinds:** the union, plus a map `kind → [EngineId]` inside the coordinator.
- **Wire types:** the union. The same id declared with a different `media` is a **conflict**: the first engine's declaration is kept, and the conflict is recorded (`Merged.warnings`). It never panics or merges silently. **Built:** the warnings are kept by the coordinator but not shown anywhere yet; step 5 surfaces them.
- **Compat rules:** the union.
- **Salt:** the coordinator's own code digest plus each sub-engine's salt, so a client's keys change when any engine changes.

The editor's `Session` sees an ordinary catalogue: the canvas, palette, graph checks and Tidy keep working unchanged.

### 3.3 Which engine runs a node

A node's engine is:

- its `engine` field, when set and that engine is connected;
- otherwise, the only connected engine that offers its kind;
- otherwise, when several offer it, the first in the workflow's `engines` list. The editor then sets the node's `engine` field (`node.engine`), so a later reconnect can't move it.

A node whose engine is not connected is reported as an unknown kind, as today, with the reason ("engine vectorcraft is not connected"). The graph is kept.

### 3.4 Wires across engines

The catalogue's compat rules decide which wires are allowed, as today. The coordinator adds a rule: a wire between nodes of different engines is allowed only for a **portable** type.

**Built (step 3, provisional until E2):** a type is portable when it is a scalar, a core type (`core_types::is_core`), or declared by both engines with the same `media`. No named list was added to `core_types`, so rule 3 is unchanged. The recommendation below (an agreed list naming each type's byte format) is still E2's to decide; `media` alone says how a value is shown, not its bytes.

With an agreed list, a portable type would also have to be on it. The list needs the maintainer's sign-off, because each entry is a promise that every engine means the same bytes.

The coordinator checks this when a run starts: a wire that crosses with a type that isn't portable, or into an input the downstream engine's compat rules don't accept, is a `graph` error naming the wire (`plan::build`). For the editor to refuse such a wire while it is being drawn, the merged catalogue carries each node's engine (`NodeDef.engine`, built), and `plumcraft_graph` would check portability when the two ends' engines differ (step 5).

### 3.5 Running across engines

For each `run` request the coordinator builds a **run plan**:

1. **Fragments.** Group the nodes to run by engine. Order the fragments so every cross-engine wire goes from an earlier fragment to a later one. If the engines form a cycle (A → B → A), split A into two fragments. An order always exists, because the graph itself is acyclic.
2. **Targets.** Each fragment's targets are its own share of the request's targets plus every node whose output crosses to another fragment. The reply carries their `outputs` (`NodeReport.outputs`, already sent for targets).
3. **Bindings.** When a fragment's reply arrives, the coordinator copies each crossing output into the later fragment's `inputs` as `"node.port": DataRef`. The client's own `inputs` are routed to the fragment that owns the node.
4. **Concurrency.** Fragments with no path between them run at the same time, each in its own engine.
5. **Events.** Each fragment has its own `run_id` inside the coordinator. Events carry the client's `run_id` and `generation`. A failed fragment's downstream nodes are reported `skipped`, as a failed node does inside one engine.
6. **The reply.** One `RunReply`: `order` across fragments, `nodes` merged, `watch` in request order, `ok` only if every fragment was, and `elapsed_ms` measured by the coordinator.
7. **Cancel and supersede.** `run.cancel` cancels every fragment in flight.

**As built (step 4):**

- **Fragments are (engine, depth) pairs**, depth being the largest of a node's inputs' depths, one more across an engine border; so A → B → A is three fragments and the fragments never form a cycle.
- **A node left without its crossing value** (the source failed, was skipped or cancelled, or its value was opaque) is reported `skipped` with the nodes downstream of it in its fragment, and isn't sent.
- **Cancel** reaches every fragment in flight (`run.cancel` with its sub run id); fragments not yet sent report their nodes `cancelled`. A cancel that overtakes its run applies when the run comes, as in the engines.
- **A fragment whose engine stops answering** `events.poll` for 30 s fails with `kind: engine`. If the engine's ring dropped the reply (`resync`), the coordinator asks `run.get`.
- **Known difference:** with `memo: refresh` the coordinator also recomputes the nodes another fragment needs, because they become that fragment's targets.
- **Switching between one engine and several** restarts the client's event numbering (the coordinator's ring replaces the engine's), as an engine restart does.

The plan is built by pure, sans-IO code (the graph and the catalogues in, a plan out; replies in, the next requests out), tested with fake engines as `RunState` is today.

**Watches and views.** A watch request names a node and port, and goes to the node's engine. Display operations (`display.*`, `run.view` renditions) are routed the same way. `resolve` tries the owning engine's connection.

### 3.6 Engines honour `inputs` (spec 7.6, already MUST)

In both SDKs' executors, a binding `node.port` overrides the wire or widget for that port:

- **json:** the value.
- **blob, array or audio:** resolved from the store like a wire's DataRef; a miss is a `missing_source` node error, not a crash.
- **list:** each item as above.

A binding to an unknown node or port is ignored (spec 18: hostile input is skipped). A wire into a bound port is dropped, so what fed it is neither waited for nor computed. The executors don't fill `NodeReport.provenance` yet, so there is no `binding` provenance either.

**Memo identity:** a bound input's identity is its content: `md5:<hex>` for a DataRef, the canonical value for json. It is not an upstream plan key, which only means something inside one engine. This also gives an early cut-off: if an upstream node in another engine recomputes the same bytes, the downstream engine's memo still hits. Spec 14.2 gains this rule.

This change is useful by itself (a CLI or test can feed a node a value), and every later step depends on it.

### 3.7 Data across a remote border

When both engines read the same store tree (spec 4.4's proof), the DataRef crosses unchanged. Otherwise the coordinator moves the bytes: `resolve` on the upstream connection, `blob.put` on the downstream one, and it binds the new DataRef. Compressed blobs (`plumz`) move uncompressed and are recompressed by the receiving store.

### 3.8 Interface changes

All additive, as capabilities:

- **`engines`:** the catalogue lists its sub-engines, and three operations manage them: `engines.list`, `engines.add {engine, mode}` or `{url, token}`, and `engines.remove {engine}`. A sub-engine's state changes arrive as `engine_state {engine, state}` events.
- **Node definitions:** an optional `engine` field.
- **Bindings:** spec 7.6 states how engines treat `inputs`. Spec 14.2 gains the identity of bound inputs.
- **Workflow JSON:** a top-level `engines: [EngineId]`, in order. An old file has none and runs as today.
- **Control channel and MCP:** `ui.inspect` adds `engines: [{id, state}]`; the existing single-engine fields stay, describing the coordinator.

### 3.9 The editor

- **The editor talks to the coordinator** through one `Link`, as it talks to any engine today.
- **The Engine menu** lists the coordinator's engines, each with its state and Reconnect, plus Add Engine (today's choices) and Remove. Each is a command (`engine.add`, `engine.remove`) that calls the coordinator. Switch stays and means "replace all with this one".
- **Nodes:** when several engines are connected, a node shows a small engine badge. `node.engine` pins a node to an engine.
- **The palette** groups Node › Add by engine when more than one is connected.

## 4. Order of work

Steps 1 to 4 change nothing for a single-engine user.

| Step | What | Visible change | Size (rough) |
|---|---|---|---|
| 1 | **Done 2026-10-29.** **Engines honour `inputs`** in the Rust and Python executors. Bound inputs are keyed by content. Tests: a blob, an array, a json value and a list bound to a port; a missing blob; a binding to an unknown node or port | None (fixes a MUST) | ~400 LOC with tests |
| 2 | **Done 2026-10-29.** **`crates/coordinator`, pass-through:** a `Connection` over one sub-engine. The conformance suite passes through it (`mode1_coordinated`, every case). Overhead below measurement noise (`pass_through_cost`: 156 ns direct, 119 ns through, whichever ran first was slower) | None | ~300 LOC |
| 3 | **Done 2026-10-29** (`merge.rs`): the merged catalogue, engine resolution, portability by declaration; `NodeDef.engine`. **Not yet:** a named portable list (E2), surfacing warnings, the workflow file's `engines` and node `engine` written by the editor (step 5; the coordinator already reads both) | None with one engine | ~500 LOC |
| 4 | **Done 2026-10-29** (`plan.rs`, `runner.rs`; with several engines the coordinator answers `hello`, `schema`, `run`, `run.cancel`, `run.get`, `events.poll` itself): fragments, ordering, bindings, merged events and reply, cancel. Tested with hand-made catalogues and with the real text and array engines | None with one engine | ~900 LOC with tests |
| 5 | **Done 2026-10-29.** Engine › Add Engine and Remove (`engine.add`, `engine.remove`) and `--engine a,b` at startup, the editor relinking through a coordinator; Reconnect re-adding; node badges (the engine that runs each node, `•` when pinned); Node › Add grouped by engine (kinds several offer under Shared); pinning on placement (E4 as recommended) and `node.engine` (the node menu's Run on row; an undo step; the node goes stale); the workflow's `engines` order written on the first placement; merge warnings in the status bar and `ui.inspect`; `NodeDef.engines` (every engine that offers a kind, the default first) replacing the single `engine`; the coordinator's state counting the merged nodes. **Left out, not needed:** the `engines.*` wire operations and `engine_state` events (the editor holds the coordinator itself) | Several engines in one graph | ~700 LOC |
| 6 | **`plumcraft-coordinator` binary and remote borders:** byte moves between stores that differ | Headless and remote multi-engine runs | ~350 LOC |
| 7 | **Done 2026-10-29:** two demos, Trace and measure (array → VectorCraft → array, with the new Decode Image node) and What a chord looks like (audio, array and text), opened by `--engine` with exactly their engines; tests through the coordinator and live screenshots | Demo | ~350 LOC |

## 5. Decisions

| # | Question | Decision or recommendation |
|---|---|---|
| E1 | Where are cross-engine runs coordinated? | **Decided (maintainer, 2026-10-29):** a separate coordinator that is an engine, hosted in-process by default |
| E2 | Which types may cross engines? | Recommended: the core types plus `IMAGE`, `SVG`, `PDF`, `JSON`, `TABLE`, `TEXT` and `AUDIO`, each with one media type and byte format named in the spec. **Partly decided (maintainer, 2026-10-30):** `SVG` is a core type ("Make SVG first class"), and `TEXT`, `TABLE`, `IMAGE` and `AUDIO` are too (decision G3 in note 17, "yes core data types"; then "Core audio, please"; built 2026-10-30, their bytes in `docs/interface.md`). **Decided:** the maintainer, 2026-10-30, "PDF no; JSON yes, (format as Yaml for display)": `JSON` is core, and `PDF` isn't |
| E3 | Identity of a bound input in the memo key | **Content** (`md5:` for blobs), for early cut-off across engines. Built in step 1 |
| E4 | Two connected engines offer the same kind (the Rust and Python text engines) | **Built as recommended** (the maintainer: "Go for 5"): **pin on placement**; the workflow's engine order breaks ties for older files |
| E5 | Does Engine › Switch stay? | Recommended: **yes**, meaning "replace all with this one"; Add Engine is new |
| E6 | Implicit conversion across a border (for example the research software's `cv2` image into `ARRAY`) | Recommended: **not now.** A conversion is an ordinary node in one engine |

## 6. Risks

- **Two engines can disagree on a shared type.** `IMAGE` from the array engine is PNG; a craft's might be another format. E2's list must name the byte format, and a conformance test must check every engine that declares a portable type.
- **Concurrent fragments can compete for the machine.** Two engines that each use every core can slow each other down. The coordinator can run fragments one at a time if this proves a problem.
- **A slower path for one engine.** The pass-through must add no measurable time per event; step 2 measures it.
- **Journals.** Replayed journals from single-engine sessions must still replay. With one sub-engine the coordinator is a pass-through, and a journal-replay test guards it.
