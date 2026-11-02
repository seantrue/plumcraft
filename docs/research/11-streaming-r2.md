# 11: Streaming in queued chunks, revision 2

*2026-10-27. A plan only: nothing here is built, and no source file, `plan.md`, STATUS, AI.md or note 09/10 was changed. This replaces `docs/research/09-streaming.md` (revision 1) and answers `docs/research/10-streaming-review.md` (the adversarial review, verdict "rework before acceptance"). The maintainer asked: "Make new revision of the streaming plan incorporating the 13 changes." The maintainer's framing for the review stands: "Sense is not an issue. If it is sound and sensible, it will find a use." Every claim about the code below was checked against the tree as it is today, including the uncommitted E2 work in `crates/session`, `crates/ui` and `apps`.*

## 1. Preface: what changed and why

Revision 1 kept the right outer shell. A stream run is a `run`. Status is per stream, never per chunk. Taps to the UI are lossy. Live sources aren't memoised. The work is proved on the text engine first. Note 10 accepted all of that, and so does this revision.

Note 10 found the inner model unsound, and it was right on the substance:

- "a stream is a list over time" contradicts spec 7.6 rule 2;
- the join rules disagreed with batch semantics and with each other;
- the deadlock watchdog could never fire behind a live source;
- seek and live parameters were out of band, which is a race across threads;
- the shared ring rested on a crate that doesn't exist.

Revision 2 adopts the review's "sounder design" (note 10, that section) almost whole:

- **two stream kinds** declared per wire type by the engine;
- **three node contracts**;
- **every ordered thing in-band**, as GStreamer events and GNU Radio tags are;
- **frontiers** instead of four join rules;
- **a threaded executor over crossbeam** for text and files, and **a fused block render** for audio;
- **taps inline first**;
- **memo eligibility as a property of the upstream graph**;
- **distribution as an engine**, with explicit recovery.

On checking the code, a few of the review's own statements turned out to be wrong or incomplete (section 3). Where they matter, this revision says so and designs around what the code actually does.

## 2. Change ledger

| # | Required change (note 10, closely paraphrased) | Disposition | Where |
|---|---|---|---|
| 1 | Drop "a stream is a list over time"; define sample streams and item streams, item streams following spec 7.6 rule 2, with engine-declared concat, split and unit per wire type | **Adopted** | §5.1–5.3 |
| 2 | Correct the false claims: no `plumcraft-shm`, `unsafe` forbidden, `MAX_FRAMES` is 10 min only at 192 kHz, the UI resampler is whole-clip; restate D29 as needing a new reviewed `unsafe` exception | **Adopted**, and two further corrections (plan 2.1 itself lists the missing crate; spec 3.7 forbids inline arrays, which the review missed) | §3, §11, D29 |
| 3 | Add the missed conflicts: invariant 4 / F47, reducer recovery without a reply, non-`Send` factory, panic rebuild with threads, `run.peek`/`run.watch` on stream ports, overloaded `RunRequest.inputs`, bytes in `stream.push`, staleness and progress in `session/run.rs`, stream typing in `plumcraft-graph` | **Adopted**; each has a row with its resolution | §10.6, §9.4, §12 |
| 4 | Reclassify `text.replace` and character-chunked case changes as unit-bound; boundary units on stream ports; equivalence tests with patterns that cross chunk boundaries | **Adopted, refined.** `text.upper` stays a `char` map; `text.lower` needs `line`, because of the final sigma; literal `text.replace` becomes stateful with a carry and is then exact at any chunking | §5.5, §13.3 |
| 5 | Fix the joins: restrict or drop `zip`; `time` joins end when all inputs end so tails survive; `latest` and `merge` are nondeterministic; consider frontiers | **Adopted**: one frontier `time` join on a shared clock and one explicit `latest` join; `zip` by `seq` and `merge` are out of v1 | §7 |
| 6 | Replace the deadlock design: local join-wait detection, Parks-style growth of the smallest full queue up to a cap, "push blocks" for the threaded executor | **Adopted.** Credit is a permit channel, so growth means issuing more credit. A push source that drops into a stalled cycle counts as blocked | §8.3 |
| 7 | Seek, format change and live parameters in-band, with offsets; `applied_at` recorded; live sets unmemoisable from that point | **Adopted** | §6 |
| 8 | Rewrite D29: measure inline taps first; any ring copies out under a seqlock, has a Rust-only writer and its own `unsafe` exception | **Adopted, modified.** Inline taps need a narrow amendment to spec 3.7 and 9.7, which the review overlooked | §11, D29 |
| 9 | Rewrite D30/Q6: `reject-publish` is not credit; stateful recovery needs checkpoints or a declared restart with a discontinuity; stateful pipekit nodes `CONCURRENCY: 1`; evaluate dora-rs first | **Adopted**: barrier checkpoints, or restart with `discontinuity`; Q6 becomes an evaluation step, and the build is gated | §14, D30 |
| 10 | The fused island is the default for audio chains; stream ≡ batch for audio holds by running the same render | **Adopted, with the argument made exact**: equality holds because each node sees the same `process` call sequence as `render_with_tail` | §9.2 |
| 11 | Default `chunk_sensitive: true` for the SoundCraft plugins until each passes the multi-size test | **Adopted** as `fixed_block: 1024` (the batch grid) | §13.4 |
| 12 | Memo eligibility as a property of the upstream graph; key bounded streams on the accumulated content handle, not the manifest | **Adopted** | §7.4 |
| 13 | Conformance: deadlock behind a live source, a `time` join with a tail, seek under load, a lost record in an unbounded run repaired without a reply, a panic in one node thread | **Adopted**, plus executor parity, the split-boundary match and non-`Send` cores | §17 |

None was declined. Three are modified: 4, 8 and 10. Each modification keeps the review's goal and fixes something the review got wrong or left open.

## 3. What the code and spec say (corrections)

| Claim | Status | Evidence |
|---|---|---|
| A `plumcraft-shm` crate allowed `unsafe` (rev 1) | **False.** No such crate. The workspace forbids `unsafe`. The same false row is in `docs/plan.md` 2.1, which revision 1 copied; it should be corrected there | `ls crates`: audio automation engine graph host proto session store text ui; `Cargo.toml:55` `unsafe_code = "forbid"`; `crates/store/src/lib.rs:10-11` ("read, not mapped") |
| The audio engine refuses anything over 10 minutes (rev 1) | **Imprecise.** `MAX_FRAMES` is 10 min at 192 kHz, which is 40 min stereo at 48 kHz. `MAX_SAMPLES` = 2 × `MAX_FRAMES` (5 min of 16 channels at 48 kHz). `MAX_FILE` is 1 GiB, which also refuses a 2-hour stereo `f32` WAV. `MAX_SECONDS` = 600 caps generated durations | `crates/audio/src/lib.rs:52-59` |
| The UI resamples to the device rate (rev 1) | True, but **whole-clip**, capped at 20 min | `crates/ui/src/playback.rs:22,249` |
| The audio node loop renders in 1024-frame blocks | True: prepare, reset, latency dropped from the front, the tail capped at 60 s, the final partial block | `crates/audio/src/fx.rs:21,153-195` |
| Batch `audio.mix` truncates or pads? | **Pads to the longer input with 0.0**, so a `time` join that ends when all inputs end *is* batch semantics | `crates/audio/src/edit.rs:208,211` |
| `Cx` borrows a non-`Send` emitter | True. `Cx` borrows `node_id`, `cancel` and `records: &mut dyn FnMut` for `'a`. A node thread can build its own `Cx`, so `Cx` needn't change; the `Emit` the executor is given must become `Arc`-owned (`TableEmit` borrows `&Shared`) | `crates/engine/src/lib.rs:155-160`; `host.rs:163-174` |
| `EngineCore` is not `Send`, so a thread per node can't host it (note 10) | **True of the spec, not of the code.** Spec 16.1 and plan 3.2 define a non-`Send` `EngineCore` (for `research-local`'s `Rc<Devices>`). PlumCraft's SDK has no `EngineCore`: `Node: Send + Sync`, and `Arc<Engine>` sits in the lock-protected table that caller threads read. So every SDK engine can run a node per thread today. Only a future non-`Send` core needs the single-thread form | `lib.rs:128`; `host.rs:36,74,257-259`; no `EngineCore` in `crates/` |
| One worker, one job at a time; the memo is owned by the worker | True | `host.rs:223,461-468` (`memo` is a local, passed `&mut` to `exec::run`); Python `host.py:114` |
| `max_runs: 1` | True | `crates/proto/src/session.rs:195` |
| The executors have no list mapping | True of `exec.rs` and `exec.py`. But **`plumcraft-graph` already predicts list mapping** (`output_is_list`, `is_mapped`), so the editor half of item streams exists | `crates/graph/src/lib.rs:546-572`, `ports.rs:114-126` |
| The existing `inline` locator already carries live audio taps (note 10) | **Wrong twice.** Spec 3.7: "arrays never travel in messages, however small", and 9.7 keeps bulk off the event channel. And **nothing emits `node.live` today**: no producer in `crates/engine` or `python/`. `LiveUpdate::Append` has `inline: Option<String>` (text) and a `ref` DataRef | `docs/interface.md` §3 rule 7, §9.7; `crates/proto/src/event.rs:120-135` |
| crossbeam "provides every policy" (note 10) | **Mostly.** `bounded` channels can't be resized or purged, and Parks growth needs resizing. §8.2 composes it from a data channel and a credit channel. `crossbeam-utils` is in `Cargo.lock` only transitively (via `concurrent-queue`) | `Cargo.lock:671-678,783` |
| Stateful pipekit nodes need `CONCURRENCY: 1` (note 10) | It is already the default; the rule is to **pin** it | pipekit `node.py:388-389` |
| Spec features revision 1's exit criteria leaned on | **Not built:** the inactivity timeout (12.3), `idle_cancel_s` and `detach` (5.2), `run.peek`, `run.watch`, SSE. They exist only as `Limits` fields and spec text. Q1 must build the parts a stream run needs | `grep` finds them only in `crates/proto` and `schema/protocol2.json` |
| The session ignores live events | True: `apply_event` handles `Status` and `RunReply`, and everything else returns `false`. Staleness clears only in `apply_reply`; `progress()` counts finished nodes | `crates/session/src/run.rs:292-311,344-379,467-470` |

## 4. Use cases

As revision 1 §2, with these demands made exact. The maintainer's framing means a case is in scope when the design is sound for it, not because there's a customer for it.

| Case | Source | What the design must guarantee |
|---|---|---|
| Live audio | Microphone (push, about 10 ms device blocks) | Bounded latency; a capture thread that never blocks; live parameters at a recorded offset; drops counted, never silent; deadlock detected even though the source never blocks |
| Long files in blocks | A 2-hour WAV (pull) | Bounded memory; lossless backpressure; **output bit-identical to the batch graph** |
| Text and logs | A file, `tail -f`, a ticker | Results independent of where chunks are cut, at the node's declared unit; unbounded runs; a viewer retention window |
| Frames | Video file or camera | Item streams that map like lists; conflated taps |
| Sensors, control | Timer, sensor | A slow control stream sampled against a fast one, with the nondeterminism marked |
| Batch over a list | A list value | Spec 7.6 list rules unchanged; item streams are the same rules over time |

## 5. The model

### 5.1 Two stream kinds, declared by the engine per wire type

A wire type MAY declare that it streams. Core reads this declaration as data and never interprets the values, which keeps golden rule 3: core names no engine types.

```json
{"id": "AUDIO", "media": "audio",
 "stream": {"kind": "samples", "unit": "frame", "boundaries": ["frame"], "pad": true, "format": ["sample_rate", "channels"]}}
{"id": "TEXT", "media": "text",
 "stream": {"kind": "samples", "unit": "char", "boundaries": ["char", "line"]}}
{"id": "IMAGE", "media": "image", "stream": {"kind": "items"}}
```

| Kind | A chunk is | Accumulate | Split | Into a single-item port |
|---|---|---|---|---|
| `samples` | A slice of one value, `[offset, offset+len)` in `unit` | **Concatenation**, implemented by the engine for its type | The engine's split, at a boundary unit | Streams if the port is stream-capable; else accumulate (bounded only) |
| `items` | One whole value of the type; offset = item index | **Collection** into a `list: true` value (spec 4.5 `{"kind": "list"}`) | Each list item is one item | **Maps the node over the items**, exactly spec 7.6 rule 2 |

- The engine registers the behaviour beside the declaration: `concat`, `split(at)`, `len` and, when `pad` is true, `silence(len)`. These are SDK hooks (§13.1). The UI needs only the declaration, for typing and badges.
- `format` names the stream-level fields that must agree across chunks. A change of format mid-stream is an in-band `format` item (§6).
- `boundaries` lists the units a chunk may be cut at, finest first. `unit` is the offset unit. For TEXT the unit is the Unicode scalar value (`char`), which Rust and Python both count without encoding.

### 5.2 What travels on a stream edge

An edge carries **items** in order. Each is data or an event:

```json
{"t": "data",  "seq": 41, "epoch": 0, "offset": 98304, "len": 1024, "ts": 1792170000.12, "value": <Value>}
{"t": "gap",   "seq": 42, "epoch": 0, "offset": 99328, "len": 2048, "reason": "drop"}
{"t": "param", "seq": 43, "epoch": 0, "at": 101376, "port": "gain_db", "value": -6.0, "req": "p7"}
{"t": "flush", "seq": 44, "epoch": 1, "at": 0}
{"t": "format","seq": 45, "epoch": 1, "at": 0, "stream": {"sample_rate": 44100, "channels": 2}}
{"t": "barrier","seq": 46, "epoch": 1, "n": 3}
{"t": "eos",   "seq": 47, "epoch": 1, "at": 345600000}
```

- `seq` is per edge and contiguous **at the producer**. A consumer that sees a jump knows a lossy edge dropped items, and the queue also inserts an explicit `gap` (§8.2).
- `epoch` rises on each seek.
- `offset` is on the stream's **clock** (§7.1).
- `ts` is wall time at the source, for latency display only. **No value may depend on `ts`**, which is part of the determinism rule.

In process, items are typed Rust or Python values, and `value` is the SDK's `Data` behind an `Arc`, so fan-out never copies. On the wire, this JSON form appears only in fixtures, `stream.push` (§10.2) and distribution.

### 5.3 Graph rules (an addition to spec 7.6, held to a shared corpus)

| From → to | Rule |
|---|---|
| sample stream → stream-capable port | Streams |
| bounded sample stream → `whole` port | Implicit **accumulate** (engine concat), shown as a badge; capped by the engine's limit for the type (`MAX_SAMPLES` for audio) |
| unbounded sample stream → `whole` port | Graph error, unless the wire names a tumbling window (`accumulate: {units: N}` or `{seconds: S}`), which emits an **item stream** of whole values, one per window |
| item stream → single-item port (any contract) | Maps the node per item; the output is an item stream on the same clock (spec 7.6 rule 2 over time) |
| bounded item stream → `list: true` port | Collects into a list; unbounded needs `accumulate: {items: N}` (a tumbling window → an item stream of lists) |
| two item streams → two single-item ports | Zip along the shared axis only if both are on **one clock**, which is a `time` join by item index (§7.2); otherwise a graph error in v1 |
| item stream and a list → one node | **Product** (spec 7.6): per stream item, the node maps over the list. This is bounded per item, so it is allowed; revision 1's rule 5 is dropped |
| value → stream-capable port | A one-chunk sample stream (engine split) or a one-item stream |
| any → port whose boundary unit the stream can't meet | Graph error (§5.5) |

`plumcraft-graph` gains `output_is_stream`, beside `output_is_list` (`lib.rs:549`). It also learns accumulate prediction, unit checks and clock checks, so the editor types a graph exactly as engines run it (spec 7.6 preamble). The corpus lives at `crates/proto/fixtures/streams/`, as the other shared fixtures do. It runs against `plumcraft-graph` and every stream executor.

### 5.4 Three node contracts

| Contract | Calls | Covers | Declares (`NodeDef.stream`) |
|---|---|---|---|
| `process` | `open(info, params)` → `process(item_in) → items_out` → `flush() → items_out` at `eos` → drop; `reset()` on `flush` or `gap` when declared; `set(port, value, at)` for live params | map (stateless), stateful, window (an SDK helper), **reduce** (emits one value at flush), **sink** (no outputs), joins (§7) | `stateless`, `latency` (units), `tail` (units, or `declared` at open), `needs` (boundary unit), `fixed_block` (rechunk to N from offset 0), `clock: same or new`, `max_out` (items per call), `live_params`, `resets_on_gap` |
| `source` | `open` → `pull(credit)` (pull) or a pusher handed to a device thread (push); `seek(at)` when seekable | Files, lists, generators, microphones, tickers, sockets | `pull` or `push`, `bounded`, `live`, `seekable`, `deterministic` (a pull source with a fixed input, or a seeded generator) |
| `whole` | Today's `Node::run` (`lib.rs:128-132`) | Everything built so far; the default | Nothing new |

One node id MAY have a `whole` and a `process` implementation. The executor picks `process` when the input is a stream, and the editor shows which. The output of a `process` node is a stream on each stream port and a **value** on each value port (a reduce). `clock: new` marks outputs whose offsets are not the input's: filters, resamplers, tokenisers and windows with a hop.

### 5.5 Boundary units and chunk independence

The contract: a `process` node's concatenated output MUST NOT depend on where its input is cut, **among cuts at its declared `needs` unit**. The executor inserts a `rechunk` to that unit (or to `fixed_block`) when the producer's cuts are finer. A graph whose producer can't cut at the unit (no such boundary in the wire type's `boundaries`) is a graph error. Revision 1's `text.replace` problem shows which nodes need which unit:

| Node | Contract and unit | Why |
|---|---|---|
| `text.upper` | `process`, stateless, `needs: char` | Rust `to_uppercase` and Python `upper` map each scalar without context (ß → SS is per character) |
| `text.lower` | `process`, stateless, `needs: line` | The final sigma: Σ lowers to ς by what follows, and the condition looks across case-ignorable characters. A newline is neither cased nor case-ignorable, so line cuts are safe and character cuts aren't |
| `text.replace` (literal `find`, as built: `str::replace`, `lib.rs:98-108`) | `process`, **stateful**, `needs: char` | It holds back a carry of `len(find) − 1` characters (or through the end of a match found in the window). Leftmost non-overlapping matching is then decided entirely within the buffer, so the result is exact at any cut |
| `text.grep` | `process`, `needs: line`, `clock: new` | Defined on whole lines; drops lines, so its offsets are new |

Floating-point DSP gets `fixed_block` rather than a unit (§13.4).

## 6. In-band ordered events

Everything whose position in the stream matters travels as an item in the same queues as the data, so every node sees it in stream order. Only two things are out of band: **cancel** (abort everything) and **stats**.

| Item | Produced by | Each node | Replaces in revision 1 |
|---|---|---|---|
| `eos{at}` | A source at its end; every source on `stream.control{stop}` | `flush()`, emit the outputs, forward `eos` (the frontier goes to ∞) | The `eos` chunk flag |
| `gap{offset, len, reason}` | A lossy queue that dropped items; a source after an overrun or a pause | Advance the frontier; `reset()` if `resets_on_gap`; pad joins (§7.2) | The `discontinuity` field |
| `flush{epoch, at}` | A seekable source on `stream.control{seek}` | `reset()`, drop held state, forward. Items of an older epoch are dropped on arrival | Out-of-band purge across threads (note 10's race) |
| `format{stream}` | A source whose format changed (sample rate, resolution) | Re-`open` with the new format by default (state discarded, a `gap` reported); a node that declares `accepts_format` handles it itself; an accumulate across a format change is an error | Revision 1 open question 1 |
| `param{at, port, value, req}` | The host on `stream.set` (below) | Apply at `at`: exactly, or at the next block start ≥ `at` for block nodes; report `applied_at` | "Applied at the next chunk boundary" |
| `barrier{n}` | The distribution coordinator (§14) | Snapshot state if able; forward when every input has delivered `n` | Not in revision 1 |

**Seek without a race.** The host raises the run's epoch (an `AtomicU64`, read only as a hint) and tells the source. The source finishes any `push` in progress, emits `flush{epoch+1, at}`, then reads from `at`. A node resets exactly when the `flush` item reaches it, in order. Edges are FIFO and every node forwards in order, so **no old-epoch item can follow a `flush` on any edge**, and correctness doesn't depend on the hint. The hint only lets consumers skip old-epoch data without processing it, which shortens seek latency under load.

**Live parameters with a position.** `stream.set{run_id, node, values}` reaches the host off the run path. The host appends a `param` item to the **tail of the target node's input queue**, stamped `at` = that queue's tail offset. The change is then ordered after every item already queued for that node: it takes effect at a recorded offset, with only that one queue's latency (in a fused island, the island's input queue). `stream.set` replies `{applied: [port], restart: [port]}` at once. The applied offset arrives in `stream.stats` and in the reply (`NodeReport.stream.params: [{port, req, at, applied_at}]`). From `at` on, the node's output is not memoisable (§7.4).

## 7. Frontiers, joins, determinism and the memo

### 7.1 Clocks and frontiers

- A **clock** is a source's offset sequence.
- A `clock: same` output stays on its input's clock, shifted back by the node's declared `latency`. A `clock: new` output starts a clock of its own.
- Every edge carries a **frontier**: the lowest offset that may still arrive (timely dataflow's progress, Flink's watermark).
  - A data item `[o, o+len)` advances it to `o+len`.
  - A `gap` advances it past the gap.
  - `eos` sets it to ∞.
  - A node's output frontier is its input frontier minus its latency. A node with no lookahead (reduce, sink) has nothing to emit.

### 7.2 Two joins

| Join | Rule | End | Determinism |
|---|---|---|---|
| **`time`** (the only aligning join) | Inputs MUST be on one clock (a graph error otherwise, which the editor predicts). The join emits output for offsets below the **minimum frontier** of its inputs. Earlier inputs are buffered up to that minimum. Latency is already in the frontiers | **When all inputs reach ∞.** A shorter input is padded with the wire type's `silence` (or, for item streams, a declared absent value, which skips per spec 7.6 rule 6) | Deterministic: output depends only on offsets |
| **`latest`** (sample and hold) | Each item of the primary input is combined with the newest item seen on each secondary input | With the primary | **Nondeterministic**: pairing depends on thread timing. Everything downstream is `memo.scope: none` and marked so in the editor |

- **A reverb tail survives the join.** Dry and wet are on one clock. The wet branch's frontier stays below ∞ until its tail is flushed, so the join keeps emitting dry padded with silence. That is exactly batch `audio.mix`, which pads to the longer input (`edit.rs:208,211`).
- **`zip` by `seq` and `merge` by arrival are out of v1.** A `time` join on one clock is the sound form of zip. It also covers item streams, joined by index, and it is refused behind a filter, because a filter's output is `clock: new`. That refusal is exactly the case where `zip` silently broke. An arrival-order merge is a `latest`-class nondeterministic node, and can come later under the same marking.

### 7.3 Why this subsumes the revision 1 rules

| Revision 1 concern | Now |
|---|---|
| EOS per join kind | `eos` is the frontier reaching ∞; one rule |
| Latency subtraction | Declared latency shifts the frontier |
| A separate `ts` join for different clocks | Refused. Two clocks meet only through a resampler (`clock: new`, explicit) or a `latest` join (marked nondeterministic) |
| "Which input holds the join back?" | The input with the minimum frontier. The deadlock detector (§8.3) and `stream.stats` report it by name |

### 7.4 Memo eligibility is a property of the upstream graph

A stream node's output is **deterministic**, and so memoisable, iff all of these hold upstream of it:

1. every source is `pull` and `deterministic`;
2. every edge is `block` (no lossy edge, so no data-dependent `gap`);
3. there is no `latest` join;
4. no `param` has been applied (from its `at` on);
5. every node is chunk-independent at its unit or runs on a `fixed_block` grid (the block size is then part of its key);
6. no value depends on `ts` or on pacing (`pace: realtime` changes timing, never values).

The executor computes this once per run from the graph and the run's policies, before anything runs. A node that fails it gets a per-run identity, as `memo.scope: none` does today (`exec.rs:180-187`), so nothing below it is reused.

For an eligible node:

- **Plan key:** spec 14.2 as today, with the source chunk size **excluded** (eligibility guarantees it doesn't matter) and `fixed_block` **included** where present.
- **Stored value:** the **accumulated** output. A sample stream is stored as one value by its content handle, hashed once at `eos`; `nd1` hashes shape first, so incremental hashing isn't possible, and hashing the finished value once is the sound choice. An item stream is stored as a list.
- **Content key** (spec 14.3, where enabled): computed from the accumulated input handles.
- A chunk manifest, if kept, is a replay aid only, **never identity**. Equal bytes at different chunkings give one entry.
- **A memo hit in a stream run** sends `cached`, never `started` (spec 14), and replays the stored value as a stream through the engine's `split`, at the run's chunk size. The replay honours the edge policies: into a lossy edge it can drop, and then everything below is ineligible for that run.

Unbounded and live streams are never memoised. Per-chunk memo stays out of Q.

## 8. Queues, backpressure and deadlock

### 8.1 Policies

| Policy | Full queue | Producer | Spec class it mirrors | Default for |
|---|---|---|---|---|
| `block` | The producer waits for credit | Any pull-driven node | `Ordered` | Edges between nodes |
| `drop_oldest` | The oldest item is dropped; a `gap` is queued for the consumer | Push-source outlets, monitoring | `DropOldest(n)` | Live-source outlets, taps |
| `conflate` | Keep only the newest | Meters, previews | `Latest` | Meter taps |

One `Delivery` enum in `plumcraft-proto` (`event.rs:274-281`, which today has `Ordered`, `Latest(key)` and `DropOldest(n)`) describes event queues, data edges and taps alike. A workflow overrides per **input port**: `"queue": {"size": 32, "policy": "drop_oldest"}`. These are dora-rs's `queue_size` and `queue_policy` (per note 10), which is independent evidence that the vocabulary is sound. Bytes are bounded by bounding items: a source declares `max_chunk` (units), and the executor checks `size × max_chunk bytes` against a per-run budget (default 64 MiB) when the run starts. That spares queues from byte accounting.

### 8.2 The threaded executor's queues, on crossbeam

Each `block` edge is two crossbeam channels:

- **data**, `bounded(cap_max)`;
- **credit**, `bounded(cap_max)`, pre-filled with `cap` permits.

The producer `recv`s a permit, which blocks when there is none: that is backpressure, so **"push blocks"**. It then `send`s the item, which never blocks, because `cap` never exceeds `cap_max`. The consumer returns a permit after each `recv`. Three properties follow:

- **Growth is issuing credit.** Parks's remedy sends k more permits. `cap_max` is the hard cap, derived from the run's byte budget.
- **`select!` waits on everything at once.** A node waits on its inputs' data channels together with a control channel, so cancel and seek wake a blocked thread at once (spec 11: within 500 ms).
- **Introspection is cheap.** Before blocking, a node writes its wait into an atomic: `ReadWait(edge)` or `CreditWait(edge)`. The detector reads these atomics.

`drop_oldest` and `conflate` edges are `crossbeam_queue::ArrayQueue` (`force_push` returns the evicted item, which the queue counts into a pending `gap`), plus a `bounded(1)` doorbell for `select!`. A push source's device callback writes only to an `ArrayQueue`: it never locks, allocates or blocks, as SoundCraft's `CaptureRing` does (`playback/src/record.rs:46`). A drain thread turns that ring into items.

Python mirrors this with `threading.Semaphore` for credit (`release(k)` grows it) and `collections.deque` with a `Condition`. That gives the same semantics, so the corpus holds both languages to it.

### 8.3 Deadlock: detect locally, grow, then fail by name

A diamond whose branches emit at different rates (a window with a hop, a filter, a latency) can stall under bounded queues, and Parks (1995) showed that no static sizing prevents it for data-dependent rates. Revision 2 handles it dynamically:

1. **The wait-for graph.** Nodes are threads.
   - `CreditWait(e)` → the consumer of `e`.
   - `ReadWait(e)` → the producer of `e`. A join waits on the input with the minimum frontier, not "any input".
   - **A push-source outlet that has dropped on a full edge `e` for longer than T, while the consumer of `e` is itself waiting, counts as `CreditWait(e)`.** This closes note 10's hole: behind a microphone, a stalled join no longer turns into silent, permanent drop counting.
2. **When to look.** The run coordinator checks at each stats tick (250 ms), and whenever a node has waited longer than T (200 ms).
3. **A cycle containing a `CreditWait`** is an artificial deadlock. The coordinator grows the full queue with the smallest capacity in the cycle (doubling its credit) and records it in `stream.stats`.
4. **At `cap_max`** the run fails with a `graph` error naming the join, the edges and the frontiers ("join n7 waits on `b` at 98304; `a` is full at 64 items"). It fails rather than hangs.
5. **Drops that are not deadlock.** A full edge whose consumer is **running** (busy, not waiting) is overload. A lossy outlet drops and counts, which is correct and needs no error. Stats show it as `busy` near 1, with `dropped` rising.

In a DAG, a cycle of pure `ReadWait`s is impossible, and so is a stall with no cycle. If either is ever observed, the run reports it as an engine error, a bug, never a hang.

Fan-out to several `block` consumers runs at the slowest one's pace. A lossy sibling still drops by its own policy, while a slow `block` sibling throttles a pull source for everyone. That is surprising but sound, and the editor's badges show it.

### 8.4 Latency profiles

As revision 1 §4.4: `live` (10–20 ms chunks, 2–4 items, `drop_oldest` outlets, 50–150 ms end to end) and `batch` (50–100 ms chunks, 8 items, `block`). Hard real-time (< 10 ms) is out of scope.

## 9. Executors

### 9.1 Which executor runs what

| Executor | Runs | Scheduling | Used for |
|---|---|---|---|
| **Threaded** | A thread per streaming node; `whole` nodes behind accumulate run on their own thread too | §8.2: push blocks; `select!` | Text, files, any Send SDK engine (all of them today, §3) |
| **Cooperative** | One thread for a subgraph. Each pass runs every runnable node in topological order. A node is runnable when it has input and room for its declared `max_out`, so a burst can't overfill a queue by more than one call's output | Credit pre-check (the rule note 10 placed here, not in the threaded form) | (a) **Audio islands**; (b) the whole graph of a **non-`Send` `EngineCore`** (spec 16.1); (c) a reference for parity tests |
| Pool | — | — | **Out of Q**, until a profile shows the threaded form is the limit |

The batch executor (`exec::run`) stays as it is, and is shared for parsing, input resolution (`resolve_inputs`, wire > widget > default), memo keys and errors as data.

### 9.2 Audio: a fused block render

Inside the audio engine, a connected subgraph of `process` nodes on one clock (`fuse: true`, length-preserving, latency and tail declared) runs as one cooperative island on one thread. This is SoundCraft's `MixEngine` pattern (`crates/mix/src/lib.rs:6`: "the same code drives realtime playback and offline bounces"; `render` at `:641`) and fundsp's `Net`. Queues exist only at the island's edges: sources, taps, sinks and the boundaries of the engine. The island is **pull-driven per node**. To produce output block k, a node pulls the input it needs (block k plus its latency) from upstream and keeps a small FIFO.

**Why stream ≡ batch for audio, exactly.** Batch `render_with_tail` (`fx.rs:153-195`) calls `process` on 1024-frame blocks from offset 0 of the node's input. The run covers `len + tail + latency` frames, then drops the first `latency` frames of output, and its last block is partial. In the island, each node is driven on **its own input's** 1024-frame grid from offset 0. At `eos` it knows `len`, so it computes the same `tail` and `total` and makes the same final partial call. By induction over the topological order, every node receives the same input samples and makes the same sequence of `process` calls with the same block contents and sizes. The output is therefore the same bits. This argument needs no fusion: the threaded executor, rechunking to `fixed_block: 1024`, satisfies it too. Fusion is for latency and overhead, not equality.

A `time` join inside an island is lockstep: inputs are already aligned, because every node removes its own latency, and the join pads by `silence` until all inputs end. A rate-changing node (resample, a window with a hop) ends an island, with a queue there.

### 9.3 The host: a stream slot beside the run worker

Today the host runs one job at a time (`host.rs:461-468`), so a stream that never ends would block every ordinary run.

- **SDK engines** (Send): a stream run gets its own coordinator thread holding `Arc<Engine>`. Ordinary runs, `output.save` and memo hits keep using the worker. The limit is `limits.max_streams` (default 1), and `run.multi` means exactly this split. The worker owns the memo (`host.rs:467`), so a stream run's end-of-stream memo insert goes to the worker over a channel, and lookups at stream start go the same way.
- **Non-`Send` cores** run streams on the worker, cooperatively. `max_streams` then shares the one slot, and the UI must stop the stream to run a batch graph. `hello.limits` says so (`max_runs: 1`, `max_streams: 1`, `streams_share_worker: true`).

### 9.4 Cancel, stop, pause, panic

| Action | Mechanism | Ends with |
|---|---|---|
| `run.cancel` | The cancel token is set; a control message goes to every node's `select!`. No flush. Sources close, and threads are joined with a deadline | `cancelled` records; `run.reply{ok: false, error: cancelled}` within 500 ms (spec 11 rule 3 becomes "between items, on every node thread") |
| `stream.control{stop}` | Sources emit `eos`; everything flushes in order | `done` everywhere; `run.reply{ok: true}` with reduce values and accumulated handles of bounded outputs |
| `pause` / `resume` | Pull sources stop pulling. Push sources either `drop` (and emit a `gap` on resume) or `close`. Queues keep their contents | The run stays live; stats keep beating |
| `seek{at}` | §6 | — |
| **A panic in one node thread** | `catch_unwind` around every `process` call. The run is aborted as on cancel. All channels are closed, so blocked threads wake with `Disconnected`; threads are joined with a 1 s deadline, and a thread that doesn't return (a node ignoring cancel) is detached and counted. Then exactly today's rebuild (`host.rs:485-516`): memo dropped, `engine_down`, `connecting`, failed reply, factory rebuild, five panics a minute stop it | A failed reply within 1 s; `connected` after the rebuild; a leaked thread is reported in `engine.state` |

## 10. Protocol

### 10.1 A stream run is a run (D26, kept)

```json
"stream": {"profile": "live", "queues": {"default": {"size": 4, "policy": "block"}},
           "retain": {"seconds": 60, "bytes": 67108864}, "stats_ms": 250,
           "inputs": {"n1.audio": {"elem": "AUDIO", "kind": "samples", "live": true, "format": {"sample_rate": 48000, "channels": 1}}}}
```

UI-fed streams are declared in **`stream.inputs`**. `RunRequest.inputs` keeps spec 8.1's meaning, `{"node.port": Value}`, so no reserved field is overloaded. Everything else in spec 8 and 11 applies: ack, `run_id`, generation, `run.get`, cancel and tombstones, ownership, `detach`, the reply at the end.

### 10.2 New operations

| Op | Params | Result | Capability | Path |
|---|---|---|---|---|
| `stream.control` | `{run_id, action: pause, resume, stop or seek, at?}` | `{}` (idempotent) | `stream.run` (seek: `stream.seek`) | Off the run path |
| `stream.set` | `{run_id, node, values}` | `{applied, restart}` | `stream.live_params` | Off the run path; in-band from there (§6) |
| `stream.push` | `{run_id, input, items: [Item]}` | `{credit: n}` | `stream.push` | Off the run path; JSON form carries **text and JSON values inline only** |

**Bytes in `stream.push`.** The JSON form never carries arrays (spec 3.7 holds). Audio from the UI reaches an engine as follows:

- **Mode 1:** typed `Arc` values; nothing is serialised.
- **Mode 3:** a binary `POST /v1/streams/{run}/in/{node}.{port}` with a chunked body of npy frames (D38). The response streams credit back.
- **Mode 2:** not needed. The engine shares the host (spec 16.3), so it opens the device itself.
- **Mode 4:** through the distribution engine (§14).

### 10.3 Status, stats and the reducer

- **`node.status` stays per stream.**
  - `started` at open.
  - `progress`, with `fraction` for bounded sources.
  - **`preview` MAY repeat while the node runs** (conflated, `Latest` per node, at most 1 per second), and its final form comes before `done`.
  - `done` after the flush.
  - A stream node ends `done`, `error` or `cancelled`. A memo hit ends `cached`.
- **`stream.stats`**, a new event kind, `Latest` per `(run, node)` every `stats_ms`, carries:
  - `state` (`running`, `credit_wait`, `read_wait`, `paused`, `flushing`);
  - counts, rates, `busy`, `lag_ms`;
  - per-input depth, capacity, frontier and drops; growth events;
  - `epoch`, and the applied `param` offsets;
  - **`status: {state, publisher, publisher_seq, fraction, ms, error}`**: the node's latest status record header.

  The reducer applies `status` exactly as the record it summarises (dedup by `(publisher, publisher_seq)`, spec 9.5 rule 5). This makes stream status **self-healing within one stats interval without a reply**, which answers spec 9.5 rule 4's gap for unbounded runs. `resync`, a gap or an inactivity check still call `run.get` (D36).
- **Conformance invariant 4 / F47 for stream runs:**
  - a node's `started` follows its upstream's `started`;
  - its `done` follows its upstream's `done` (`eos` travels in order, so this is structural);
  - its final `preview` precedes its `done` and any downstream `done`.

  The batch form ("`preview` precedes downstream `started`") stays for non-stream runs and for `whole` nodes behind accumulate, which still finish before anything downstream of them starts.
- **Inactivity (spec 12.3):** `stream.stats` counts as activity. Q1 builds the timeout, and `idle_cancel_s`/`detach` for stream runs, because neither exists yet (§3).

### 10.4 Taps

A watched stream port is a **tap**: `run.watch` or the run's `watch`, with `WatchRequest.stream: {policy, max_rate_hz, want}`. Tap appends are `node.live` `append` updates with `lossy: true`, in their own delivery class (`drop_oldest` per tap). They are **outside the session ring**, so they never evict status events. A gap is shown as a gap and refilled from retention. Appends are batched to 50 ms or 64 KiB (spec 10.6). Each tap has a **retention window** (`retain`), and `run.get` `partials` gains `from` (`{"from": 1440000, "offset": 4320000, "partial": DataRef}`).

- **`run.watch` on a stream port** of a live run opens a tap at once.
- **`run.watch` on a finished bounded stream** emits the accumulated display if it is retained, and otherwise `missing`.
- **`run.peek` on a stream port** returns `{display}` of the retained window (`provisional: true`), or `{pending}` before the first item, or the accumulated value after `eos`.

These close note 10's rows on spec 8.6 and 8.7.

### 10.5 Capabilities

| Capability | Meaning | If absent |
|---|---|---|
| `stream.run` | `RunRequest.stream`, stream declarations, `stream.control` (pause, resume, stop), `stream.stats`, lossy taps | Stream nodes hidden; conformance skips stream cases |
| `stream.seek` | Seek on seekable sources | Seek hidden |
| `stream.live_params` | `stream.set` | Every edit restarts the stream |
| `stream.push` | UI-fed sources | UI microphone hidden for this engine |
| `stream.taps.inline` | Sample-stream tap chunks inline under amended rule 3.7 (D29) | Taps by blob or binary endpoint |

`lossy: true` appends change the meaning of `append` only inside a `stream.run` session, so an engine that doesn't stream is untouched. Host-side emulation for engines without `stream.run` (accumulate the source, run once, split the result) is kept, as revision 1 §5.5 had it.

### 10.6 Every spec section that changes

| Spec | Today | Change |
|---|---|---|
| 3 rule 7, 9.7 bullet 3 | No arrays in messages; bulk never shares a channel with events | One exception, behind `stream.taps.inline`: lossy tap appends of sample streams MAY carry `inline` b64 up to `limits.max_tap_inline` (16 KiB socket, 0 on HTTP, where the binary endpoint is used), within `limits.max_tap_rate` per session (default 1 MiB/s). They are dropped first and are outside the ring (D29) |
| 5.3 | — | The capabilities in §10.5 |
| 5.5 | Off-run-path list | Adds `stream.control`, `stream.set`, `stream.push` |
| 6.1 | — | The three ops; `limits.max_streams`, `streams_share_worker`, `max_tap_inline`, `max_tap_rate` |
| 7.2 | Wire types | `stream` declaration (§5.1) |
| 7.3 | NodeDef | `stream` contract and declarations (§5.4) |
| 7.6 | Lists only | The stream rules in §5.3 and the stream corpus |
| 8.1 | `inputs` | `stream` field with `stream.inputs`; `inputs` unchanged |
| 8.4 | `ok` = all `done` or `cached` | The same for stream runs after `stop`; outputs of bounded streams are accumulated handles; `NodeReport.stream` (params applied, drops, growth) |
| 8.5 | Partials kept until `done` | Retention window; `partials.from` |
| 8.6, 8.7 | Values that exist; nodes that finished | §10.4 rules for stream ports |
| 9.2 | Event kinds | `stream.stats` (`Latest` per node); tap appends `drop_oldest` per tap, outside the ring |
| 9.4 | `preview` once, before downstream runs (F47) | Stream form of F47 and invariant 4 (§10.3) |
| 9.5 | Rule 4: the reply repairs losses | Stats carry the status header; self-healing without a reply |
| 10.4, 10.6 | `append` ordered, never dropped | `WatchRequest.stream`; `lossy: true` |
| 11 rule 3 | Between nodes and list items | Between items, on every node thread; blocked threads woken by control |
| 12.3 | Inactivity timeout | `stream.stats` is activity (and Q1 builds the timeout) |
| 14 | Memo per node | §7.4 eligibility; accumulated identity; cached replay as a stream |
| 16.1 | Non-`Send` core on one worker; panic rebuild | Cooperative executor for non-`Send` cores; stream slot for Send engines; panic with node threads (§9.4) |
| 16.2 | Session consumer for peek, watch | `stream.*` ops on the `{system}.Session` consumer (D22) |
| 16.3 | No byte framing | Inline taps under amended 3.7 |
| 16.4 | `/v1/rpc` | `GET /v1/streams/{run}/{node}.{port}?from=` (taps) and `POST …/in/…` (push), npy-framed (D38); long-poll suffices for events at stats rate, so SSE stays optional |
| 18 | Hostile input | New parsers: stream declarations, items, npy frames, stats; frontier and offset arithmetic checked; a `seq` or offset jump beyond the run budget is a node error |
| 19 | Conformance list | Stream cases (§17, Q1 exit); invariant 4 scoped |

## 11. Data plane

| Bytes | Mode 1 | Mode 2 (socket) | Mode 3 (HTTP) |
|---|---|---|---|
| Between nodes in one engine | `Arc` values in queues; never serialised | Same (inside the engine) | Same |
| Text taps | Typed `LiveUpdate` | `inline` text (allowed today) | `inline` text, or the tap endpoint |
| Audio taps | Typed values (D19 `mem` locators, built here if the profile shows the commonkit write matters) | **Inline b64 under the amended rule 3.7**: a 20 ms stereo `f32` block is 7.5 KiB, and 50 ms batches are about 19 KiB, so about 0.5 MB/s per tap | The binary tap endpoint |
| Frame taps | Typed | A commonkit blob of a downscaled `rgba8` rendition, `conflate` at ≤ 10 Hz | The same, via `url` |
| Kept results | Accumulated value as a blob (`md5:nd1:`/`raw`), hashed once at `eos` | Same | Same |

- **No shared ring in Q.** If the inline measurement in Q1 and Q4 fails its budget (tap latency over 50 ms, or event latency rising by more than 5 ms under 4 audio taps), a ring is proposed separately: a **Rust-only writer**, **copy-out reads** under a seqlock (stamp, copy, re-check; never mapped zero-copy, because a `&[u8]` over memory another process writes is undefined behaviour), and a **new, reviewed `unsafe` exception**. AGENTS.md rule 2 allows only "a future shared-memory **reader** crate", so a writer is a larger exception, needing its own decision.
- **commonkit** is unchanged. Its segments stay immutable and content-addressed. Audio taps don't go there, because about 340 MB per live stereo tap would sit there at the 900 s TTL, against a 2 GB budget.
- **Record this tap** writes through a sink, or to the durable CAS (D11 leases), and returns a handle at the end.

## 12. The editor and the session

- **Typing:** `plumcraft-graph` grows stream typing (§5.3). Accumulate badges, unit and clock errors, and `latest` nondeterminism marks are predicted from the catalogue, before running.
- **Wires and nodes:**
  - stream wires: dash while flowing, still when paused, tinted when dropping;
  - badges: depth/capacity, rate, frontier lag, drops and growth;
  - nodes: a ring for stats state, plus busy.
- **Transport bar:** Start, Pause/Resume, Stop (drain), Cancel, a scrubber for seekable sources, uptime, latency per tap, total drops.
- **Live views:**
  - a scrolling waveform and meter;
  - a scrolling spectrogram (the existing rustfft code, fed column by column);
  - scrolling text with follow and search;
  - the latest frame;
  - a sparkline;
  - an appending table.
- **Listen:** cpal playback of a tap through a jitter buffer with a **streaming** resampler and drift correction. Today's `resample_linear` is whole-clip (`playback.rs:249`), so this is new code in `crates/ui`.
- **Session state** (`crates/session/src/run.rs`):
  - `StreamState` beside the run state: transport phase, epoch, latest stats per node, taps, pending params.
  - `apply_event` learns `stream.stats` (applying its status header through the reducer) and `node.live`.
  - **Staleness:** for a stream run, a node's stale mark clears when the current generation's `started` arrives for it (its output reflects the request's edits from the start), not on the reply (`run.rs:366-373`).
  - **Progress:** `progress()` reports streaming nodes as active with their bounded-source fraction, not "finished/total".
  - **Busy:** `is_busy` treats a live stream as long by design. Auto-run supersedes (restarts) a stream on a non-live edit; a `live_params` edit sends `stream.set` with no debounce.
- **Commands** (golden rule 4), in `crates/session/src/stream_cmds.rs`, each with `panic_hunt` coverage:
  - `stream.start`, `stream.pause`, `stream.resume`, `stream.stop`, `stream.seek{at}`;
  - `wire.queue{wire, size, policy}`, `wire.accumulate{wire, units|seconds|items}`;
  - `watch.listen`, `view.follow`, `tap.record`.
- **Control channel:** `ui.waitStream{state, min_items, timeout}`; stats under `v2` only (D21).

## 13. Engines and SDKs

### 13.1 Rust SDK

```rust
pub trait StreamNode: Send + Sync {
    fn def(&self) -> NodeDef;                          // def.stream filled
    fn open(&self, info: &StreamInfo, params: &Inputs, cx: &mut Cx) -> Result<Box<dyn StreamState>, NodeError>;
}
pub trait StreamState: Send {
    fn process(&mut self, item: &Item, out: &mut Out, cx: &mut Cx) -> Result<(), NodeError>;
    fn flush(&mut self, _out: &mut Out, _cx: &mut Cx) -> Result<(), NodeError> { Ok(()) }
    fn reset(&mut self) {}
    fn set(&mut self, _port: &str, _v: &Data, _at: u64) -> Option<u64> { None } // Some(applied_at)
    fn snapshot(&self) -> Option<Data> { None }        // barrier checkpoints (§14)
    fn restore(&mut self, _s: &Data) -> bool { false }
}
pub trait StreamType: Send + Sync {                    // per wire type (§5.1)
    fn len(&self, v: &Data) -> u64;
    fn concat(&self, parts: &[Data]) -> Result<Data, NodeError>;
    fn split(&self, v: &Data, at: u64) -> Result<(Data, Data), NodeError>;
    fn silence(&self, _len: u64, _format: &Value) -> Option<Data> { None }
}
```

- **Registration:** `Engine::with_stream_node` and `Engine::with_stream_type`. Sources get `PullSource` and `PushSource` (a pusher over an `ArrayQueue`).
- **Helpers:** `map`, `Carry` (the `replace` pattern), `Windowed`, `Fold` with snapshots, `TimeJoin`.
- **`Cx`** is built per node thread, unchanged in shape. The executor's `Emit` becomes `Arc`-owned. Record `publisher_seq` is assigned under the table lock, so one publisher's sequence stays monotonic across threads.
- **Executors:** `exec::stream_threaded` and `exec::stream_coop`.

### 13.2 Python SDK

- **Contract:** the same `StreamNode`/`StreamState`, plus a generator form, where the generator's end is the flush.
- **Executor:** threaded only (Semaphore credit, §8.2). There's no non-`Send` problem in Python, and no Python audio engine (plan 10.4).
- **The GIL:** pure-Python text nodes serialise on it, which is accepted and stated. numpy releases it.

### 13.3 The text engine (the thin proof)

| Node | Contract |
|---|---|
| `text.lines` | Pull source over a text value or file: N lines per chunk, bounded, seekable, deterministic |
| `text.ticker` | Push source: a numbered line every N ms; live; `memo.scope: none` |
| `text.upper` / `text.lower` | `process`, stateless, `needs: char` / `needs: line` (§5.5) |
| `text.replace` | `process`, stateful carry, `needs: char` |
| `text.grep` | `process`, `needs: line`, `clock: new` |
| `text.number` | `process`, stateful (line counter); resets on `flush` |
| `text.count` | Reduce (words and lines; snapshots) |
| `text.side_by_side` | `time` join of two line streams on one clock (each line `a`, a bar, then `b`); pads the shorter with empty lines |
| `text.tag` | `latest` join: each line tagged with the newest ticker label (nondeterministic, marked) |
| `text.window` | A window helper: N lines, hop H (`clock: new`, rate-changing), to build the diamond for the deadlock case |
| `text.collect`, `text.slow_line` | Accumulate sink; a per-item delay, to force backpressure |

### 13.4 The audio engine

- **Plugins:** SoundCraft's `Plugin` (`dsp/src/lib.rs:195-212`: `prepare`, `reset`, `set_param`, `process` in place, `latency`, `tail_samples`) maps onto `StreamState`.
- **`fixed_block` default:** all SoundCraft effect plugins default to **`fixed_block: 1024`**, the batch grid. They are lifted one by one, only after the multi-size test (blocks of 64, 256, 1024, 4096 and 1, against batch) passes for that plugin (change 11).
- **Defaults on the fused island:** gain, invert, mix (`time` join with `silence`), biquad and the effects. Peak, LUFS and silence are reduces; the spectrogram is a window (`clock: new`).
- **Sources and sink:**
  - `stream_load` is a pull source, which needs packet decode. Symphonia decodes by packet, and SoundCraft's wrapper decodes whole; that is a source proposal (golden rule 6).
  - `mic` is a push source on cpal with an `ArrayQueue`, modes 1 and 2.
  - `save` is a streaming WAV sink.
- **Whole-only:** normalize, reverse, trim, time stretch and pitch shift stay `whole` behind accumulate.

### 13.5 Other engines

- **OpenCV:** video frames are an item stream, so per-frame ops map like lists.
- **The research software supporting www.virtimp.com VI:** a per-frame style is an item-stream map. It isn't in Q, and nothing here blocks it. `research-local`'s non-`Send` core runs on the cooperative executor.
- **MCP adapter:** request/response only, so a craft reached over MCP can be `whole` or a per-item map. This is recorded against D15.

## 14. Distribution

Distribution is **an engine, not a binding change**: an engine whose internals span machines. It speaks protocol 2 to the UI unchanged and maps its framework's stats onto `stream.stats` and its EOS onto `done`.

| Concern | Revision 1 | Revision 2 |
|---|---|---|
| Backpressure across machines | `x-overflow: reject-publish` | **Not credit.** RabbitMQ nacks and discards, and the publisher must retry, which is polling, and reorders unless retries are serialised. Instead, either dora-rs's `queue_policy: backpressure`, or explicit credit messages from consumer to producer on a per-edge credit queue (the same permit scheme as §8.2) |
| Recovery of stateful nodes | "Chunks redeliver without reordering" | Redelivery restores items, not state, so it isn't enough. Two declared modes: (a) **barrier checkpoints** (Chandy–Lamport, Flink): the coordinator injects `barrier{n}` at sources; each stateful node snapshots on receiving `n` from all inputs, and sinks commit at `n`. On a worker's death, every node restores checkpoint n and pull sources seek to the barrier offsets. The output is identical to an uncrashed run. (b) **Restart with discontinuity**, for nodes without `snapshot` or for live sources: reset at the failure offset with a `gap` announced. The output equals the reference "reset at X" |
| Ordering in pipekit | `replicas: 1` | `replicas: 1` per stream, **and** `CONCURRENCY` pinned to 1 for stateful nodes (it defaults to 1, `node.py:389`; `PREFETCH` 10 then only buffers) |
| Substrate | pipekit queues | **Evaluate dora-rs first** (2–3 days): per-input queue policies, cross-language shared memory, Apache-2.0, Rust and Python (per note 10; its 1.0 status not re-checked here). Compare it with pipekit plus the credit and barrier work. Then decide (D30) |
| Replayable taps | RabbitMQ stream queues | Kept: the right tool for replay by offset on the installed broker |

The build is gated on that decision, on the AMQP binding (G3) and on D22. It is outside the Q total.

## 15. Risks

| Risk | Mitigation |
|---|---|
| Executors drift: batch and two stream forms, in Rust and Python | One stream corpus run against threaded Rust, cooperative Rust and threaded Python, with stream-vs-batch equality as a property test |
| A unit or `fixed_block` declaration is wrong, so a node is silently boundary-dependent | The multi-size and every-split-position tests per stream node; `fixed_block` by default for third-party DSP |
| Parks growth hides a real design error until the cap | Growth events in stats and on wire badges; caps from the byte budget; a named error at the cap |
| Unbounded memory (accumulate, retention, join buffers) | Per-type accumulate caps; retention by bytes; join buffers bounded by credit; the byte budget checked at start |
| Spec 3.7 exception erodes the "no bytes in events" rule | Capability-gated, lossy class only, outside the ring, rate-capped, measured; the ring or the binary endpoint if the measurement fails |
| A thread that ignores cancel during a panic rebuild | Detached and reported; the rebuilt engine is independent (`Arc` keeps the old one alive until the thread exits) |
| Live-param latency in threaded chains | Injected at the target's own input queue (§6), so only one queue's latency |
| The non-`Send` path is untested in this repo (no such engine exists) | A test engine holding `Rc` in Q5 |

## 16. Decisions for the maintainer

The numbers keep revision 1's topics (D25–D34), so notes 09 and 10 still cross-reference. D35–D38 are new.

| | Question | Options | Recommendation |
|---|---|---|---|
| D25 | Model; milestone name | (a) two stream kinds per wire type, three contracts, in-band items, frontiers; (b) revision 1's "list over time" with seven classes; (c) rerun the whole graph per chunk | **(a)**, as milestone **Q** (G0–G11 are taken) |
| D26 | How a stream starts | (a) `run` with `stream` (UI-fed streams in `stream.inputs`); (b) a `stream.open` family | **(a)** |
| D27 | Queue policies | Defaults per edge, outlet and tap | **`block`** between nodes (8 items), **`drop_oldest`** for live outlets and taps, **`conflate`** for meters; per-input `queue: {size, policy}`; one `Delivery` enum; bytes bounded through `max_chunk` |
| D28 | Executors | (a) threaded everywhere; (b) threaded for text and files, a cooperative fused block render for audio islands and non-`Send` cores; (c) a pool now | **(b)**; the pool later, when profiled |
| D29 | Tap bytes on one host | (a) typed values in mode 1, inline under an amended 3.7 on the socket, the binary endpoint on HTTP; (b) a commonkit blob per batched chunk; (c) a ring now | **(a)**, measured in Q1 and Q4. The ring only on a failed measurement, as its own `unsafe` decision |
| D30 | Distribution | (a) pipekit with credit queues and barriers; (b) dora-rs under a distribution engine; (c) defer | **Evaluate (a) against (b) in Q6**, then decide. Build gated |
| D31 | Memo | (a) graph-property eligibility, accumulated identity, cached replay as a stream; (b) none for streams | **(a)** |
| D32 | Monitoring | (a) in the UI through taps, 50–150 ms; (b) engine device sinks only | **(a)**; hard real-time out of scope |
| D33 | Live parameters | (a) in-band `param` at the target's input queue, `applied_at` recorded, unmemoisable from `at`; (b) direct to the node, unrecorded; (c) restart on every edit | **(a)** |
| D34 | Thin slice | (a) Rust text in modes 1 and 2, Python next; (b) both at once | **(a)** |
| D35 | Joins in v1 | (a) frontier `time` join (one clock, ends when all end, pads) plus marked `latest`; (b) also `zip` and `merge` | **(a)** |
| D36 | Status in unbounded runs | (a) stats carry the status header (self-healing), plus `run.get` on gaps; (b) periodic `run.get` only | **(a)**: the newest summary always arrives, and `run.get` of a large graph is 100 KB+ (spec 9.6) |
| D37 | Deadlock | (a) wait-for detection, Parks growth to a cap, a named error; (b) static sizing plus a global watchdog | **(a)**; cap from the 64 MiB per-run budget |
| D38 | Binary framing on the HTTP tap and push endpoints | (a) length-prefixed npy frames (parsers exist in `crates/store/src/npy.rs` and numpy); (b) the Arrow IPC stream format (`arrow-ipc` crate; pyarrow in the Python engine) | **(a)** now; Arrow IPC when a `TABLE` stream engine wants it. This declines part of note 10's "borrow Arrow IPC": its framing ideas (schema once, batches, an explicit end) are kept, and the dependency is not taken |

**Earlier decisions this touches:**

| Decision | Effect |
|---|---|
| D5 (WebSocket deferred) | No longer needed for push: HTTP binary `POST` covers it |
| D6 (SSE deferred) | Not forced: tap bytes are off the event channel, and events at stats rate suit long-poll |
| D14 (`inputs` reserved) | Left unoverloaded; UI-fed streams use `stream.inputs` |
| D15 (MCP) | MCP can't stream; a reason SoundCraft streams through its crates |
| D19 (`mem` locators) | Mode 1 audio taps are the profile that decides it |
| D22, D18 (Session consumer) | Carries `stream.*` on pipekit |
| D23 (ownership; idle cancel) | Unbuilt in code; Q1 builds the stream-run part |
| D10, D11 | Tap recordings go to the durable tier under the same leases |
| Plan 2.1 | The `plumcraft-shm` row should be corrected: no such crate, and `unsafe` is forbidden |

## 17. Milestone Q

| | Step | Content | Exit criteria | Days |
|---|---|---|---|---|
| Q0 | Spec and types | Every row of §10.6 written into `docs/interface.md`; `plumcraft-proto` types (stream declarations, items, `stream.stats` with status header, ops, `Delivery` shared); schema export; the stream corpus format in `crates/proto/fixtures/streams/`; hostile sweep entries for every new parser | Schema regenerated; fixtures round-trip; hostile sweep green; the maintainer reviews the spec text | 3–4 |
| Q1 | Thin slice: Rust text, modes 1 and 2 | `StreamNode`/`StreamType`/sources and helpers; the threaded executor (crossbeam data and credit channels, `select!`, `ArrayQueue` outlets, in-band items, frontiers, `time` and `latest` joins, the wait-for detector with Parks growth); the stream slot; stats with status headers; inline taps with retention; the inactivity timeout, `idle_cancel_s` and `detach` for stream runs; panic handling with node threads; text nodes (§13.3); `plumcraft-cli stream` | Conformance in modes 1 and 2: **stream ≡ batch** byte for byte for every deterministic corpus graph at chunk sizes 1, 7, 64 and 1000; **split-boundary match**: `replace("foo")` correct with the input cut at every position; `lower` of Greek text with final sigmas correct under line rechunk, and a char-chunked `lower` refused by the graph check; a `time` join behind `grep` refused by both executor and `plumcraft-graph`; **deadlock**: the window diamond resolves by growth with output equal to batch, and with a small cap fails within 1 s naming the join; **deadlock behind a live source**: the same diamond under `text.ticker` is detected (not silent drops) and resolves; overload behind a ticker counts drops with no error; **seek under load**: no old-epoch item observed at any node after `flush`, and `number` restarts; **lost record**: drop a `done` and a `started` in an unbounded run, and the reducer is right within 2 stats intervals with no reply; **panic in one node thread**: failed reply within 1 s, `engine_down`, `connected`, no thread left, next run ok; cancel from blocked threads under 500 ms; stop drains and gives `ok`; a `param` reports `applied_at`; the memo hits on a bounded deterministic rerun (cached replays as a stream) and doesn't store behind `latest`, a lossy edge or a param; invariant 4 in its stream form; a 10-minute stream isn't timed out; an idle session cancels unless `detach`; `kill -9` mid-stream in mode 2; tap inline rate and event latency measured (D29) | 11–14 |
| Q2 | Streams in the editor | Stream typing in `plumcraft-graph`; wires, badges, node state, transport bar, scrolling text with follow and search, `stream.set` edits, `StreamState` in the session (staleness on `started`, progress, busy); `stream_cmds.rs` with `panic_hunt`; `ui.waitStream`; P06 checklist | P06: the maintainer streams a ticker through grep, number and count, pauses, seeks a file source, changes a live parameter without a restart, and sees a refused `time` join explained before running; kittests and control end-to-end green | 5–7 |
| Q3 | Python parity and HTTP | The Python `StreamNode`, generator form and threaded executor (Semaphore credit); the same text nodes; HTTP tap and push endpoints with npy frames (D38) in both SDKs | The Q1 set in modes 2 and 3 for both engines; the stream corpus identical in Rust and Python (items, outputs, stats invariants); a the development machine → a remote server stream whose tap resumes from retention after a dropped connection | 6–8 |
| Q4 | Audio: fused islands and live monitoring | The cooperative executor and islands; audio stream nodes with `fixed_block: 1024`; the multi-size test per plugin; `stream_load`, `mic`, `save`; meters, spectrogram; UI Listen with a streaming resampler and jitter buffer; SoundCraft proposals (packet decode, a public block render with tail, `InputRing` outside `Player`) | **Stream ≡ batch** bit for bit for gain → EQ → reverb → save on a 5-minute file; **a reverb tail survives the join**: dry + reverb mix has length `len + tail` and equals batch `audio.mix`; a 2-hour WAV under 200 MB resident; mic → gain → compressor monitored under 150 ms for 10 minutes with zero drops and underruns on the development machine; a knob turned while listening, `applied_at` shown; plugins passing the multi-size test listed, the rest still `fixed_block`; P07 listen test | 11–14 |
| Q5 | Cooperative parity and non-`Send` cores | The cooperative executor runs whole graphs; a test engine holding `Rc` (non-`Send`) on the worker with `streams_share_worker` | The full text corpus gives identical outputs and stats invariants on threaded and cooperative executors; the non-`Send` test engine passes the Q1 set minus the concurrency cases | 3–4 |
| Q6 | Distribution evaluation | A dora-rs spike (text graph across the development machine and a remote server, backpressure across machines, Python node) against pipekit with credit queues; barrier-checkpoint design and a single-machine prototype on text (`number` snapshots) | A written comparison with measurements, and a D30 recommendation; the prototype shows a killed node restored from barrier n with output identical to an uncrashed run, and a non-snapshot node restarting with an announced `gap` | 4–6 |
| | **Total** | | | **43–57** |

The distribution build itself, after D30, is estimated at 10–14 days and is outside the total, gated on G3 and D22.

## Proposed plan.md section

### 10.5 Milestone Q (proposed): streaming in queued chunks

*Proposed 2026-10-27 in `docs/research/11-streaming-r2.md` (revision 2, after the review in note 10); not decided.*

The maintainer asked: "plan processing data in queued chunks, to deal with continuously streaming data."

**The model.** A wire type may declare that it streams, as one of two kinds:

- **sample streams** (audio frames, characters), whose chunks are slices that the engine concatenates and splits at declared boundary units;
- **item streams** (frames, files), whose chunks are whole values. They follow spec 7.6's list rules over time: a single-item port maps, a list port collects.

**Nodes.** There are three node contracts: `process` (with state, flush, reset and live parameters; it covers map, stateful, window, reduce, sink and joins), `source` (pull or push) and `whole` (today's nodes, behind an implicit accumulate on bounded streams).

**Order and alignment.** Everything ordered travels in-band through the same queues: data, `gap`, `flush` (seek), `format`, `param` (at an offset) and `eos`. Edges carry frontiers. There is one `time` join on a shared clock, which ends when all inputs end and so keeps reverb tails, and one explicitly nondeterministic `latest` join.

**Queues and executors.** Edges are bounded crossbeam queues with credit: `block`, `drop_oldest` or `conflate`. Deadlock is detected on the wait-for graph, including push sources that drop into a stalled cycle, and resolved by growing the smallest full queue up to a cap, then failing by name. Text and files run a thread per node. Audio runs as a fused block render, the SoundCraft `MixEngine` pattern, which makes stream ≡ batch hold by construction.

**Protocol.** A stream run is a `run` with a `stream` field, plus `stream.control`, `stream.set`, `stream.push` and `stream.stats`. The stats carry each node's status header, so status heals without a reply. Taps are lossy and kept outside the event ring. Memo eligibility is a property of the whole upstream graph, keyed on accumulated content.

| Decision | Choice (AI's, for review; note 11 §16) |
|---|---|
| Model | Two stream kinds per wire type; three contracts; in-band items; frontiers (D25) |
| Start | `run` with `stream`; UI-fed streams in `stream.inputs` (D26) |
| Queues | `block` 8 items between nodes; `drop_oldest` for live outlets and taps; `conflate` for meters; one `Delivery` enum (D27) |
| Executors | Threaded for text and files; cooperative fused render for audio and non-`Send` cores (D28) |
| Tap bytes | Typed in mode 1; inline under an amended spec 3.7 on the socket; npy-framed endpoints on HTTP; no ring until measured (D29, D38) |
| Distribution | An engine; dora-rs evaluated against pipekit first; recovery by barrier checkpoints or a declared restart (D30) |
| Memo | Graph-property eligibility; accumulated identity (D31) |
| Live parameters | In-band at the target's input queue; `applied_at` recorded (D33) |
| Joins | Frontier `time` join plus marked `latest` (D35) |
| Status | Stats carry the status header (D36) |
| Deadlock | Wait-for detection, Parks growth, a named error at the cap (D37) |

| | Step | Content | Exit criteria | Days |
|---|---|---|---|---|
| Q0 | Spec and types | Stream sections in `docs/interface.md` (§10.6 of note 11); proto types; corpus format; hostile sweep | Schema, fixtures, sweep green; spec reviewed | 3–4 |
| Q1 | Rust text, modes 1 and 2 | Threaded executor on crossbeam credit; in-band items; frontiers and joins; deadlock detector; stream slot; stats; inline taps; inactivity and idle cancel for stream runs; text stream nodes | Stream ≡ batch at four chunk sizes; split-boundary match; deadlock with and behind a live source; seek under load; lost record healed without a reply; panic in one node thread; cancel, stop, memo eligibility, `kill -9` | 11–14 |
| Q2 | Editor | Stream typing, wires, badges, transport, live views, session `StreamState`, commands, P06 | P06 with the maintainer; kittests and control green | 5–7 |
| Q3 | Python and HTTP | Python stream SDK and executor; HTTP tap and push endpoints | Q1 set in modes 2 and 3, both engines; Rust–Python corpus parity; remote tap resumes | 6–8 |
| Q4 | Audio | Cooperative islands; audio stream nodes (`fixed_block: 1024`); load, mic, save; Listen; SoundCraft proposals | Bit-exact stream ≡ batch; a reverb tail survives the mix; 2-hour file bounded; live monitoring under 150 ms; P07 | 11–14 |
| Q5 | Cooperative parity | Whole graphs on the cooperative executor; a non-`Send` test engine | Corpus identical on both executors; the non-`Send` engine passes | 3–4 |
| Q6 | Distribution evaluation | dora-rs spike against pipekit with credit; barrier prototype | Comparison and D30 recommendation; barrier restore gives identical output | 4–6 |
| | **Total** | | | **43–57** |

**Not in Q:** the distribution build (10–14 days, after D30, gated on G3 and D22), a pool scheduler, per-chunk memo, `zip` and `merge` joins, hard real-time, video (with the OpenCV engine), MIDI, a shared-memory ring (a separate `unsafe` decision, only if inline taps fail their measurement).

**Source changes proposed (golden rule 6):**

- **SoundCraft:** packet decode, a public block render with tail, `InputRing` outside `Player`.
- **pipekit:** credit queues and barrier support, only if D30 chooses pipekit; `CONCURRENCY` pinned for stateful nodes.
- **commonkit:** none.
- **This repository:** correct plan 2.1's `plumcraft-shm` row.
