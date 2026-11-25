# N1 slice 9: the two Linux C ABI failures of pull request #105

Branch `native/n1-linux-fixes` (at d3cc1fa), working tree only: nothing committed, staged or stashed. 2026-11-19.

## Summary

- **Failure 1** (`control_port`, `"cached"` where `"done"` was expected) is caused by slice 7, and the extra run is intended behaviour, not a defect.
  - `run.watch` through the C ABI now starts a run of the watched node, as the editor's `command.run` always has and as `docs/shell-interface.md` 6.2 requires.
  - The test's `run.start` is therefore the second run. When the first has already finished (a slow machine), every node is `cached`.
  - I widened that one assertion to accept `done` or `cached`.
  - With the timing forced: 50 of 50 runs failed before the change, 0 of 50 after.
- **Failure 2** (`switch`, `plum_request` timed at 56 ms against a 50 ms limit) is not a wait.
  - `plum_request` only queues: it takes 13 to 134 µs on the Mac, even with 150 busy processes beside it.
  - Since slice 7, the timed call overlaps the core's own startup, because `plum_open` no longer waits for the engine. On a loaded 4-core runner the caller's thread can be descheduled meanwhile.
  - I could not reproduce the 56 ms on macOS.
  - The test now waits for its first engine before timing, and holds `plum_request` to under a quarter of the switch it must not wait for, instead of a fixed 50 ms.
- **Load runs:**
  - after the fixes, 50 of 50 runs pass under load for each test;
  - the `plumcraft-ffi` tests pass 30 times out of 30 with 28 busy loops beside them;
  - the suite as CI runs it: under b3 and under md5, 1,395 run, 1,394 pass, and the one failure is the expected `params_coverage hidden_params_match_the_baseline`.

## Failure 1: `plumcraft-ffi::control_port a_core_opened_through_the_c_abi_is_driven_over_its_control_port`

### What starts the first run

The test sends `node.add` ×2, `wire.connect`, `node.set`, `run.watch {node: up}`, then one `run.start`, `ui.waitRun` and `ui.inspect`. Linux's reply shows two runs, and the first was not superseded:

- `runId` ending `-2`, `run.generation` 2, `superseded` 0;
- `auto` false, `awaitingReview` false.

I went through every candidate for the first run:

- **`run.watch` starts it.** This is the cause.
  - Since slice 7, the C ABI runs commands through `document::execute`. Its `run.watch` follow-up runs `run.start {targets: [node]}` when `run.start` is enabled (`crates/editor-core/src/document.rs`, in `execute`).
  - At `f22f770` the C ABI's `Host::run` was only `session.execute` plus `send_outgoing` (`git show f22f770:crates/ffi/src/core.rs`, line 517), and the session's own `run.watch` only sets the watch (`crates/session/src/run_cmds.rs` `watch`).
  - So before slice 7, one `run.start` meant one run.
- **Auto-run: no.** `auto` is false and `autoPending` false.
- **The review prompt: no.** `awaitingReview` is false, and nothing here was read from a file.
- **`ui.waitRun`: no.** It only parks a reply, which `control::tick` answers.
- **Held requests at startup: no.** They are replayed in order through the same dispatcher and start nothing of their own.

### Regression or a wrong expectation

It is the test's expectation that is out of date, and it predates nothing: it is slice 7's deliberate unification.

- **The egui editor.** It has always done this. `git show f22f770:crates/ui/src/lib.rs`, lines 759 to 765: `if id == "run.watch" { … self.run("run.start", &json!({"targets": [node]})) }`.
- **The contract.** `docs/shell-interface.md` 6.2 says `command.run` "runs through the core's `try_run`, including its follow-ups: `run.watch` reruns".
- **What the editor does in the same sequence.**
  1. `command.run run.watch` queues the watched node's run.
  2. `command.run run.start` then either supersedes it (a fresh run: `done`) if it is still in flight, or, if it has finished, runs a graph that is now all memoised: `cached`.
- **Why it usually passed.** On a fast machine the second request arrives while the first run is still in flight.
- **Why the slice 8 agent saw it locally.** Its 2 failures in 4 runs are the same race, made likelier by slower turns.

### The fix

`crates/ffi/tests/control_port.rs` line 109: `assert_eq!(state, "done")` became `assert!(matches!(state, Some("done" | "cached")))`, with a comment saying why. Either state means the node was computed in a finished run and is current. Nothing else in the test changed, and the core is unchanged.

### Reproduction

`claude-debug/native-n1/s9_loop.py` runs the test binary directly, one test at a time, optionally with busy loops beside it (started by captured PID).

| Condition | Before | After |
|---|---|---|
| A 400 ms sleep put between `run.watch` and `run.start` (a temporary edit, removed afterwards), so the watch's run always finishes first | 50 of 50 fail (`"cached"`) | 0 of 50 fail |
| Unforced, 28 busy loops on the 14-core Mac | 2 of 20 fail | 0 of 50 fail |

## Failure 2: `plumcraft-ffi::switch starting_a_process_engine_never_holds_the_core_thread`

### What the timed window does

It covers `Client::send` alone: `req.to_string()`, then `plum_request`, which:

- copies the bytes;
- runs `guard`;
- briefly locks `CORES`, which only `plum_open`, `plum_close` and the per-call lookups take; the core thread never does;
- does an `AtomicU64::fetch_add`;
- sends on an unbounded `mpsc` channel.

Nothing there waits for the core thread, `settle_startup`, a lock the core thread holds, or the engine.

- **The engine binary build** (`engine_binary()`) happens before the timed window.
- **On the Mac,** with a temporary `eprintln!` of `sent`, 30 runs each:

| Load | `sent` minimum | median | maximum |
|---|---|---|---|
| None | 0.014 ms | 0.016 ms | 0.134 ms |
| 28 busy loops | 0.015 ms | 0.023 ms | 0.036 ms |
| 150 busy loops | 0.013 ms | 0.022 ms | 0.110 ms |
| 120 bursty loops (4 ms work, 0.3 ms sleep) | 0.013 ms | 0.020 ms | 0.028 ms |

### What changed with slice 7, and where the 56 ms most likely went

- **The overlap.** Before slice 7, `plum_open` returned only after the in-process text engine was built, connected and its catalogue read. Now it returns at once, so the timed `plum_request` runs while the same process is building that engine and turning (`plum_open` to ready is about 116 ms on the Mac in debug: slice 7's `documents.rs` measurement).
- **The runner.** On GitHub's 4 slow cores, nextest runs every test in parallel, and each FFI test process has several busy threads. A runnable thread there can wait tens of milliseconds for a core.
- **Inference, not reproduced.** The 56 ms is the test thread being off the CPU, not `plum_request` waiting. macOS's scheduler favours a mostly-sleeping thread over busy loops, so I could not reproduce it here (0 of 50 runs above 0.2 ms).

### The fix

In `crates/ffi/tests/switch.rs`:

- **Wait for the first engine.** Before timing, the test calls `graph.get`, which is held until the document's first engine is there. That restores the quiet core the measurement had before slice 7.
- **An honest limit.** The assertion is now `sent * 4 < switched`, in place of a fixed 50 ms. Its intent is "doesn't wait for the process engine's start", and that start took 768 to 1,564 ms under load on the Mac. A thread preempted for 56 ms still passes; one that waited for the engine cannot.
- **The core is unchanged:** there was no wait to remove.
- **After:** 0 of 50 failures with 28 busy loops beside it (none before either, on the Mac).

## Similar assertions in the C ABI tests (slices 4 to 8) and elsewhere

| Where | What | Judgement |
|---|---|---|
| `crates/ffi/tests/c_abi.rs` lines 32, 82, 217–218 | `"done"` after one run | Safe: a fresh graph, one `run.start` or `run` with no `run.watch` before it, and a core of its own each |
| `crates/ffi/tests/switch.rs` line 26 (`runs_text`) | `"done"` | Safe: `run.start {watch}` is one run, on an engine instance just started |
| `crates/ffi/tests/media.rs` `run.watch` then `run` (lines 81–85) | The same race as failure 1 | Safe: it asserts only `running: false` and the watched value's handle, which exist whichever run made them |
| `crates/ffi/tests/media.rs` lines 99, 104, 113, 119 | `media.get` under 150 ms; `plum_blob` under 50 ms (twice); a store read under 250 ms, against a 300 ms slow resolve | Somewhat fragile on a loaded runner. Each separates "waited for the 300 ms read" from "didn't" with a 2 to 6× margin. Not failed on Linux or in 30 loaded runs; left, but these are the next candidates if one flakes |
| `crates/ffi/tests/documents.rs` line 272 | `plum_open` under 500 ms | Generous for a call that does no engine work; left |
| `crates/ffi/tests/documents.rs` (other tests) | Counts of nodes and wires, undo labels, texts | Deterministic; no run-state assertions |
| `crates/ffi/tests/menus_log.rs`, `asks.rs` | Only 10 to 60 s "no reply" limits | Safe |
| `crates/ui/tests/engine_thread.rs` `values_never_wait_for_an_engine`, its audio phase held under 150 ms | It measures the in-place spectrogram, which is a real stall of the editor's thread but not an engine wait (slice 6: 35 to 97 ms alone; I saw 195 ms once under full parallel load during slice 7, and 77 to 80 ms alone) | Not changed. It passed on Linux in this run and in all later local runs. Its fix is the code's: a `Want::Spectrogram` on the media service (slice 6's plan), not a looser limit |
| `crates/ui/tests/engine_thread.rs` lines 279, 294, 670 | Frames under 400 ms against an 800 ms or 1.5 s fake start | Margins of 2× and more; left |

## Test files edited

- **`crates/ffi/tests/control_port.rs`:** one assertion, `done` to `done` or `cached`, with the reason in a comment. It encoded the C ABI's pre-slice-7 divergence from the editor (`run.watch` didn't run).
- **`crates/ffi/tests/switch.rs`:** waits for the first engine before timing (`graph.get`), and holds `plum_request` to under a quarter of the switch's time, with comments.

No other file changed. The temporary sleep and `eprintln!` edits used for the measurements were reverted (`git checkout` of both files) before the fixes.

## Tests

| Run | Ran | Passed | Failed | Skipped | Failing |
|---|---|---|---|---|---|
| Final b3 (`s9-final-b3`) | 1,395 | 1,394 | 1 | 12 | `plumcraft-craft::params_coverage hidden_params_match_the_baseline` |
| Final md5 (`s9-final-md5`) | 1,395 | 1,394 | 1 | 12 | the same |
| `plumcraft-ffi` ×30 beside 28 busy loops (`s9-ffi-load.txt`) | 26 each | 26 each | 0 | 4 each | none |

The counts are slice 8's 1,395 (slice 7's 1,378 plus slice 8's 17). Each run folder's store and craft data were removed after its result was read.

Gates:

- `cargo fmt --all --check` and `cargo clippy --all-targets --all-features -- -D warnings`: clean;
- `scripts/check-hash-gates.sh`: ok;
- `scripts/check-core-toolkit-free.sh`: ok.

No real editor was started this slice.

## Disk

38 GB free at the end (37 GB at the start). The `s9-` scratch is a few MB: the scripts `s9_loop.py` and `s9_ffi_load.py`, their outputs, and the run logs.

## What the docs should say

- **`docs/shell-interface.md` 6.2,** the `command.run` row: over the C ABI this now holds too. `run.watch` starts a run of the watched node, so a client that follows it with `run.start` sees a second run: `cached` if the first had finished.
- **The same file, 5.2:** `plum_request` only queues, and its cost is microseconds. Since `plum_open` returns before its engine, the first requests overlap the engine's start in the same process.
- **`docs/STATUS.md`:** the two Linux C ABI failures of #105 are fixed in the tests. Neither was a core defect: one was the expected second run after `run.watch`, the other a fixed 50 ms limit measuring the scheduler.
- **`docs/BUGS.md`:** optionally, the spectrogram stall that `values_never_wait_for_an_engine` measures at 150 ms, as a flake risk on loaded runners until it moves to the media service.
- **`AI.md`:** at commit time. Slice 7 shipped a test whose expectation its own behaviour change made racy; it was not run under load before handing back.
