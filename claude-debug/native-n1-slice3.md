# N1 slice 3: the UI thread never waits for an engine

Branch `native/n1-off-ui-thread`, working tree only (nothing committed, staged or stashed). 2026-11-19.

## Summary

- **New: `plumcraft_editor_core::worker::EngineWorker`.** It is one thread per document, started on first use. It makes every engine call the document asks for, in order (runs, cancels, releases, and the waiting part of an engine switch). Results come back through an inbox that the owner drains on its own thread, and the owner's `Waker` says when something is there.
- **The switchboard now works in three parts.** `Switchboard::begin` runs on the shell's thread and does what waits for nothing. `Work` (stop the old engine, start the new one, read its catalogue) runs on the worker. `Switchboard::finish` runs on the shell's thread again. The synchronous `pump` is kept for callers that may wait.
- **The egui shell:**
  - Runs, releases and cancels go to the worker.
  - A switch made inside a frame goes to the worker, and the title and status bar say "connecting".
  - A control request that switches (`command.run`, `ui.invoke`, `ui.prompt`) replies once the switch is done, so `docs/control-protocol.md`'s "switch before they reply" still holds.
  - Callers outside a frame (tests, scripts, PlumImpress) still get the outcome when `try_run` returns. They wait for the worker.
- **The FFI's core thread uses the same worker.** `plum_request` of a run never blocks it, and `engine.*` commands over the C ABI are now performed. Each replies once its switch is done.
- **Stalls on the main thread** (debug build, longest frame):
  - Switching text to process mode: from 190 to 244 ms down to 3 to 6 ms.
  - With a fake engine that takes 2 s to start: from 2.2 s down to 10 ms.
  - Starting a run on an engine that takes 500 ms to accept it: from 776 ms down to 228 ms. That is the same as on a fast engine; the remaining time is egui redrawing 500 nodes.
- **Tests:**
  - The same single expected failure before and after, in three b3 runs and one md5 run.
  - 1,314 tests ran before and 1,320 after: 6 new, none gone.
  - One existing test was edited (the Attach window's), plus a forced one-field change to a core test helper.
  - A new test, `the_ui_thread_never_calls_an_engine`, fails on the baseline and passes now.
- **Left for later:** item 3 (startup still waits for the first engine), item 4 (`resolve` on the UI thread), and one deliberate exception: an engine compiled in (in-process) still connects in place, in about 3 ms.

## The worker's design

| Aspect | What it does |
|---|---|
| Threads | One `plumcraft-engine-worker` thread per document (`EngineWorker`), started by the first job, so an editor that never talks to an engine has none. The FFI gets one per core; the egui editor one per `EditorApp` |
| Queue | An `mpsc` channel of boxed jobs (`FnOnce() -> Option<Done> + Send`) and a counter of jobs not yet finished. Jobs run strictly in the order given |
| Ordering | One FIFO lane per document. A cancel sent after a run goes out after it. A release goes out after its graph's runs. A superseded run's `Cancel(old)` precedes `Run(new)`, as `RunState::request` queues them. The old engine is told and stopped (`Retired`) before the next one starts, inside the same `Work` |
| What comes back | `Done::RunFailed {run_id, generation, why}`, which the owner applies only if that run is still the latest, so a superseded run's refusal is ignored as its late events are. `Done::Switched {worked, back}` carries a switch's result and the connect function the job was given (`back`, a `Box<dyn Any + Send>`; the egui shell moves its `Connector` into the job and takes it back here) |
| How results reach the session's thread | `Done`s wait in the inbox. The worker wakes the owner (the same `link::Waker` its `Link` uses: egui's `request_repaint`, or the FFI's `Msg::Drain`) only when it has put something there. The owner drains at the start of a frame (`EditorApp::pump_engine`) or on `Drain` (FFI). `RunState` and the `Session` are only ever touched on the owner's thread |
| Panics | Each job runs under `catch_unwind`, so a panicking engine neither kills the worker nor leaves its counter up |
| Shutdown | Dropping the worker sets `stop`, closes the channel and waits for the thread at most `SHUTDOWN_WAIT` (2 s). Jobs not yet started are dropped unrun; dropping them drops their links and connections on the worker. A job stuck inside a hung engine call is left to finish on its own: every host's calls time out, then the thread ends. Nothing joins a thread that might not end. Tested by `worker::tests::dropping_it_stops_its_thread_and_never_waits_long_for_a_hung_engine` |
| Test-visible idle | `EngineWorker::idle()`, `queued()`, `wait(timeout)`. On the editor: `EditorApp::engine_busy()` and `EditorApp::wait_engines(timeout)` |
| Manifests | `engines::EXTERNALS` is per thread, so each switch's `Work` carries a snapshot (`engines::Manifests::here()`), which the worker installs before starting engines. Without it, a manifest engine (`pytext`) is unknown on the worker |

**Engine switches.** `Switchboard::begin` runs on the shell's thread and does what waits for nothing:

- takes the session's request;
- forgets the old run state and collects the old link, its run cancels and the old coordinator as `Retired` (never dropped on this thread);
- sets `engines.current` to the target, so the title and status bar name what is coming;
- returns `Next::Work`, or `Next::Done` for an operation that needs no engine (clear, reload, install, and the refusals).

`Work::run` runs on the worker: stop the retired engines, then connect or attach, then call `schema`. For an add or remove it also performs `Coordinator::add`/`remove`, `Link::start` on the coordinator, `schema` and `warnings`. `Switchboard::finish` runs on the shell's thread: it installs the link and catalogue and produces exactly the outcome JSON and `last_error` it produced before. A reconnect with added engines chains: switch, then each add again, returning the switch's outcome, as before. While a switch is in flight, `Switching::busy()` is true. New requests wait in the session and begin when it finishes.

**What also moved, which the brief didn't list.** Dropping the old `Link` joins its pump: up to 250 ms, one `events.poll`. Dropping a spawned engine's last holder stops it: `ProcessHost`'s drop allows a 2 s grace (`crates/host/src/process.rs:274`), and `HttpHost` kills and waits. Both happened on the UI thread at every switch and clear. They now happen on the worker. `forget_runs` also clears `session.docs.resolver`, so the retired link holds the engine's last reference.

**How "switch before they reply" is kept:**

- **In `EditorApp::handle_control`,** `command.run`, `engine.execute`, `ui.invoke` and `ui.prompt` run their command in frame mode.
  - If the command asked for a switch (`engine_asked`) and it is under way, the reply is parked in `engine_waits`, as `ui.waitRun` and `ui.screenshot` already park theirs, and the frame goes on.
  - When the engines are idle again, it gets the outcome with `how` merged in (`command.run`), or a fresh `ui.inspect` with `invoked` (`ui.invoke`).
  - The first error is answered at once, as the synchronous path returned it.
- **`control_now`** (the synchronous Rust call that tests use) still returns the finished outcome; it waits for the worker.
- **In the FFI,** `command.run engine.*` parks the request (`SwitchWait`) and replies the same way.

**One deliberate exception: engines compiled in.** When `Work::in_process()` holds (an `inproc` engine that `EngineKind::spawns` says starts no program: text, array, an application's packs; not the craft adapter), the egui shell runs the connect in place. The old engine is still retired on the worker.

- It waits for no process and no network, only this program's own code: about 3 ms in a debug build (the probe's switch back to in-process replied in 3 ms).
- It keeps a pick from the Engine menu connected in the same frame. Two kittests (`a_pick_adds_or_changes_an_engine_and_clear_starts_over`, `a_failed_switch_says_why_and_the_editor_still_works`) click "Rust · in-process (mode 1)" and assert three frames later, and they pass unedited.
- The FFI does not take this exception: there everything goes to the worker.
- If in-process engine starts ever grow slow, removing the exception is one `match` arm in `engine_view.rs` (`EditorApp::go`), and those two kittests would need to wait for `engine_busy()`.

## Each formerly blocking call

| Call | Where it was | Where it runs now, or why it stays |
|---|---|---|
| `conn.call("run")` | `crates/ui/src/lib.rs` `pump_runs` | Worker (`EngineWorker::run`). A refusal comes back as `RunFailed` and fails the run if it is still the latest |
| `conn.call("graph.release")`, `conn.call("run.cancel")` | `pump_runs` | Worker, in order after the runs |
| `conn.uploads_pending(&body)` | `pump_runs`, `pump_saves` | Stays. It is the host's own list of local files not yet allowed (`crates/host/src/http.rs:400`), with no engine call. Kept so the Send Files prompt shows in the same frame (`kittest_trust.rs`) |
| `conn.allow_uploads` | `answer_prompt` | Stays: a set insert |
| `output.save` | `pump_saves` | Already on its own thread (unchanged) |
| Engine switch: process spawn, `wait_ready` (10 to 60 s), `HttpHost::spawn`, `attach`, `schema` | `Switchboard::switch` via `pump_engine` | Worker (`Work`), for every hosting that spawns or talks over the network |
| In-process engine start and `schema` | Same | Stays in place (about 3 ms; see the exception above). The FFI uses the worker |
| `run.cancel` to the old engine, dropping the old link (pump join of 250 ms or less), the engine's shutdown (up to 2 s) | `forget_runs`, `clear` | Worker (`Retired::stop`) |
| `engine.add`: the new engine's start | `Switchboard::add` | Worker |
| `Coordinator::add`/`remove` (a removed engine stops), `Link::start` on the coordinator, its `schema`, `warnings()` | `relink` | Worker. During a relink the switchboard holds no coordinator, so `ui.inspect`'s `coordinator.warnings()` can't trigger `schema` calls on the UI thread. Afterwards they are cached |
| `link.state()` | `drain_engine`, every frame | Stays. A lock-protected cached read in every host (`process.rs:264`, `http.rs:440`, `InProc` → `Host::state`, `Coordinator::states`), never I/O |
| `with_link`'s `schema` | `crates/ui/src/lib.rs` `with_link` | Stays (item 3; see below) |
| `editor_with`: connect, `schema`, `--engine` extras via `try_run("engine.add")` | `apps/plumcraft-editor/src/lib.rs` | Stays (item 3) |
| `resolve`: thumbnails, pictures, peaks | `crates/ui/src/run_view.rs:92`, `:109`, `:130` (through `audio.rs:142`) | Stays (item 4). The bytes are resolved on the UI thread; only decoding is on a `Job` |
| `resolve`: full values | `crates/ui/src/watched.rs:46-52` (`Fulls::read`), reached from `lib.rs` `full_reader`, used by `watched.update`, `panel_pane.sync`, `sync_media`, `sync_compare`, `popouts.rs:60`, `value_editors` (`lib.rs` `value_editor_windows`), `wire_view` | Stays (item 4) |
| `session.docs.resolver` | Set in `lib.rs` `with_link` and `drain_engine`, and in `switchboard.rs` `relinked`; used by `plumcraft_session` `doc_cmds` (`doc.open`) and `open_cmds` | Stays (item 4) |
| `Session::connection()`, used by an application's commands (PlumImpress `app.askEngine` and others) | The application's | Stays: application commands are synchronous today |
| `EditorApp::switch_engine`, `add_engine`, `remove_engine` (public API) | `engine_view.rs` | Unchanged and synchronous, on the caller's thread. Nothing in a frame calls them. They are the API for scripts and tests |
| FFI: `run`, `graph.release`, `run.cancel` | `crates/ffi/src/core.rs` `send_outgoing` | Worker |
| FFI: `engine.*` | Recorded, never performed | Performed: begun on the core thread, waiting part on the worker, reply when done |
| FFI: `plum_open`'s connect and `schema` | `Core::open`, on the shell's calling thread | Stays (item 3). With no `platform` declared, only in-process engines can open |
| FFI: `plum_blob`'s `resolve` | `Shared::blob`, on the shell's calling thread | Stays (item 4). It now follows switches (`Shared.conn` is a `Mutex<Option<…>>` updated after each) |
| FFI: `run.cancel` at close | Core thread | Stays: the shell is waiting for `plum_close` anyway |

### Item 3, startup: not done, and what blocks it

- **`EditorApp::with_link` promises the catalogue is set when it returns.** It is a builder that nine kittest files and four PlumImpress test files call, then read the catalogue at once (nodes, labels, `run.start` enabled).
- **`editor_with` builds the session around the catalogue it has just read.**
  - `Session::with_graph(catalog, graph)`, or `graph.open` against it, computes the graph's problems and titles against that catalogue. So do `last_error`'s "opened with N problems" and the first fit (`catalog_changed(first)`).
  - Opening against an empty catalogue and setting the real one later would change `lastError`, and possibly the undo history and fit, for anything that reads them early.
- **`--engine a,b` adds the extras** with a synchronous `try_run("engine.add")` in eframe's creator.
- **The control server starts in the creator** (`start_control`), and probes start sending as soon as the token file and port appear. If the window opened first, a probe's first `ui.inspect` or `node.add` would see no catalogue.
- **A way through, for its own slice:**
  1. `run()` (not `editor_with`, so no test changes) builds the editor with no engine and records the startup engine as a switch, so the first frame begins it on the worker.
  2. It defers opening the graph or demo until the first catalogue arrives (a core `Startup` step).
  3. It starts the control server only then, so control clients observe what they observe today, a little later.
  4. The FFI's `plum_open` would do the same on its core thread, at the cost of reporting a failed engine start in the feed rather than from `plum_open`. That is a C ABI behaviour change.

### Item 4, `resolve`: listed, not moved

- **Which hosts block:**
  - In-process and process engines resolve from the local store (`Store::resolve`, `resolve_in`: a map or a read, plus inflating a deflated blob: about 12 ms for 64 MB, N0).
  - `HttpHost::resolve` falls back to `cas.read_handle` and then `fetch`, an HTTP `GET` (`crates/host/src/http.rs:444-456`, `:352`). That one blocks on the network.
  - The coordinator delegates to whichever engine has the value.
- **The views have a "not yet" for decoding but not for the bytes.** `Thumbs` caches a `None` from `resolve` as `ThumbMiss::Missing` ("re-run to refresh"), so a resolver that answered "not yet" would be taken for "gone".
- **Doing it cleanly means:**
  - a `Pending` state in `Thumbs`, `Fulls` and `MediaViews`;
  - resolves started on the worker (or each view's `Job`) and finished by a wake.
- That fits the media-crate slice, where decoding moves anyway.
- **In this slice's test,** `the_ui_thread_never_calls_an_engine` counts resolves on the UI thread and prints them (0 for its text graph); it doesn't fail on them.

## The thread-rule test

`crates/ui/tests/engine_thread.rs`, `the_ui_thread_never_calls_an_engine` (runs in CI):

- **Setup.**
  - Every connection is wrapped in `Watch`, which records any `call` made on the thread that owns the editor (the test's thread, which steps the frames as eframe's main thread does).
  - The engine takes 600 ms to accept a run.
  - The connector's "process" hosting is a fake that takes 800 ms to start, and it notes any start made on the owner's thread.
- **What it does:** runs; supersedes; cancels; switches through the control channel (`handle_control`, the reply deferred); `ui.invoke engine.reconnect`; `engine.clear`; and a pick from the real Engine menu (Engine › Text › "Rust · process (mode 2)") inside a frame.
- **It asserts:**
  - that no engine call was made, and no engine was started, on the owner's thread;
  - that no frame took longer than half the slow engine's delay (300 ms), or 400 ms for switches;
  - that the deferred switch reply says `connected: true` for the new hosting.
- **On the baseline it fails** at the first frame assertion: the frame waited for the run (`claude-debug/native-n1/s3-rule-before.txt`).

Also new: `worker::tests` (order, off-thread, wakes only for results, shutdown with a hung engine); `switchboard::tests::a_switch_waits_on_the_worker_and_a_reconnect_adds_its_engines_again` (begin, work and finish through a real worker, with a thread check inside the connect; a reconnect re-adds the array engine; `begin` names the target before it is connected); and `crates/ffi/tests/switch.rs` (below).

## Stalls, before and after

`crates/ui/tests/engine_thread.rs` `stalls` (ignored; `cargo test -p plumcraft-ui --test engine_thread -- --ignored --nocapture`). Debug build, this M4 Max. Raw output: `claude-debug/native-n1/s3-stalls-before.txt` and `s3-stalls-after.txt`.

(a) A run started on a 500-node, about 800-wire text graph. A frame with nothing happening takes about 140 ms in a debug build (egui drawing 500 nodes), so the column that matters is the first frame minus an idle one.

| Engine | Before: first frame (minus idle) | After: first frame (minus idle) |
|---|---|---|
| Text, in-process | 241.7 ms (+101) | 228.7 ms (+90) |
| Text, process (mode 2) | 250.4 ms (+111) | 234.2 ms (+96) |
| Slow fake, 500 ms to accept a run | 776.1 ms (+636) | 228.2 ms (+89) |

- The engine's share was about 10 ms in-process, 15 to 20 ms by process and 500 ms on the slow one. It is now nothing: the three rows are alike.
- The remaining +90 ms is the editor's own work: the session building and serializing the request (`run.start` alone takes 25 ms), and redrawing 500 newly queued nodes.

(b) Switching the text engine from in-process to process mode:

| Path | Before | After |
|---|---|---|
| Control channel (`handle_control`, then frames until the reply), real process | handle_control blocked 189.8 ms | 0.0 ms to hand over; longest frame 2.9 ms |
| Engine menu pick inside a frame, real process | longest frame 244.4 ms | 5.8 ms |
| Control channel, fake 2 s start | 2176.5 ms | 0.1 ms; longest frame 10.1 ms |
| Engine menu pick, fake 2 s start | 2241.9 ms | 10.3 ms |

## Tests

The CI command, `COMMONKIT_STORE_MIN_FREE_GB=2`, a fresh run folder each, logs beside them in `claude-debug/native-n1/`.

| Run | Ran | Passed | Failed | Skipped | Failing |
|---|---|---|---|---|---|
| Baseline b3 (`s3-base-b3`) | 1,314 | 1,313 | 1 | 9 | `plumcraft-craft::params_coverage hidden_params_match_the_baseline` |
| Final b3, run 1 (`s3-final1-b3`) | 1,320 | 1,319 | 1 | 10 | the same |
| Final b3, run 2 (`s3-final2-b3`) | 1,320 | 1,319 | 1 | 10 | the same |
| Final b3, run 3 (`s3-final3-b3`) | 1,320 | 1,319 | 1 | 10 | the same |
| Final md5 (`s3-final-md5`) | 1,320 | 1,319 | 1 | 10 | the same |

- **No flake** in any of the four final runs; no `LEAK` in nextest's output; no `shm_foreign_digest` or `shm_bad_key` mark (only `*.started`).
- **The test names differ by exactly the six new tests** (same in all three b3 runs; none gone):
  - `plumcraft-editor-core worker::tests::{jobs_run_in_order_off_the_callers_thread_and_wake_it_with_what_came_back, dropping_it_stops_its_thread_and_never_waits_long_for_a_hung_engine}`
  - `plumcraft-editor-core switchboard::tests::a_switch_waits_on_the_worker_and_a_reconnect_adds_its_engines_again`
  - `plumcraft-ffi::switch {engine_commands_switch_add_remove_and_clear, starting_a_process_engine_never_holds_the_core_thread}`
  - `plumcraft-ui::engine_thread the_ui_thread_never_calls_an_engine`
  - The tenth skip is the ignored `stalls` measurement.
- **The md5 baseline** wasn't rerun this slice. Slice 2's final md5 (`s2-last-md5`, the code now committed as `e1012e7`) gave the same 1,314 / 1,313 / 1 / 9.
- **`cargo test` (threads, not nextest)** of `plumcraft-editor`, `-editor-core` and `-ffi` passes. That matters for the per-thread manifests.
- **Gates:**
  - `cargo fmt --all --check` and `cargo clippy --all-targets --all-features -- -D warnings`: clean.
  - `scripts/check-hash-gates.sh`: ok. Its Rust pattern, run by hand over the untracked `worker.rs`, finds nothing.
  - `scripts/check-core-toolkit-free.sh`: ok.
- **Python, as CI runs it:** 224 passed (b3); `ruff check`: clean. Untouched.

## Existing test files touched, and why

1. **`apps/plumcraft-editor/src/lib.rs`, `attach_connects_to_a_running_http_engine`:**
   - Two `h.run_steps(3)` after clicking Attach became `h.run_steps(1); until(&mut h, "the attach", |a| !a.engine_busy());`.
   - Why it was forced: attaching is an HTTP call, now made on the worker, so it can't be done within three frames by design.
   - What a user sees: the Attach button reads "Connecting…" and is disabled for that moment, instead of a frozen window. Nothing else is reordered: the window stays open on a refusal and closes on success, as before.
   - The assertions are unchanged; only the harness's notion of "settled" is. The one step before `until` lets the click land.
2. **`crates/editor-core/src/switchboard.rs`'s test helper `Doc`** (slice 2's own test):
   - It gained a `switching` field, lent in `board()`.
   - Why it was forced: `Switchboard` has a new field and the helper builds it as a struct literal.
   - The test's body is unchanged.

No file in `crates/*/tests` or `apps/*/tests` was edited; the three test files there are new.

## Driving the real editor

`claude-debug/native-n1/s3_probe.py` (output `s3-probe.txt`, picture `s3-probe/s3-editor.png`) starts `target/debug/plumcraft-editor --control 0`, waits on the log and token file with a hard limit, and stops the editor by its captured PID. What it saw:

- **Start:** "engine: Text 0.7.0 · Rust · mode 1 · in-process · connected · 46 nodes", demo of 7 nodes. `graph.new`, two nodes and a wire, a run: both `done`.
- **`engine.switch` to process:**
  - The reply came after 465 ms, once switched (`connected: true`, the mode 2 label).
  - Meanwhile a second connection's `ui.inspect`, every 20 ms, was answered 9 times, slowest 18 ms. So the window went on answering and drawing.
  - Five of those saw `engine.pending: true` and the title "Untitled • — PlumCraft (Text · Rust · process (mode 2), connecting)".
  - A run then: both `done`.
- **Back to in-process:** replied in 3 ms (connected in place), run: both `done`.
- **`ui.invoke graph.tidy`, `ui.set {fit}`, `ui.screenshot`:** a window screenshot of the two nodes, results shown ("A CORGI NAPS ON A FLORAL CARPET."), status bar "Text 0.7.0 · connected", "last run 107 ms".
- **One fix came from this:** `ui.inspect`'s `engine.pending` was false while a switch was in flight (it read only the session's request). It is now `engine_busy()`, and the title and status say "connecting". Both are the core's `*_connecting` label functions; nothing changes when idle.

## Downstream apps

- **`/Users/seant/workspace/plumimpress`:**
  - Copied without `target`, `.git`, `dist` or `claude-debug` to `claude-debug/native-n1/s3-plumimpress-check/`, with slice 2's `.cargo/config.toml` patching every `plumcraft-*` crate to this tree, and the workspace's own `target/`.
  - `cargo check --tests` exits 0 with no source change (`check.log` there).
  - The repository itself was not touched.
- **What a downstream app would notice:**
  - `plumcraft_ui::engine_view::Connector` is now `+ Send`: it is called on the engine worker. PlumImpress's connectors (capturing `Arc`s and paths) compile as they are. One capturing an `Rc` would not.
  - `EditorApp::connector` is `None` while a switch is in flight (the connector travels with the job and comes back).
  - New on `EditorApp`: `engine_busy()` and `wait_engines(timeout)`. `pump_engine()` keeps its signature but no longer waits.
  - `try_run`, `run` and `control_now` called outside a frame behave as before (they wait). Inside a frame (menus, shortcuts, `ui.*`), a switch that waits for I/O returns the command's own result and finishes later.
  - `ui.inspect` `engine.pending` is true while a switch is in flight, and the engine label says "connecting" then.
  - `plumcraft_editor_core::switchboard::Switchboard` has a new public field, `switching`. New: `begin`, `finish`, `Next`, `Work`, `Worked`, `Retired`, `Switching`, `engine_title_connecting`, `engine_label_connecting`, `engine_short_label_connecting`. `pump`, `switch`, `add` and `remove` keep their signatures and still run in place.
  - `forget_runs` no longer calls the old engine or drops its link; they go to `Switching::take_retired`.
  - `engines::Manifests` is new.

## For the slices that follow

- **Dialogs and prompts as asks under `EditorApp::invoke`:**
  - `invoke` and `answer_prompt` reach `try_run`. Inside a frame, an engine switch they cause now completes later.
  - An ask answered later should run its command in frame mode and, if `engine_asked` is set, park its reply as `wait_for_switch` does.
  - `Prompt::Uploads` relies on `uploads_pending` staying synchronous.
- **The `ui.*` control methods into the core:**
  - There are now two implementations of "reply once switched": `EngineWait` in `crates/ui/src/engine_view.rs` and `SwitchWait` in `crates/ffi/src/core.rs`. They should become one in the core's dispatcher.
  - So should `in_frame` and `engine_asked`, which are the egui shell's way of saying "this caller may not wait". A native shell's requests are always that.
  - `EditorApp::step_engines` (egui) and `Worker::step_engines` (FFI) are the same loop over `Switchboard::begin`/`finish` and `Done`, and belong in the core's `Document`.
- **The media crate:**
  - Item 4's `resolve`s go with decoding: a `Pending` state in `Thumbs`, `Fulls` and the media views, resolves on a worker, and a wake.
  - The FFI's `plum_blob` should hand out only what is already resolved (shell-interface 10.1, status 1 "pending").
- **The menu model:**
  - While `engine_busy()`, the Engine menu could show the target as connecting and disable its picks. Today a pick made during a switch is recorded and begun when it finishes.
  - The Attach window's "Connecting…" is the only new piece of UI.
- **Startup (item 3)** needs its own step, described above.
- **Quitting:** dropping `EditorApp` still drops its link on the UI thread. That is up to 250 ms for the pump, plus a process engine's 2 s shutdown grace, at quit. It could go to the worker too.
- **Two `Waker`s in the FFI** remain (slice 1's note). The worker reuses the core's `link::Waker`, which sends `Msg::Drain`.
- **In the FFI, `session.engines.choices` is `Launcher::choices()`,** already filtered by the platform's capabilities. So a refused hosting is refused by the session as "not one of the choices" (listing them), not with the launcher's reason ("this device can't start programs"). If shells should show the reason, offer `engines::offers(caps)` through the menu model.

## Lines of code

From `git diff --numstat`, plus the untracked files.

| Area | Added | Removed | Notes |
|---|---|---|---|
| `crates/editor-core/src/worker.rs` | 347 | 0 | New. 89 of them tests |
| `crates/editor-core/src/switchboard.rs` | 454 | 96 | The three-part split, `Retired`, `Switching`, the connecting labels. About 70 of them a new test |
| `crates/editor-core/src/engines.rs`, `lib.rs` | 20 | 0 | `Manifests` |
| `crates/ui/src/engine_view.rs` | 247 | 16 | The frame-mode driver, waits, attach |
| `crates/ui/src/lib.rs`, `control.rs` | 80 | 18 | Fields, `try_run`, `show`, `pump_runs`, `handle_control` |
| `crates/ffi/src/core.rs`, `Cargo.toml`, `Cargo.lock` | 177 | 26 | The worker and switching in the core thread |
| `apps/plumcraft-editor/src/lib.rs` | 5 | 2 | The Attach test (above) |
| `crates/ui/tests/engine_thread.rs` | 357 | 0 | New: the rule test and the stall measurement |
| `crates/ffi/tests/switch.rs` | 102 | 0 | New |
| **Total** | **1,789** | **158** | Net +1,631, of which about 620 are tests |

## What the docs should say

- **`docs/STATUS.md`:** N1 slice 3 done.
  - The engine worker, runs, cancels and releases off the UI thread. Switches go to the worker with a "connecting" state; in-process engines still connect in place, in about 3 ms.
  - The FFI performs `engine.*` and never waits for an engine on its core thread.
  - Test counts 1,314 → 1,320, with the same single local failure.
  - Startup and `resolve` are still on the UI thread.
- **`docs/research/24-native-shells.md`:**
  - **Section 16:** a slice 3 bullet with the above: the worker's design, the three-part switch, the in-process exception, the stall numbers, items 3 and 4 left with their blockers.
  - **Section 4.2:** "Engine calls go to workers" now exists as one FIFO worker per document. Its inbox is drained on the owner's thread after a `Waker` wake.
  - **Section 4.3:** the engine manifests are per thread, so a document's worker gets a snapshot from its owner.
  - **Section 10's "Launch to a usable window" row:** not met yet (item 3).
- **`docs/shell-interface.md`:**
  - **Section 5.4:** today's line ("`pump_runs` calls `conn.call("run")` on the UI thread … which N1 moves off it") is done. Engine calls go to `plumcraft_editor_core::worker::EngineWorker`, in order per document.
  - **Section 5.1:** `call` may still be used for engine methods by in-process Rust callers outside a frame; it waits on the worker, as tests and PlumImpress do. The UI thread uses the request-and-reply path.
  - **Section 5.2:** `engine.*` over the C ABI replies once the switch is done.
  - **Section 3.1 rule 6:** note the one exception kept in the egui shell: engines compiled in connect on the UI thread. Either accept it by name, with its budget, or schedule its removal.
  - **Section 10.1:** `plum_blob` still resolves on the caller's thread.
- **`docs/control-protocol.md`:** no edit is required, since replies are unchanged. One sentence would help: `ui.inspect`'s `engine.pending` is true from the request until the switch is done, the engine `label` says "connecting" meanwhile, and `engine.*` replies come after it.
