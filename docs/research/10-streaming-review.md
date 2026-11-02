# 10: Adversarial review of note 09 (streaming in queued chunks)

*2026-10-27. Review of `docs/research/09-streaming.md` as written the same day, against `AGENTS.md`, `docs/plan.md` (D1–D24), `docs/interface.md` (protocol 2), the code at `bbfd4ea` (committed files only; the uncommitted work in `crates/session` and `crates/ui` was ignored), the floor inventory–08, pipekit, commonkit, the research software supporting www.virtimp.com and SoundCraft. Per the maintainer's correction, question 2 judges technical soundness and internal coherence, not whether today's use cases justify the work. Nothing in the repository was changed except this file and its PDF.*

## Summary verdict

**Rework before acceptance.** The outer shell is right and should be kept. That means: a stream run is a `run` (D26); status is per stream, never per chunk; taps to the UI are lossy and never backpressure the graph; there is no memo for live sources; and the work is proved on the text engine first (D34). The inner model is not yet sound, and several claims about the code are wrong.

1. **"A stream is a list that arrives over time" is false under spec 7.6.** A list into a single-item port *maps* the node over the items. Note 09's stream into a `whole` port *accumulates* the chunks into one value. Those are opposite semantics. The note also uses "chunk" for two different things: a slice of one value (audio frames, characters) and one item of a collection (video frames, list items). Accumulate means concatenation for the first and collection for the second. Concatenation is type-specific, and core can't define it without naming engine types (golden rule 3).
2. **The ordering and determinism guarantees contradict each other or are missing.** `text.replace` is classed as `map`, but it can't be chunk-boundary independent. A `zip` join keyed by `seq` breaks behind any filter, because a filter renumbers `seq`. The deadlock watchdog ("every node blocked") can never fire in a graph with a push source. The `latest` and `merge` joins are nondeterministic even on file sources, yet those streams are memoised.
3. **The shared-memory ring (D29) rests on a crate that doesn't exist and on a read model that can't work.** No crate is allowed `unsafe` (`Cargo.toml:55`, `unsafe_code = "forbid"`). `plumcraft-shm` is only a row in plan 2.1. A seqlock-stamped slot can't be both mapped zero-copy and checked for torn reads. The existing `inline` locator already carries live audio taps.
4. **Several protocol-2 invariants break, and section 11 doesn't list them.** These include conformance invariant 4 ("`preview` precedes downstream `started`"), F47, reducer rule 4's recovery-by-reply, and the non-`Send` `EngineCore` factory.

The scheduler choice (a thread per node) is reasonable for a first version. For audio, though, the established answer is one pull-rendered block graph per callback (SoundCraft's `MixEngine`, fundsp's `Net`), not queues between threads. Of the existing frameworks, **borrow GStreamer's in-band event model and queue limits, dora-rs's per-input `queue_size`/`queue_policy`, and timely's frontiers. Use crossbeam as the in-process substrate.** None should replace the protocol-2 interface. dora-rs and GStreamer could each sit *under* it inside a specific engine.

## Question 1: does it fit the architecture?

### Where it fits

- **D26, a run with a `stream` field.** This keeps ownership (D23), tombstones (spec 11.2), `run.get`, `detach` and the inactivity rule (spec 12.3) without a second lifecycle, and it is the right call. Counting `stream.stats` as activity for spec 12.3 is correct.
- **Status per stream, not per chunk** (09 §5.3). This keeps the reducer's load where spec 9.5 expects it.
- **Lossy taps outside the resume ring** (09 §5.4). Spec 9.7's "the executor never blocks on a slow UI" demands it. The note correctly sees that today's ordered `append` class (spec 9.2) can't carry an unbounded tap.
- **Capability-gated** (09 §5.5). This follows golden rule 5: additive changes are capabilities. A `whole`-only engine is untouched, so streaming stays opt-in per engine, and the SDK cost falls only on engines that declare stream nodes.
- **Commands in `stream_cmds.rs` with `panic_hunt`**, plus control `v2`-only fields (D21). This is consistent with golden rule 4.

### Claims checked against the code

| Claim in 09 | Finding |
|---|---|
| `Cx` holds `&mut dyn FnMut(StatusRecord)`, not `Send` (§8.1) | **True**: `crates/engine/src/lib.rs` `Cx { records: &'a mut dyn FnMut(StatusRecord) }`; it also borrows `cancel` and `node_id` for `'a`, so it can't outlive the call or cross threads |
| One worker, one job at a time (§8.1, §11) | **True**: `host.rs:223` spawns one `plumcraft-engine` thread; Python `host.py:114` likewise |
| `max_runs: 1` (§11) | **True**: `crates/proto/src/session.rs:195` |
| "`plumcraft-shm`, the crate already allowed `unsafe`" (§1, §6.1, D29) | **False.** No such crate exists (`ls crates`: audio automation engine graph host proto session store text ui). The workspace forbids unsafe (`Cargo.toml:55`). `crates/store/src/lib.rs:10` says blobs are "read, not mapped". AGENTS.md rule 2 permits only "a *future* shared-memory reader crate", meaning a *reader*, for `Mmap::map`. A writable ring shared across processes is a different and larger exception |
| "today's audio engine refuses anything over 10 minutes" (§2) | **Imprecise**: `MAX_FRAMES = 600 * 192_000` (`crates/audio/src/lib.rs:52`) is 10 minutes at 192 kHz, which is 40 minutes at 48 kHz |
| "the UI already resamples linearly to the device rate" (§3.4) | True, but **whole-clip** (`playback.rs:249`, `resample_linear` over the clip). There is no streaming resampler and no drift correction to build on |
| `fx.rs` renders block by block at 1024 frames (§8.3) | True (`fx.rs:21`), and checked block-for-block only against SoundCraft's own 1024-frame render. Equivalence at other block sizes, which the chunk-independence contract needs, is unproven |
| pipekit queues are unbounded; `queue_arguments` exists (§5.7) | True: `topology.py:377` derives only `x-max-priority`. The proposed `x-max-length`/`x-overflow` is a real pipekit change |

### Conflicts the note missed

| Where | What breaks | Note 09 says |
|---|---|---|
| Spec 9.4 and conformance invariant 4, F47 | "The preview record for a node is sent as soon as that node finishes, **before downstream nodes run**"; "`preview` precedes downstream `started`". In a stream every node is `started` before its upstream is `done` | Nothing. The invariant must be scoped to non-stream runs, or redefined (a stream node's `started` follows its upstream's `started`) |
| Spec 9.5 rule 4 and the "MAY lose" paragraph | Lost or reordered `node.status` events are repaired by the authoritative reply. An unbounded run has no reply, so a lost record stays wrong until stop | "The reducer is untouched." The code is untouched but its guarantee is not: stream runs need a periodic `run.get`, or `stream.stats` must carry state so it is self-healing |
| Plan 3.2, `CoreFactory`; plan 2.2 | `EngineCore` is deliberately **not `Send`** (the research software's `Rc<Devices>`, GPU contexts); the factory builds it on the worker thread. A thread per node needs every stream node on its own thread | Only the `Cx` emitter. A non-`Send` engine can't run option A at all. It needs a single-thread cooperative scheduler, which the note ranks last |
| Spec 16.1 "Panics", conformance 12 | A panic rebuilds the engine from the factory. With N node threads holding `Arc<Engine>`, the rebuild has to stop every thread, discard queues and fail the run | Not addressed |
| Spec 8.6 `run.peek`, 8.7 `run.watch` | Both are defined over values that "exist" or nodes that "already finished". A stream port's value never exists as a whole | Taps are defined; `peek`/`watch` on a stream port are not |
| Spec 8.1 `inputs` | `inputs` is `{"node.port": Value}`, values that override a port. 09 §5.2 puts a stream *descriptor* there, which is a different type in the same map | Overloads a reserved field. Use `stream.inputs` instead |
| Spec 3.7 and 4.4 "bytes never travel in messages" | `stream.push` from the UI microphone to a remote engine carries audio in a JSON request | 09 lists only ring and AMQP in-band exceptions |
| Spec 11 rule 3 | "checks the run's cancel token between nodes **and between list items**". The stream analogue is between chunks, on every node thread | Implied, not stated |
| `crates/session/src/run.rs` `apply_reply` | Staleness clears only on a reply (`run.rs:344–372`), and `progress()` counts finished nodes. A stream never "finishes" for the canvas progress bar | `StreamState` is named; the semantics of stale-while-streaming are not |
| `plumcraft-graph` `accepts` | Type rules must learn the stream flag, accumulate insertion and window requirements so the editor can type a graph exactly as engines run it (spec 7.6 preamble) | §3.2 says the rules are "reused"; the UI must also *predict* accumulate badges, so these rules move into the shared corpus |

### Is "a stream is a list" compatible with spec 7.6 and the executor?

No, and the note's own rules show it. Spec 7.6 rule 2 says "a list into a single-item port **maps** the node over the list". 09 §3.2 rule 2 says "a stream into a `whole` port gets an implicit **accumulate**". Under the list analogy, `audio.normalize` fed a stream would run once per chunk and normalize each block on its own; under the accumulate rule it runs once on the whole recording. 09 picks the second, which is right for audio and wrong for video frames, where per-frame mapping is the point (09 §8.5). Rule 5 ("a stream and a list into the same node is a graph error") is a symptom: if streams really were lists, the product and zip rules of 7.6 would already define the combination.

The executor implements neither yet. `exec.rs` and `exec.py` have no list mapping at all; `Port.list` exists only in `proto/src/catalogue.rs:209`. So the analogy rests on semantics that are specified but unbuilt. The sound framing is two stream *kinds*, declared per wire type by the engine:

- **Sample streams** (audio frames, characters, table rows): a chunk is a slice of one value, and accumulate concatenates. The engine must declare the concatenation and split for each wire type, because core can't know them (golden rule 3).
- **Item streams** (video frames, files, list items): a chunk is one item. Accumulate collects into a `list: true` value, and a single-item port maps, exactly as 7.6 rule 2 does. This is the only kind that is "a list over time", and it should reuse the list corpus.

### Does complexity leak into every engine?

Mostly no: `whole` is the default and stream classes are opt-in. But three costs land on everyone. The UI and the shared corpus must type stream wires. The conformance suite grows a stream section that every *stream-capable* engine passes in both languages, which adds a third and fourth executor (09 §9 admits this). And the existing `append` class changes meaning through `lossy: true`. That last change is safe only because it is gated by `stream.run`; the note should say so explicitly.

## Question 2: is it technically sound?

### Chunk semantics and chunk-size independence

- **`map` is misclassified for text.** `text.replace` with a pattern that crosses a chunk boundary (`"fo|o"` → `"foo"` never matches) isn't boundary-independent. Neither is `text.lower` when chunks are cut by characters: Rust's `str::to_lowercase` maps Σ to final ς by word context. `grep` is defined only on whole lines. The equivalence test `collect(upper(lines(T))) = upper(T)` passes because `lines` cuts at line boundaries and `upper` happens to be context-free; it proves nothing about `replace`. Fix: a stream port declares its **boundary unit** (`line`, `frame`, `item`, `sample`). A node declares which units it is independent over. The executor inserts `rechunk` to that unit, and the three-size equivalence test runs at sizes that are multiples of the unit.
- **Floating-point equivalence.** Bit-equality "at 64, 1024 and 4096" holds for a biquad or gain. It does not hold for a plugin that smooths parameters or updates coefficients per block (common in SoundCraft-style DSP), and today's audio test checks 1024 only. `chunk_sensitive` plus `rechunk` is the right escape; the default for third-party plugins should be `chunk_sensitive: true` until each is tested.
- **`seq` contiguity vs drops.** 09 §3.1 says `seq` is contiguous per stream, and a filter renumbers. A `drop_oldest` edge removes chunks *after* numbering, so the consumer sees gaps. The invariant should be "contiguous at the producer; gaps are announced by `discontinuity`", and the conformance case should say which.

### Ordering and joins

- **`zip` by `seq` breaks behind any filter or `drop_oldest`.** Both branches renumber or lose chunks independently. `zip` is sound only between branches that are both `map` without filtering and `block` all the way. The executor should reject a `zip` otherwise, not merely insert `rechunk`.
- **`time` join and tails.** "`zip` and `time` end when either input ends". But a reverb's wet branch outlives the dry branch by its tail, so the dry/wet mix truncates the tail in streaming and not in batch. That makes stream ≢ batch for the note's own headline example. A `time` join should end when *all* inputs end, padding the shorter with silence: that is mix semantics.
- **`latest` (sample and hold) and `merge` (by arrival, ordered by `ts`) are nondeterministic** even on deterministic file sources, because pairing depends on thread timing. 09 §6.2 memoises "bounded, deterministic pull sources", but determinism is a property of the whole upstream graph, not the source. Rule needed: any `latest` or `merge` join, any non-`block` edge, any live parameter set, or any pool `map` without a reorder buffer makes everything downstream `memo.scope: none`.
- **The alignment model is ad hoc.** Four join rules, an EOS rule per join kind, latency subtraction and a separate `ts` join for different clocks all approximate one concept: the **frontier** (timely dataflow) or **watermark** (Beam, Flink). That is a per-edge lower bound on offsets still to come. With frontiers, end of stream is the frontier reaching ∞, a `time` join emits up to the minimum frontier, and the deadlock detector can say exactly which input holds a join back.

### Backpressure and deadlock

- **The credit rule is underspecified.** "A node is runnable when every `block` output has room" works for one-in-one-out nodes. `window` (with hop < window), `split`, `flush` of a long tail, and a filter emitting a burst can each emit more chunks than one slot of room. With a thread per node, the correct rule is simply "`push` blocks", and the credit pre-check belongs to option B only. The note mixes the two.
- **Queue sizing from latency doesn't prevent diamond deadlock.** Sizing "to at least its declared latency plus one chunk" assumes both branches emit the same chunk count per unit. A `window` with a hop changes the chunk rate, and a filter emits data-dependent counts. The general result for bounded Kahn networks (Parks, 1995) is that no static sizing is safe for data-dependent rates. The known remedy is dynamic: grow the *smallest full queue* in the deadlocked cycle, up to a cap, and fail only at the cap.
- **The watchdog can't fire with a live source.** "Every node blocked, none runnable, for 1 s" is never true when a push source exists, because its drain thread never blocks: it drops, by design (09 §3.4). A deadlocked join behind a microphone therefore turns into silent, permanent drop counting. Detection must be local: a join whose inputs are blocked against each other for T.
- **Fan-out to `block` consumers runs at the slowest pace.** That is correct, but combined with a `conflate` or `drop_oldest` branch in the same fan-out, the shared `Arc` chunk is fine and the *slow* branch then throttles the source for the lossy one. That is surprising but sound; state it.

### State, tails, seek, pause

- **Seek and purge are out of band.** "The epoch rises, queues are purged of the old epoch, stateful nodes reset" across N threads is a race. A node mid-`process` on an old-epoch chunk may push into a queue already purged. GStreamer's answer is the right one: **in-band** flush events (`FLUSH_START`/`FLUSH_STOP`, then a `SEGMENT`) that travel through the queues in order, so each node resets when the event reaches it. EOS and discontinuity are already in-band in 09 as chunk fields; seek, format change (open question 1: GStreamer's `CAPS` event) and live-parameter changes should be in-band too.
- **Live parameters (D33) need a position.** "Applied at the next chunk boundary" is ambiguous with 8-deep queues: the change reaches the node after up to 8 chunks of latency, and *which* offset it took effect at is unrecorded. Send it in-band from the source position, or record `applied_at: offset` in stats and the reply. Either way a run with a live set is unmemoisable from that offset on.
- **Pause with `block` queues and push sources** is sound as described. Pause of a *pull* source while a `time` join waits on it is the deadlock case again; the frontier rule handles it.

### Status, memo and handles

- **The memo key and the manifest.** Memoising a bounded stream as its chunk manifest is sound. But the manifest's chunk handles depend on chunking, so two runs at different chunk sizes produce different manifests for equal bytes. Key the memo on the *accumulated* content handle, and treat the manifest as a replay aid. The `nd1` shape-first observation is correct; hashing once at end of stream is the sound choice.
- **`cached` stream nodes.** A cached node "replays chunks from the manifest", so in a stream run it becomes a source. Spec 14's `cached` is sent without `started`, and downstream must still stream. That is consistent, but the replay must honour the run's queue policies (a cached upstream into a `drop_oldest` edge can drop).

### The ring (D29)

Beyond the missing crate:

- **Torn-read detection and zero-copy are incompatible.** A slot stamped with `seq` before and after (a seqlock) is validated *after* reading: you copy, then re-check the stamp. Mapping the slot into the viewer zero-copy means the bytes can change under the renderer after the check. In Rust, a `&[u8]` over memory another process mutates is undefined behaviour unless every access is volatile or atomic.
- **The Python engine can't write a seqlock correctly.** numpy writes into an mmap have no release fence between the payload and the trailing stamp.
- **It isn't needed for the stated cases.** A 20 ms stereo `f32` block at 48 kHz is 7.5 KiB. Spec 10.6 batches appends to 50 ms, which is 19 KiB per tap per 50 ms, about 0.5 MB/s in base64. That fits the existing `inline` locator (spec 4.4) on the socket binding with no new mechanism. Video taps are `conflate` at 10–30 Hz and want a downscaled `rgba8` rendition, which a commonkit blob carries well under the reaper budget. Data *between* nodes in one engine never leaves the process (09 §5.6), so the ring serves only taps. Measure inline taps before building D29.

### Distribution (D30, Q6)

- **`x-overflow: reject-publish` doesn't make the producer wait.** RabbitMQ nacks the publish and discards the message, and the publisher has to retry with backoff. That is polling, not credit, and it reorders unless the producer serialises its retries.
- **The Q6 exit criterion "a killed worker's chunks redeliver without reordering a stateful node's input" is insufficient.** Redelivery restores the *chunks* but not the dead worker's *state* (filter memory, reverb tail, line counter). The output after recovery differs from a run without the crash. Sound recovery needs state checkpoints aligned with the stream: Flink's barrier snapshots, Chandy–Lamport style. Alternatively, define recovery as "restart the stream from the last checkpointable offset with a `discontinuity`". Q6 specifies neither.
- **Consistent-hash sharding by stream id** gives ordering per stream, but a pipekit node's `PREFETCH` of 10 (`node.py:388`) with `CONCURRENCY > 1` processes deliveries concurrently inside one replica. `stateful` nodes need `CONCURRENCY: 1` too, not only `replicas: 1`.

### The scheduler (D28)

A thread per node is a sound first step for text and files. Two corrections apply:

- **Substrate.** `Mutex<VecDeque> + Condvar` is fine, but crossbeam already provides every policy: `crossbeam_channel::bounded` for `block`, with `select!` for joins and cancel, and `crossbeam_queue::ArrayQueue::force_push` for lock-free `drop_oldest`. Both are MIT/Apache, and `crossbeam-utils` is already in `Cargo.lock`.
- **Audio.** Live audio through N threads and N queues is the worst of both worlds. Every hop adds a block of latency plus scheduling jitter, and the result still isn't real-time. SoundCraft's `MixEngine::render` (one block through the whole graph, "the same code drives realtime playback and offline bounces", no allocation in steady state) and fundsp's `Net` (a runtime-editable graph with a real-time-safe backend and atomics for live parameters) are the known-good design. For audio, the fused island (option C) should be the *default* for `map`/`stateful` chains inside one engine, with queues only at sources, sinks, taps and engine boundaries. This also makes stream ≡ batch hold by construction, because the same render runs both.

## Question 3: existing frameworks

### Under the interface, or instead of it?

No candidate offers PlumCraft's interface: session-scoped ordered events, a status reducer with generations, `run.get` snapshots, CAS handles, and capabilities. Replacing protocol 2 with any of them would discard D2–D24. Two can sit **under** it inside one engine. That engine translates a stream run into the framework's graph, maps its stats onto `stream.stats` and its EOS onto `done`, and stays opaque to the editor:

- **dora-rs** for a distributed or multi-language engine;
- **GStreamer** for a video and media engine.

Everything else is better borrowed than adopted.

### Notes per candidate

- **dora-rs** ([github.com/dora-rs/dora](https://github.com/dora-rs/dora); 1.0 announced 2026; Apache-2.0; Rust, Python 3.11+, C, C++). Nodes are processes, and dataflows are YAML. Messages over 4 KB go through Zenoh shared memory, with TCP across machines. Each input has `queue_size`, `queue_policy` (`drop_oldest` or `backpressure`) and `input_timeout`, which is D27's vocabulary almost exactly and independent evidence that it is sound. It has no node-status model, no CAS, and Arrow data rather than `Data`/`DataRef`. It could replace D29, D30 and Q6 for a distributed stream engine, and it handles cross-language shared memory, the hard part, already.
- **GStreamer / gstreamer-rs** ([gstreamer.freedesktop.org/documentation/rust](https://gstreamer.freedesktop.org/documentation/rust); core LGPL-2.1+, bindings MIT/Apache; Python via PyGObject). This is the reference design for media streaming:
  - pads with caps negotiation;
  - in-band events (`EOS`, `SEGMENT`, `FLUSH_START`/`STOP`, `CAPS`, `GAP`);
  - a `LATENCY` query summed over the pipeline;
  - live versus non-live sources;
  - `queue` elements with `max-size-buffers`/`bytes`/`time` and `leaky=upstream|downstream`;
  - `appsrc`/`appsink` to bridge to application code.

  The cost is a large C dependency installed through brew and the GObject type system. It suits a video engine; it doesn't belong in core.
- **Zenoh** ([github.com/eclipse-zenoh/zenoh](https://github.com/eclipse-zenoh/zenoh); EPL-2.0/Apache-2.0; Python bindings; opt-in shared memory; congestion control `Block`/`Drop`). It is a pub/sub transport, and dora uses it. Compared with pipekit it is another protocol and router to run, and RabbitMQ is already the maintainer's broker (D5's reasoning for rejecting NATS applies).
- **timely / differential dataflow** ([github.com/TimelyDataflow/timely-dataflow](https://github.com/TimelyDataflow/timely-dataflow); MIT; Rust only). Its frontier-based progress tracking is the cleanest model for EOS, joins and "has this offset finished". It is built for incremental relational computation with no media types and no Python. Borrow the frontier, not the crate.
- **Hydro / DFIR** ([github.com/hydro-project/hydro](https://github.com/hydro-project/hydro); Apache-2.0; Rust only). Graphs are written in a compile-time macro DSL, which is incompatible with graphs edited at runtime from JSON.
- **Apache Arrow** (IPC stream format, Flight, DataFusion). The IPC *stream* format (a schema message, then record batches, then an end-of-stream marker) is a ready, versioned framing for `TABLE` streams and for the HTTP binary tap endpoint, instead of the custom header 09 §5.6 invents. Flight was already rejected (plan 5). DataFusion's streaming is SQL over record batches, not media.
- **tokio streams / futures channels.** These would make the SDK async, which colours every node, and the SDK is synchronous by design. crossbeam gives the same bounded semantics synchronously. tokio is only a transitive dependency today.
- **Bytewax** ([github.com/bytewax/bytewax](https://github.com/bytewax/bytewax); Apache-2.0). It has been community-maintained since its company stopped being viable in May 2025.
- **Apache Beam / Flink** (JVM). These are counter-examples to the dependency but sources for three ideas: event time with watermarks, windowing semantics, and barrier-aligned checkpoints for stateful recovery (which Q6 needs).
- **NATS JetStream / RabbitMQ Streams.** RabbitMQ streams (`x-queue-type: stream`) run on the installed 4.3 broker and give offset replay and size or age retention, the right tool for replayable taps (09 D30(c)). NATS is a second broker, as D5 already decided.
- **ROS 2** (DDS). It is heavy and robotics-centred. Its QoS vocabulary (`reliable`/`best_effort`, `keep_last(depth)`, `deadline`, `liveliness`) is a well-tested naming for exactly D27's knobs.
- **NNStreamer.** It is GStreamer plugins for tensors, LGPL. It doesn't fit except through GStreamer.
- **fundsp** ([github.com/SamiPerttu/fundsp](https://github.com/SamiPerttu/fundsp); MIT/Apache; 0.23). `Net` is a runtime-editable audio graph split into a frontend (edits, `commit`) and a real-time-safe backend, with 64-sample SIMD blocks and `shared()` atomics for live parameters. It is the design pattern for live audio in a node editor. The DSP itself already comes from SoundCraft.
- **SoundCraft's own `MixEngine` and `playback`** (the maintainer's code). It renders blocks with no allocation, is offline and realtime identical, and has `CaptureRing`/`InputRing`. This is the strongest local precedent, and 09 cites only the rings.
- **Faust.** It is a DSP language compiler (GPL with an exception for generated code). Wrong layer.
- **GNU Radio / FutureSDR.** GNU Radio (GPL-3) is the longest-running thread-per-block stream scheduler, with *stream tags*: metadata pinned to item offsets that travel in-band, which is a better answer to live-parameter positioning than chunk-boundary application. FutureSDR ([github.com/FutureSDR/FutureSDR](https://github.com/FutureSDR/FutureSDR); Apache-2.0, experimental) is its Rust analogue, with pluggable schedulers and custom buffers.

### Framework verdicts

| Framework | Licence | Rust / Python | Would replace in 09 | Impedance with protocol 2 and CAS | Verdict |
|---|---|---|---|---|---|
| dora-rs 1.0 | Apache-2.0 | Yes / Yes | D28 scheduler, D29 ring, D30 and Q6 distribution | Own daemon and YAML; Arrow not DataRef; no status model | **Borrow** queue vocabulary now; **candidate under the interface** for a distributed stream engine instead of Q6 |
| GStreamer (gstreamer-rs) | LGPL-2.1+ core, MIT/Apache bindings | Yes / Yes (PyGObject) | Stream executor for media engines | C dependency, GObject, caps not wire types | **Borrow** in-band events, queue limits, latency query; **adopt under the interface** in a video engine if one is built |
| Zenoh | EPL-2.0 / Apache-2.0 | Yes / Yes | D30 transport | Another protocol and router | Reject now; revisit only through dora |
| timely / differential | MIT | Yes / No | Join and EOS rules | Relational, no media | **Borrow frontiers**; reject crate |
| Hydro / DFIR | Apache-2.0 | Yes / No | Executor | Compile-time graphs | Reject |
| Arrow IPC stream format | Apache-2.0 | Yes / Yes | Custom tap framing (09 §5.6) | Good for tables; heavy crate | **Borrow** the framing for `TABLE` streams and HTTP taps |
| Arrow Flight / DataFusion | Apache-2.0 | Yes / Yes | — | Very heavy; no status model | Reject (as plan 5) |
| tokio streams | MIT | Yes / No | Queues | Async colours the sync SDK | Reject as SDK substrate |
| crossbeam (channel, queue) | MIT/Apache | Yes / — | `Mutex<VecDeque>+Condvar` | None; sync, tiny | **Adopt** for option A |
| Bytewax | Apache-2.0 | — / Yes | Python stream executor | Company gone 2025; timely underneath | Reject |
| Beam / Flink | Apache-2.0 | No / partial | — | JVM | Reject; **borrow** watermarks, windows, barrier checkpoints |
| NATS JetStream | Apache-2.0 | Yes / Yes | D30 | Second broker | Reject (D5) |
| RabbitMQ Streams | MPL-2.0 (broker) | via AMQP / via pipekit | Tap replay | Installed already | **Adopt** for replayable taps only, as 09 says |
| ROS 2 | Apache-2.0 | Yes / Yes | — | DDS, heavy | Reject; **borrow** QoS names |
| NNStreamer | LGPL-2.1 | — / — | — | GStreamer plugins | Reject |
| fundsp | MIT/Apache | Yes / No | Audio stream nodes and live params | Engine-internal only | **Borrow** the Net frontend/backend pattern for audio islands |
| SoundCraft MixEngine | maintainer's | Yes / No | Audio streaming executor | Engine-internal | **Borrow** (block render, offline = realtime) |
| Faust | GPL + exception | via C++ / — | — | DSP language | Reject |
| GNU Radio / FutureSDR | GPL-3 / Apache-2.0 | C++, Rust / Python (GR) | Scheduler | GR licence; FutureSDR experimental | **Borrow** stream tags; reject as dependencies |

## A sounder, simpler design for the same capability

Keep the shell of note 09: a run with `stream`, lossy taps, per-stream status, `stream.stats`, capabilities, text first. Change the core:

- **Two stream kinds, declared by the engine per wire type.** Sample streams declare `concat`, `split` and a boundary unit. Item streams reuse spec 7.6's list rules over time, so a single-item port maps. Core types both without knowing the types.
- **Three node contracts, not seven.**
  - `process(chunk) → chunks` with optional state, `flush` and `reset`. This covers `map`, `stateful` and `window`; windowing is an SDK helper, and `map` is a flag allowing parallelism.
  - `source`.
  - `whole`.

  `reduce` is a stateful node that emits only at flush, and `sink` is a stateful node with no outputs.
- **Everything ordered travels in-band.** Data, `eos`, `discontinuity`, `flush` (seek), `caps` (format change) and `param` (a live parameter at an offset, as GNU Radio tags) all go as items in the same queue, so each node sees them in stream order. Out of band are only cancel (abort everything) and stats.
- **Frontiers instead of four join rules.** Every edge carries a frontier (the lowest offset still to come; ∞ at end). There is one `time` join that emits up to the minimum frontier of its inputs, plus an explicit, nondeterministic `latest` join that marks downstream `memo.scope: none`. `zip` by `seq` and `merge` are dropped from v1.
- **Executors.**
  - For text and files: a thread per node over crossbeam, where `push` blocks and the queue that grows is the smallest full one in a blocked cycle (Parks), up to a byte cap, then a named graph error.
  - For audio: a fused block render (the SoundCraft `MixEngine` pattern) inside the audio engine, with queues only at sources, taps and sinks.
  - Non-`Send` engines get the fused or cooperative single-thread form.
- **Taps inline first.** Use the existing `inline` locator for audio taps and commonkit blobs for conflated frame renditions. Build no ring until a measurement says so. If one is built, it copies out under a seqlock and never maps zero-copy, and it gets its own reviewed `unsafe` exception, not an assumed one.
- **Determinism stated as a graph property.** A stream subgraph is memoisable only if its sources are pull and deterministic, every edge upstream is `block`, there is no `latest` join and no live parameter. The key is the *accumulated* content handle.
- **Distribution as an engine, not a binding change.** Distribution is an engine whose internals are dora-rs (or pipekit once bounded queues and checkpoints exist), with recovery defined explicitly as a restart from a checkpointed offset with a `discontinuity`.

## Changes required before note 09 is accepted

1. **Drop "a stream is a list that arrives over time"** as the model. Define sample streams and item streams, with item streams following spec 7.6 rule 2, and engine-declared `concat`/`split`/unit per wire type (golden rule 3).
2. **Correct the false claims.** `plumcraft-shm` doesn't exist and the workspace forbids `unsafe`. `MAX_FRAMES` is 10 minutes only at 192 kHz. The UI resampler is whole-clip. Restate D29 as needing a new reviewed `unsafe` exception (AGENTS.md rule 2 allows a *reader*).
3. **Add the missed conflicts to §11:**
   - conformance invariant 4 and F47 (preview before downstream started);
   - reducer rule 4's recovery without a reply;
   - the non-`Send` `EngineCore` factory (plan 3.2);
   - panic rebuild with node threads (spec 16.1);
   - `run.peek`/`run.watch` on stream ports;
   - the overloaded `RunRequest.inputs`;
   - bytes in `stream.push`;
   - staleness and progress in `session/run.rs`;
   - stream typing in `plumcraft-graph`.
4. **Reclassify `text.replace`** (and character-chunked `lower`, `upper`) as unit-bound. Add a boundary unit to stream ports, and make the equivalence test use a pattern that crosses chunk boundaries.
5. **Fix the joins.** Restrict `zip` to unfiltered `block` branches or drop it. End `time` joins when all inputs end, so reverb tails survive. Mark `latest` and `merge` nondeterministic and unmemoisable. Consider frontiers in place of the per-join EOS rules.
6. **Replace the deadlock design.** "Every node blocked" can't fire with a push source. Use local join-wait detection and Parks-style growth of the smallest full queue up to a cap. Make the credit rule "push blocks" for option A.
7. **Move seek, format change and live parameters in-band**, with offsets. Record `applied_at` for `stream.set`, and make runs with live sets unmemoisable from that point.
8. **Rewrite D29.** Measure inline taps first. If a ring is built, specify copy-out seqlock reads (no zero-copy mapping), a Rust-only writer, and the `unsafe` exception.
9. **Rewrite D30/Q6 recovery.** `reject-publish` is not credit. Stateful recovery needs checkpointed state or a declared restart with a `discontinuity`. Stateful pipekit nodes need `CONCURRENCY: 1`. Evaluate dora-rs as the distributed substrate before committing to pipekit queue changes.
10. **Make the fused island the default for audio chains** (D28(c) for audio, (a) for text and files), citing SoundCraft's `MixEngine`. State that stream ≡ batch for audio holds by running the same render in both.
11. **Default `chunk_sensitive: true` for the 25 SoundCraft plugins** until each passes the multi-size equivalence test.
12. **Specify the determinism rule** for memo eligibility (09 §6.2) as a property of the upstream graph, and key bounded stream memo entries on the accumulated content handle, not the chunk manifest.
13. **Add to the conformance list:**
    - a deadlock case behind a live source;
    - a `time` join with a tail;
    - a seek under load (no old-epoch chunk after `flush`);
    - a lost status record during an unbounded run, repaired without a reply;
    - a panic in one node thread.
