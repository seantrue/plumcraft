# 08: Adversarial review of the generic-base plan and interface

*2026-10-27, branch `generic-base-research`. Reviewed: `docs/plan.md` (the plan), `docs/interface.md` (the spec), the floor inventory (the floor), with spot checks of `crates/bridge/src/status.rs`, `crates/local/src/engine.rs`, `python/src/research_engine/service.py` and `publish.py`, `python/research.yml`, `../pipekit/src/pipekit/node.py`, and `crates/ui-egui/src/{viewer.rs,control.rs,export_view.rs,mask_view.rs,curves.rs}` in the `inspector-window` checkout.*

The goal was to refute two claims: that the plan **loses no the research software supporting www.virtimp.com functionality**, and that the research software **runs efficiently** on the base. Neither claim survives as written. Every problem below has a contained fix, and none forces a different architecture. The interface shape (one op set, one ordered event stream, DataRefs, capabilities, four bindings) holds up.

## Verdict

| Claim | Verdict | Main reasons |
|---|---|---|
| No functionality lost | **Not yet shown** | H1 (polling and peek block behind a run on pipekit), H2 (memo canonicalisation merges `1` and `1.0` and refuses values that work today), H5 (VI UI code has no home outside core), M9 (control-channel shapes change) |
| The research software runs efficiently | **Not yet shown** | H3 (content keys hash every intermediate; memo persistence copies every output), M1 (streamed displays don't fit the status inline budget), M5 (remote thumbs cost one round trip each) |
| Node status tracking never weaker | Mostly holds | H7 (fixed 300 s timeout), H8 (a cancel that overtakes its run is lost), M3 (orphaned remote runs), M4 (append streams can't be recovered), L1 |
| Data in flight stronger | UI only | M2: no VI engine emits `progress` or `snapshot` in any milestone |
| Four hosting modes concrete | Three of four | Mode 4 remote has no blob server (M7). Mode 2 is never tested with image data (M12) |
| Estimates | Optimistic | M10: the plan cuts below its own notes' figures. Realistic: 65–80 days to "nothing lost", 130–160 in total |

## High severity

### H1. On pipekit, `run.get`, `run.peek`, `run.watch` and `blob.has` queue behind the running graph

- **Evidence:**
  - Spec 16.2 has these new ops answered by `Plan` as new `op` values.
  - `research.yml` runs `Plan: {replicas: 1}`, and `Plan` sets no `CONCURRENCY`, so it uses pipekit's default.
  - `Plan.process_item` (`service.py:305`) runs the whole graph synchronously.
  - Its own `cancel` branch says: "this message can't [reach a run], since it waits behind the run it means to stop".
- **Consequence:**
  - `run.get` is mandatory for every engine and is the polling fallback. It answers only after the run ends, which is exactly when it is no longer needed.
  - `run.peek` "in flight" never returns `pending` while the run is live.
  - `blob.has` before a run waits behind the previous run.
  - Conformance items 5 and 8 cannot pass on mode 4.
- **Fix:**
  - The AMQP host emulates `run.get` and `events.poll` from the records it has received, for protocol 2 engines as well as protocol 1. The spec already does this for protocol 1, and it needs no engine change.
  - Serve `run.peek`, `run.watch` and `blob.has` off the run path, in one of two ways:
    - a second consumer in the Plan process (a `{system}.Session` queue with its own thread that reads a lock-protected run table);
    - the control fanout with a `reply_to`, as cancel does.
  - Add a conformance test: during a 5 s run, `run.get` answers within 200 ms.
  - The Python socket and HTTP fronts need the same thread-safe run table. Say so in the SDK section.

### H2. Canonical-JSON memo keys merge distinct values and refuse values that work today

- **Evidence:**
  - Spec 14.1 claims "shortest round-trip floats (Rust `ryu`, Python `repr` agree)". They don't:
    - `repr(1e16)` is `1e+16`;
    - `ryu` writes `1e16`;
    - RFC 8785 (ECMAScript) writes `10000000000000000`.
  - RFC 8785 also serialises `1.0` as `1`, so an INT `1` and a FLOAT `1.0` get one memo key. In numpy they give different results: integer versus float dtype promotion, `np.full`, and division.
  - Today's `json.dumps` keeps `1` and `1.0` apart, so this is a **regression to false cache hits**, which breaks golden rule 5.
  - "Non-JSON parameter values are refused, never `repr`'d" turns numpy scalars, tuples, NaN and Inf into errors where `default=repr` accepts them today. That is lost functionality.
- **Fix:**
  - Before hashing, coerce each resolved parameter to its declared port type.
  - Encode numbers with a type tag: `{"i": "1"}` for integers and `{"f": "0x3ff0000000000000"}` for floats as IEEE-754 bit patterns. This also covers NaN, ±Inf and −0.0 exactly.
  - Define tuples as lists and numpy scalars as their Python value.
  - Publish shared test vectors (Rust and pytest) that include `1` against `1.0`, `1e16`, `5e-324`, NaN, −0.0 and large ints.
  - Keep today's SHA-1 key in Python until the vectors pass.
  - Don't call this RFC 8785. It is a custom canonical form.

### H3. Content keys and default-on memo persistence add hot-path cost; plan 9.1's "No new hashing" is false

- **Evidence, hashing:**
  - Spec 14.3: a content key needs "the input's content handle" for every node.
  - Today only published outputs (targets and previews) are hashed. F27 notes that publishing every node would cost about 40 ms each.
  - MD5 runs at roughly 0.6–0.8 GB/s on one core, so a Lab 812×808×3 f32 array (7.9 MB) costs about 10–13 ms. A 50-node style graph adds about 0.5 s per run.
- **Evidence, persistence:**
  - D10 makes persistence the default ("persist locally ... opt-out").
  - A persisted record's outputs must exist in the durable tier (spec 14.5), which means one extra write of every intermediate array.
  - On Linux, commonkit lives on `/dev/shm`, so the durable tier can't hard-link to it.
- **Fix:**
  - Compute content keys only for nodes whose outputs are already published, or that declare `memo.content: true` (expensive nodes and group boundaries).
  - Make persistence opt-in, and only for checkpoint-marked or group-boundary nodes.
  - Write on a background thread that never delays a `preview` record. Use `clonefile` on APFS when the file is on the same volume.
  - Add "hashes per run" and "bytes written per run" to the section 9.1 benchmark gate.

### H4. Persisted memo can serve stale results across dependency upgrades and mask exactness failures

- **Evidence:**
  - The salt is "the research software's `code_version()` over package, yapocis and kernels" (spec 14.2). It leaves out numpy, OpenCV, Pillow, Metal/MLX, GPU driver and compiler versions.
  - Today the memo lives in RAM, so a restart recomputes everything. With persistence, a numpy upgrade returns `cached` with the old bytes.
  - The G5 exit criterion ("a restart gives `cached`") rewards exactly this behaviour.
  - Nothing says that the exactness gate (92 of 92) and the conformance suite run with the memo off.
- **Fix:**
  - The salt includes a digest of the resolved dependency set (`uv.lock`, `Cargo.lock`, `numpy.__version__`, `cv2.__version__`) and the device or driver identity.
  - `xtask exactness` and `xtask conformance` run with `memo: "off"` and a fresh store.
  - Add `memo.audit`, which recomputes a sample of hits.

### H5. VI UI code has no home outside core, so G4's "no-VI-in-core" exit can't be met

- **The UI code in question:**
  - `export_view.rs` renders `ExportReply`.
  - `mask_view.rs` carries VI semantics: `label`, `lo`, `hi`, the `<workflow>.edits/` sidecar and the digest check.
  - `curves.rs` holds the curve maths.
  - `bundle_view.rs` and `groups_view.rs` (checkpoint and codegen gating) are VI UI.
  - The recipe chunk `research.recipe` (F77) is VI-named.
- **Where the plan puts it:**
  - Plan 2.2's `packs/vi/` lists no Rust UI crate.
  - Plan 2.3 makes the curve maths "the core definition". That conflicts with note 04, where the pack registers the curve and mask editors at startup, and with golden rule 6, which says the maths belongs in the research software.
- **Fix:**
  - Define a `PackUi` registration API in `wirecraft-ui` covering editors by `editor.kind`, result windows by `x.<pack>.<op>`, extra commands and texts.
  - Add a `packs/vi/ui` crate for export, curves and mask.
  - Name the recipe chunk `<app_id>.recipe`, and keep reading `research.recipe`.
  - For curves, the engine declares the interpolant and ships the sample fixture. Core gets a generic monotone-spline editor only when the interpolant is declared.
  - Re-cost G4 for this work: +3–4 days.

### H6. `EngineCore: Send` doesn't fit `research-local`, and `catch_unwind` alone is not "never crash"

- **Evidence:**
  - `engine.rs:128-144` builds `Rc<Devices>`, `Context` and `Executor` on the worker thread. `Rc` is not `Send`, so `impl EngineCore for ResearchLocal` with that state won't compile under the spec's trait bound.
  - Today there is no `catch_unwind` at all.
  - After a caught panic, the executor's memo and the wgpu device may be inconsistent. wgpu's default uncaptured-error handler panics.
- **Fix:**
  - The host takes a factory, `Box<dyn FnOnce() -> Result<Box<dyn EngineCore>, String> + Send>`, and runs it on the worker thread. Drop `Send` from the trait.
  - After a caught panic, drop and rebuild the core, emit `engine_down`, then `connected`, and fail the run in flight.
  - Install a non-panicking wgpu error handler.
  - Add a regression test that panics inside a node.

### H7. A fixed 300 s reply timeout contradicts ack-plus-progress and fails long runs falsely

- **Evidence:**
  - Spec 6.1 gives `run` "ack 10 s; reply 300 s", and spec 12.3 then synthesises `RunReply{ok:false, kind: engine}`.
  - Remote runs, audio stretch, `x.vi.deliver` and large renders can exceed 300 s while they are still emitting events.
  - The real reply then arrives after the synthesised failure. Nothing says which one wins.
- **Fix:**
  - After the ack, time out on inactivity: no event and no `ping` for N seconds, with N set per binding.
  - Before synthesising a failure, try `run.get`.
  - A real reply of the current generation replaces a synthesised failure.
  - Keep the 300 s ceiling only for protocol 1 engines, which have no ack.

### H8. A cancel that overtakes its run is silently lost

- **Evidence:**
  - Spec 11.2 says "cancelling an unknown ... run is not an error".
  - On HTTP (`ThreadingHTTPServer`, with cancel on its own connection) and on AMQP (a fanout broadcast against a run not yet delivered to Plan), a cancel can arrive first. The run then executes in full.
  - Supersede (F29) sends `cancel(old)` and then `run(new)`, so under a burst of auto-runs the engine runs stale graphs serially. That is wasted work and delays the current one.
- **Fix:**
  - Engines keep tombstones of cancelled `run_id`s for `run_ttl_s`. A tombstoned run ends at once with `cancelled`.
  - The host also drops queued, not-yet-sent runs when it supersedes them.
  - Add a conformance case: cancel sent before the run.

## Medium severity

### M1. `display.stream` doesn't fit the 4 KiB status-event inline budget

A Lab display carries up to 4 channels of 256-bin histograms (about 1.3 KB of JSON each) plus up to 4 renditions with three locators each. That is about 6–10 KB. Under spec 10.2 it becomes a DataRef, which adds a fetch and a parse on the path the plan sells as faster.

**Fix:**

- Give `display` on `preview` records its own budget (`max_display_inline`, 64 KiB, local bindings).
- Or move histograms into a `stats` blob.
- Measure with the Lab fixture.

### M2. "Data in flight gets stronger" covers only the UI for the research software

No milestone makes the Python executor or `research_graph` emit `progress`, `stage` or `snapshot`. Neither engine emits `progress` today (floor F39).

**Fix:**

- Add G3/G6 tasks:
  - per-list-item `progress` from both executors;
  - The research software's commands stage snapshots through the research software hooks, fixed at source (rule 7);
  - `Cx::snapshot` in `research_graph`.
- Exit criterion: watching a pencil-style node shows its stages before `done`.

### M3. Orphaned remote runs and undefined multi-client sessions

- **Evidence:**
  - Runs survive a dropped connection for 10 min (spec 16.4), and `max_runs: 1`. A UI that crashes mid-run blocks the next session's runs for up to 10 min.
  - The spec doesn't say whether session A sees session B's events, how generations from two UIs interact, or who may cancel which run.
- **Fix:**
  - A session owns its runs.
  - Cancel a run when its session has had no subscriber or poll for 30 s, unless the run was sent with `detach: true`.
  - Events are scoped per session.
  - Cancel is allowed only by the owning session, or by a token holder with an admin capability.

### M4. Append streams can't be recovered after a resync or gap

Spec 10.6 says "a gap ... makes the viewer fetch the full value once", but no full value exists before `done`, and `run.get` carries no live state.

**Fix:**

- `run.get` returns `{offset, partial: DataRef}` per (node, port) being appended.
- Engines keep append buffers until `done`.

### M5. Remote thumbnail latency

In a remote run each thumb is a `url` locator. The client then makes one GET per thumb, hashes it, publishes it into the local store, maps it and uploads the texture. A 100-node graph pays about 100 round trips.

**Fix:**

- On remote sessions, inline encoded thumbs of 4 KiB or less (QOI or WebP at 80 px) in `preview` records.
- Add `blob.get_many`, or rely on HTTP/2 multiplexing.
- Keep remote thumbs in memory and don't publish them to commonkit.

### M6. "Publish into the local store" needs a Rust commonkit writer that doesn't exist

`crates/shm` only reads: `Key::parse`, `open`, `NpyView`. commonkit's layout, index, TTL and reaper rules belong to the Python commonkit, and the plan doesn't estimate a writer.

**Fix:**

- The UI publishes downloads only into its own durable tier.
- Or specify a commonkit writer at source (golden rule 7) and add 2–3 days.

### M7. Mode 4 remote and mode 2 bytes are not concrete

- Spec 16.2 says "a remote fleet serves blobs through the HTTP blob endpoints from its CAS root". No milestone builds that server, and `research.yml` has no such service.
- Socket `blob.put` and `blob.get` are "binding-specific", but the binding has no byte framing.

**Fix:**

- Name a blob daemon (Python stdlib, shared with the mode 3 front) as a `research.yml` service in G8.
- Declare same-host REQUIRED for mode 2. When the proof fails, fail closed with a clear `unavailable`, rather than inventing byte frames.

### M8. Multi-engine graphs are the shoehorn left for later

Under D14 a workflow has one engine. Joining engines "by CAS handles" later needs run inputs that are DataRefs bound to ports, and a Session that holds several clients. Today `RunState` assumes one engine.

**Fix, reserved now:**

- `RunRequest.inputs: {"node.port": Value}` as external bindings.
- An optional per-node `engine` field in the workflow.
- `Session` holds `BTreeMap<EngineId, EngineClient>`, even if v1 uses one.

### M9. Control-channel shapes change despite F81's "unchanged"

`ui.inspect` serialises `engine_status` directly (`control.rs:156`). Several things change shape:

- `BrokerUnreachable` becomes `unreachable`;
- `Connected` gains `caps`;
- `InspectRequest` becomes `WatchRequest`;
- `Display` v1 becomes v2.

The maintainer's journals and replayed regression tests (golden rule 8) depend on these shapes.

**Fix:**

- Either keep a v1 projection for the control channel, or version the control protocol and migrate the journals.
- Add a journal-replay test in G0.

### M10. Estimates are below the plan's own notes

| Item | Plan | Source note | Note |
|---|---|---|---|
| G6 viewer host and live data | 12–14 | 04: 14–16 core path | The plan is under its own evidence |
| G0 84 F-items as tests, with live checks on both engines | 4–5 | — | 8–10 is realistic: fixtures, recorded replays and two live engines |
| G3 AMQP host and Python split | 5–6 | 06: Python SDK 5–6 alone | Plus H1's rework: 8–10 |
| G4 extraction | 12–15 | FloCraft 30–40 overall | Plus H5's pack UI: +3–4. Uses the text engine before the SDK exists (G7): sequencing conflict |
| G9 audio engine and viewers | 8–11 | 06: 6–8 engine and viewers; 04: about 7–8 viewers | Tight |

Realistic figures are 65–80 days to "the research software on the base, nothing lost" and 130–160 days in total. Either write the G4 text engine by hand against `EngineCore` and accept the rewrite in G7, or move a minimal SDK (`Node` trait and executor, without the macro) into G4.

### M11. Three executors must agree on semantics and memo keys

Python, `research_graph` and the new SDK executor ("lifted" in G7) each implement spec 7.6 and memo keys, and `research-local` stays on `research_graph`. Drift is likely.

**Fix:**

- Lift the generic executor once, in G4/G5, into a crate that both `research_graph` and the SDK use.
- The graph corpus runs against all three.

### M12. Mode 2 is never exercised with the research software's data

Only the text engine runs in mode 2, so the plan never shows that a separate-process Rust engine is efficient on 8 MB arrays.

**Fix:** in G7, `research-local` also runs as a mode 2 engine (`--connect unix:…`) under conformance and the 9.1 benchmark.

## Low severity

| Id | Issue | Fix |
|---|---|---|
| L1 | Reducer orders by `(ts, seq)`. Under M9 fan-out, Plan and the workers have different clocks and per-run seqs, so a `started` from a lagging worker is dropped as "older". Envelope `seq` assigned on receipt can't dedupe AMQP redelivery (spec 9.5 rule 5) | Order within a node by `(publisher, publisher_seq)` and then by state rank. Keep the terminal rule |
| L2 | The UI-side queue is "bounded" while ordered classes are "never dropped". When it is full, the engine thread must either block, which breaks "never blocks", or drop | Ordered classes overflow to `resync`. Only the conflated slots are bounded |
| L3 | `nd1` hashes the pointers of object and structured dtypes, which can give false hits with persistence. Big-endian normalisation doesn't say whether `dtype.str` changes. A raw blob wrapped as a 1-d `u1` array has two identities | Refuse object dtype. Hash the normalised `<` dtype string, with a test vector. Key wrapper segments by the raw digest |
| L4 | "Memo keys come from inputs the engine already trusts" is wrong with several clients: parameters and templates are client-controlled, so chosen-prefix collisions apply | Use `b3` memo keys whenever an engine accepts more than one client token |
| L5 | `region` and `probe` readouts inline as JSON can't carry NaN, Inf or −0.0 exactly | Return regions as `.npy` DataRefs (`inline` b64 if small) |
| L6 | New actions are not listed as commands: freeze pin, view as, peek, engine select, connect, restart and attach, upload cancel, token entry (golden rule 3) | List them in G6/G7 with `panic_hunt` coverage |
| L7 | Windows named pipe in G7, although the development machine is macOS and a remote server is Linux | Drop it to save about 1 day |
| L8 | `run.watch` for a node that has already finished doesn't require a display, so clicking a done node during a run shows nothing until the reply | `run.watch` MUST emit displays for already-finished watched nodes |
| L9 | Polling `run.get` every 100 ms returns the whole snapshot (300 nodes of DataRefs, about 100 KB+) | Poll `events.poll{since}`, and use `run.get` only on resync |
| L10 | Not covered by the floor: `research-cli models install`. The F57 T16 window depends on the `inspector-window` work being merged first | Add F85 (local model management is engine-specific: `x.vi.models`, or stays CLI-local). Gate G0 on T16 merged |

## What held up

- Typed in-process messages do remove the `serde_json::Value` round trip (`engine.rs:85`, `:156`).
- The reducer rules are carried forward verbatim (`status.rs:82-152`).
- The `nd1` recipe matches `publish.py:26-30` exactly (dtype string, `repr(shape)`, `ascontiguousarray` bytes), so existing `impc` keys stay valid.
- Cancel stays out of band on every binding.
- Arrays stay out of messages.
- The two-way same-host proof is sound.
- The JSON tagged-union and string-key rules do keep gRPC a mapping exercise.
- The example engines (text, OpenCV, audio, VectorCraft) are specified concretely enough to build. VectorCraft depends on the craft-libs `params_schema` proposal landing.
