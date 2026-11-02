# 13: Streaming in queued chunks, revision 3

*2026-10-27. A plan only: nothing here is built. No source file, `plan.md`, STATUS, AI.md or note 09–12 was changed. This replaces `docs/research/11-streaming-r2.md` (revision 2) and answers `docs/research/12-streaming-review2.md` (the second adversarial review, verdict "accept with changes"). The maintainer asked: "Write revision 3 incorporating the 14 changes." The framing stands: "Sense is not an issue. If it is sound and sensible, it will find a use." Claims about code were checked against the tree at `aa217e0` plus the uncommitted groups work, which is read as current but not depended on. The experiments cited as "exp N" are review 2's, under `claude-debug/streaming-review2/`.*

## 1. Preface: what changed from revision 2 and why

Review 2 accepted the model: two stream kinds declared per wire type, three node contracts, ordered things in-band, frontiers, crossbeam credit, a fused audio render, and memo eligibility as a graph property. Its experiments confirmed the parts that carry load:

- credit, growth and wait-for detection (exp3);
- the `replace` carry, and the `lower`/final-sigma unit (exp2, exp6);
- 23 of 25 plugins block-size independent (exp1);
- cheap npy framing (exp4).

Revision 3 keeps all of that. It fixes six places where revision 2 contradicted itself or the stack it runs on, and it makes the smaller items exact.

| Revision 2 said | The problem | Revision 3 |
|---|---|---|
| The deadlock diamond uses `text.window` with a hop, joined by `time` | That window is `clock: new`, which the `time` join refuses (F1) | A sliding `text.window` (hop 1, `clock: same`, `latency: n-1` items) on a new `LINE` item stream, as exp3 built it. The corpus is five cases (§3.6.4) |
| `param` is appended to the target's input queue from off the run path | A second writer either blocks on credit or breaks `cap_max` (F2) | `param` goes on the node's **control channel**, stamped from a per-edge `sent_end` atomic and ordered by offset at the consumer. One writer per data edge (§3.4) |
| 50 ms stereo inline taps; 16 KiB per append; 1 MiB/s per session; measured with 4 taps | 25 KiB in base64, over the cap; 4 taps is 2 MB/s, over the rate (F3) | 32 KiB per append, 4 MiB/s per session, batches closed at 50 ms or the cap; the arithmetic is in §3.8.4 |
| HTTP push: a chunked `POST` whose response streams credit back | ureq 3.5 sends the whole body before reading the response; tiny_http's `respond` takes `self` (F4) | Each `POST` carries at most the credit it holds, and its response returns new credit, long-polling when none is free. Plain request/response. D5 stays deferred, not closed (§3.8.3, D39) |
| A stream panic does "exactly today's rebuild" | That rebuild lives in the worker loop, and only the worker owns the factory. A batch run may be in flight (F5) | A poison flag, a rebuild message to the worker, cancel and fail of the batch in flight, and `Mutex<Memo>` with an incarnation number that refuses stale inserts (§3.7.4) |
| `flush` is one call | A 60 s tail is about 2,800 blocks, which breaks the cooperative `max_out` rule (F6) | `process` and `flush` return `Step::More` or `Step::Done` and are resumed until done (§3.3.4) |

**One model refinement** came out of fixing F1 and A7: a push source's lossy point is its **device ring**, in front of the source node, and not a graph edge. The source node's own edges are `block`. A stall downstream of a microphone is then an ordinary wait-for cycle through the source's drain thread. Revision 2 needed a special rule for push sources to see that, and review 2 rightly doubted the rule could be tested. Revision 3 drops it (§3.6.3).

Nothing else in the model changes.

## 2. Change ledger

| # | Required change (note 12 §7, paraphrased) | Disposition | Where |
|---|---|---|---|
| 1 | Fix the deadlock corpus: a `clock: same` window, or `latest` in the wait-for graph; resolution time as a function of N, or grow every full edge together | **Adopted, both halves.** A sliding window on `LINE` items; `latest` waits only on its primary and drains its secondaries eagerly, so it can't stall; growth doubles every full edge in the cycle at once; the bound is stated. Added: the push-source refinement above | §3.3.1, §3.5.2, §3.6.3–3.6.4 |
| 2 | Live parameters: one writer per data edge; `param` on the control channel, ordered by `at` at the consumer; `at` from a per-edge atomic; `stream.set` off the run path by construction | **Adopted** | §3.4 |
| 3 | Make the tap numbers consistent in base64, per tap or per session, and reconcile with spec 10.6 | **Adopted** | §3.8.4 |
| 4 | Replace duplex HTTP push with long-poll credit, or reopen D5; delete "D5 no longer needed" | **Adopted: long-poll credit.** The argument is in D39; D5 stays deferred, with a stated criterion for reopening it | §3.8.3, D39 |
| 5 | Specify the stream-slot panic path, the batch run in flight and memo ownership (`Mutex<Memo>`) | **Adopted**, plus the converse case: a batch panic while a stream runs | §3.7.4 |
| 6 | Make `flush` resumable for `max_out` | **Adopted** for `process` too, since a `clock: new` node can burst as well | §3.3.4 |
| 7 | Don't reuse `Delivery` unchanged; define the edge policy type in Q0 | **Adopted**: `EdgeQueue { size, policy }` | §3.6.1 |
| 8 | Default a `latest` secondary to `conflate`, shown in the editor | **Adopted**, and eager draining specified, so a `block` override can't stall either | §3.5.2 |
| 9 | Rewrite the dora-rs row from exp7; Q6 tests lossless delivery | **Adopted.** Exp7 is read precisely below (§3.1): the conclusion stands, but "44 of 150" overstates the loss the policy caused | §3.12, Q6 |
| 10 | State the four memory caps; the byte budget bounds queues only | **Adopted**, with a rule that makes join buffers at most one item per input | §3.6.5 |
| 11 | Scope D36 to state; name the repair path for a lost `preview` | **Adopted**, plus a reducer rule so a header applied before its record doesn't swallow the record's payload | §3.8.5, D36 |
| 12 | Record exp1 in §13.4; `de_esser` ≥ 64; `dither` outside the equality set; add non-default and mid-stream parameter cases | **Modified.** Results recorded; `de_esser` gets `min_block: 64`; parameter cases added. **`dither` stays at `fixed_block: 1024`, not outside the equality set**: at the batch grid it makes the same call sequence, so stream ≡ batch holds for it too. Its block dependence comes from channel-major draws of one seeded RNG, not "noise per call" (§3.1); a SoundCraft fix is proposed | §3.11.4 |
| 13 | Re-estimate Q1 and Q4, or split Q1 into executor and host | **Adopted, both.** Q1 → Q1a (executor) + Q1b (host); Q4 → Q4a (offline) + Q4b (live); total 56–72 days (was 43–57) | §5 |
| 14 | Minor: leave `run.multi` alone, advertise `max_streams`; pin lone surrogates; reproduce `fx.rs`'s non-finite rule and zero-padded scratch; cap the line rechunker | **Adopted.** Also F9's growth timing (in 1) and the exact end-of-stream block rule for the island | §3.7.2–3.7.3, §3.3.5, §3.8.6 |

None was declined. One is modified (12), with the argument given.

## 3. The plan

### 3.1 What the code, spec and experiments say

Revision 2's corrections table (no `plumcraft-shm`, `unsafe` forbidden, `MAX_FRAMES`, the whole-clip resampler, `EngineCore` not in the code, unbuilt spec features, the session ignoring live events) was re-confirmed by review 2 §5 and still holds. Plan 2.1 still lists `plumcraft-shm` (`docs/plan.md:43`). New rows from this revision's checks:

| Claim | Status | Evidence |
|---|---|---|
| The worker owns the factory and the memo | True: `worker_loop(shared, mut factory, jobs)` takes the factory by value; `memo` is a local; rebuild and panic counting are in the loop. Only the worker can rebuild | `crates/engine/src/host.rs:461-516` (memo `:467`, `catch_unwind` `:482`, drop memo `:492`, rebuild `:504`) |
| Memo access is short | One `get`, one `put`, never held while a node runs, so a `Mutex` costs nothing | `crates/engine/src/exec.rs:188,235` |
| HTTP can't be duplex on this stack | ureq runs `SendRequest -> Await100 -> SendBody -> RecvResponse -> RecvBody`; tiny_http's `respond` takes `self`, and only `upgrade` hands back a raw stream. Each request has its own thread (Rust and Python), so a long-poll inside one is cheap | `ureq-3.5.0/src/run.rs:178-221`; `tiny_http-0.12.0/src/request.rs:305,432`; `crates/engine/src/http.rs:75`; `python/src/plumcraft_engine/http.py:10-11` |
| `Delivery` fits data edges | **No.** `Ordered`, `Latest(String)` (keyed), `DropOldest(usize)` | `crates/proto/src/event.rs:274-281` |
| The non-finite rule | `render_with_tail` writes `0.0` for a non-finite output sample, zeroes the scratch past `n`, and calls `prepare` twice before `reset` | `crates/audio/src/fx.rs:157,161-162,165,177-179,189` (review 2 says `:188`) |
| The batch end-of-input block | **Not partial.** Input past `len` reads as `0.0` (`c.get(done + k)...unwrap_or(0.0)`), so the block that holds the end of the input is full. Only the last block of `total = len + tail + latency` is partial | `fx.rs:176-179` |
| `dither` is "noise per call" (review 2) | **Imprecise.** One xorshift seeded `0xD17E` at construction is drawn channel-major within each `process` call (all of channel 0's frames, then channel 1's). The noise-to-sample mapping therefore depends on the block size for more than one channel. It is deterministic at a fixed block size. `reset()` zeroes the error feedback and doesn't reseed. Prediction (untested): mono `dither` is block-size independent | soundcraft `40f593a` `crates/dsp/src/plugins/utility.rs:472-510`; `util.rs:177-190` |
| Exp1's 25 | The 25 offered plugins; 5 are excluded from the engine (instruments, generators, non-`audiosuite`) | `exp1-audio-blocks/result.md`; `crates/audio/src/fx.rs:41-50` |
| Exp7: "consumer got 44 of 150" (review 2) | **Overstates the policy's loss.** The consumer received items 1–51 with 3 gaps, so **7 items were dropped in the delivered range**, which matches the 7 "hit hard cap (100), dropping oldest" lines. Items 52–150 were never seen because the consumer's loop ended at end of input (`STOP`/`INPUT_CLOSED`, `exp7_slow.py`) about 30 ms after the source finished. That they were dropped by policy is not shown. What *is* shown: the producer was not throttled (150 sent in 1.49 s, the timer's pace), and `backpressure` drops once its hard cap is hit. The conclusion (not lossless) stands | `exp7-dora-run/result.txt`; `scripts/exp7_slow.py`, `exp7_src.py` |
| Spec 10.6 batching | "batches `append` to ≥ 50 ms or ≥ 64 KiB" | `docs/interface.md:684` |
| `run.multi` | "Several runs may execute concurrently (`limits.max_runs`)" | `docs/interface.md:232` |
| Lone surrogates | Rust `text.load` refuses non-UTF-8 (`crates/text/src/lib.rs:185`). Python `text.load` decodes strictly (`python/src/plumcraft_text/__init__.py:137`). But a Python node can build one, Python's `json` accepts `"\ud800"`, and the Python memo hashes with `surrogatepass` (`memo.py:76,93`). serde_json refuses a lone surrogate escape | as cited |
| Python rebuild | An exception escaping the executor fails the run as `engine` and rebuilds; five a minute stop it, as in Rust | `python/src/plumcraft_engine/host.py:7-9,369-373` |

### 3.2 Use cases

As revision 2 §4, in scope because the design is sound for them: **live audio** (a microphone; bounded latency, a callback that never blocks, recorded parameter offsets, counted drops, deadlock detected behind the source); **long files** (a 2-hour WAV; bounded memory, lossless backpressure, output bit-identical to batch); **text and logs** (a file, `tail -f`, a ticker; results independent of cuts at the declared unit, unbounded runs, retention); **frames** (item streams mapped like lists); **sensors and control** (a slow stream sampled against a fast one, marked nondeterministic); and **batch over a list** (spec 7.6 unchanged).

### 3.3 The model

#### 3.3.1 Two stream kinds, declared by the engine per wire type

A wire type MAY declare that it streams. Core reads this as data and never interprets values (golden rule 3).

```json
{"id": "AUDIO", "media": "audio",
 "stream": {"kind": "samples", "unit": "frame", "boundaries": ["frame"], "pad": true, "format": ["sample_rate", "channels"]}}
{"id": "TEXT", "media": "text", "stream": {"kind": "samples", "unit": "char", "boundaries": ["char", "line"], "max_line": 1048576}}
{"id": "LINE", "media": "text", "stream": {"kind": "items", "absent": ""}}
{"id": "IMAGE", "media": "image", "stream": {"kind": "items"}}
```

| Kind | A chunk is | Accumulate | Split | Into a single-item port |
|---|---|---|---|---|
| `samples` | A slice `[offset, offset+len)` of one value, in `unit` | Concatenation (the engine's) | The engine's, at a boundary unit | Streams if the port can; else accumulate (bounded only) |
| `items` | One whole value; offset = item index | Collection into a `list: true` value (spec 4.5) | Each list item is one item | Maps the node per item (spec 7.6 rule 2) |

- The engine registers `len`, `concat`, `split(at)` and, where `pad` is true, `silence(len)` as SDK hooks (§3.11.1).
- `format` names the stream-level fields that must agree across chunks. A change is an in-band `format` item.
- `boundaries` lists the units a chunk may be cut at, finest first. The TEXT unit is the Unicode scalar value.
- `absent` is the item an aligning join pads with. Without it, spec 7.6 rule 6 skips.
- **`LINE` is new**, in the text engine. Its items are single lines. It gives text a clock whose unit is the item, so a sliding window has a constant latency in units. On TEXT the unit is the character, and a window of lines has no fixed latency in characters.

#### 3.3.2 What travels on a stream edge

```json
{"t": "data",   "seq": 41, "epoch": 0, "offset": 98304, "len": 1024, "ts": 1792170000.12, "value": <Value>}
{"t": "gap",    "seq": 42, "epoch": 0, "offset": 99328, "len": 2048, "reason": "drop"}
{"t": "flush",  "seq": 43, "epoch": 1, "at": 0}
{"t": "format", "seq": 44, "epoch": 1, "at": 0, "stream": {"sample_rate": 44100, "channels": 2}}
{"t": "barrier","seq": 45, "epoch": 1, "n": 3}
{"t": "eos",    "seq": 46, "epoch": 1, "at": 345600000}
```

- `seq` is per edge and contiguous at the producer.
- `epoch` rises on each seek.
- `offset` is on the stream's clock.
- `ts` is for latency display only: **no value may depend on it**.
- **`param` is no longer an edge item.** It is a control message ordered by offset (§3.4).
- In process, `value` is the SDK's `Data` behind an `Arc`. The JSON form appears only in fixtures, `stream.push` and distribution.

#### 3.3.3 Graph rules (an addition to spec 7.6, held to a shared corpus)

| From → to | Rule |
|---|---|
| sample stream → stream-capable port | Streams |
| bounded sample stream → `whole` port | Implicit accumulate (badge), capped per type (§3.6.5) |
| unbounded sample stream → `whole` port | Graph error, unless the wire names a tumbling window (`accumulate: {units: N}` or `{seconds: S}`), which gives an item stream |
| item stream → single-item port | Maps per item; the output stays on the same clock |
| bounded item stream → `list: true` port | Collects; unbounded needs `accumulate: {items: N}` |
| two item streams → two single-item ports | Allowed only on one clock (a `time` join by index); otherwise a graph error |
| item stream and a list → one node | Product (spec 7.6), bounded per item |
| value → stream-capable port | A one-chunk or one-item stream |
| any → a port whose unit the stream can't meet | Graph error |
| a lossy edge into one input of a `time` join whose other input shares the source | Editor warning: loss becomes padding by policy (§3.6.3) |

`plumcraft-graph` gains `output_is_stream` beside `output_is_list` (`crates/graph/src/lib.rs:549`), with accumulate, unit and clock checks. `check_wire` (`ports.rs:122`) has none of these today, so this is new code. The corpus lives at `crates/proto/fixtures/streams/`. It runs against `plumcraft-graph` and every stream executor.

#### 3.3.4 Three node contracts, with resumable calls

| Contract | Calls | Covers | Declares (`NodeDef.stream`) |
|---|---|---|---|
| `process` | `open` → `process(item) -> Step`, then `resume()` while `More` → at `eos`, `flush() -> Step` until `Done` → drop; `reset()` on `flush`, or on `gap` when declared; `set(port, value, at)` | map, stateful, window, reduce, sink, joins | `stateless`, `latency`, `tail`, `needs` (unit), `fixed_block` or `min_block`, `clock: same or new`, `max_out` (items per call), `out_item` (item bound: `same`, `xk` or absolute), `hold` (items kept as state), `live_params`, `param_exact`, `resets_on_gap` |
| `source` | `open` → `pull(credit)`, or a push ring drained by the source node; `seek(at)` when seekable | Files, lists, generators, microphones, tickers | `pull` or `push`, `bounded`, `live`, `seekable`, `deterministic`, `max_chunk` |
| `whole` | Today's `Node::run` (`crates/engine/src/lib.rs:128-132`) | Everything built so far | Nothing new |

**Resumable calls (change 6).**

- `Out` holds at most `max_out` items. Pushing past that is a node `op` error, a bug in the node.
- A node with more to emit returns `Step::More`. The executor sends what it holds, waiting for credit, and calls `resume` (or `flush` again) until `Done`. Only then does it offer the next item.
- So no call overfills a queue. The cooperative executor's runnable rule ("input, and room for `max_out`") holds at island edges and for non-`Send` cores.
- An audio plugin's tail is `flush` returning one block per call, with `max_out: 1`.

One node id MAY have both a `whole` and a `process` implementation; the executor picks `process` for a stream input. `clock: new` marks outputs whose offsets are not the input's: filters, resamplers, tokenisers, `split_lines`, and windows with hop > 1.

#### 3.3.5 Boundary units and chunk independence

A `process` node's concatenated output MUST NOT depend on where its input is cut, among cuts at its declared `needs` unit. The executor inserts a `rechunk` when the producer cuts finer.

| Node | Contract and unit | Why (evidence) |
|---|---|---|
| `text.upper` | stateless, `needs: char` | Per-scalar; never differed at any cut (exp2) |
| `text.lower` | stateless, `needs: line` | Final sigma. It differed exactly at the cuts before a final Σ, across a combining acute too, and was exact after `\n`, in Rust and Python (exp2) |
| `text.replace` (literal) | stateful, carry of `len(find) - 1`, `needs: char` | 0 mismatches over every cut of ten cases, including `aa`/`aaa`, `aba`/`ababa` and `issi`/`mississippi` (exp6) |
| `text.grep` | `needs: line`, `clock: new` | Drops lines |

**Two limits (change 14).**

- **The line rechunker is capped** at the type's `max_line` (1 Mi scalar values). A stream with no newline within that is a node `op` error naming the edge, never unbounded buffering.
- **Lone surrogates are not scalar values.** A TEXT or LINE item holding one is refused:
  - from `stream.push` or a fixture: `bad_params`;
  - when a Python node emits one: an `op` error at that node, checked by the Python executor's strict UTF-8 encode at emit.

  Both SDKs then count offsets identically. The corpus pins both cases.

### 3.4 Ordered events and live parameters

Things whose position matters travel in the data queues: `eos`, `gap`, `flush`, `format` and `barrier`, with revision 2's meanings.

Seek is unchanged. The host tells the source on the source's control channel. The source emits `flush{epoch+1, at}` in order, and nodes reset when the `flush` reaches them. Edges are FIFO, so no old-epoch item follows a `flush` on any edge.

**Live parameters (change 2): one writer per data edge.**

| Step | What happens |
|---|---|
| Stamp | Every edge has `sent_end: AtomicU64`: the end offset of the last item sent, written by the producer after each `send` (Release). On `stream.set{run_id, node, values}` the host reads `sent_end` of the target's clock input (Acquire). That is the primary for `latest`, the minimum over inputs for `time`, and the source's own next offset for a source. The value read is `at` |
| Deliver | The host `try_send`s `param{at, port, value, req}` on the node's **control channel**, a crossbeam `bounded(64)` already in the node's `select!` for cancel and seek. If it is full, `stream.set` fails with `busy` (retryable). It never waits on a data edge, so it is off the run path by construction |
| Order | The node keeps pending params sorted by `(at, arrival)`. Before processing data item `[o, o+len)`, it applies every pending param with `at <= o`. With `param_exact`, an item that straddles `at` is split there with the engine's `split`. Without it, `fixed_block` nodes apply at the next block start ≥ `at` |
| Late stamp | `sent_end` may lag a `send` that just happened, so `at` can be below the true queue tail. If the node has already consumed past `at`, it applies at once, at its consumed offset. Always `applied_at >= at` |
| Report | `stream.set` replies at once `{applied: [port], restart: [port], at}`. `applied_at` arrives in `stream.stats` and in `NodeReport.stream.params: [{port, req, at, applied_at}]` |
| Memo | From `applied_at` on, the node's output and everything below it are ineligible (§3.5.3) |

Nothing but the producer writes a data edge, so §3.6.2's invariant (`send` never blocks, because `cap <= cap_max`) holds. In a fused island the island coordinator owns the control channel; `at` comes from the island's input edge, and the param is applied at the target node's first block start ≥ `at` (island nodes share one clock). The UI coalesces knob drags to at most 50 `stream.set` per second per node.

### 3.5 Frontiers, joins and the memo

#### 3.5.1 Clocks and frontiers

As revision 2:

- A clock is a source's offset sequence.
- `clock: same` outputs are shifted back by the declared `latency`.
- Every edge carries a frontier, the lowest offset that may still arrive. Data advances it, `gap` advances it past the gap, and `eos` sets it to ∞.

#### 3.5.2 Two joins

| Join | Reads | Rule | End | Determinism |
|---|---|---|---|---|
| **`time`** | **Only the input at the minimum frontier**, in a `select!` limited to those inputs; the others stay in their queues | Inputs MUST share a clock (a graph error otherwise). Output is emitted for offsets below the minimum frontier | When all inputs reach ∞; a shorter input is padded with `silence`, or with the item type's `absent` | Deterministic |
| **`latest`** | The primary, waiting on it; **secondaries drained eagerly** whenever they are ready, keeping only the newest | Each primary item is paired with the newest secondary item seen | With the primary | Nondeterministic: everything below is `memo.scope: none`, marked in the editor |

- **Join buffers are at most one item per input** outside its queue, because a `time` join reads only from the lagging input. A full queue on the leading input is visible as `CreditWait` to the detector, not hidden in the join.
- **`latest` secondaries default to `conflate`, size 1** (change 8), shown on the wire in the editor. Eager draining means even a `block` override can't stall the shared source. `latest` enters the wait-for graph only as `ReadWait(primary)`, so a `latest` diamond never deadlocks (corpus D5).
- A reverb tail survives a `time` join: the wet frontier stays finite through the tail (`room_reverb` 80,160 frames and `plate_reverb` 161,760 at 48 kHz, exp1), and dry is padded with silence. This is batch `audio.mix`, which pads to the longer input with 0.0 and sums `a*ga + b*gb` (`crates/audio/src/edit.rs:208-213`). The join uses the same arithmetic in the same order.
- `zip` by `seq` and `merge` by arrival are out of v1.

#### 3.5.3 Memo eligibility

A stream node's output is memoisable iff, upstream of it:

- every source is `pull` and `deterministic`;
- every edge is `block`;
- there is no `latest` join;
- no param has applied;
- every node is chunk-independent at its unit, or on a `fixed_block` grid (part of its key);
- nothing depends on `ts` or pacing.

This is computed once per run, before anything runs. The plan key excludes the source chunk size and includes `fixed_block`. The stored value is the accumulated output, hashed once at `eos` (`nd1` hashes shape first, so it can't be incremental). A chunk manifest is never identity. A hit sends `cached`, never `started`, and replays through the engine's `split` at the run's chunk size, honouring edge policies. Unbounded and live streams are never memoised. Ownership is in §3.7.4.

### 3.6 Queues, backpressure, deadlock and memory

#### 3.6.1 Policies and the edge type (change 7)

```rust
pub struct EdgeQueue { pub size: u32, pub policy: EdgePolicy }   // plumcraft-proto, Q0
pub enum EdgePolicy { Block, DropOldest, Conflate }               // no key: one queue per input port
```

| Policy | Full queue | Default for |
|---|---|---|
| `block` | The producer waits for credit | Edges between nodes, **including a source node's outgoing edges** (size 8) |
| `drop_oldest` | The oldest item goes; a `gap` is queued | Taps; a push source's **device ring** (inside the source); explicit overrides |
| `conflate` | Keep the newest | `latest` secondaries (size 1); meter taps |

`Delivery` (`event.rs:274-281`) stays for the event channel only. Taps are events, so they use `Delivery::DropOldest(n)` per tap, and `stream.stats` uses `Latest(key)`. A workflow overrides per input port: `"queue": {"size": 32, "policy": "drop_oldest"}`. These names match dora-rs's `queue_size` and `queue_policy`, as vocabulary only (§3.12).

#### 3.6.2 The threaded executor's queues on crossbeam

A `block` edge is a data channel, `bounded(cap_max)`, plus a credit channel, `bounded(cap_max)`, pre-filled with `cap` permits.

- The producer `recv`s a permit (blocking when there is none: backpressure), then `send`s, which never blocks.
- The consumer returns a permit after each `recv`; a `time` join returns it when it reads the item.
- Growth is sending more permits.
- `select!` over a node's inputs and its control channel wakes a blocked thread at once. Exp3 measured 101 µs.
- Before blocking, a node writes `ReadWait(edge)` or `CreditWait(edge)` into an atomic.

Lossy edges are `crossbeam_queue::ArrayQueue`. `force_push` returns the evicted item (exp3), which is counted into a pending `gap`. A `bounded(1)` doorbell serves the `select!`. Python mirrors this with `threading.Semaphore` (`release(k)` grows) and a `deque` with a `Condition`.

#### 3.6.3 Push sources: the lossy point is the device ring

A push source is two parts.

- **The device callback** writes into an `ArrayQueue` ring. It never locks, allocates or blocks, as SoundCraft's `CaptureRing` does (`playback/src/record.rs:46`, atomics, no `unsafe`).
- **The source node's drain thread** turns the ring into items and sends on ordinary `block` edges. When the drain waits for credit, the ring overflows, and the drain emits one `gap{reason: overrun}` for the lost span when it resumes.

The drain is a node like any other, so it writes `CreditWait` and appears in the wait-for graph. A stall downstream of a microphone is an ordinary cycle, detected and grown like a bounded one, while the ring's drops are counted.

Revision 2's rule ("a push outlet dropping for longer than T while its consumer waits counts as `CreditWait`") is **dropped**. Review 2 (A7) showed it was unprovable as written.

An explicit lossy edge stays outside the wait-for graph: its loss is the policy, not a deadlock. The editor warns when one feeds a reconvergent `time` join (§3.3.3).

#### 3.6.4 Deadlock: detect, grow every full edge, fail by name

1. **Wait-for graph.** `CreditWait(e)` points at the consumer of `e`. `ReadWait(e)` points at the producer of `e`. A `time` join waits only on its minimum-frontier input, and a `latest` join only on its primary.
2. **When to look.** When any node has waited longer than T = 100 ms (as exp3), and at each stats tick (250 ms).
3. **Grow.** In a cycle containing a `CreditWait`, **double the credit of every full edge in the cycle at once**, up to `cap_max`, and record each growth in `stream.stats`. Exp3 grew one edge per step and needed 10 steps, alternating S→A and A→J; growing together halves that.
4. **Fail at the cap** with a `graph` error naming the join, the edges, caps, depths and frontiers. Exp3's form was: "deadlock at cap_max: join waits on W→J; S→A cap 64 len 64; …".
5. **Not deadlock.** A full edge whose consumer is running is overload: lossy points drop and count, with no error. A cycle of pure `ReadWait`s, or a stall with no cycle, is impossible in a DAG. If either is seen, it is reported as an engine bug, never left as a hang.

**The bound (F9).** Let k = ⌈log₂(cap_max / cap)⌉. Resolution or failure comes within (k + 1) detection rounds, and each round within 2T of the stall: **(k + 1) · 200 ms**. For cap 4 and cap_max 64 that is 1.0 s. Exp3 took about 140 ms a round; growing one edge per round, it needed 8 growths plus the failing round (1.26 s), where growing both edges together needs 4 plus 1 (about 0.7 s). Resolving a sliding window of n needs the e full edges on the bypass path to hold n between them: ⌈log₂(n / (cap · e))⌉ rounds, which is 5 for D1 (n = 200, cap 4, e = 2). The criteria in §5 use these formulas, and the tests report measured times.

**The deadlock corpus (change 1).** S = `text.lines` → `text.split_lines` (LINE items). A = `text.number`. W = `text.window{n: 200, hop: 1}` (`clock: same`, `latency: 199` items). J = `text.side_by_side` (a `time` join on LINE, `absent: ""`). Default caps 4, run budget default.

| Case | Graph | Expect |
|---|---|---|
| D1 resolve | S → {A, W} → J → collect | Growth events on S→A and A→J; output equal to the corpus's expected output (the cooperative reference with unbounded queues, which can't deadlock); resolved within the bound |
| D2 cap | D1 with `cap_max: 64` | `graph` error naming J, W→J and every cap and depth, within (k + 1) · 200 ms |
| D3 live | `text.ticker` → split_lines → {A, W} → J | The drain shows `CreditWait`; growth resolves the cycle; the ring's `gap`s are counted while it was stalled; no hang; then no further drops at ticker rate |
| D4 refused | D1 with `hop: 2` (`clock: new`) | Graph error before running, in `plumcraft-graph` and in the executor |
| D5 latest | S → {A (primary), W hop 2 (secondary)} → `text.tag` (`latest`), secondary forced `block` | Never stalls; every primary item emitted once, in order; tags non-decreasing; marked nondeterministic; nothing stored |

#### 3.6.5 Memory: four caps (change 10)

| Cap | Bounds | Default | Enforced |
|---|---|---|---|
| Queue budget | Σ over edges of `cap_max` × the edge's item bound, plus every node's declared `hold` × its item bound. Item bounds propagate from the source's `max_chunk` through each node's `out_item` | 64 MiB per run | At start (`cap_max` is derived from it, and an over-budget graph is a `graph` error). At runtime, an item over its edge's bound is a node `op` error (spec 18) |
| Join buffers | At most one item per input outside its queue (§3.5.2), so they sit inside the queue budget | — | By construction |
| Accumulate | One implicit or explicit accumulate, per the wire type's limit | AUDIO: `MAX_SAMPLES` = 230,400,000 samples (0.86 GiB of `f32`; **40 minutes of 48 kHz stereo is allowed**); TEXT: 64 MiB (as `MAX_LOAD`) | As items arrive; over the limit is a `config` error naming the wire |
| Retention | Each tap's window | 60 s and 64 MiB per tap; 256 MiB per session | Eviction, oldest first |

**The byte budget bounds queues, join buffers and declared holds, and nothing else.** Accumulate and retention have their own caps. A `whole` node behind an accumulate uses memory as it does in batch today.

#### 3.6.6 Latency profiles

- `live`: 10–20 ms chunks, 2–4 items, device rings lossy, 50–150 ms end to end.
- `batch`: 50–100 ms chunks, 8 items, `block`.
- Hard real-time (< 10 ms) is out of scope.

For audio at `fixed_block: 1024` and 48 kHz, a block is 21.3 ms. A 10 ms device block, one block of rechunk, one of processing and a jitter buffer give 60–90 ms before plugin latency. `pitch_shifter` adds 1,201 frames, 25 ms (review 2 A4, arithmetic only).

### 3.7 Executors and the host

#### 3.7.1 Which executor runs what

| Executor | Runs | Used for |
|---|---|---|
| Threaded | A thread per streaming node (and per `whole` node behind accumulate); credit; `select!` | Text, files, any `Send` SDK engine (all today: `Node: Send + Sync`, `lib.rs:128`) |
| Cooperative | One thread per subgraph. Each pass runs every node that has input and room for `max_out`, resuming `More` first. A stall with nothing runnable goes through §3.6.4's analysis on queue states | Audio islands; the whole graph of a non-`Send` `EngineCore` (spec 16.1); the parity reference |
| Pool | — | Out of Q |

The batch `exec::run` is unchanged apart from the memo handle (§3.7.4).

#### 3.7.2 Audio: a fused block render, exactly batch

A connected subgraph of `process` nodes on one clock (`fuse: true`, length-preserving, latency and tail declared) runs as one cooperative island on one thread. This is SoundCraft's `MixEngine` pattern (`mix/src/lib.rs:6,641`). Each node runs on its own input's 1024 grid from offset 0, and reproduces `render_with_tail` call for call:

| Batch (`fx.rs:154-195`) | Island node |
|---|---|
| `prepare(sr, 1024, ch)`; read `tail_samples()` (capped at 60 s); `prepare` again; `reset`; read `latency()` | The same sequence at `open` |
| Blocks over `total = len + tail + latency`; input past `len` reads 0.0, so the end-of-input block is full | At `eos`, the pending partial input block is **completed with zeros**, not processed short. Zero blocks then continue as `flush` steps until `total`; only the last call is partial (`n = total - done`) |
| `scratch` is 1024 long, zeroed past `n` | The same scratch rule |
| The first `latency` output frames are dropped | The same |
| A non-finite output sample becomes 0.0 (`fx.rs:189`) | The same |

By induction over topological order, every node gets the same input and makes the same `process` calls with the same contents and sizes, so the output is the same bits. The threaded executor with `fixed_block: 1024` satisfies the same argument; fusion is for latency and overhead. A rate-changing node ends an island.

#### 3.7.3 The stream slot (change 14: `run.multi`)

- **SDK engines (`Send`).** A stream run gets its own coordinator thread, holding an `Arc<Engine>` cloned from the table as `hello` does (`host.rs:74`). Batch runs, `output.save` and memo lookups continue on the worker.
- **`hello.limits`** adds `max_streams` (default 1) and `streams_share_worker`. `max_runs` stays 1, and **`run.multi` keeps its spec meaning** ("several runs may execute concurrently", `interface.md:232`). An engine with one stream slot beside a serial worker doesn't advertise it.
- **Non-`Send` cores** run streams on the worker, cooperatively, with `streams_share_worker: true`. The UI stops a stream to run a batch graph.

#### 3.7.4 Memo ownership and the panic path (change 5)

**The memo** moves from a worker local (`host.rs:467`) to `Shared`: `memo: Mutex<Memo>`, with `Memo { incarnation: u64, ... }`. The lock is held only inside `get` and `put` (`exec.rs:188,235`), never while a node runs. Every run, batch or stream, captures the incarnation at its start. `put` with a stale incarnation is a no-op, so an untrusted engine's results never land. A stream's lookups at start no longer wait behind a batch node.

**A panic in a stream node thread**:

| Step | Who | What |
|---|---|---|
| 1 | Node thread | `catch_unwind` around every `process`, `resume` and `flush` call catches it and tells the coordinator |
| 2 | Coordinator | Under the table lock: set `poisoned`; `engine = None` (no new run starts on it); `memo.incarnation += 1` and clear it. Then `engine_down` and `connecting` (`connecting{url: "... (restarting)"}`), as today |
| 3 | Coordinator | Close every channel of the stream; join node threads with a 1 s deadline. A thread that doesn't return is detached and counted in `engine.state`; `Arc` keeps the old engine alive until it exits |
| 4 | Coordinator | Set the **batch run in flight**'s cancel token (in the run table) with cause `engine` |
| 5 | Coordinator | Finish the stream: `run.reply{ok: false, error: engine "a node panicked (...); restarting"}` |
| 6 | Coordinator | Send `Work::Rebuild` on the worker's channel. `Job` becomes `enum Work { Run(Job), Rebuild }` |
| 7 | Worker | The batch run stops between nodes. The worker sees cause `engine` and finishes it as **failed `engine`**, not `cancelled`. Its late memo puts are refused by incarnation |
| 8 | Worker | Before each job and on `Rebuild`, if `poisoned.swap(false)`, it rebuilds with the factory it owns (`build`, `host.rs:441`). The same panic window counts stream and batch panics; five a minute stop it |
| 9 | Worker | `connected`; jobs queued meanwhile run on the new engine and are not failed |

**The converse**, a batch panic while a stream runs: the worker's own `catch_unwind` (`host.rs:482`) cancels the stream coordinator with cause `engine` and joins it (1 s). The stream ends failed `engine`. Then today's rebuild runs, now incrementing the incarnation.

**Python** follows the same steps. A node exception is that node's `op` error; only an exception escaping the executor rebuilds (`host.py:7-9`).

#### 3.7.5 Cancel, stop, pause

| Action | Ends with |
|---|---|
| `run.cancel` | The token is set and a control message reaches every `select!`; no flush; `cancelled` within 500 ms (spec 11 rule 3 becomes "between items, on every node thread") |
| `stream.control{stop}` | Sources emit `eos`; flush in order (resumable); `done`; `ok: true` with reduce values and accumulated handles |
| `pause` / `resume` | Pull sources stop pulling. Push drains drop to the ring (a `gap` on resume) or close. Stats keep beating |

### 3.8 Protocol

#### 3.8.1 A stream run is a run

```json
"stream": {"profile": "live", "queues": {"default": {"size": 4, "policy": "block"}},
           "retain": {"seconds": 60, "bytes": 67108864}, "stats_ms": 250,
           "inputs": {"n1.audio": {"elem": "AUDIO", "kind": "samples", "live": true, "format": {"sample_rate": 48000, "channels": 1}}}}
```

UI-fed streams go in `stream.inputs`, so `RunRequest.inputs` keeps spec 8.1's meaning (D14). Everything else in spec 8 and 11 applies.

#### 3.8.2 New operations

| Op | Params | Result | Capability | Path |
|---|---|---|---|---|
| `stream.control` | `{run_id, action: pause, resume, stop or seek, at?}` | `{}` (idempotent) | `stream.run` (seek: `stream.seek`) | Off the run path |
| `stream.set` | `{run_id, node, values}` | `{applied, restart, at}`, or `busy` | `stream.live_params` | Off the run path by construction (§3.4) |
| `stream.push` | `{run_id, input, seq, items: [Item], wait_ms?}` | `{accepted, credit, next_seq}` | `stream.push` | Off the run path; JSON carries text and JSON values only (spec 3.7 holds) |

#### 3.8.3 UI-fed bytes, and HTTP push without duplex (change 4)

| Mode | How |
|---|---|
| 1 | Typed `Arc` values |
| 2 | Not needed for audio: the engine shares the host and opens the device itself. Text and JSON go by `stream.push` |
| 3 | `POST /v1/streams/{run}/in/{node}.{port}?seq=s&wait_ms=w` with a **complete body** (`Content-Length`) of length-prefixed npy frames (D38) |
| 4 | Through the distribution engine (§3.12) |

The mode 3 push protocol, in steps:

1. The client sends at most as many frames as the credit it holds.
2. The server parses and validates every frame, then enqueues each frame with a permit the client's credit already covers.
3. The server answers `{accepted, credit, next_seq}`. If no credit is free, the response long-polls up to `wait_ms` (default 1000) for some. This is the same pattern as `events.poll`, on its own request thread (`http.rs:75`).
4. Credit is granted in batches, so there is one round trip per batch, not per frame.
5. `seq` makes a retry after a dropped connection idempotent. Frames below `next_seq` are acknowledged and ignored, and a jump is `409`, except for an explicit `gap` frame.
6. For a **live** UI source the client keeps a local `drop_oldest` ring while a request is outstanding, and sends one `gap` for what it dropped.
7. Mode 2's `stream.push` uses the same `{seq, credit}` fields.

**Why not duplex, or WebSocket now:** see D39.

#### 3.8.4 Taps and their numbers (change 3)

A watched stream port is a tap: `run.watch`, or the run's `watch`, with `WatchRequest.stream: {policy, max_rate_hz, want: view or listen, batch_ms}`.

- Tap appends are `node.live` `append` with `lossy: true`, `Delivery::DropOldest(n)` per tap, **outside the session ring**.
- `run.get` `partials` gains `from`.
- `run.watch` on a live stream port opens a tap. On a finished bounded stream it gives the retained accumulated display, or `missing`.
- `run.peek` gives the retained window (`provisional`), `{pending}`, or the accumulated value.

**Inline sample taps on the socket (D29), checked against their limits.**

| Quantity | Value |
|---|---|
| Stereo `f32` at 48 kHz | 384,000 B/s raw; **512,000 B/s in base64** (0.49 MiB/s) per full-rate tap |
| A 50 ms stereo batch (`want: view`) | 19,200 B raw → 25,600 B b64, + 128 B npy header (exp4: 8,320 − 8,192) + about 200 B envelope ≈ **25.3 KiB** |
| A 20 ms stereo batch (`want: listen`) | 7,680 B raw → 10,240 B b64 ≈ **10.3 KiB** |
| `limits.max_tap_inline` | **32 KiB per append** (encoded, envelope included) |
| Batch close rule | At `batch_ms` (default 50 for view, 20 for listen) **or** when the next frame would pass `max_tap_inline`, whichever is first. So 8-channel audio closes at about 16 ms |
| `limits.max_tap_rate` | **4 MiB/s per session** (encoded): about 8 full-rate stereo taps. Above it, the oldest batches of the tap with the most bytes in the last second are dropped first, counted as `gap`s |
| The measurement (Q1b synthetic, Q4b real) | **4 full-rate stereo taps** = 2,048,000 B/s ≈ 1.95 MiB/s, half the cap. Pass: tap latency ≤ 50 ms (view) and ≤ 25 ms (listen), and event latency rising ≤ 5 ms |
| HTTP | `max_tap_inline` = 0: `GET /v1/streams/{run}/{node}.{port}?from=` with npy frames |
| Spec 10.6 | "≥ 50 ms or ≥ 64 KiB" stays the rule for ordered appends (text). Lossy sample taps use the close rule above (an amendment to 10.6) |

A view tap MAY ask for `want: peaks` (min/max per 64 frames, 1/32 of the bytes), as `blob.derive`'s `peaks:<n>` does. Listen and record need full rate, so the measurement uses full rate.

#### 3.8.5 Status, stats and the reducer (change 11)

`node.status` stays per stream: `started` at open, `progress` (with `fraction` for bounded sources), `preview` repeatable (at most 1/s, the final one before `done`), and `done` after flush.

`stream.stats` (`Latest` per `(run, node)`, every `stats_ms`) carries:

- `state`, counts, rates, `busy`, `lag_ms`;
- per-input depth, capacity, frontier and drops; growths; `epoch`; params with `at` and `applied_at`;
- `status: {state, publisher, publisher_seq, fraction, ms, error}`: the header of the node's last **state-changing** record. Previews are never summarised.

**What the header heals: state only.**

- The reducer applies the header as a record of that state with no payload.
- A repeat is dropped by `(publisher, publisher_seq)` (spec 9.5 rule 5), so sending it every interval is safe.
- **Rule 5 is amended:** a late record whose key was already applied through a header still contributes its payload (thumb, outputs, display) under rule 3, and nothing else. Without this, a header that overtook its record would swallow the record's payload.
- **A lost `preview`** (thumb, outputs, watched display) or lost tap data is repaired by `run.watch`, which reopens the tap and refills from retention, or by `run.get` (`partials`, outputs). The header can't carry these.

Invariant 4 and F47 take the stream form: `started` after upstream's `started`, `done` after upstream's `done`, and the final `preview` before its `done`. `stream.stats` counts as activity for spec 12.3; Q1b builds the timeout, `idle_cancel_s` and `detach` for stream runs.

#### 3.8.6 Capabilities and spec changes

| Capability | Meaning | If absent |
|---|---|---|
| `stream.run` | `RunRequest.stream`, declarations, `stream.control` (pause, resume, stop), `stream.stats`, lossy taps | Stream nodes hidden |
| `stream.seek` | Seek on seekable sources | Seek hidden |
| `stream.live_params` | `stream.set` | Every edit restarts the stream |
| `stream.push` | UI-fed sources | UI microphone hidden |
| `stream.taps.inline` | Sample-stream taps inline under amended 3.7 | Taps by the binary endpoint |

| Spec | Change |
|---|---|
| 3 rule 7, 9.7 | One exception behind `stream.taps.inline`: lossy sample-tap appends MAY carry `inline` b64 up to `max_tap_inline` (32 KiB socket, 0 HTTP) within `max_tap_rate` (4 MiB/s per session), dropped first, outside the ring |
| 5.3, 5.5, 6.1 | The capabilities above; `stream.*` ops off the run path; `limits.max_streams`, `streams_share_worker`, `max_tap_inline`, `max_tap_rate`; **`run.multi` unchanged** |
| 7.2, 7.3, 7.6 | `stream` on wire types (incl. `absent`, `max_line`); `NodeDef.stream` (§3.3.4); the §3.3.3 rules and the corpus; the `EdgeQueue` type |
| 8.1, 8.4, 8.5–8.7 | `stream` field and `stream.inputs`; `ok` after `stop`; accumulated handles; `NodeReport.stream`; retention and `partials.from`; tap rules for peek and watch |
| 9.2, 9.4, 9.5 | `stream.stats`; tap delivery; F47 stream form; header self-healing for state; **rule 5's payload amendment** |
| 10.4, 10.6 | `WatchRequest.stream`; `lossy: true`; the lossy close rule |
| 11, 12.3 | Between items on every thread; stats are activity |
| 14 | §3.5.3 eligibility; incarnation-guarded inserts |
| 16.1 | Stream slot; cooperative executor for non-`Send` cores; §3.7.4's panic path |
| 16.4 | `GET .../{node}.{port}?from=` (taps) and `POST .../in/...` (push, credit in the response, long-poll), npy framed; SSE optional |
| 18 | New parsers: declarations, items, npy frames, stats, push batches; checked offset arithmetic; an item over its bound, a `seq` jump, a lone surrogate, or a line over `max_line` is an error, never a panic |
| 19 | The stream corpus, D1–D5, invariant 4 scoped |

Floats in `stream.set` values and stats use the spec's tagged encoding (spec 14, numbers rule 2). A non-finite param value is `bad_params` unless the port declares it. Audio frames may carry NaN, which the island maps to 0.0 on output, as batch does.

### 3.9 Data plane

| Bytes | Mode 1 | Mode 2 | Mode 3 |
|---|---|---|---|
| Between nodes | `Arc` values; never serialised | Same | Same |
| Text taps | Typed | `inline` text | `inline` text, or the endpoint |
| Audio taps | Typed (D19 `mem` locators only if profiled) | Inline b64 under amended 3.7 (§3.8.4) | Binary tap endpoint |
| Frame taps | Typed | A commonkit blob of a downscaled `rgba8`, `conflate` ≤ 10 Hz | Same, by `url` |
| Kept results | The accumulated value as a blob, hashed once at `eos` | Same | Same |

There is no shared ring in Q. If the measurement fails, a ring is a separate decision: a Rust-only writer, copy-out reads under a seqlock, and a new reviewed `unsafe` exception (AGENTS.md rule 2 allows only a reader). commonkit is unchanged; audio taps stay out of it (about 340 MB per live stereo tap at the 900 s TTL).

### 3.10 Editor and session

- **Typing:** stream typing in `plumcraft-graph`; accumulate badges; unit and clock errors; `latest` marks; **the `conflate` badge on `latest` secondaries**; the lossy-into-join warning.
- **Wires and nodes:** flowing, paused and dropping states; depth/capacity, rate, frontier lag, drops and growth.
- **Transport:** Start, Pause/Resume, Stop (drain), Cancel, a scrubber, uptime, tap latency, drops.
- **Live views:** waveform and meter, spectrogram (column by column), scrolling text with follow and search, the latest frame, sparkline, appending table.
- **Listen:** cpal playback through a jitter buffer with a **streaming** resampler. Today's `resample_linear` is whole-clip (`crates/ui/src/playback.rs:22,249`).
- **Session (`crates/session/src/run.rs`):**
  - `StreamState`;
  - `apply_event` learns `stream.stats` and `node.live` (today it handles `Status` and `RunReply` only, `:292-311`);
  - staleness for stream runs clears on the current generation's `started` (not on the reply, `:366-373`);
  - `progress()` shows streaming nodes as active (`:467-470`);
  - auto-run restarts a stream on a non-live edit; live edits send `stream.set`, coalesced to 50 Hz, and retry on `busy`.

  These files are in flux under the groups work, so Q2 rebases.
- **Commands** in `crates/session/src/stream_cmds.rs`, with `panic_hunt`: `stream.start`, `stream.pause`, `stream.resume`, `stream.stop`, `stream.seek{at}`, `wire.queue{wire, size, policy}`, `wire.accumulate{...}`, `watch.listen`, `view.follow`, `tap.record`. Control channel: `ui.waitStream`.

### 3.11 Engines and SDKs

#### 3.11.1 Rust SDK

```rust
pub enum Step { Done, More }
pub trait StreamNode: Send + Sync {
    fn def(&self) -> NodeDef;
    fn open(&self, info: &StreamInfo, params: &Inputs, cx: &mut Cx) -> Result<Box<dyn StreamState>, NodeError>;
}
pub trait StreamState: Send {
    fn process(&mut self, item: &Item, out: &mut Out, cx: &mut Cx) -> Result<Step, NodeError>;
    fn resume(&mut self, _out: &mut Out, _cx: &mut Cx) -> Result<Step, NodeError> { Ok(Step::Done) }
    fn flush(&mut self, _out: &mut Out, _cx: &mut Cx) -> Result<Step, NodeError> { Ok(Step::Done) } // until Done
    fn reset(&mut self) {}
    fn set(&mut self, _port: &str, _v: &Data, _at: u64) -> Option<u64> { None } // Some(applied_at)
    fn snapshot(&self) -> Option<Data> { None }
    fn restore(&mut self, _s: &Data) -> bool { false }
}
pub trait StreamType: Send + Sync {
    fn len(&self, v: &Data) -> u64;
    fn concat(&self, parts: &[Data]) -> Result<Data, NodeError>;
    fn split(&self, v: &Data, at: u64) -> Result<(Data, Data), NodeError>;
    fn silence(&self, _len: u64, _format: &Value) -> Option<Data> { None }
}
```

- **Registration:** `Engine::with_stream_node`, `with_stream_type`; `PullSource` and `PushSource` (a ring plus a drain).
- **Helpers:** `map`, `Carry`, `Windowed` (sliding or hopping), `Fold`, `TimeJoin`, `LatestJoin`.
- **`Cx`** is built per node thread. The executor's `Emit` becomes `Arc`-owned, since `TableEmit` borrows `&Shared` (`host.rs:163-174`). `publisher_seq` is assigned under the table lock.
- **Executors:** `exec::stream_threaded`, `exec::stream_coop`.

#### 3.11.2 Python SDK

- The same contract, plus a generator form whose end is the flush.
- Threaded only: `Semaphore` credit, `queue.Queue(64)` control with `put_nowait`, a lock around the memo with the incarnation rule.
- Pure-Python text nodes serialise on the GIL (accepted); numpy releases it.
- Emitted TEXT/LINE items are checked with a strict UTF-8 encode (§3.3.5).

#### 3.11.3 The text engine (the thin proof)

| Node | Contract |
|---|---|
| `text.lines` | Pull source over a value or file: N lines per chunk, TEXT, bounded, seekable, deterministic |
| `text.ticker` | Push source (ring plus drain): a numbered line every N ms; live |
| `text.split_lines` | TEXT → LINE items; `needs: line`, `clock: new` |
| `text.upper` / `text.lower` | Stateless; `needs: char` / `needs: line` |
| `text.replace` | Stateful carry; `needs: char` |
| `text.grep` | `needs: line`, `clock: new` |
| `text.number` | Stateful on LINE; resets on `flush` |
| `text.count` | Reduce, with snapshots |
| `text.window` | On LINE. `hop: 1`: sliding, `clock: same`, `latency: n-1`, `hold: n`. `hop > 1`: `clock: new` |
| `text.side_by_side` | `time` join of two LINE streams (`a / b`); pads with `absent` |
| `text.tag` | `latest` join; nondeterministic |
| `text.collect`, `text.slow_line` | Accumulate sink; a per-item delay |

#### 3.11.4 The audio engine (change 12)

- SoundCraft's `Plugin` (`dsp/src/lib.rs:195-212`) maps onto `StreamState`.
- `stream_load` is a pull source (packet decode, proposed to SoundCraft); `mic` is a push source; `save` is a streaming WAV sink.
- Normalize, reverse, trim, time stretch and pitch shift stay `whole` behind accumulate.

**Exp1, at default parameters, 2 s stereo, against `render_effect` at 1024:**

| Result | Plugins | Declaration in Q4a |
|---|---|---|
| Equal at 1, 64, 256 and 4096 | 23: eq_1band, eq_7band, compressor, expander_gate, maximizer (latency 71), channel_strip, room_reverb (tail 80,160), plate_reverb (161,760), mod_delay (80,849), auto_pan, chorus, flanger, phaser, ring_mod, saturator, lofi, rectifier, subharmonic, pitch_shifter (latency 1,201), gain, trim, invert, dc_offset_removal | Candidates to lift from `fixed_block: 1024`, once the cases below also pass |
| Differs at block 1 only (434 samples, max 6.5e-2) | de_esser | `min_block: 64` once the cases below pass at sizes ≥ 64 |
| Differs at every size (max 6.1e-5, one LSB at 16 bits) | dither | **Stays `fixed_block: 1024`**: same call sequence, so stream ≡ batch. A SoundCraft proposal draws noise frame-major, which would make it block-independent |

**The multi-size test, extended.** A plugin is lifted only when all of these give the same bits as batch:

1. fixed blocks of 1, 64, 256 and 4096 (or ≥ `min_block`);
2. **seeded random block sizes** per call, in [max(1, `min_block`), 4096] (an island's rechunked cuts vary);
3. **three parameter sets** per plugin from its `ParamInfo`: minimum, maximum and a mid value of every continuous parameter, and each choice;
4. **a mid-stream `set_param`** at a frame offset X, against the batch reference that applies it at the 1024-grid block start ≥ X. A plugin that is exact here at other block sizes may declare `param_exact`; otherwise param changes keep `fixed_block` semantics, applied at grid block starts.

Exp1 covered only case 1, at defaults. Its `latency()` and `tail_samples()` didn't change with `max_block`, and the test asserts that too.

#### 3.11.5 Other engines

- **OpenCV:** frames are item streams.
- **The research software supporting www.virtimp.com VI:** a per-frame style is an item map, not in Q; `research-local`'s non-`Send` core runs cooperatively.
- **MCP:** request/response only (D15).

### 3.12 Distribution (change 9)

Distribution is **an engine**, speaking protocol 2 unchanged.

| Concern | Revision 3 |
|---|---|
| Backpressure across machines | **Explicit credit** messages from consumer to producer on a per-edge credit queue, the §3.6.2 permit scheme. RabbitMQ `reject-publish` is not credit (nack and discard, then retry) |
| dora-rs | **Not a substitute for credit in 1.1.0.** In exp7 (`queue_size: 4`, `queue_policy: backpressure`, a 10 ms producer, a 30 ms consumer), the producer was **not throttled** (150 in 1.49 s). The input buffered past `queue_size` to a hard cap of 100, then dropped oldest: **7 items lost** in the delivered range 1–51, matching 7 logged drops. Its binary also says the direct Zenoh path "cannot honor that policy". It runs on this Mac (Python, arm64). Fit: **lossy or live streams**; lossless only if Q6's test passes |
| Recovery of stateful nodes | (a) Barrier checkpoints (Chandy–Lamport, Flink): `barrier{n}` from the sources, a snapshot when all inputs deliver `n`, restore plus seek on death, output identical. (b) Restart with an announced `gap` for nodes without `snapshot`, and for live sources |
| pipekit | `replicas: 1` per stream; `CONCURRENCY` pinned to 1 for stateful nodes (the default, `node.py:389`) |
| Replayable taps | RabbitMQ stream queues |

**Q6 tests lossless delivery explicitly**, on both candidates:

- 100,000 items, a producer 3× faster than the consumer, the development machine → a remote server;
- pass: no `seq` gap; the producer's rate within 5 % of the consumer's (it was throttled); resident memory bounded by the configured queue;
- the same with the consumer process killed and restarted (barrier restore).

The build is gated on D30, G3 and D22, and is outside the Q total.

### 3.13 Risks

| Risk | Mitigation |
|---|---|
| Executors drift (batch, threaded and cooperative; Rust and Python) | One corpus against all; stream ≡ batch as a property test |
| A wrong unit or block declaration | Every-cut and multi-size tests per node, with the §3.11.4 extensions |
| Growth hides a design error | Growth in stats and on badges; a named error at the cap; the time bound stated |
| The spec 3.7 exception erodes | Capability-gated, lossy only, outside the ring, rate-capped, measured |
| Long-poll push latency over a WAN | Batched credit; D39's reopening criterion, measured in Q3 |
| Untested non-`Send` path | A test engine holding `Rc` in Q5 |

## 4. Decisions for the maintainer

D25–D38 keep revision 2's numbers. D39 is new.

| | Question | Options | Recommendation |
|---|---|---|---|
| D25 | Model; milestone | (a) two kinds per wire type, three contracts, in-band items, frontiers; (b) "list over time"; (c) rerun per chunk | **(a)**, milestone **Q**. Refined: push sources are lossy at their device ring, and the source node's edges are `block` |
| D26 | How a stream starts | (a) `run` with `stream` (`stream.inputs`); (b) `stream.open` | **(a)** |
| D27 | Queue policies | Defaults per edge, ring, tap | **`block`** size 8 between nodes and out of sources; **`drop_oldest`** at push-source rings and taps; **`conflate`** size 1 for `latest` secondaries and meters. **A new `EdgeQueue{size, policy}` type**; `Delivery` for events only |
| D28 | Executors | (a) threaded everywhere; (b) threaded for text and files, cooperative fused render for audio and non-`Send` cores; (c) a pool now | **(b)**, with resumable `process`/`flush` (`Step`) |
| D29 | Tap bytes on one host | (a) typed in mode 1, inline on the socket, endpoint on HTTP; (b) a blob per chunk; (c) a ring now | **(a)** with **32 KiB per append, 4 MiB/s per session, batches at 50/20 ms or the cap**; measured with 4 full-rate stereo taps |
| D30 | Distribution | (a) pipekit with credit and barriers; (b) dora-rs under an engine; (c) defer | **Evaluate both in Q6 with the lossless test.** (b) is for lossy or live streams unless it passes |
| D31 | Memo | (a) graph-property eligibility, accumulated identity, cached replay; (b) none | **(a)**, with **`Mutex<Memo>` in `Shared` and incarnation-guarded inserts** |
| D32 | Monitoring | (a) through taps, 50–150 ms; (b) device sinks only | **(a)** |
| D33 | Live parameters | (a) `param` on the node's control channel, `at` from the clock input's `sent_end`, ordered by offset at the consumer, `applied_at >= at` recorded, unmemoisable from there; (b) appended to the data queue (revision 2; breaks single-writer); (c) restart per edit | **(a)** |
| D34 | Thin slice | (a) Rust text in modes 1 and 2, Python next; (b) both | **(a)** |
| D35 | Joins | (a) frontier `time` join reading only the lagging input, plus `latest` with eager secondaries; (b) also `zip` and `merge` | **(a)** |
| D36 | Status in unbounded runs | (a) stats carry the last state-changing header (state only), rule 5 amended to keep late payloads, `run.watch`/`run.get` for lost previews and data; (b) periodic `run.get` | **(a)** |
| D37 | Deadlock | (a) wait-for detection, doubling every full edge in the cycle, a named error at the cap, (k + 1) · 2T bound; (b) static sizing plus a watchdog | **(a)**; T = 100 ms; the five-case corpus |
| D38 | Binary framing on HTTP | (a) length-prefixed npy (6.9 µs encode, 15.1 µs decode per 21 ms chunk, exp4); (b) Arrow IPC (4.0 / 0.7 µs) | **(a)** now: both are under 0.2 % of a chunk, and npy has parsers in both SDKs (`crates/store/src/npy.rs`, numpy). Arrow IPC when a `TABLE` stream wants it |
| **D39** | HTTP push transport | (a) complete-body `POST`s of at most the held credit, the response returning credit and long-polling when none is free, `seq` for idempotent retry; (b) WebSocket (reopen D5): `tiny_http::Request::upgrade` (`request.rs:305`) plus a WebSocket crate on both sides and a Python server; (c) chunked duplex `POST` | **(a).** (c) is impossible on ureq 3.5 and tiny_http 0.12 (§3.1). (a) needs no dependency, fits spec 16.4's request/response and `events.poll` pattern, keeps the thread-per-request servers in both SDKs, and costs one round trip per credit batch: well under 1 ms on loopback, a few ms on the LAN, against 20–50 ms batches. **Reopen D5 (b)** if Q3 measures a sustained push of 20 ms stereo frames the development machine → a remote server adding more than one batch of latency, or falling behind real time |

**Earlier decisions this touches:**

| Decision | Effect |
|---|---|
| D5 (WebSocket deferred) | **Stays deferred.** Revision 2's "no longer needed for push" is withdrawn; D39 states when to reopen it |
| D6 (SSE deferred) | Not forced: tap bytes are off the event channel; long-poll suffices at stats rate |
| D14 (`inputs` reserved) | Not overloaded: `stream.inputs` |
| D15 (MCP) | MCP can't stream |
| D19 (`mem` locators) | Mode 1 audio taps are the profile that decides it |
| D18, D22 (Session consumer) | Carries `stream.*` on pipekit |
| D23 (ownership, idle cancel) | Unbuilt; Q1b builds the stream-run part |
| D10, D11 | Tap recordings go to the durable tier under the same leases |
| Plan 2.1 | The `plumcraft-shm` row (`plan.md:43`) should be corrected |

## 5. Milestone Q

| | Step | Content | Exit criteria (conformance cases) | Days |
|---|---|---|---|---|
| Q0 | Spec and types | Every §3.8.6 row in `docs/interface.md`; `plumcraft-proto`: declarations (incl. `absent`, `max_line`), items, `EdgeQueue`, control messages, `stream.stats` with header, ops, push batches; schema; corpus format with D1–D5 and the surrogate cases; hostile sweep entries | Schema regenerated; fixtures round-trip; sweep green; spec text reviewed by the maintainer | 4–5 |
| Q1a | Executor (Rust text, mode 1) | `StreamNode`/`StreamType`/`Step`; sources (pull; push = ring plus drain); the threaded executor (credit, `select!`, `ArrayQueue`, control channels, in-band items, frontiers, both joins, the detector with all-edge growth); live params by control channel; memo eligibility (static); the text nodes (§3.11.3); `plumcraft-cli stream` | **Stream ≡ batch** byte for byte for every deterministic corpus graph at chunk sizes 1, 7, 64, 1000; **split-boundary**: `replace` at every cut, Greek final sigma under line rechunk, char-chunked `lower` refused; line over `max_line` is an error; **D1–D5** as §3.6.4, with D1/D2 times within (k + 1) · 200 ms, reported; overload behind a ticker counts drops with no error; **seek under load**: no old-epoch item at any node after `flush`, `number` restarts; **single writer**: 1,000 `stream.set` under full backpressure never block over 1 ms, a data channel never exceeds `cap_max`, every `applied_at >= at`; **resumable flush**: a 2,800-step flush through `max_out: 1` stays within caps; cancel from blocked threads under 500 ms; stop drains to `ok` | 9–11 |
| Q1b | Host (modes 1 and 2) | Stream slot; `Mutex<Memo>` with incarnation; §3.7.4 panic path both ways; `max_streams`/`streams_share_worker` in `hello`; `stream.*` ops on the socket; stats with headers and the rule 5 amendment; taps, retention, peek and watch on stream ports; inactivity timeout, `idle_cancel_s`, `detach` for stream runs | **Panic in a node thread with a batch in flight**: stream reply failed within 1 s, batch reply failed `engine`, no memo entry from either, `engine_down` → `connecting` → `connected`, no thread left, the queued job runs; **batch panic with a stream running**: the stream ends failed `engine`; memo hit on a bounded deterministic rerun replays as a stream with `cached`; nothing stored behind `latest`, a lossy edge or a param; **lost record**: a dropped `started` and `done` healed within 2 stats intervals without a reply; **lost preview** repaired by `run.watch`; a header before its record keeps the record's payload; invariant 4 stream form; a 10-minute stream not timed out; idle cancel unless `detach`; `kill -9` mid-stream in mode 2; **synthetic tap measurement**: 4 full-rate stereo-equivalent sample taps within 32 KiB and 4 MiB/s, event latency rise ≤ 5 ms | 8–10 |
| Q2 | Editor | Stream typing in `plumcraft-graph`; wires, badges (`conflate` on `latest` secondaries, the lossy-into-join warning), transport, live text views; `StreamState`; `stream_cmds.rs` with `panic_hunt`; `ui.waitStream`; rebase on the groups work; P06 | P06: the maintainer streams a ticker through grep, number and count, pauses, seeks a file source, turns a live parameter without a restart and sees `applied_at`, and sees a refused `time` join explained before running; kittests and control green | 6–8 |
| Q3 | Python parity and HTTP | Python SDK and threaded executor; the same text nodes; HTTP tap `GET ...?from=` and push `POST` with credit long-poll (D39), npy frames, both SDKs | Q1a and Q1b sets in modes 2 and 3 for both engines; corpus identical in Rust and Python, **including the surrogate cases** (`"\ud800"` in a push is `bad_params` in both; a Python node emitting one is an `op` error); a **lossless push** of a 10-minute TEXT stream the development machine → a remote server with a connection dropped mid-way: no loss or duplicate, by `seq`; remote tap resumes from retention; push latency measured against D39's criterion | 6–8 |
| Q4a | Audio, offline | Cooperative islands reproducing §3.7.2 call for call; audio stream nodes at `fixed_block: 1024`; the extended multi-size test (§3.11.4); `stream_load`, `save`; SoundCraft proposals (packet decode, a public block render with tail, frame-major dither) | **Stream ≡ batch** bit for bit for gain → EQ → reverb → save on a 5-minute file; **reverb tail through a `time` join** equals batch `audio.mix` at length `len + tail`; NaN input gives the batch's 0.0 output; a 2-hour WAV under 200 MB resident; the per-plugin table regenerated (expect 23 lifted at defaults, `de_esser` at `min_block: 64`, `dither` fixed) and extended with the parameter cases | 8–10 |
| Q4b | Audio, live | `mic` (cpal ring plus drain); meters, spectrogram; Listen with jitter buffer and streaming resampler; `InputRing` outside `Player` (proposal); P07 | mic → gain → compressor monitored under 150 ms for 10 minutes with zero drops and underruns on the development machine; a knob turned while listening shows `applied_at`; **4 full-rate stereo taps** inline: tap latency ≤ 50 ms view and ≤ 25 ms listen, event latency rise ≤ 5 ms; P07 listen test | 7–9 |
| Q5 | Cooperative parity, non-`Send` | Whole graphs on the cooperative executor; a test engine holding `Rc` | The full corpus (D1–D5 included) identical on both executors; the non-`Send` engine passes Q1a and Q1b minus the concurrency cases | 3–4 |
| Q6 | Distribution evaluation | dora-rs and pipekit-with-credit spikes, the development machine ↔ a remote server; a barrier prototype on text | **Lossless test** (§3.12) on both, with numbers; the lossy/live case on both; a written D30 recommendation; a killed node restored from barrier n gives identical output; a non-snapshot node restarts with an announced `gap` | 5–7 |
| | **Total** | | | **56–72** |

The distribution build, after D30, is 10–14 days, outside the total and gated on G3 and D22. Against revision 2's 43–57, the increase is the split of Q1 (review 2 §6 judged 11–14 optimistic by about half: now 17–21), the split of Q4 (now 15–19), Q0's added types, Q2's rebase, and Q6's lossless test.

## 6. Proposed plan.md section

### 10.5 Milestone Q (proposed): streaming in queued chunks

*Proposed 2026-10-27 in `docs/research/13-streaming-r3.md` (revision 3, after reviews in notes 10 and 12); not decided.*

The maintainer asked: "plan processing data in queued chunks, to deal with continuously streaming data."

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
