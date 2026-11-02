# 09: Streaming in queued chunks

*2026-10-27. Research and design note, a plan only: nothing here is built. The maintainer asked: "plan processing data in queued chunks, to deal with continuously streaming data. Do not implement, just plan." Read for it: `AGENTS.md`, `docs/STATUS.md`, `docs/plan.md` (sections 3, 4, 10.1–10.4, 11 and the source-changes table), `docs/interface.md` (sections 3, 5.3, 5.5, 6, 7.6, 8, 9, 10.6, 11, 12.3, 13, 14, 16, 19); the code in `crates/engine` (`lib.rs`, `exec.rs`, `host.rs`), `crates/host`, `crates/session/src/run.rs`, `crates/audio`, `crates/text`, `python/src/plumcraft_engine`; the floor inventory–08; pipekit (`CLAUDE.md`, `src/pipekit/node.py`, the research software's `docs/pipekit-changes.md`); commonkit's README; SoundCraft's `crates/dsp` (`Plugin`), `crates/playback` (`lib.rs`, `record.rs`).*

## 1. Summary

- **A stream is a list that arrives over time.** A port can be marked `stream: true`, as it can be marked `list: true` today. A stream carries **chunks**: a sequence number, an offset and length in the stream's own units (frames, characters, lines, items, video frames), a capture timestamp, an end-of-stream flag, and one value, inline or by DataRef. Batch-over-a-list is the degenerate case: a finite stream whose chunks are the list's items, all of which are already there.
- **Nodes declare how they stream.** There are seven classes: `map` (stateless), `stateful` (carried state, a tail flushed at end of stream), `window` (overlap and hop), `reduce` (one value at the end, with running snapshots), `source` (pull or push), `sink`, and `whole`. `whole` is the default, so every existing node keeps working unchanged: the executor puts an **accumulate** adapter in front of it for a bounded stream, and refuses an unbounded one unless the wire names a window. A node may offer both a whole and a stream implementation under one id, so `audio.gain` processes a clip when wired from a value and blocks when wired from a stream.
- **Every edge is a bounded queue with a policy:** `block` (lossless; backpressure), `drop_oldest` (bounded latency), or `conflate` (latest only). These are the delivery classes the spec already has (`Ordered`, `DropOldest`, `Latest`), applied to data. `block` is the default between nodes. Backpressure is **credit-based**: a node runs only when every one of its output queues has room. A pull source (a file) simply stops reading. A push source (a microphone, a socket) can't be stopped, so its outlet is a lock-free ring that drops and counts what it drops, as SoundCraft's `CaptureRing` does. **The UI never backpressures the graph**: taps to the UI are lossy.
- **A stream run is a run.** The `run` op gains a `stream` field. The ack, `run_id`, generation, `run.get`, cancel, tombstones, session ownership and `run.reply` all stay as they are. Four things are added: `stream.control` (pause, resume, stop and drain, seek), `stream.set` (live parameters), `stream.push` (a source fed by the UI, such as a remote engine listening to the editor's microphone), and one event kind, `stream.stats`, conflated per node (rates, queue depths, lag, drops, busy time). Status stays **per stream, never per chunk**: `started` at open, `progress` while it flows, `done` after the flush at end of stream. The reducer is untouched.
- **Chunk bytes:** inline for text; a **ring locator** for audio and frames on one host (a per-stream mmap'd ring in `plumcraft-shm`, the crate already allowed `unsafe`); DataRefs to blobs only for large chunks and for whatever is kept. Memo: none for live sources; a bounded, deterministic stream is memoised as one output (its chunk manifest), never chunk by chunk in v1.
- **Thin first:** text lines streaming through two or three nodes in modes 1 and 2, held to a conformance set (ordering, backpressure, drop policies, cancel, stop and drain, end of stream through fan-out and fan-in, and stream-versus-batch equivalence). Then the editor and Python parity, then audio with live monitoring, then distribution over pipekit. About **48–63 days** in all, of which the thin slice is **10–13**.

## 2. Use cases

| Case | Source | Chain | Sink and view | What it demands |
|---|---|---|---|---|
| Live audio | Microphone (push, 48 kHz, about 10 ms device blocks) or a file played at real time | gain → biquad → compressor → reverb → meters | Monitoring (playback), level meter, scrolling spectrogram, a WAV recorder | Bounded latency (50–150 ms end to end), never blocking the capture thread, live parameter changes, drop counts, drift between input and output clocks |
| Long files in blocks | A 2-hour WAV (about 2.6 GB of stereo `f32`), pull | the same chain, run as fast as the CPU allows | WAV writer; integrated LUFS and true peak at the end | Bounded memory (today's audio engine refuses anything over 10 minutes, `MAX_FRAMES`), progress as a fraction, lossless backpressure, results identical to the whole-value graph |
| Text and log streams | `tail -f` of a log, a socket, an LLM's tokens, or lines of a large file | grep → replace → parse → count | Scrolling text with follow and search, running counts, a table of rows | Filters (one chunk in, zero or one out), unbounded runs, cheap inline chunks, a bounded retention window in the viewer |
| Image and video frames | A camera, a video file (OpenCV `VideoCapture`), a folder of frames | per-frame VI style or OpenCV ops; temporal smoothing (stateful) | Latest frame (conflated), a frame strip, an encoder sink | Large chunks (8 MB a frame) by reference, conflated taps, parallel `map` across frames with reordering, frame drops under load |
| Sensors and events | MQTT, serial, a pipekit topic, a timer | debounce, window average, threshold → event | Sparkline, event log, alert | Irregular rates, joins by time rather than by sequence, a slow control stream sampled against a fast data stream |
| Batch over a list | A list value (the research software's list mapping: scales, frames, files) | any `map` node | sheet | The floor's list semantics (F20, F61) must not change; streaming only adds per-item progress and bounded memory |

## 3. The model

### 3.1 Chunks and streams

A **stream** belongs to one output port in one run. When it opens it announces itself once:

```json
{"stream": "r7.n3.audio.e0", "elem": "AUDIO", "units": "frames", "rate": 48000, "channels": 2,
 "bounded": true, "length": 345600000, "live": false, "seekable": true, "clock": "n1.audio"}
```

A **chunk** is a window of it:

```json
{"stream": "r7.n3.audio.e0", "seq": 1041, "offset": 49968000, "len": 2400, "ts": 1792170000.123,
 "eos": false, "discontinuity": null, "value": {"kind": "blob", "ref": <DataRef with a ring locator>}}
```

| Field | Meaning |
|---|---|
| `stream` | `{run}.{node}.{port}.e{epoch}`. The epoch rises on every seek or restart, so chunks from before a seek are recognised and dropped anywhere, as an older generation is dropped today |
| `seq` | u64 from 0, contiguous per stream. A filter renumbers its output; lineage is kept through `offset` |
| `offset`, `len` | Position in the stream's units: frames at `rate` for audio, characters or lines for text, frame index for video, item index for a list. Joins align by these, not by `seq` |
| `ts` | Wall time at the source when the chunk was captured or read. End-to-end latency is the time at a tap minus `ts` |
| `eos` | The last chunk. It may be empty. Every node passes it on after flushing |
| `discontinuity` | `{dropped_chunks, dropped_units, reason: drop or seek or overrun}`: units are missing before this chunk. Stateful nodes may reset; joins re-align |
| `value` | One value of the element type: inline JSON, or `{"kind": "blob", "ref": DataRef}` |

`clock` names the source a stream derives from. Two streams with the same clock and units are aligned by `offset` with no resampling. That is how a join knows that a dry branch and a wet branch of one microphone belong together.

### 3.2 Ports and wire types

- `Port.stream: bool` sits beside `Port.list`. The element type is the port's ordinary `type` (`AUDIO`, `TEXT`, `IMAGE`), so no `STREAM<…>` wire types are needed and the UI's `accepts` rules are reused. A stream wire is drawn differently, but it type-checks on its element type plus the stream flag.
- **New graph semantics** (an addition to spec 7.6, held to a corpus like the list rules):
  1. a stream into a stream-capable port streams;
  2. a stream into a `whole` port gets an implicit accumulate when the stream is bounded, shown as a badge on the wire;
  3. it is a graph error when the stream is unbounded, unless the wire names a window (`accumulate: {seconds: 5}`, which emits one whole value per window, a tumbling window);
  4. a value into a stream port is a one-chunk stream;
  5. a stream and a list into the same node is a graph error in v1.

### 3.3 Node classes

Declared in `NodeDef.stream`:

```json
{"kind": "stateful", "latency": 0, "tail": "declared", "live_params": ["gain_db"], "chunk_sensitive": false}
```

| Class | Contract | Examples | Parallel | Per-chunk memo |
|---|---|---|---|---|
| `map` | One chunk in, zero or more out, no state. The output is a function of the chunk and the parameters | gain (constant), upper, replace, grep, colour convert, per-frame VI style | Yes: a pool, re-sequenced at the output | Possible (content key of the chunk) |
| `stateful` | Carries state between chunks. `open` → `process` per chunk → `flush` at end of stream (tails) → drop. `reset` on a discontinuity or seek | biquad, compressor, reverb (tail), smoothed gain, line numbering, temporal denoise, a streaming resampler | No: serial per stream | No (only as a prefix of the whole stream) |
| `window` | A `stateful` node that sees frames of `window` units every `hop` units. The SDK does the framing. It declares its latency (`window - hop`) and the output's units and rate | STFT and scrolling spectrogram, moving average, onset detection | Serial (or parallel per window when the frames are independent) | No |
| `reduce` | Consumes the whole stream and emits one **value** at end of stream, plus running `node.live` snapshots while it runs | peak, integrated LUFS, word count, histogram, a file writer that returns its file | Serial | The final value, when the stream is memoisable |
| `source` | Produces a stream. **Pull**: produces when it has credit (file, list, generator). **Push**: produces at its own rate into a ring (microphone, socket, timer, log tail). Declares `rate`, `bounded`, `live`, `seekable` | `audio.stream_load`, `audio.mic`, `text.lines`, `text.ticker`, `cv.video` | — | Pull and deterministic: yes. Live: `memo.scope: none` |
| `sink` | Consumes a stream with an effect outside the graph | WAV or video writer, device output, a pipekit topic | — | No |
| `whole` | Today's `Node::run` on whole values (the default) | normalize, reverse, time stretch, everything built so far | — | Today's memo |

**Adapters**, which are ordinary nodes the executor can also insert:

- **accumulate** (stream → value): bounded by a cap, which for audio is today's `MAX_SAMPLES`;
- **split** (value → stream): chunk size in units;
- **rechunk** (stream → stream): fixed block size, used in front of `zip` joins and fixed-block DSP;
- **window**: the framing helper.

**Chunk-size independence is a contract.** A `stateful` or `map` node's concatenated output MUST NOT depend on where chunk boundaries fall: blocks of 64, 1024 and 4096 give the same bytes. A node that can't promise this declares `chunk_sensitive: true` and gets a `rechunk` to its declared block in front of it. This is what makes stream-versus-batch equivalence testable bit for bit, in the spirit of the exactness gate.

### 3.4 Sources, rates and clocks

- **Pull sources** run only when they have credit, so they are as fast as the slowest `block` consumer. A file source plays "as fast as possible" by default and can be paced at the stream's rate (`pace: realtime`) to simulate a live input or to feed monitoring of a file.
- **Push sources** run on their own thread or device callback. That thread writes into a preallocated ring and never locks, allocates or blocks (SoundCraft `record.rs`). The ring's drain thread turns ring contents into chunks of the declared block size. When the downstream queue is full, the outlet policy applies (default `drop_oldest` for live sources), and each drop becomes a `discontinuity` on the next chunk plus a count in `stream.stats`.
- **Clock drift.** A microphone at a nominal 48 kHz and an output device at a nominal 48 kHz drift apart by tens of parts per million. Monitoring absorbs this with a jitter buffer and small rate correction at the sink (the UI already resamples linearly to the device rate in `crates/ui/src/playback.rs`). Two different live clocks are never aligned by `offset`; they join by `ts`.

## 4. Queues and flow

### 4.1 Per-edge queues and policies

Every wire in a stream run is a queue with a capacity in **chunks and bytes**, whichever fills first. The default is 8 chunks or 4 MiB. The workflow can override it per wire (`"queue": {"cap": 32, "bytes": 16777216, "policy": "drop_oldest"}`), and the editor exposes this as a command.

| Policy | Behaviour when full | Spec delivery class it mirrors | Use |
|---|---|---|---|
| `block` (default) | The producer waits; credit propagates upstream to a pull source | `Ordered`, never dropped | Files, batch, anything whose output must be complete |
| `drop_oldest` | The oldest queued chunk is dropped; the next chunk carries a `discontinuity` | `DropOldest(n)` | Live sources, monitoring, any edge where latency matters more than completeness |
| `conflate` | Only the newest chunk is kept | `Latest` per key | Meters, video previews, control streams sampled by a join |
| `drop_newest` | The arriving chunk is dropped | — | Rarely; offered for parity with RabbitMQ's `reject-publish` |

**Recommendation:** make these one enum in `plumcraft-proto`, shared with the event `Delivery` classes, so data queues, event queues and pipekit queues are described by the same words.

### 4.2 Backpressure all the way to the source

- **Credit.** A node is runnable when it has an input chunk (or, for a source, credit) **and** every output edge with `block` has room. Fan-out to several `block` consumers runs at the pace of the slowest. Chunks are shared by reference (`Arc` in process, one DataRef across processes), so fan-out never copies.
- **Where backpressure ends:** at a pull source, which stops reading, or at a push source's ring, which drops and counts. "All the way to the source" is therefore exact for files and lossy by design for microphones. The graph can't slow down the physical world.
- **The UI is never upstream of anything.** Taps (section 5.4) are `drop_oldest` or `conflate` at the session layer, so a slow or remote editor can't stall a recording. This keeps spec 9.7's rule that the executor never blocks on a slow UI.

### 4.3 Ordering, fan-out and fan-in

- **Within a stream, order is by `seq`, always.** A `map` node running on a pool can finish out of order, so its output passes through a reorder buffer as wide as the pool before it enters the queue.
- **Fan-in is declared per input port** (`align`):

| Align | Rule | Example |
|---|---|---|
| `zip` | Chunks paired by `seq`; both inputs must have the same chunking (the executor inserts `rechunk`) and the same clock | A/B compare of two branches of one source |
| `time` | Paired by `offset` range on a shared clock and units, after subtracting each branch's declared latency; the earlier branch is buffered | Dry/wet mix after a reverb with look-ahead; sidechain compression |
| `latest` | Each chunk of the primary input is combined with the newest chunk of the secondary (sample and hold) | A slow control stream (an LFO, a sensor) modulating a fast audio stream |
| `merge` | Interleaved by arrival, ordered by `ts`; one output stream | Two log sources into one viewer |

- **End of stream through joins:** `zip` and `time` end when either input ends (the other is drained and dropped, counted), `latest` ends with its primary, and `merge` ends when all its inputs have ended.
- **Deadlock.** A diamond with `block` queues and unequal latency on its branches (a `window` node on one side) can stall: the join waits on the slow branch while the fast branch's queue is full. Two rules prevent it:
  1. the executor sizes each branch's queue to at least its declared latency plus one chunk;
  2. a watchdog notices "every node blocked, none runnable, for 1 s" and fails the run with a graph error naming the join and the queue, rather than hanging.

### 4.4 Latency and throughput

Latency is roughly the sum of queue depths times chunk duration, plus processing time. Throughput needs larger chunks and more parallelism. The knobs are the chunk size at the source and the queue caps.

| Profile | Chunk | Default caps | Typical end-to-end latency |
|---|---|---|---|
| `live` | 10–20 ms of audio, 1 frame, 1 line | 2–4 chunks, `drop_oldest` | 50–150 ms |
| `batch` | 50–100 ms of audio (≥ 64 KiB), 64 lines, 1 frame | 8 chunks, `block` | Irrelevant; maximum CPU |

A `profile` field on the stream run sets these defaults. Hard real-time (under 10 ms, glitch-free, for performers) is **out of scope**. It needs a fused callback on the audio thread, with no queues, locks or allocation. That is SoundCraft's job; section 9 lists it as an open question.

### 4.5 The scheduler

Today `exec::run` walks the graph once in topological order on one worker thread (`host.rs` `worker_loop`), and `Node::run` gets whole values. A stream run needs a different executor beside it, sharing graph parsing, input resolution, parameter precedence and errors-as-data:

| Option | For | Against | Recommendation |
|---|---|---|---|
| **A. A thread per streaming node**, bounded queues (`Mutex<VecDeque>` + `Condvar`, which supports every policy; `std::sync_channel` covers only `block`) | Simple to reason about and to make correct; matches pipekit's worker per node; a blocking node blocks only itself; cancel is one flag plus a notify | A thread per node (fine for 5–50 nodes); context switches at high chunk rates | **v1 (thin slice and audio)** |
| B. A pool of N workers; a node is runnable when it has input and credit; `stateful` nodes are pinned to one runner at a time, `map` nodes run N wide | Scales to many nodes and many concurrent streams; parallel `map` across frames | A real scheduler, with fairness, starvation, reorder and wakeups; harder to keep identical in Rust and Python | Later step (Q5), once a profile shows A is the limit |
| C. Fused islands: a chain of `map` and `stateful` nodes with no fan-in compiled into one loop | Lowest overhead; the route to real-time audio | Loses per-node taps and stats inside the island | Optional, after B |

**Python:** a thread per node works, because numpy releases the GIL. Pure-Python text processing serialises on the GIL, which is acceptable for the text engine and is noted as a limit.

### 4.6 Cancel, pause, stop and end of stream

| Action | Effect | Ends with |
|---|---|---|
| `run.cancel` | Abort: every node's flag is set, queues are discarded, no flush, sources closed. Within 500 ms (spec 11) | `cancelled` records; `run.reply{ok: false, error: cancelled}` |
| `stream.control{stop}` | Drain: sources emit `eos`; every node flushes its tail; reducers emit their values; sinks close files | `done` everywhere; `run.reply{ok: true}` with reduce outputs and the manifests of bounded outputs |
| `stream.control{pause}` | Pull sources stop pulling; push sources either hold their device open and drop, or close (`pause: drop` or `pause: close`); queues keep their contents; stats keep beating | Nothing; the run stays live |
| `stream.control{resume}` | Undo pause; the first chunk after a push source's pause carries a `discontinuity` | — |
| `stream.control{seek, at}` | Seekable pull sources only: the epoch rises, queues are purged of the old epoch, `stateful` nodes `reset`, reducers restart | — |
| End of stream from a source | As `stop`, started by the source | `done`; reply |

A paused view in the UI (freeze the scroll) is a UI action and touches nothing in the engine.

## 5. The protocol

### 5.1 A stream run is a `run`

`RunRequest` gains one optional field:

```json
"stream": {"profile": "live", "queues": {"default": {"cap": 4, "policy": "block"}},
           "retain": {"seconds": 60, "bytes": 67108864}, "stats_ms": 250}
```

Its presence makes the run a **stream run**. Everything in spec 8 and 11 applies unchanged: an immediate ack, `run_id` and `generation`, `run.get`, `run.cancel` and tombstones, session ownership (D23), `detach`, and an authoritative `run.reply` at the end. A recording should outlive a UI that crashes only when it is sent with `detach: true`; otherwise `idle_cancel_s` stops it after 30 s with no subscriber, which is the right default for a live microphone.

**The alternative**, a new op family (`stream.open`, `stream.close`), would duplicate ownership, tombstones, snapshots and the reply, and would give the reducer a second lifecycle to learn. It is rejected here (D26).

### 5.2 New operations

| Op | Params | Result | Capability | Path |
|---|---|---|---|---|
| `stream.control` | `{run_id, action: pause or resume or stop or seek, node?, at?}` | `{}` (idempotent) | `stream.run` (`seek` needs `stream.seek`) | Off the run path, like `run.cancel` |
| `stream.set` | `{run_id, node, values: {port: value}}` | `{applied: [port], restart: [port]}` | `stream.live_params` | Off the run path; applied at the next chunk boundary |
| `stream.push` | `{run_id, input: "node.port", chunks: [Chunk]}` | `{credit: n}` | `stream.push` | Off the run path; the reply's credit is the UI's backpressure |
| `run.watch` (extended) | WatchRequest gains `stream: {policy, max_rate_hz, want}` | `{}` | `stream.run` | Unchanged path |

`stream.push` uses the `RunRequest.inputs` bindings reserved for multi-engine graphs (D14): `inputs: {"n1.audio": {"kind": "stream", "elem": "AUDIO", "units": "frames", "rate": 48000, "channels": 1, "live": true}}` declares a stream the UI will feed. That one mechanism covers a remote engine listening to the editor's microphone and, later, one engine's stream feeding another's.

### 5.3 Status per stream, statistics per node

- **`node.status` stays per stream.** A streaming node sends:
  - `started` when it opens;
  - `progress` while it runs, with `fraction` only for a bounded stream (a 2-hour file) and with `stage` such as `"1:03:12 of 2:00:00"`;
  - `done` after its flush;
  - `preview` at end of stream for its output (a thumb of the accumulated value or of the last chunk).

  Never a record per chunk. At 100 chunks a second across 20 nodes that would be 2,000 records a second through a reducer built for tens.
- **A new event kind, `stream.stats`**, with delivery class `Latest` per `(run, node)`, at `stats_ms` (default 250 ms; 1 s remotely):

```json
{"node_id": "n4", "state": "running", "chunks_in": 41210, "chunks_out": 41210, "units_out": 98904000,
 "rate": {"chunks_hz": 20.0, "units_hz": 48000.0}, "busy": 0.31, "lag_ms": 84.0, "latency_ms": 21.3,
 "inputs": {"audio": {"depth": 3, "cap": 8, "bytes": 1152000, "dropped": 0, "align_wait_ms": 0.0}},
 "outputs": {"audio": {"dropped": 0, "discontinuities": 0}}, "epoch": 0}
```

  `state` is `running`, `blocked` (an output is full), `starved` (no input), `paused` or `flushing`. Old UIs ignore the unknown kind, as spec 3.5 requires. A stats event also counts as activity for the inactivity timeout (spec 12.3), so a quiet, paused stream run is never mistaken for a dead engine.

### 5.4 Taps: `node.live` appends for the UI

A watched stream port is a **tap**. Its chunks reach the UI as `node.live` `append` updates (offset = the chunk's offset), batched to 50 ms or 64 KiB as spec 10.6 already says. Three rules must change for unbounded streams:

| Today (spec) | Problem for a stream | Change |
|---|---|---|
| Appends are `Ordered`, never dropped; an ordered-queue overflow means `resync` (9.2, 9.7) | A continuous tap to a slow or remote UI overflows constantly and causes resync storms | A tap append carries `lossy: true` and has its own class, `drop_oldest` per tap. A gap is shown as a gap, or refilled from retention |
| Engines keep append buffers until `done`; `run.get` `partials` gives the whole prefix (8.5, 10.6) | A live stream is never `done`, so the buffer grows without bound | Taps keep a **retention window** (`retain`: seconds and bytes). `partials` gains `from`: `{"from": 1440000, "offset": 4320000, "partial": DataRef}`. Older data is gone and the viewer says so |
| Every event enters the resume ring (4,096 events or 60 s) | 20 appends a second per tap evict status events from the ring in a few minutes | Tap appends bypass the session ring; their recovery path is retention (`partials`), not replay |

The UI chooses the tap's policy in its WatchRequest. A level meter conflates (`conflate`, 10 Hz). Monitoring takes every chunk it can (`drop_oldest`, `max_rate_hz: 0`). A remote spectrogram may ask the engine for a `spectrogram` rendition per chunk instead of raw audio.

### 5.5 Capabilities

| Capability | Meaning | If absent |
|---|---|---|
| `stream.run` | `RunRequest.stream`, stream ports and classes in the catalogue, `stream.control` (pause, resume, stop), `stream.stats`, lossy taps with retention | Stream-classed nodes are absent from the catalogue; the UI hides the transport bar; conformance skips the stream cases |
| `stream.seek` | Seek on seekable sources | Seek hidden |
| `stream.live_params` | `stream.set` | Any parameter edit restarts the stream run (supersede) |
| `stream.push` | Sources fed by the client | UI microphone sources hidden for this engine |
| `stream.parallel` | `map` nodes run in a pool | Serial |
| `data.ring` | Ring locators (same host, proved) | Chunks inline or as commonkit blobs |

**Emulation for engines without `stream.run`** (optional, host side): the host can run a bounded stream against a whole-value engine by accumulating the source, running the graph once, and splitting the result. That is batch with a progress bar, and it is enough to keep the editor's stream wires usable on such an engine.

### 5.6 Bindings

| Binding | Chunks between nodes | Taps to the UI | Control | Needs |
|---|---|---|---|---|
| In-process (mode 1) | Inside the engine: `Arc` values in queues, no serialisation | Typed `LiveUpdate` through `Emit`; `mem` locators | Direct calls to the host table | `mem` locators (D19) become worth building: a chunk written as a commonkit file just to be mapped back by the UI is pure cost |
| Socket (mode 2) | Inside the engine | Notifications; text inline, audio and frames by ring locator (same host is already required on this binding) | Notifications read by the reader thread, as cancel is | The ring locator in `plumcraft-shm` |
| HTTP (mode 3) | Inside the engine | Events over **SSE**; tap bytes on their own long-lived response, `GET /v1/streams/{run}/{node}.{port}?from=<offset>` (binary chunked frames with a small header), never inside events | `POST /v1/rpc`; `stream.push` as a `POST` with a chunked body, or WebSocket | SSE (D6), deferred until now, becomes required: long-polling at 20 chunks a second works but costs a request each time. WebSocket (deferred in plan section 5) earns a second look for `stream.push` |
| AMQP / pipekit (mode 4) | Either inside one `Plan` worker (a stream run like any run) or **distributed**, as in section 5.7 | `{system}.status` topic for stats and small appends; bulk through the blob daemon or in-band (D30) | `{system}.control` fanout for cancel; the `{system}.Session` consumer (D22) for `stream.control`, `stream.set`, `stream.push` | D22 built; pipekit queue arguments; P5 for joins |

### 5.7 Distributed streaming over pipekit

When a stream graph is spread across machines, each node (or a fused chain) is a pipekit node and each edge is a pipekit queue. The mapping is close, with four gaps:

| Concern | pipekit today | For streaming |
|---|---|---|
| Bounded queues | Queues are unbounded; flow control is per-consumer `PREFETCH` (default 10) and broker memory alarms | Declare `x-max-length` (and `x-max-length-bytes`) per edge. `block` = `x-overflow: reject-publish` with publisher confirms, so a nack makes the producer wait; `drop_oldest` = `drop-head`. Prefetch is the consumer's credit window. A queue-arguments change in `topology.queue_arguments` (to propose to pipekit) |
| Ordering | FIFO per queue with one consumer; competing consumers (`replicas > 1`) reorder | `stateful` nodes run `replicas: 1` per stream; many concurrent streams shard by stream id with a consistent-hash exchange, as P5 shards join groups; `map` nodes may have replicas, followed by a reorder stage |
| Joins | P5 (scatter/join, `JoinNode` holding parts as unacked deliveries) is designed, not built | `zip` and `time` joins are a `JoinNode` keyed by `(stream, seq)` or by offset window, with a deadline per group (a missing part becomes a `discontinuity`, not a failed run) |
| Chunk bytes | Records are "pixel-free"; bytes go by commonkit key or the blob daemon | Small chunks in-band as msgpack (pipekit M-6 already carries numpy in-band) up to a limit (1 MiB); larger by DataRef. The no-bytes rule exists to keep bulk off the **event** channel, and a chunk queue is not the event channel |
| Replay and retention | Classic queues forget on ack | A RabbitMQ **stream queue** (`x-queue-type: stream`) per tap edge gives replay by offset and retention by size and age: a tap becomes just another consumer at an offset. Already named in plan 5 as "the upgrade if full replay is ever wanted" |

**Is pipekit the transport for chunks between machines?** Recommended: **yes, for graphs that are distributed** (a GPU node on a remote server in the middle of a the development machine graph), because it already has competing consumers, supervision, cancel by correlation id and a status topic, and its 1.6 ms hop (P1) is small against 20–100 ms chunks. **No, for a single remote engine**: there, every chunk stays inside the engine and only taps cross over HTTP. Pipekit is a poor fit for sub-20 ms live audio across machines: each hop adds broker latency and jitter, so live monitoring across machines uses the larger `batch` chunks or stays local.

## 6. Data plane and memo

### 6.1 Where chunks live

| Chunk | Store | Why |
|---|---|---|
| Text, events, small JSON (≤ 4 KiB) | Inline in the chunk | Cheap; the spec's inline rule |
| Audio blocks and video frames on one host | **Ring locator** `{"kind": "ring", "path": …, "slot": 17, "seq": 1041}`: a per-stream mmap'd file of fixed slots, one writer, each slot stamped with its `seq` before and after the payload, so a reader detects a slot overwritten under it (torn read = miss) | No file per chunk, no reaper work, bounded by construction (`slots × slot_bytes`); the UI maps it zero-copy as it maps commonkit. A slot overwritten before the UI read it is exactly a `drop_oldest` drop |
| Large chunks, remote, or anything kept | A blob in commonkit (`plum:v1:…`, TTL 900 s) or the CAS, named by handle | Today's data plane; content-addressed |
| Accumulated outputs (a recorded WAV, a bounded stream's whole output) | A blob, as today; plus a **chunk manifest** (JSON of chunk handles and offsets, `md5:raw:` handle) for replay | Retention and memo work on whole values |

**Handles per chunk or per stream:** per stream by default (the `stream` id plus epoch is the identity in flight). A per-chunk handle exists only when a chunk is stored as a blob. A bounded stream gets one content handle at end of stream, for its accumulated value. **A catch:** the `nd1` recipe hashes the dtype and shape **before** the bytes, and the shape (frame count) is unknown until end of stream, so it can't be hashed incrementally. Two answers: hash the accumulated file once at end of stream (one extra read of a file that was just written, so it is in page cache), or add a streamable recipe (`nd1s`: bytes first, then a trailer with dtype and shape). The first answer is recommended; the second adds a handle form that every reader must learn.

The ring is mutable, which breaks commonkit's invariant of immutable, content-addressed segments. It therefore belongs in `plumcraft-shm`, which is already the one crate allowed `unsafe` for `Mmap::map`, and not in commonkit. A shared ring format could be proposed to commonkit later if SoundCraft or the research software supporting www.virtimp.com want it (golden rule 6).

### 6.2 What memoising a stream means

| Stream | Memo | Key |
|---|---|---|
| Live source and everything below it | None (`memo.scope: none`, as `text.slow`'s pattern today: a per-run identity passes downstream) | — |
| Bounded, deterministic pull source (a file, a list, a seeded generator) | **Whole output per node**: the chunk manifest and, for `reduce` nodes, the value. A rerun with an unchanged upstream replays chunks from the manifest (state `cached`, stream played from blobs) | Today's plan key, computed from the source's content handle, the chain, and **the chunking** (block size is part of the key only for `chunk_sensitive` nodes) |
| Per chunk (`map` nodes on bounded streams) | Later, only if a profile shows it pays: a content key per chunk, `(node kind, version, salt, params, chunk content handle)` | Spec 14.3 content keys; hashing costs about 1.3 ms per MB with MD5, so only for expensive nodes (per-frame VI style) |

The gates keep `memo: "off"` (spec 14.5). A seek invalidates nothing; it simply isn't memoised in v1.

### 6.3 Retention and the reaper

- **Rings** reclaim themselves; their files are deleted when the run ends, or after `run_ttl_s` for a detached run.
- **commonkit chunk blobs** keep the 900 s TTL. A tap that kept every 64 KiB chunk of a stereo 48 kHz stream would hold about 345 MB per stream at that TTL, against a 2 GB budget, so taps of live audio use rings, never commonkit.
- **Tap retention** (`retain`) is per run, in the engine's memory or ring; the UI keeps its own bounded window for scroll-back: 60 s of audio, 10,000 lines, the last 30 frames.
- **Record:** "Record this tap" writes it through to a sink (WAV, text file) or to the durable CAS (`~/Library/Caches/plumcraft/cas`, with pins and leases, D11) and returns a handle at the end.

## 7. The editor

- **Stream wires** are drawn with a moving dash while chunks flow, still when paused, and with a warning tint when they drop.
- **Badges.** A wire's badge shows queue depth as a small bar (`3/8`), a rate (`48 kHz · 20/s`, `12 lines/s`, `29.97 fps`) and a drop count when it is non-zero. Hovering it shows lag, latency, bytes queued and the policy.
- **Node chrome.** A streaming node shows its `stream.stats` state as a ring on the node: running, blocked, starved, paused or flushing. It also shows its busy fraction and, for a bounded source, a progress fraction.
- **Transport bar** (shown when the graph has a stream source):
  - Start (`stream.start`, the same `run` with `stream`), Pause and Resume, Stop (drain), Cancel (abort);
  - a seek scrubber for seekable sources;
  - an uptime clock, the end-to-end latency (tap time minus source `ts`, shown per tap) and a total drop count.
- **Watched panel live views,** each a `MediaViewer` with the `stream` capability (plan section 7):
  - a scrolling waveform with a peak/RMS meter (from tapped chunks, or from a `reduce` node's snapshots);
  - a scrolling spectrogram (the existing rustfft code, fed column by column);
  - a scrolling text view with follow, pause-scroll and search in the retained window;
  - the latest video frame (conflated) with a short strip;
  - a sparkline for a running scalar (word count, LUFS);
  - an appending table.
- **Listen.** On a watched audio stream port, Listen plays tapped chunks through cpal with a jitter buffer (100 ms local, 300 ms remote) and drift correction, and counts underruns. It works in every mode, because it is a tap; an engine-side device sink is an optional extra node for same-host engines.
- **Edits during a stream.** An edit to a port listed in `live_params` is sent as `stream.set` at once, with no restart, which gives knob-turning while listening. Any other edit marks the stream stale with today's reasons (F45). With auto-run on, the stream restarts (supersede, as today); with it off, a banner offers Restart. Auto-run's debounce applies to restarts, never to `stream.set`.
- **Commands** (golden rule 4), in a new `crates/session/src/stream_cmds.rs`, each with `panic_hunt` coverage:
  - `stream.start`, `stream.pause`, `stream.resume`, `stream.stop`, `stream.seek{at}`;
  - `wire.queue{wire, cap, bytes, policy}`, `wire.accumulate{wire, seconds}`;
  - `watch.listen{node, port}`, `view.follow{on}`, `tap.record{node, port, to}`.
- **Control channel:** `ui.waitStream{state, min_chunks, timeout}` for tests; `ui.inspect` reports stream stats under `v2` only, keeping the v1 projection byte-compatible (D21).
- **Session state** (`crates/session/src/run.rs`) gains a `StreamState` beside `RunState`: transport phase, epoch, latest stats per node, taps with their windows, and live parameters pending. `is_busy` and auto-run learn that a stream run is meant to be long.

## 8. Engines

### 8.1 Rust SDK

`Node` stays as it is. Streaming is a second trait, registered beside it (`Engine::with_stream_node`). One id may have both implementations, and the executor picks the stream one when the input is a stream.

```rust
pub trait StreamNode: Send + Sync {
    fn def(&self) -> NodeDef;                                   // def.stream filled
    fn open(&self, info: &StreamInfo, params: &Inputs, cx: &mut Cx) -> Result<Box<dyn StreamState>, NodeError>;
}
pub trait StreamState: Send {
    fn process(&mut self, input: &ChunkIn, out: &mut ChunkOut, cx: &mut Cx) -> Result<(), NodeError>;
    fn flush(&mut self, out: &mut ChunkOut, cx: &mut Cx) -> Result<(), NodeError> { Ok(()) } // end of stream: tails
    fn reset(&mut self) {}                                       // discontinuity or seek
    fn set(&mut self, _port: &str, _v: &Data) -> bool { false } // live parameters
}
pub trait Source: Send { fn open(&self, …) -> Result<Box<dyn Pull>, NodeError>; } // pull: next() under credit
// push sources get a Pusher: a preallocated lock-free ring the device callback writes
```

Helpers cover the common shapes so most nodes are a few lines: `map(|chunk, params| …)`, `Windowed::new(window, hop, f)`, `Fold::new(init, step, finish).snapshot_every(…)`.

`Cx` today holds `&mut dyn FnMut(StatusRecord)`, which is not `Send`. Stream nodes on their own threads need a `Send` emitter: a channel into the host's run table. That changes `Cx`'s internals, not its methods. `Cx` gains `snapshot()` (spec 10.6) and `stat(name, value)` for node-specific counters.

The host's single worker (`host.rs` `worker_loop`, one job at a time) can't run a stream run, which may never end. Stream runs get **their own slot** (`limits.max_streams`, default 1), so ordinary runs, `output.save` and memo hits keep working beside a live stream. The `run.multi` capability means exactly this split.

### 8.2 Python SDK

The same shape, `StreamNode.open → state.process / flush / reset / set`, for parity with Rust, plus a generator form, which suits Python because a generator carries state and its end is the flush:

```python
@stream_node("text.number", kind="stateful", version=1)
def number(chunks, cx, *, start: int = 1):
    n = start
    for c in chunks:              # blocks on the input queue; returns at end of stream
        yield [f"{n + i}: {line}" for i, line in enumerate(c.value)]
        n += len(c.value)
```

The run table (`host.py`) gains stats, tap retention and stream control. The executor runs a thread per node, as in Rust.

### 8.3 The audio engine

SoundCraft's `Plugin` is already a block processor with state: `prepare(sample_rate, max_block, channels)`, `process(io, frames)` in place, `reset`, `latency()`, `tail_samples()` and `set_param`. Its 25 effect plugins map one to one:

| Plugin call | Stream class |
|---|---|
| `prepare` | `open` |
| `process` | `process` |
| a render of `tail_samples()` of silence | `flush` |
| `reset` | `reset` |
| `latency()` | the declared latency, used for `time` joins (dry/wet compensation) |
| `set_param` | live parameters: every plugin parameter can be live |

The existing node loop in `crates/audio/src/fx.rs` already renders `apply_plugin_with_tail` block by block (1024 frames), so the stream implementation is that loop turned inside out.

| Audio node | Class | Note |
|---|---|---|
| gain, invert, remove DC (with a one-pole filter), mix | `map` / `stateful` | Mix takes `time` alignment |
| biquad, the 25 effect plugins | `stateful` | Tails flushed; latency declared |
| peak, loudness (LUFS integrated, short-term snapshots), silence, transients | `reduce` (with snapshots) | `soundcraft_dsp::meter` |
| spectrum and spectrogram | `window` | FFT 1024, hop 256, as the UI's |
| load | pull `source` | Needs a packet-by-packet decode in `soundcraft-audio-io` (symphonia decodes by packet; the wrapper decodes whole). Lifts the 10-minute cap for streams |
| microphone | push `source` | cpal in the engine, reusing `soundcraft-playback`'s `CaptureRing` and `InputRing`; modes 1 and 2 (same host) only; remote engines get the UI's microphone through `stream.push` |
| save | `sink` | Streaming WAV writer, header rewritten every second, as `record.rs` |
| resample | `stateful` | Needs a streaming resampler (SoundCraft's `resample` is whole-buffer) |
| fade in | `stateful` | Fade out needs the length: bounded streams only |
| normalize, reverse, trim, time stretch, pitch shift | `whole` | Two-pass or whole-buffer by nature; behind accumulate on bounded streams |

**Source changes to propose to SoundCraft** (golden rule 6, added to the table in plan 10.4): a streaming decoder (`decode_packets`); a streaming resampler; a public block-render API with tail; an `InputRing` usable outside `Player`.

### 8.4 The text engine: the thin proof

New nodes, in Rust first:

- **Sources:** `text.lines` (a pull source from a text value or a file: N lines per chunk, bounded, seekable) and `text.ticker` (a push source: a numbered line every N ms, unbounded, live, `memo.scope: none`; it exercises drops).
- **Map:** stream implementations of `text.upper`, `text.lower` and `text.replace`, plus `text.grep` (a filter: one chunk in, zero or one out).
- **Stateful:** `text.number` (line numbers: carried state).
- **Reduce:** `text.count` (words and lines, running snapshots).
- **Sink and accumulate:** `text.collect`.
- **Test node:** `text.slow_line` (a `map` with a per-chunk delay, to force backpressure).

The core equivalence test is `collect(upper(lines(T)))` = `upper(T)`, byte for byte, for chunk sizes 1, 7 and 64.

### 8.5 Other engines

- **OpenCV (D16, Python):** `cv.video` is a pull source over `VideoCapture` (frames by DataRef), `cv.camera` a push source, and the per-frame ops are `map` with `stream.parallel`.
- **The research software's VI:** a per-frame the research software's commands style is a `map` over frames, so video stylisation is a stream graph whose element is the research software's existing image value. It is not in this milestone, but nothing here blocks it.
- **The MCP adapter (D15):** MCP tools are request and response, with progress notifications only. A craft app reached over MCP can therefore be only a `whole` node, or a `map` at one tool call per chunk, which is far too slow for audio. **That is an argument for F's choice of SoundCraft's crates over SoundCraft-as-MCP,** and it is worth recording against D15.

## 9. Risks and open questions

| Risk | Why it matters | Mitigation |
|---|---|---|
| A third and fourth executor drift (whole and stream, Rust and Python; the review's M-finding warned of this for the whole-value executor alone) | Two streaming executors that differ give different outputs in different modes | One shared stream corpus (graph, chunking, expected chunks and stats invariants) run against both; stream-versus-batch equivalence as a property test in both languages |
| Deadlock in diamonds and joins | A hung stream run looks like a hung editor | Queue sizing from declared latency; the 1 s all-blocked watchdog fails the run with a named cause; a conformance case builds the deadlock on purpose |
| Unbounded memory: accumulate, retention, join buffers, reorder buffers, a hostile `seq` jump | A 2-hour stream into a whole node; a stream that never ends | A cap on every buffer (`MAX_SAMPLES` for accumulate; retention by bytes; join buffers bounded by queue caps); a `seq` that jumps by more than the cap is a node error; chunk size checked against `max_blob` |
| Chunk-boundary dependence (smoothing that resets per block, FFT framing) | Results change with chunk size, so stream ≠ batch and the exactness idea breaks | The chunk-size independence contract, tested at three sizes per stream node; `chunk_sensitive` plus `rechunk` where a node can't comply |
| Expectations of real time | Users expect hardware-monitoring latency | Profiles state 50–150 ms; hard real-time out of scope (fused islands later, or SoundCraft itself) |
| Clock drift between microphone and output | Clicks or growing latency in monitoring | Jitter buffer with rate correction at the UI sink; underrun and overrun counters |
| Event-channel floods | Taps evict status events from the resume ring; remote resync storms | Lossy taps with their own class, outside the ring; conflated stats; a 1 s stats interval remotely |
| The GIL in Python | Pure-Python stream nodes serialise | Accepted for text; numpy and OpenCV release the GIL; heavy streaming engines are Rust |
| Hashing a bounded stream's output (`nd1` shape first) | No handle until the end, then a second read | Hash the file once at end of stream (page cache); `nd1s` only if a profile shows the cost matters |
| pipekit queues are unbounded; P5 joins aren't built; PlumCraft has no AMQP binding yet (G3) | Distribution can't start until those exist | Distribution is the last step and is gated on them; a single remote engine over HTTP covers remote streaming before it |

**Open questions** (no recommendation yet):

1. Format changes mid-stream (a sample rate change in a live input, a resolution change in video): end the stream and reopen with a new epoch, or allow a `format` chunk? A new epoch is simpler and is the lean.
2. Should lists become streams internally (per-item progress, bounded memory for the research software's list mapping)? Their semantics (zip and product over axes, F61) must not change. The lean is not in this milestone: a later optimisation behind the same corpus.
3. Several stream runs at once (`max_streams > 1`): needed for two monitors or a recording plus an analysis. Default 1, with the limit in `hello`.
4. Hard real-time fused islands: worth building in PlumCraft at all, or does live performance stay SoundCraft's?

## 10. Decisions for the maintainer

Numbered after the plan's D1–D24. The proposed milestone is called **Q** ("queued"), because `docs/plan.md` already uses G0–G11 for the original the research software milestones. The caller's title, "Milestone G", is kept below for the section heading, but its steps are named Q0–Q6 to avoid the clash; renaming it is part of D25.

| | Question | Options | Recommendation |
|---|---|---|---|
| D25 | The stream model, and the milestone's name | (a) `stream: true` ports with chunks, node classes, a separate `StreamNode` trait; (b) every value is a one-chunk stream (one executor, but a rewrite of both executors and of the reducer's assumptions); (c) no protocol change: the UI reruns the whole graph per chunk (works for per-frame video, carries no state, high latency) | **(a)**, as milestone **Q** |
| D26 | How a stream run is started | (a) `run` with a `stream` field; (b) a new `stream.open` family | **(a)**: ownership, tombstones, snapshots and the reply are reused, and the reducer is untouched |
| D27 | Default queue policies and sizes | Defaults per edge, tap and source outlet | **`block`** between nodes (8 chunks or 4 MiB), **`drop_oldest`** for taps and live-source outlets, **`conflate`** for meters; one shared `Delivery` enum |
| D28 | Scheduler | (a) a thread per streaming node; (b) a pool from the start; (c) fused islands | **(a)** for Q1–Q4, **(b)** in Q5 when profiled, (c) only if real-time is wanted |
| D29 | Chunk bytes on one host | (a) a ring locator in `plumcraft-shm`; (b) a commonkit blob per batched chunk; (c) relax "arrays never travel in messages" for small chunks | **(a)**. (b) loads the reaper and the 2 GB budget; (c) undoes a rule that keeps bulk off the event channel |
| D30 | Chunks between machines | (a) pipekit queues, bounded with `x-max-length` and `reject-publish` or `drop-head`, msgpack in-band up to 1 MiB, DataRef above; (b) always DataRef plus the blob daemon; (c) RabbitMQ stream queues for every edge | **(a)** for edges, **(c)** only for tap edges that need replay |
| D31 | Memo for streams | (a) none for live sources; whole output for bounded deterministic streams; (b) per-chunk keys from the start | **(a)**; per-chunk content keys for `map` nodes later, measured |
| D32 | Monitoring and real time | (a) monitoring in the UI through taps, 50–150 ms; engine device sinks optional; (b) engine-side device sinks only; (c) real-time islands now | **(a)**; hard real-time out of scope |
| D33 | Edits during a stream | (a) `stream.set` for `live_params`, a restart for everything else; (b) restart on every edit | **(a)** |
| D34 | Thin slice language and modes | (a) Rust text engine in modes 1 and 2, with Python parity in the next step; (b) both languages at once | **(a)**, following the standing rule to prove the interface on a thin engine first |

**Earlier deferred decisions this touches:**

| Earlier decision | How streaming touches it |
|---|---|
| **D6** (SSE for remote events, deferred) | Becomes required for streaming over HTTP (Q3) |
| **D5** (WebSocket deferred) | Worth revisiting for `stream.push` from a UI microphone to a remote engine |
| **D19** (`mem` locators, decided as "defined now, built only when a profile shows the commonkit write matters") | Streaming in mode 1 is that profile; Q1 should measure it and probably build them |
| **D22 and D18** (the `{system}.Session` consumer on pipekit) | `stream.control`, `stream.set` and `stream.push` need the same off-run-path service |
| **D14** (one engine per workflow, `inputs` reserved) | `stream.push` is the first use of `inputs`, for a stream the UI feeds |
| **D15** (MCP engine, VectorCraft first, SoundCraft second) | MCP can't stream; record this as a reason SoundCraft streams through its crates, not MCP |
| **D10 and D11** (memo persistence, leases and budgets) | Tap recordings go to the durable tier under the same leases and the 20 GB budget |

## 11. What in the spec and the code conflicts with streaming

| Where | What it assumes | Needed change |
|---|---|---|
| Spec 10.6 and 8.5 | Append buffers kept until `done`; `partials` is the whole prefix | A retention window; `partials.from` |
| Spec 9.2 and 9.7 | Appends ordered, never dropped; overflow → `resync` | A lossy tap class outside the resume ring |
| Spec 3.7 and 4.4 | Arrays never travel in messages | The ring locator on one host; in-band msgpack on AMQP chunk queues only (D30) |
| Spec 7.6 | Graph semantics know lists, not streams | Stream rules 1–5 (section 3.2) and a stream corpus |
| Spec 8.4 | `ok` means every node is `done` or `cached`; outputs are values | Stream outputs in the reply are manifests or summaries; `stop` gives `ok: true`, cancel `ok: false` |
| `Limits.max_runs: 1` and serial runs (spec 8.2) | A run ends | `limits.max_streams` and a stream slot beside the run worker |
| `crates/engine/src/host.rs` | One worker thread, one job at a time; ring of 4,096 envelopes | A stream thread group; tap retention separate from the ring; a stats ticker |
| `crates/engine/src/exec.rs` and `python/…/exec.py` | One topological pass; `Node::run` on whole inputs; a memo per node per run | A stream executor beside them, sharing parsing, input resolution and errors |
| `crates/engine/src/lib.rs` `Cx` | `records: &mut dyn FnMut` (not `Send`) | A `Send` emitter for node threads |
| `crates/audio` | `MAX_FRAMES` 10 minutes; whole-buffer decode and resample | Streaming load and resample (SoundCraft source changes); caps stay for whole values |
| `crates/session/src/run.rs` | A run finishes; auto-run supersedes on every edit; staleness per run | `StreamState`; live parameters; restart semantics |
| `crates/ui/src/playback.rs` | Plays a whole clip | A streaming feed with a jitter buffer |
| pipekit | Unbounded queues; joins not built | `x-max-length` and overflow arguments (to propose); P5 |
| commonkit | Immutable content-addressed segments, TTL reaper | Unchanged; rings live in `plumcraft-shm` |

## Proposed plan.md section

### 10.5 Milestone G (proposed): streaming in queued chunks

*Proposed 2026-10-27 in `docs/research/09-streaming.md`; not decided. Steps are named Q0–Q6 because G0–G11 already name the original milestones (D25).* The maintainer asked: "plan processing data in queued chunks, to deal with continuously streaming data." A stream is a list that arrives over time: `stream: true` ports carry chunks (sequence, offset and length in the stream's units, capture time, end of stream, a value inline or by DataRef). Nodes declare a class: `map`, `stateful`, `window`, `reduce`, `source`, `sink` or `whole` (the default, so every existing node keeps working behind an accumulate adapter). Every edge is a bounded queue with a policy (`block`, `drop_oldest`, `conflate`), with credit-based backpressure to pull sources and counted drops at push sources. A stream run is a `run` with a `stream` field, plus `stream.control`, `stream.set`, `stream.push` and a conflated `stream.stats` event; status stays per stream, never per chunk.

| Decision | Choice (AI's, for review; section 10 of note 09) |
|---|---|
| Model | `stream: true` ports, seven node classes, a `StreamNode` trait beside `Node` (D25) |
| Start | `run` with `stream`; reuses ack, ownership, tombstones, `run.get` and the reply (D26) |
| Queues | `block` between nodes (8 chunks or 4 MiB), `drop_oldest` for taps and live outlets, `conflate` for meters (D27) |
| Scheduler | A thread per streaming node first; a pool when profiled (D28) |
| Bytes | Inline text; a ring locator in `plumcraft-shm` for audio and frames on one host; blobs for large or kept chunks (D29) |
| Memo | None for live sources; whole output for bounded deterministic streams (D31) |
| Monitoring | In the UI through lossy taps, 50–150 ms; hard real-time out of scope (D32) |

| | Step | Content | Exit criteria | Days |
|---|---|---|---|---|
| Q0 | Spec and types | `docs/interface.md` gains the stream sections: ports and classes, chunk and stream info, graph rules, `RunRequest.stream`, `stream.control`, `stream.set`, `stream.push`, `stream.stats`, lossy taps with retention and `partials.from`, the ring locator, capabilities. `plumcraft-proto` types, schema export, fixtures, hostile-input sweep for every new parser; a shared `Delivery` enum for data and events | Schema regenerated; fixtures round-trip; hostile sweep green; maintainer review of the spec text | 2–3 |
| Q1 | Thin slice: text streams, modes 1 and 2 (Rust) | SDK `StreamNode`, `Source`, `map`/`Fold` helpers; a `Send` emitter in `Cx`; the stream executor (a thread per node, bounded queues with all policies, credit, end of stream and flush, discontinuities, the deadlock watchdog); the host's stream slot, tap retention and stats ticker; text nodes `lines`, `ticker`, stream `upper`/`replace`/`grep`, `number`, `count`, `collect`, `slow_line`; `plumcraft-cli stream` printing taps and stats; measure `mem` locators (D19) | Conformance in modes 1 and 2: ordering (contiguous `seq`, a filter renumbers); **stream ≡ batch** byte for byte at chunk sizes 1, 7, 64; backpressure (a slow node holds source read-ahead to the queue caps, with memory bounded); `drop_oldest` and `conflate` count drops and mark discontinuities; cancel under 500 ms with no events after; stop drains, flushes and gives `ok`; end of stream through fan-out and a `zip` join; pause and resume; the deadlock case fails with a named cause; a lossy tap recovers from retention; no inactivity timeout over a 10-minute stream; idle session cancels unless `detach`; `kill -9` mid-stream in mode 2 | 8–10 |
| Q2 | Streams in the editor | Stream wires, badges (depth, rate, drops), node stream state, transport bar (start, pause, resume, stop, cancel, seek, latency, drops); scrolling text view with follow and search; live parameter edits through `stream.set`, restart otherwise; `stream_cmds.rs` with `panic_hunt`; `ui.waitStream`; `ui.inspect` v2 stats; P06 checklist | The maintainer streams a ticker through grep, number and count, watches it scroll, pauses, stops, and changes a live parameter without a restart (P06); kittests and control end-to-end green | 5–6 |
| Q3 | Python parity and HTTP | Python `StreamNode` and generator form, the stream executor and run table; the same text nodes; SSE on both HTTP fronts (D6); the binary tap endpoint; `stream.push` | The Q1 conformance set in mode 2 and mode 3 for both engines; a Rust–Python parity test (same chunks, same outputs, same stats invariants); a stream from one machine to another with taps resuming after a dropped connection | 6–8 |
| Q4 | Audio streaming with live monitoring | Stream implementations for gain, mix (`time` join with latency compensation), biquad, the 25 effects (live parameters), peak, LUFS (snapshots), spectrogram (`window`); `stream_load` (packet decode), `mic` (push, cpal), `save` (streaming WAV); the ring locator in `plumcraft-shm`; UI scrolling waveform, meter, scrolling spectrogram, Listen with a jitter buffer and drift correction; SoundCraft source proposals (streaming decode, streaming resampler, block render, `InputRing`) | A 2-hour WAV through gain, EQ and reverb to WAV with bounded memory (under 200 MB) and output identical to the whole-value chain on a 5-minute excerpt; microphone through gain and compressor monitored at under 150 ms with drop and underrun counters at zero for 10 minutes on the development machine; a knob turned while listening; P07 listen test | 10–13 |
| Q5 | Pool scheduler and parallel map | A worker pool with credit, pinned stateful nodes, `map` N wide with reorder; `stream.parallel`; optional per-chunk content keys for expensive `map` nodes | The Q1 and Q4 sets unchanged on the pool; a per-frame `map` scales with cores; no executor drift on the corpus | 5–7 |
| Q6 | Distribution over pipekit | **Gated** on the AMQP binding (G3), D22's Session consumer and pipekit P5. Edges as bounded pipekit queues (`x-max-length`, `reject-publish` or `drop-head`; to propose to pipekit), in-band msgpack chunks up to 1 MiB (D30), stateful nodes `replicas: 1` per stream with consistent-hash sharding, `zip`/`time` joins on P5's `JoinNode`, `stream.control` over the Session consumer, stats from the status topic; RabbitMQ stream queues for replayable taps | A text and an audio stream graph split between the development machine and a remote server passes the Q1 set (minus the in-process cases); cancel across machines under 1 s; a killed worker's chunks redeliver without reordering a stateful node's input | 12–16 |
| | **Total** | | | **48–63** |

**Not in Q:** hard real-time fused islands, lists as streams internally, video frames (with the OpenCV engine, about 4–6 days after it exists), several concurrent stream runs beyond `max_streams: 1`, MIDI.

**Source changes proposed (golden rule 6):** SoundCraft (streaming decode, streaming resampler, a public block render with tail, `InputRing` outside `Player`); pipekit (`x-max-length` and `x-overflow` queue arguments in `topology.queue_arguments`; P5 as designed); commonkit none (rings stay in `plumcraft-shm`).
