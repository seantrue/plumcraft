# N1 slice 7: a core `Document`, and a window before the engine

Branch `native/n1-document`, working tree only (nothing committed, staged or stashed). 2026-11-19.

## Summary

- **Item 1, done.** `plumcraft_editor_core::document::Document` owns an open document: session, engine link, coordinator, engine state, engine worker, switch in flight, asks, launches, media service and its `Follow`, last error, convert offer, the save in flight, the run held for the uploads prompt, the rerun flag, the starter that starts engines. The per-turn work is the core's (`document::turn`, `pump`, `after`, `execute`, `Document::drain_engine`, `settle_startup`). The egui `EditorApp` holds one (`app.doc`, reached through `Deref`, so `app.session`, `app.link`, `app.last_error` and the rest compile unchanged). The C ABI's core holds several. Neither keeps a copy of the logic.
- **Closing never waits.** `Document::close` (and dropping one) withdraws the asks, answers parked requests, cancels the run, and hands the link and coordinator to the worker. It then lets the worker go without a join (`EngineWorker::close`). `document::wait_closed` waits, bounded, once the window has gone.
- **Item 3, done.** The editor's `run()` builds the window with no engine and records the startup engine as a switch. The graph opens once the catalogue arrives (`Opening`), `--engine` extras are added after it, and the control server starts then (`EditorApp::on_ready`).
  - With a fake engine that takes 2 s to start, the first frame came at 264 ms instead of 2.3 to 2.5 s, and the title said "connecting" at 316 ms.
  - `plum_open` returns at once (0.2 to 4 ms). Requests sent meanwhile are held and answered in order, and the feed's new `changed` item says when the engine is there.
- **Item 2, done.** Each document has an id. Raised asks in the C ABI feed carry `doc`, and so do forwarded events. Hooks get the document id additively (decision 3). The C ABI core holds documents by id (`document.open`, `document.list`, `document.close`, a request's `doc`), and documents share one in-process engine per kind (decision 11B). Requests with no `doc` go to the first open document.
- **Item 4, not done.** The in-process exception stays: `a_pick_adds_or_changes_an_engine_and_clear_starts_over` fails 3 runs out of 3 without it (details below). It is now one flag, `Document::start_in_place`.
- **The gate:**
  - **Tests:** the same single expected failure before and after, in three b3 runs and one md5 run. There are 1,378 tests against 1,368: 10 new, none gone. One existing test file gained one test (sanctioned); none was otherwise edited.
  - **Control replies:** 41 control replies from the real editor, before and after, differ in nothing. The first capture showed a window-focus artefact, explained below.
  - **The research software** builds and passes its editor, recipe, view, identity and stored tests with no source change.
  - **Your preferences:** your `prefs.json` and `trusted.json` are untouched.

## `Document`'s design

| Aspect | What it is |
|---|---|
| Owns | `id`, `session`, `link`, `coordinator`, `engine` (state), `last_error`, `convert_offer`, `document` (the untitled name), `asks`, `launches`, `media_service`, `media_follow`, `engine_worker`, `switching`, `parked` (for a shell with no control state of its own), `reloader`, `installer`, `installing`, `starter`, `start_in_place`, `rerun_pending`, `upload_held`, the save in flight, the waker, `close_asked`, `folders` (in-memory folder memory for a shell without prefs), the startup state, its own run ids, an asks sink |
| Shell-facing traits | `Document` implements `invoke::Host`, `control::Shell`, `control::Driver` and the new `document::Holder`, so it is a complete headless shell: the C ABI uses it as it is. `EditorApp` implements `Holder` too (its look via `shell_command`, fit and selection and Watched via `after_command`, `may_wait` = outside a frame, `note_engine_asked`) and delegates `Driver` to the document, applying `Effects` to its canvas after each operation |
| The turn | `document::turn(h)`: `control::step` (worker results, switches begun), `settle_startup`, `drain_engine` (events applied; media misses forgotten on a reply; read-ahead), `pump` (auto-run due, review prompt, outbox to the worker or held for the uploads prompt, saves on their own thread, `media.frame()`), `after` (doc.open, doc edits, value.open, rerun), asks to the sink |
| What it returns | `Turn {events, outcomes, effects, reread, ready, asked, again_in}`: the events applied (the C ABI forwards them), engine operations finished, what the shell must redo for what it keeps beside the core, whether thumbnails missed before may be there again, whether startup finished, whether a prompt was raised, and when to look again though nothing wakes |
| The egui frame | It calls the same steps at the same points of its frame as before (`pump_engine` then `settle_startup` at frame start, `drain_engine`, `pump_runs` twice: core `pump`, its own view syncs, core `after`), so nothing moves within a frame |
| Commands | `document::execute(h, id, params)` is what `EditorApp::try_run` was: the session's command, then the follow-ups every shell needs and the engine switch (stepped, or waited for by a caller that may wait); `graph.open`/`graph.new` are held while the first engine connects |
| Starting engines | `Starter = Box<dyn FnMut(&str, &str, Option<Waker>) -> Result<Link, String> + Send>`. The egui `Connector` becomes one in `with_engines` (its context comes from a cell filled by the first frame); the C ABI's is the launcher behind `shared_starter`. It travels with a switch on the worker and comes back |
| Closing | `close()`: asks withdrawn (and fed), parked requests answered "the document closed", startup dropped, run cancelled, coordinator and link retired with their run cancels, session's connection and resolver cleared, media unfollowed, worker let go (`EngineWorker::close`: no join; its queued jobs still run). Idempotent; `Drop` calls it |

### Logic that moved

| Piece | From | To |
|---|---|---|
| `try_run` and its follow-ups (document name, problems, missing engines, `openedFrom`, `run.watch` runs, bundle/detach/expand notes, recipe copy, convert offer, engine switch step or wait) | `crates/ui/src/lib.rs` `EditorApp::try_run`, `offer_for`, `expand_note` (about 150 lines) | `document::execute`, `Document::offer_for`, `document::expand_note`; egui keeps `window_command` and its canvas/Watched reactions behind `Holder` |
| `pump_runs`: auto-run, review prompt, outbox (uploads prompt, runs, releases, cancels to the worker), held-outbox repaint, `media.frame()`, doc.open/doc edits/value.open polls, rerun, running repaint | `crates/ui/src/lib.rs` `pump_runs` (about 110 lines) | `document::pump`, `document::after`; egui keeps the Watched, audio, media, pane, compare, pop-out and cache syncs between them |
| `pump_saves` | `crates/ui/src/lib.rs` (43 lines) | `document::pump` (`pump_saves`) |
| `drain_engine` | `crates/ui/src/lib.rs` (about 30 lines) | `Document::drain_engine`; egui only forgets its thumbnail misses |
| `with_link`'s catalogue, state, follow, resolver | `crates/ui/src/lib.rs` | `Document::link_now` (still synchronous for tests and downstream) |
| `with_media`, `share_connection`, `sync_engine`, `enabled`, `start_run_if_enabled` | `crates/ui/src/lib.rs` | `Document::set_media`, the switchboard helpers, `Document::enabled`, `document::start_run_if_enabled` |
| The switchboard's lending (`board!` macro), the in-place exception, carrying the connector | `crates/ui/src/engine_view.rs` `board!`, `Driver::start`, `take_back` | `Document::board`, `Document::start` (`start_in_place`), `Document::take_back` |
| `send_outgoing` | `crates/ffi/src/core.rs` | `document::pump` |
| `events` (apply, then send) | `crates/ffi/src/core.rs` | `Document::drain_engine` plus the C ABI's `forward` (filter by own runs, tag `doc`, collect refs) |
| `Host`, `Shell`, `Driver` for the core thread's `Worker`; `board`, `step_engines` | `crates/ffi/src/core.rs` | `Document`'s own impls |
| `plum_open`'s synchronous connect, catalogue, session and demo | `crates/ffi/src/core.rs` `Core::open` | `Document::start_engines` with `Opening::Edit("Open Demo", …)`; the core thread opens the first document |
| Startup: connect, catalogue, open the graph or file, extras, control server | `apps/plumcraft-editor` `editor_with` and the creator | `editor_starting` (`EditorApp::start_engines`, `on_ready`); `editor_with` stays, test-only, synchronous |

## Document ids, hooks, and "one shared in-process engine"

- **Ids.** `document::next_id()` is a process-wide counter that never reuses a number. `Session::doc_id()` carries it.
- **Hooks (decision 3), additive.** `before`/`after` already receive the session, so they read `session.doc_id()`. The hooks without a session (`node_overlays`, `wire_marks`, `status`, `status_tone`, `hold_runs`) can ask `plumcraft_session::commands::current_doc()`. That is a thread-local the core sets around a document's commands and turn (`with_doc`) and the egui shell sets around its frame.
- **Why not a new argument.** Changing the `fn()` signatures would break `crates/ui/tests/kittest_app_hooks.rs` and `crates/session/tests/app_hooks.rs` (which the brief forbids editing) and the research software's `impress-ui::hooks()`. Decision 3 said the research software would change with it; the additive route needs no change, and a signature change can follow when you say so.
- **Asks (`doc`).** Each raised ask in the C ABI feed is `{ask, doc, kind, modal, by, params}`. `asksEnded` items stay `{ask, ended}`: `crates/ffi/tests/asks.rs` compares them for exact equality. `ui.inspect`'s `asks` gains nothing, so control replies stay byte-identical.
- **Ask ids** are per document. `ask.answer` addresses the request's `doc`, else the first document.
- **The C ABI core** holds `BTreeMap<DocId, Doc>`, each `{Document, ready, held requests, engine generation}`.
  - `document.open {demo?, path?}` replies at once with `{doc, title}`.
  - `document.list` lists `{doc, title, path, dirty, ready, first, engine {connected, pending, label}}`.
  - `document.close {doc, discard?}` replies `{closing: true}`, or `{closing: false, ask}` with the unsaved-changes prompt asked first.
  - A top-level `doc` addresses a request. Without it, a request (and every control-port request) goes to the first document still open.
  - An unknown or closed id is refused with `code: "unknown_doc"`; a malformed one with `code: "bad_params"`.
  - Requests to a document whose engine hasn't connected are held, up to 256; past that they are refused with "busy". They are answered in order once the engine is there or failed to start.
  - `changed` in `plum_poll`: `{doc, domain: "engine", gen, connected, label, ready}` when an engine operation finishes, and `{doc, domain: "document", closing: true}` when a document closes.
- **What "shared" means, as built (decision 11B).** The in-process text and array engines are each a `plumcraft_engine::host::Host` with one engine worker thread, one event ring read with `events.poll {since}`, one store, a memo and cache, runs keyed by `run_id`, graph scopes released by name, and one session id `s-<pid>`. As built, `SharedEngines` keeps, per engine id, a weak handle to the one connection:
  - **One engine, a link each.** The first document's starter starts the engine; every later document gets a `Link` (its own pump) over the same `Arc<dyn Connection>`. The engine stops when no document links to it.
  - **What the documents share:** the engine instance, its worker (their runs queue on it in turn, so a long run in one delays the other), its store and cache (content-addressed, so this is a gain), and the session id.
  - **What each document keeps:** its link, engine worker, run ids, graph scopes, cancels and releases.
  - **Run ids are made strictly increasing per process** (`plumcraft_session::run::new_run_id`, same format). Two documents can never give the engine the same id: the engine refuses a reused `run_id`.
  - **Events.** Each document's pump sees every run's events. `RunState::apply_event` already ignores runs that are not current, and the C ABI forwards only a document's own runs' events (`Document::owns_run`), each tagged `doc`.
  - **What is not shared.** Process and HTTP engines start per document, as before. An `engine.switch` in one document retires only its own link.
  - **Tested:** `document::tests::two_documents_share_one_engine_compiled_in_and_run_their_own_graphs` and `crates/ffi/tests/documents.rs`.

## Startup

### The sequence before and after

- **Before.** The creator connected the engine (process start included) and read `schema` on the UI thread, then built the session around the catalogue and opened the graph or file. It then added the `--engine` extras with a synchronous `engine.add` and started the control server. Only then did eframe draw the first frame. `plum_open` connected, read `schema` and loaded the demo on the caller's thread.
- **After, egui.**
  1. The creator builds the editor on an empty catalogue, with its document name and the Engine menu.
  2. `start_engines` records a switch to the startup engine, the extras as adds, and an `Opening` (the demo or read workflow as `Graph`, a file as `Path`).
  3. `on_ready` is set to start the control server.
  4. `wake_with(ctx)` and one `pump_engine()` begin the switch:
     - **An engine compiled in** connects in place there (slice 3's exception, a few ms). Startup settles and the control server starts before the first frame, as before.
     - **A process or HTTP engine** starts on the worker while the window opens. The title and status bar say "connecting". When the switch and the adds are done, `settle_startup` opens the graph (`Session::start_graph`, which has `with_graph`'s semantics: no undo step, not dirty) or runs `graph.open`, re-runs `engine.auto` if on, runs held commands, then `on_ready`.
  5. After the window closes, `run()` waits up to 3 s for the closed document's worker (the window has gone by then).
- **After, C ABI.** `plum_open` checks the config (still synchronous; a bad config still returns 0), starts the core thread, and returns. The core thread opens document 1, whose first turn begins the switch on its worker.
  - **A changed behaviour:** an engine that fails to start no longer makes `plum_open` return 0. The document opens with no engine, and its `lastError` and `changed` say why.

### Measurements

Debug build, this Mac, `claude-debug/native-n1/s7_startup.py` (`PLUMCRAFT_TRACE_STARTUP=1`: the editor prints `first frame` and title changes). "Ready" is the `control channel on` line; "first reply" is the first `ui.inspect` answered. Each cell is a range over warm runs (the first run of a set is cold: about 1.8 to 2 s, both builds).

| Engine | Before: first frame | Before: ready | Before: first reply | After: first frame | After: ready | After: first reply |
|---|---|---|---|---|---|---|
| Text, in-process | 263–268 ms | 209–214 ms | 266–271 ms | 263–274 ms | 208–217 ms | 265–277 ms |
| Text, process | 276–286 ms | 221–231 ms | 279–288 ms | 253–256 ms | 253–256 ms | 357–359 ms |
| Fake, 2 s to start (process) | 2,305–2,512 ms | 2,251–2,457 ms | 2,308–2,514 ms | 264 ms (cold 1,189) | 2,244 ms (cold 3,248) | 2,312 ms (cold 3,311) |

- **In-process:** unchanged, by design.
- **Process:** the window comes 25 ms sooner. Control is ready 30 ms later and the first reply about 75 ms later. The server now starts during the first frame, and the first frame itself takes about 100 ms in a debug build, so a request waits for the second. That is the price of not waiting.
- **The 2 s fake:** the title read "Demo — PlumCraft (Text · Rust · process (mode 2), connecting)" at 316 ms (cold 1,241 ms) and the plain title once connected.
- **Responsiveness while connecting** (`crates/ui/tests/engine_thread.rs` `startup_never_calls_an_engine_on_the_editors_thread`, a 1.5 s fake start): first frame after 29 ms, ready at 1.59 s, longest frame 52 ms. No engine call and no engine start happened on the editor's thread from launch to ready.
- **C ABI** (`crates/ffi/tests/documents.rs`, `plum_open_returns_before_the_engine_and_holds_what_comes_meanwhile`):
  - in-process: `plum_open` 3.7 ms, three `node.add`s sent at once answered by 116 ms;
  - process: `plum_open` 0.23 ms, answered by 127 ms;
  - `document.list` sent at once answers immediately with `ready: false` and the title "…, connecting)".
  - Not measured before: it was the engine's start and `schema`.
- **Quitting** (`claude-debug/native-n1/s7_quit.py`, `ui.invoke app.quit` to the window gone and to the process gone):

| Engine | Before: to exit | After: window closed | After: to exit |
|---|---|---|---|
| Text, process | 422–430 ms | 77–85 ms | 172–221 ms |
| Text, in-process | 365–420 ms | 57–94 ms | 98–374 ms |

After, "window closed" includes eframe dropping the editor: the document's drop no longer waits for the pump join or the engine's grace. The rest is `wait_closed` after the window has gone.

### What a control client sees before ready

Nothing. The control server starts in `on_ready`, so the port and token appear only once the engine has connected and the graph is open, exactly as a client saw them when startup waited (for an engine compiled in, still before the first frame). Probes that start the editor and send at once keep working:

- `s5_probe.py`: 41 replies, 0 differences;
- `s3_probe.py` (`s7-s3probe-after/s3-probe.txt`): everything as in slice 3, including a switch to process answered after 241 ms while a second connection's `ui.inspect` was answered 11 times, 7 of them pending, slowest 17 ms.

Over the C ABI, a request is held, not refused. **`docs/control-protocol.md` needs no change:** nothing a control client observes changed.

## Item 4: the in-process exception

- **Tried:** `start_in_place = false` in `EditorApp::new` (so in-process connects go through the worker too), three runs of the two kittests.
- **`apps/plumcraft-editor/src/lib.rs` `a_pick_adds_or_changes_an_engine_and_clear_starts_over` failed 3 of 3** at what is now line 1031 (1025 when tried), `assert!(h.state().session.connected, …)`. That assertion comes three frames (`h.run_steps(3)`) after clicking Engine › Text › "Rust · in-process (mode 1)" following an `engine.clear`.
- **`a_failed_switch_says_why_and_the_editor_still_works` passed 3 of 3,** but its assertion at line 1230 has the same shape (connected three frames after the same click) and passes only on timing.
- **Why it can't work unedited.** The harness's frames don't wait for the worker: `step` only drains what has already come back.
- **Left as is.** The exception is now one field, `Document::start_in_place`: `true` in the egui shell, `false` in the C ABI.
- **To remove it:** those two tests would need `until(…, |a| !a.engine_busy())` after the click, as slice 3 did for Attach. That is a test edit, so it is for you to decide.

## Control-reply diff

- **Method.** `s5_probe.py` with `s5_diff.py`'s masking, into `claude-debug/native-n1/s7-inspect-{before,after}/`. The before binary was built from this branch's HEAD with only the env-gated first-frame trace added; it is kept, hard-linked, in `s7-bin-before/`.
- **First after capture:** 7 differences, all `popouts[0].on_top: true != false`. That value follows whether the editor window had focus (B19's float rule): an environmental flip, not a reply change.
- **Repeated:** before against a second before run (`s7-inspect-before2`): 0. Before against `s7-inspect-after2` and `s7-inspect-after3` (the final binary): **0 differences in 41 replies**. Before2 against after3: 0.

## Tests

The CI command, `COMMONKIT_STORE_MIN_FREE_GB=2`, a fresh run folder each (`claude-debug/native-n1/s7_suite.sh`), logs beside them.

| Run | Ran | Passed | Failed | Skipped | Failing |
|---|---|---|---|---|---|
| Baseline b3 (`s7-base-b3`) | 1,368 | 1,367 | 1 | 12 | `plumcraft-craft::params_coverage hidden_params_match_the_baseline` |
| Final b3, run 1 (`s7-final1-b3`) | 1,378 | 1,377 | 1 | 12 | the same |
| Final b3, run 2 (`s7-final2-b3`) | 1,378 | 1,377 | 1 | 12 | the same |
| Final b3, run 3 (`s7-final3-b3`) | 1,378 | 1,377 | 1 | 12 | the same |
| Final md5 (`s7-final-md5`) | 1,378 | 1,377 | 1 | 12 | the same |

- **The names differ by exactly the ten new tests,** the same in all four runs; none gone:
  - `plumcraft-editor-core document::tests`: `documents_have_their_own_ids_and_the_session_knows_it`, `startup_waits_for_the_catalogue_then_opens_and_runs_what_was_held`, `a_failed_first_engine_still_opens_the_graph_and_says_why`, `closing_cancels_the_run_and_never_waits_for_the_engine`, `two_documents_share_one_engine_compiled_in_and_run_their_own_graphs`;
  - `plumcraft-ffi::documents`: `two_documents_edit_undo_and_run_apart_on_one_engine`, `an_ask_in_each_document_is_answered_apart_and_closing_one_leaves_the_other_working`, `unknown_and_closed_documents_and_bad_requests_are_refused`, `plum_open_returns_before_the_engine_and_holds_what_comes_meanwhile`;
  - `plumcraft-ui::engine_thread`: `startup_never_calls_an_engine_on_the_editors_thread`.
- **No flake in the four final runs,** and no `LEAK`.
- **A pre-existing flake risk, seen once mid-work.** In a partial run during development (`s7-try1`), `plumcraft-ui::engine_thread values_never_wait_for_an_engine` failed once: its "watch n5" frame took 195 ms against a 150 ms limit, under the full parallel load.
  - That phase is the audio view's in-place spectrogram. Slice 6's report gives it as 35 to 97 ms, as a known remaining stall.
  - Alone it measures 77 to 80 ms now, and it passed in all four final runs and in the baseline.
  - I can't measure the baseline under the same load (its test binary was rebuilt over), so I report it rather than claim it.
  - Slice 6's plan, a `Want::Spectrogram` on the media service, removes it.
- **`cargo test` (threads, not nextest)** of `plumcraft-editor`, `-editor-core` and `-ffi`: 159 passed, 0 failed.
- **Python, as CI runs it (b3):** 224 passed; `ruff check`: clean.
- **Gates:**
  - `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D warnings`: clean;
  - `scripts/check-hash-gates.sh`: ok;
  - `scripts/check-core-toolkit-free.sh`: ok.

## Existing test files touched

1. **`crates/ui/tests/engine_thread.rs`:** one test appended, `startup_never_calls_an_engine_on_the_editors_thread`, the startup case the brief asked for. No existing line changed (`git diff`: 47 additions, 0 deletions).

No other existing test file was edited. `crates/ffi/tests/documents.rs` is new, and so are the unit tests in `crates/editor-core/src/document.rs`. Every existing kittest, FFI test and the research software test compiles unchanged thanks to the `Deref`.

## Preferences

`~/Library/Application Support/plumcraft-editor/` before the first run and after the last, SHA-256:

| File | Before | After |
|---|---|---|
| `prefs.json` | `9221fa662e648228030c4af3ca118330e0e6a4a950c1db4206dd66a688aca727` | `9221fa662e648228030c4af3ca118330e0e6a4a950c1db4206dd66a688aca727` |
| `trusted.json` | `a10d6fc822e72d77cbf715c89bde4fa809897db6cde6ccdf7be80341464f4704` | `a10d6fc822e72d77cbf715c89bde4fa809897db6cde6ccdf7be80341464f4704` |

How each real-editor run was isolated:

- `s7_startup.py` and `s7_quit.py` set `PLUMCRAFT_EDITOR_PREFS` under `claude-debug/native-n1/s7-probe/`.
- `s5_probe.py` sets it under its own output folder in `claude-debug/native-n1/`.
- `s3_probe.py` sets none, so I ran it with `PLUMCRAFT_EDITOR_PREFS` exported to `s7-probe/`.

## Downstream: the research software

- **How it was checked.** I copied it to `claude-debug/native-n1/s7-<app>-check/` (without `target`, `.git`, `dist` or `claude-debug`), with the patch file of earlier slices plus `plumcraft-media`, built into the workspace `target/`. The repository itself was not touched.
- `cargo check --tests` exits 0 with **no source change**.
- **Tests:** `editors` 7, `open_recipe` 2, `views` 5, `recipe` 3, `identity` 2 and `stored` 5 pass (`tests-1.log`, `tests-2.log`).
- **How its fields stay reachable: decided as fields, through `Deref`.** `EditorApp` derefs to its `Document`, so `.session`, `.link`, `.coordinator`, `.engine`, `.installing`, `.last_error`, `.asks`, `.document`, `.convert_offer`, `.media_service` and `.switching` read and write as before. Field access auto-derefs, and every existing test and the research software compiled unchanged.
- **The one thing this costs.** Code that borrows a moved field and an `EditorApp` field mutably in one expression no longer compiles. Inside `crates/ui` I wrote `self.doc.session` for that reason; no test and nothing in the research software does it.
- **API a downstream crate would notice:**
  - `EditorApp.connector` is gone: `with_engines` turns the `Connector` into the document's `starter`.
  - New on `EditorApp`: `doc`, `start_engines`, `wake_with`, `on_ready` (`OnReady`), and `trace_startup()`.
  - `engine_view::START_WAIT` is `document::START_WAIT` (same value).
  - On the session side: `Session::{doc_id, set_doc_id, start_graph}`, `Engines::{ask_switch, ask_change}`, `commands::{current_doc, with_doc}`.
  - `Switchboard::retire_coordinator` is public; `Parked::close` is new.
  - `EngineWorker::close` and `worker::wait_closed` are new; `EngineWorker`'s drop no longer sets `stop` when it holds no thread.
  - `with_link` and `editor_with` keep their synchronous promise.

## What remains in `crates/ui` that is logic

Rough line counts. `crates/ui/src/lib.rs` is now 1,853 lines (was 2,164).

| Item | Where | Lines | For |
|---|---|---|---|
| The menu model: `MENUS`, `menu_bar`, `checked`, `command_menu`/`command_items`/`menu_item`, `parse_shortcut`, `shortcuts`, the Engine menu's choices | `lib.rs`, `engine_view.rs` | about 80 + 7 + 40 + 15 + 36 + 57 | N1's menu-model slice |
| A log: today about 40 `eprintln!` calls (now plus the env-gated startup trace), no ring, no `log.tail` | everywhere | n/a | N1's log slice (contract section 15) |
| The Watched panel's syncs between `pump` and `after` (watched update, audio sync, `sync_media`, pane, compare, pop-outs, `cache_displays`), setting `rerun_pending` | `lib.rs` `pump_runs`, `sync_media`, `sync_compare`, `popouts.rs` | about 28 + 54 + 15, plus the views' own | N2 (viewer state as commands, 13.2) |
| The in-place spectrogram for short clips | `audio.rs` `AudioView::sync` | about 60 | the media service (`Want::Spectrogram`) |
| `window_command` (appearance commands), `pump_theme`, `use_prefs`, `retheme` | `settings_view.rs`, `lib.rs` | 12 + 10 + 19 + 10 | appearance into the core |
| The run animation's pump and plan | `lib.rs` `pump_run_anim`, `run_anim_plan` | 23 + 18 | core (its model is already there) |
| Close handling over unsaved changes, the prompt mirror | `lib.rs` `handle_close`, `asks_view.rs` `sync_asks` | 19 + 15 | with the menu model |
| Rename as a `text` ask | `rename_view.rs` | about 40 of 177 | the asks follow-up |
| Selection, focus, hit testing, align and distribute, copy, wire routing, groups | `canvas.rs` (2,180), `edit_view.rs` (727), `wire_route.rs` (240), `groups_view.rs` (1,079) | several hundred | N2 |
| The status bar's assembly (cells, what is shown when) | `lib.rs` `status_bar` | about 130, mostly drawing | N2 (the `status` domain) |

Duplication left between the shells:

- The C ABI's `spike.waitRun`, kept for the N0 measurements beside `ui.waitRun`.
- The egui shell's `ControlState.parked` beside `Document.parked`: the egui shell keeps its screenshots there.

## Lines of code

From `git diff --numstat`, plus the two new files.

| Area | Added | Removed | Notes |
|---|---|---|---|
| `crates/editor-core/src/document.rs` | 1,302 | 0 | New; about 170 of them tests |
| `crates/editor-core` other (`worker.rs`, `control.rs`, `switchboard.rs`, `lib.rs`) | 61 | 2 | `EngineWorker::close`, `wait_closed`, `Parked::close`, `retire_coordinator` public |
| `crates/session` | 79 | 1 | Doc id, `start_graph`, `ask_switch`/`ask_change`, `current_doc`/`with_doc`, strictly increasing run ids |
| `crates/ui/src` | 365 | 679 | `EditorApp` holds a `Document`; `try_run`, `pump_saves`, `drain_engine`, half of `pump_runs`, the `board!` lending gone |
| `crates/ui/tests/engine_thread.rs` | 47 | 0 | The startup case |
| `crates/ffi/src/core.rs`, `include/plumcraft.h` | 475 | 356 | Documents by id, held requests, `changed`, shared engines; the duplicates gone |
| `crates/ffi/tests/documents.rs` | 275 | 0 | New |
| `apps/plumcraft-editor/src/lib.rs` | 87 | 14 | `editor_starting`, `on_ready`, `wait_closed` at exit; `editor_with` test-only |
| **Total** | **2,691** | **1,052** | Net +1,639, of which about 490 are tests. `crates/ui/src` is 314 lines smaller |

## Disk

- 40 GB free at the start (not the 70 GB the brief expected), 36 GB at the end.
- `claude-debug/native-n1/s7-*` is about 1.3 GB (four final run folders of 172 MB each, their stores removed).
- `target/` is 89 GB. No extra target directory was made; the research software built into the workspace `target/`.

## What the docs should say

- **`docs/STATUS.md`:** N1 slice 7 is done.
  - `plumcraft_editor_core::document::Document` is the one document for every shell: the egui editor holds one, the C ABI holds several by id.
  - Startup no longer waits for a process or HTTP engine: the window is up in the "connecting" state, and `plum_open` returns at once.
  - Closing never waits for an engine.
  - Hooks can tell documents apart.
  - Test counts 1,368 → 1,378, with the same single local failure.
- **`docs/research/24-native-shells.md`:**
  - **Section 16:** a slice 7 bullet with the above, including:
    - the `Turn`;
    - the `Deref` decision;
    - the startup sequence and its numbers, among them the process engine's first reply about 75 ms later than before (the first frame's own time);
    - item 4's two assertions;
    - the C ABI behaviour change: a failed engine start is reported in the feed, not by `plum_open` returning 0.
  - **Section 4.3:** "whether two sessions may share one in-process engine" is answered as built: one engine, a link each, runs queued on its worker, unique run ids, events filtered by run. Add the thread-local current document beside the process-wide hooks.
  - **Section 10's launch row:** met for process and HTTP engines (window first, "connecting"). For engines compiled in, they still connect on the UI thread, in a few ms.
  - **Section 4.2:** each document has its own worker, which is let go without a join on close.
- **`docs/shell-interface.md`:**
  - **4.3:** documents as built:
    - `document.open {demo?, path?}` → `{doc, title}` (no `grant`, no `autosave` yet);
    - `document.close {doc, discard?}` → `{closing}` or `{closing: false, ask}`;
    - `document.list` (with `ready`, `first`, `engine`);
    - a request's top-level `doc`, defaulting to the first open document;
    - control-port requests go to the first document (no `document.focus` yet).
  - **6.1:** the codes `unknown_doc` and `bad_params` are built for `doc`.
  - **7.1:** `changed` as built: `{doc, domain: "engine", gen, connected, label, ready}` and `{doc, domain: "document", closing: true}`. Events and raised asks carry `doc`; `asksEnded` does not.
  - **11.1:** the `doc` field is in the feed's raised asks, not in `ui.inspect`'s `asks` (kept byte-identical).
  - **5.2:** `plum_open` returns before the engine; requests are held until it has connected.
  - **17, question 3:** built additively (`Session::doc_id`, `commands::current_doc`), not as a new hook argument; say whether to change the signatures later.
  - **17, question 11:** built as above.
  - **5.1:** the in-process binding is `Document` plus `document::turn`/`execute`, not the sketch's `Core::call`.
- **`docs/control-protocol.md`:** no change needed. Nothing a control client observes changed: the port appears when the editor is ready, as before. One optional sentence: for an engine that has to be started, the port now appears after the window does.
- **`AI.md`:** at commit time.
- **`docs/BUGS.md`:** nothing from this slice. The pre-existing in-place spectrogram stall that can exceed `values_never_wait_for_an_engine`'s 150 ms limit under parallel load could be recorded as a flake risk.
