# N1 slice 5: debug driving belongs to the core

Branch `native/n1-control`, working tree only: nothing committed, staged or stashed. 2026-11-19.

## Summary

- **One dispatcher, in the core.** `plumcraft_editor_core::control` handles every control-channel method: routing, `command.run`, `command.list`, `graph.get`, `ui.inspect`, `ui.invoke`, `ui.prompt`, `ask.answer`, `ui.waitRun`, the parked replies and the error shapes.
  - It serves the egui editor's `--control` port.
  - It serves `crates/ffi`'s `plum_request`.
  - It serves a new control port on a C ABI core.
- **One "reply once switched", in the core.** The egui shell's `EngineWait` and the FFI's `SwitchWait` became `control::Parked`. Their two `step_engines` loops became `control::step` over a `Driver` trait.
- **`ui.inspect` is split by owner.** The core builds what it owns and the shell adds a block of its own. The egui editor's replies are unchanged: two before-captures against two after-captures of 41 replies each, with 0 differences after masking timings, run ids, scratch paths and the canvas transform.
- **Over the C ABI:**
  - `ui.screenshot` is the contract's `screenshot` ask, which the shell answers.
  - `ui.set` and `ui.audio` reply "… isn't available from this shell".
  - `ui.inspect` gives the core's parts with no geometry.
- **The control port over the C ABI.** The config key `control: {port, token_file}` (shell-interface 4.1) starts the same loopback server. Closing the core closes it. A new C ABI test drives a core over TCP, and the C function list is unchanged.
- **Logging.** There is no log framework today. The one place that assembles what the editor tells the person, the status bar's messages, moved to the core (`control::status_messages`). The contract's `log.tail` facade is not built.
- **Tests:**
  - 1,336 ran before and 1,342 after, with the same single expected failure in two b3 runs and one md5 run: 6 new tests, none gone.
  - No existing test file was edited.
  - Python: 224 passed under md5 and b3; ruff is clean.
  - Gates: fmt, clippy, the hash gates and the toolkit-free gate are all clean.
- **Downstream and prefs:**
  - The research software (scratch copy) checks with no source change, and its `open_recipe` test passes.
  - The maintainer's real `prefs.json` and `trusted.json` are byte-identical before and after.

## The dispatcher and the shell trait

`crates/editor-core/src/control.rs` (1,011 lines after rustfmt, 190 of them tests). Its entry points:

- `handle(s, method, params, reply)`: one request. `reply` is called exactly once, now or later.
- `now(s, method, params)`: the methods that reply at once. The egui shell's `control_now` uses it.
- `inspect(s)`: the core's part, built by `inspect_core`, with the shell's block merged in.
- `tick(s)`: answers what waits on time (run waits, screenshot asks that timed out).
- `settle(s)`: answers what waited on a switch, once the engines are idle.
- `step(d, out)`, `engines_now(d, limit)`, `wait_engines(d, timeout)`: engine switching without waiting, and the waiting forms for callers that may wait.

### `Shell: invoke::Host`

This trait is new; it sits beside slice 4's `Host`, which is unchanged. Three methods are required and the rest have defaults.

| Method | What it is for | egui shell (`crates/ui/src/control.rs`) | C ABI core (`crates/ffi/src/core.rs`) |
|---|---|---|---|
| `view()` (required) | Read-only parts the shell keeps beside the session: link, engine state, coordinator, switching, last error, asks, document name, app title, run animation, prefs, theme | Its fields; `run_anim`, `prefs` and `theme` are `Some` | Its fields; document `Untitled`, app title `PlumCraft`, `run_anim` and `theme` `None`, its in-memory prefs |
| `parked()` (required) | Where the dispatcher keeps replies that wait | `EditorApp.control.parked` | `Worker.parked` |
| `clear_last_error()` (required) | `ui.invoke` starts afresh | Clears `last_error` | Clears `last_error` |
| `request(f)` | Run a request so that what it starts on an engine doesn't make the shell wait | Sets `in_frame`, resets `engine_asked`, restores `in_frame` (what `handle_control` did before) | Default: runs `f` (this core never waits) |
| `engine_asked()` | Did the command ask for a switch? | Its `engine_asked` flag, which `try_run` sets | Default: `session.engines.is_pending()` |
| `presented()` | Show what the core asked, after a request | `present_asks()` (the native dialogs, the prompt and form windows) | `feed_asks()` (tidy, then the asks into `plum_poll`'s feed) |
| `redraw()` | Something changed | `request_repaint` on the request's context | Default: nothing (the outbox wakes the shell) |
| `inspect_shell()` | The shell's block of `ui.inspect` | `shell_inspect`: `popouts`, `parameters`, `editors`, `wirePreview`, `copied`, `selection`, `canvas`, `popups`, `watched`, `groups`, `rename` | Default: empty (no geometry) |
| `screenshot(shot, reply)` | `ui.screenshot`; its params are checked by the core first | The existing window, then offscreen, path (`Shot`), unchanged | Default: `ask_screenshot`, the `screenshot` ask |
| `ui_set(params)` | `ui.set` | The existing code; replies the `canvas` block | Default: "ui.set isn't available from this shell" |
| `ui_audio(params)` | `ui.audio` | The existing code; replies `audio.inspect()` | Default: "ui.audio isn't available from this shell" |

### `Driver: Shell`

Engine switching is one loop for every shell.

| Method | egui shell (`engine_view.rs`) | C ABI core |
|---|---|---|
| `worker()` | `engine_worker`, given the repaint waker on first use (what `step_engines` did first) | `engines` |
| `switching()` | `switching` | `switching` |
| `board(f)` | Its existing `board` (lends the parts, then applies the `Effects` to the canvas and the Watched panel) | Its existing `board` |
| `start(work)` | In place for an engine compiled in (slice 3's exception, kept), else on the worker carrying the connector | Always on the worker, carrying the launcher |
| `take_back(back)` | The connector comes back | Default |
| `outcome(r)` | The Attach… window closes when connected | Default |

`View` lets the core build `title`, `banner`, `engineStatus` and `engine` from the same code for both shells. `EditorApp::window_title`, `document_name`, `banner_text`, `engine_status` and `status_messages`, and `frame_view::inspect`, now delegate to the core.

## Each control method

| Method | Was | Now | Needs from the shell |
|---|---|---|---|
| `command.run`, `engine.execute` | egui: `control_now` with `try_run`. FFI: `automation::dispatch` with `session.execute` | `control::now` with `Host::run` (egui: `try_run`; FFI: `execute`, then send outgoing) | `Host::run`, and `request` and `engine_asked` for parking |
| `command.list`, `engine.commands` | egui: `automation::command_rows`. FFI: `automation::dispatch` | `control::command_rows` (same rows) | none |
| `graph.get` | Both, separately | `control::now` | none |
| `ui.inspect` | egui only, in `EditorApp::inspect` | Core: `inspect_core` plus `Shell::inspect_shell` | `view()`, and its block |
| `ui.invoke` | egui: `control_now`. FFI: its own copy, with its own error texts and a reply of `{invoked, asks, lastError}` | `control::now` (`invoke::invoke`, by `Script`) | `presented` |
| `ui.prompt` | egui only | `control::now` (`invoke::answer_prompt`) | `presented` |
| `ask.answer` | egui: `control_now`. FFI: its own copy | `control::now`; a `screenshot` ask's answer goes to the waiting `ui.screenshot` | `presented` |
| `ui.waitRun` | egui: `ControlState.waits` and `answer_waits` | `Parked` and `control::tick` (FFI ticks every 50 ms; egui at every frame's end) | none |
| `ui.screenshot` | egui only | Params checked by the core, then `Shell::screenshot` | Pixels (egui), else the `screenshot` ask |
| `ui.set` | egui only | `Shell::ui_set` | Canvas and widget state |
| `ui.audio` | egui only | `Shell::ui_audio` | The audio device |
| Unknown method | egui: `unknown method "x"`. FFI: the same, cut to 64 chars | egui's text, for both | none |

The transport itself (loopback TCP, token, JSON lines) was already toolkit-free in `plumcraft-automation`. It stays there. It gained `server::Server`, which stops when dropped (below). The existing `server::start` is unchanged.

FFI-only requests stay in `crates/ffi`: `run`, `ping`, `spike.*`, and the new `core.control`.

## How the two reply-parking implementations became one

**Before:**

- **egui:** `EngineWait {reply, ctx, command, invoked, last}` in `EditorApp.engine_waits`. `engine_outcome` updated or failed the waits. `answer_engine_waits` replied once `!engine_busy()`. `wait_for_switch` parked.
- **FFI:** `SwitchWait {id, tag, command, last, asks}`, failed or answered inside its own `step_engines`. Each shell had its own `step_engines` loop (drain the worker, finish, begin while pending, retire).

**Now:**

- `control::Parked` holds `switches`, `runs` and `shots`.
- `handle` parks a request when `engine_asked() && engine_busy() && ok`, the egui rule unchanged.
- `control::step` is the single loop. It feeds each outcome to `Parked::outcome`: the latest outcome is kept, and the first error fails the `command.run` waits. Then `settle` answers the rest when idle: a command's outcome with its `how`, or `ui.inspect` with `invoked`.
- `in_frame` and `engine_asked` remain egui fields because `try_run` uses them to decide whether to wait. The dispatcher only sees them through `Shell::request` and `Shell::engine_asked`, and the FFI takes both defaults.

Two FFI replies change, and the egui editor's are unchanged:

- When a switch fails, a parked `ui.invoke` or `ask.answer` on the FFI now gets the UI state (the egui rule) rather than the error.
- FFI `ui.invoke` and `ask.answer` replies are now the full `ui.inspect` (plus `invoked`), a superset of the old `{asks, lastError}`.

## `ui.inspect` before and after

**How it was captured:**

- `claude-debug/native-n1/s5_probe.py` drives `target/debug/plumcraft-editor --control 0` with scratch preferences. Each reply is saved, giving 41 per run.
- The states are:
  - empty;
  - a two-node graph after `ui.invoke graph.tidy`, `run.watch` and `ui.waitRun`;
  - `graph.get` and `command.list` in the empty and ran states;
  - a pop-out open (`view.popout`);
  - a prompt open (`ui.invoke graph.new` over changes), then `ui.prompt cancel`;
  - a `file.save` ask open (`ui.invoke graph.saveAs`), then `ask.answer cancel`;
  - an engine switch to process mode in flight, seen from a second connection polling `ui.inspect` until `engine.pending` was true, then the switch's own reply, the state after, and the switch back.
- It also saves the replies of:
  - `ui.set` (good, bad and unknown fields) and `ui.audio` (good and bad);
  - `ui.prompt` with no prompt and with a bad answer;
  - an unknown method;
  - bad `command.run` and `ui.invoke` params;
  - `ask.answer` with an unknown ask and a non-numeric ask;
  - bad `ui.waitRun` and `ui.screenshot` params;
  - an offscreen `ui.screenshot`;
  - `app.quit`.

`claude-debug/native-n1/s5_diff.py` compares every key, and masks only these:

- `*Ms`, `ms`, `strength`;
- `runId`;
- `pan`, `zoom`, `rect` (the first fit's timing varies);
- paths under each capture's own folder.

| Comparison | Replies | Differences |
|---|---|---|
| before run 1 against before run 2 (noise check) | 41 | 0 |
| before run 1 against after run 1 | 41 | 0 |
| before run 1 against after run 2 | 41 | 0 |
| before run 2 against after run 1 | 41 | 0 |

The captures are in `claude-debug/native-n1/s5-inspect-before/run{1,2}/` and `s5-inspect-after/run{1,2}/`. The after binary was rebuilt after the last edit (its timestamp postdates both `control.rs` files).

## `ui.screenshot`, `ui.set` and `ui.audio` over the C ABI

- **`ui.screenshot`: the `screenshot` ask (built).**
  - `asks::Kind::Screenshot`, `{path, offscreen}`, non-modal, `by: script`, raised with a 2 s timeout (`control::SHOT_WAIT`).
  - The answer is `{path, width, height, source?}` (`source` `window` or `offscreen`, default `window`; sides 1 to 65,536) or `{error}`, checked as hostile input in `asks.rs`.
  - `ask.answer` on it replies to the waiting `ui.screenshot` with `{path, width, height, source}`, the egui reply's shape. A cancel or a timeout replies with an error. A late answer is told "ask N was already timedOut".
  - The egui shell never raises it: it keeps its window-then-offscreen path, unchanged.
- **`ui.set` and `ui.audio`:** "… isn't available from this shell". There is no canvas or audio device in the core yet (shell-interface 6.2 wants `ui.set` to act on core state, `view` and `palette`, and audio in the core, 13.4; both are later work).
- **`ui.inspect` on the C ABI core** gives:
  - `title`, `document`, `path`, `dirty`, `undo`, `redo`;
  - `prompt`, `asks`, `closing`;
  - `graph`, `engine`, `banner`, `engineStatus`, `about`, `catalog`;
  - `lastError`, `run`;
  - `theme: null`, `prefs` (its in-memory store);
  - `frames`, `docs`, `save`.

  It has no `canvas`, `selection`, `popups`, `watched`, `groups`, `popouts`, `parameters`, `editors`, `wirePreview`, `rename`, `copied` or `runAnim`.

## Logging

What the editor logs today:

- **No logging framework.**
  - 40 `eprintln!`s across `crates/ui`, `editor-core`, `apps/plumcraft-editor` and `session`, printed to stderr. They report:
    - startup and argument errors;
    - the control port and token;
    - headless `run` and `check` problems;
    - preferences not saved;
    - an audio output error;
    - screenshot save errors;
    - duplicate command, value-editor or viewer ids.
  - There is no level, target, ring or file. The C ABI core prints the same control-port lines on its own.
- **No log panel.** The person sees the **status bar's messages** and the Messages window (`view.messages`). Those are assembled from:
  - the session's run (node errors, `run_error`);
  - the application's hook segment;
  - the session's last save outcome;
  - the shell's `last_error`.
- **`lastError`** is per shell. Both shells keep one, set by `Host::say` and `run`, and `ui.inspect` reports it.
- **`PLUMCRAFT_*` switches:** none controls logging.
  - They name files and tools: `*_PREFS`, `PLUMCRAFT_UV`, `PLUMCRAFT_SHOT_DIR`, `PLUMCRAFT_ENGINE_TOKEN`, `PLUMCRAFT_ENGINE_SOCKET`, the craft app paths, `PLUMCRAFT_FFMPEG`, `PLUMCRAFT_CRAFT_ROOT`, digests and test fixtures.
  - The engine's `stderr_tail` reaches the banner through `EngineState`.
- **Files written:**
  - `prefs.json`, `trusted.json` and `labels.toml` beside it;
  - screenshots under `$TMPDIR/plumcraft-screenshots`;
  - opened copies in the cache.
  - No log file.

What I did:

- The status bar's message list is now the core's: `control::Msg` and `control::status_messages(session, last_error, app)`. `status_msgs::Msg` re-exports it and the egui shell only draws chips, so a native shell builds the same list.
- The inputs to it (`lastError`, `run`, `save`) already reach any shell through `ui.inspect`. The application's hook segment is egui-only (`app_hooks`).
- The contract's section 15 is not built. That is the facade with levels and targets, the ring read by `log.tail`, the bounded file in `dirs.logs`, `log.write` and Export Diagnostics. It is new behaviour and a protocol addition, so it deserves its own slice.

## The control port over the C ABI

- **Config.** `plum_open`'s JSON takes `control: {port, token_file}` (shell-interface 4.1 and 5.3).
  - A null or absent `port` serves nothing.
  - A bad port or token file fails `plum_open` with a reason naming `control`.
- **Server.** It always binds `127.0.0.1`, with the same token rules, framing, limits and dispatcher as the editor's.
- **Learning the port.** The core prints `plumcraft: control channel on 127.0.0.1:<port>` and the token (or its file), as `--control` does. `core.control` over `plum_request` answers `{port, tokenFile}`.
- **Plumbing.** The server's requests reach the core thread through a new `Msg::Control` wake and are handled between `plum_request`s.
- **Shutdown.** `automation::server::Server` stops when dropped:
  - its listener polls a stop flag every 50 ms (non-blocking accept);
  - every connection is shut;
  - the listener thread is joined.

  Connection threads end once their sockets are shut. The `Worker` declares the request receiver before the server, so a connection waiting on a queued request is answered (the channel closes) before its socket is shut.
- **C function list and `PLUM_ABI_VERSION`:** unchanged; `header.rs` passes.

**Test evidence.** `crates/ffi/tests/control_port.rs`, through the C ABI (`plum_open`, `plum_request`, `plum_poll`, `plum_close`) and a TCP client. Both tests pass in all three final runs.

`a_core_opened_through_the_c_abi_is_driven_over_its_control_port`:

1. `core.control` gives the port and token file.
2. An unauthenticated `ui.inspect` gets "authentication required".
3. Over TCP:
   1. two nodes are added, wired and set, then watched and run;
   2. `ui.waitRun` replies `running: false`;
   3. `command.list` lists `run.start`.
4. `ui.inspect` over TCP has 2 nodes, the upper node `done`, the title "Untitled • — PlumCraft (Text …", no asks, engine connected, and no `canvas`. The same request through `plum_request` gives the same `graph`.
5. `ui.invoke graph.saveAs` over TCP replies with a `file.save` ask (`by: script`, `invoked`). `ask.answer` with a path saves the file. A second answer gets "ask N was already answered".
6. `ui.screenshot` on a second TCP connection is parked:
   1. the `screenshot` ask arrives in `plum_poll`'s `asks`;
   2. the C side answers `{path, width: 640, height: 480}`;
   3. the TCP request replies `{path, width: 640, height: 480, source: "window"}`.
7. `ui.set` and `ui.audio` say they aren't available from this shell.
8. Hostile input over the socket:
   - 13 methods (including a 10,000-character name and `"U+202E\u0000"`) times 10 hostile params each;
   - six junk lines (bad JSON, arrays, numbers, a numeric method, a deep id).

   Every one gets a boolean `ok`, and `ui.inspect` still works afterwards.
9. `plum_close`: the port refuses or EOFs within the 5 s limit, and the open connection reads EOF.

`a_bad_control_config_is_refused_and_none_means_no_port`:

- four bad `control` values fail `plum_open`, each with a reason naming `control`;
- three empty ones (absent, null, a null port) open with `core.control` answering `{port: null}`.

**The moved dispatcher's own hostile tests,** in `control::tests`:

- `hostile_requests_are_refused_and_never_panic`: 15 methods, including empty and bidi names, times 16 hostile params (null, numbers, strings, arrays, wrong types, a 100,000-character command, negative, fractional and huge ask ids, deep nesting, `"\u0000"` keys). Each reply must carry a boolean `ok`, and parked ones are flushed afterwards.
- `a_screenshot_is_asked_of_the_shell_and_replies_with_its_answer_or_times_out`: five hostile screenshot answers are refused, and the ask stays open.

## Tests

| Run | Ran | Passed | Failed | Skipped | Failing |
|---|---|---|---|---|---|
| Baseline b3 (`s5-base-b3`) | 1,336 | 1,335 | 1 | 10 | `plumcraft-craft::params_coverage hidden_params_match_the_baseline` |
| Final b3, run 1 (`s5-final1-b3`) | 1,342 | 1,341 | 1 | 10 | the same |
| Final b3, run 2 (`s5-final2-b3`) | 1,342 | 1,341 | 1 | 10 | the same |
| Final md5 (`s5-final-md5`) | 1,342 | 1,341 | 1 | 10 | the same |

- **Test names** (`s5-base-names.txt` against `s5-final1-names.txt`): exactly 6 new, none gone.
  - `plumcraft-editor-core control::tests` ×4: `a_shell_with_no_geometry_still_inspects_the_core_parts`, `a_screenshot_is_asked_of_the_shell_and_replies_with_its_answer_or_times_out`, `a_wait_for_the_run_replies_when_nothing_runs`, `hostile_requests_are_refused_and_never_panic`.
  - `plumcraft-ffi::control_port` ×2.
- No `LEAK`, and no `shm_foreign_digest` or `shm_bad_key` marks.
- **Python, as CI runs it:** 224 passed under md5 and under b3, no bad marks; `ruff check`: clean.
- **Gates:** `cargo fmt --all --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `scripts/check-hash-gates.sh` and `scripts/check-core-toolkit-free.sh` are all clean.

## Existing test files touched

None.

- No file under `crates/*/tests` or `apps/*/tests` changed.
- No `#[cfg(test)]` module changed. `crates/ui/src/control.rs`'s `fields_are_checked` still reaches `check_fields` through `use super::*`: the module imports it from the core.
- The only new test file is `crates/ffi/tests/control_port.rs`. The four core tests are in the new `control.rs`.

## Preferences checksums

The maintainer's `~/Library/Application Support/plumcraft-editor/` (SHA-256):

| File | Before the first run | After the last run |
|---|---|---|
| `prefs.json` | `c1a8cf3f860c6052f3ed804729461e1840853ed8e5e87483a74bcea05f8d279d` | the same |
| `trusted.json` | `a10d6fc822e72d77cbf715c89bde4fa809897db6cde6ccdf7be80341464f4704` | the same |

**Unchanged.**

- Every editor run set `PLUMCRAFT_EDITOR_PREFS` to the probe's own folder. I checked in the code that `prefs::path_for("plumcraft-editor")` reads it.
- `trust_path_for` and `labels.toml` follow it to the same folder.
- `ui.inspect`'s `prefs.path` in every capture names the scratch file.

The records are `claude-debug/native-n1/s5-userprefs-before.txt` and `s5-userprefs-after.txt`.

## Downstream effects

**The research software.**

- Copied without `target`, `.git`, `dist` or `claude-debug` to `claude-debug/native-n1/s5-<app>-check/`, with slice 4's `.cargo/config.toml` patching every `plumcraft-*` crate to this tree. It built into the workspace's own `target/`.
- `cargo check --tests` exits 0 with no source change, re-checked after touching both `control.rs` files.
- `cargo test -p <app> --test open_recipe` passes 2 of 2.
- The repository was not touched. It has uncommitted changes of its own, which I left alone.

**API changes a downstream application would see:**

- **New:**
  - `plumcraft_editor_core::control`: `Shell`, `Driver`, `View`, `Parked`, `ShotRequest`, `handle`, `now`, `inspect`, `inspect_core`, `tick`, `settle`, `step`, `engines_now`, `wait_engines`, `engine_busy`, `ask_screenshot`, `command_rows`, `status_messages`, `Msg`, `banner_text`, `engine_status`, `window_title`, `document_name`, `frames_inspect`, `run_anim_inspect`, `engine_inspect`, `short_path`, `APP_TITLE`, `UNTITLED`, `SHOT_WAIT`.
  - `plumcraft_automation::server::Server`.
- **Changed:**
  - `asks::Kind` gains `Screenshot`, and `Kind::ALL` has 11. An exhaustive `match` on `Kind` downstream breaks; the research software has none.
  - `asks::Answer` gains `Shot`.
  - `plumcraft_ui::control::ControlState` has a new pub field `parked`. It lost the private `waits` and `next_token`. `pending()` also counts replies parked on a switch.
  - `plumcraft_ui::control::{Reply, ok, err, WAIT_RUN_MAX_S, default_screenshot_path}` are re-exports of the core's, at the same paths.
  - `plumcraft_ui::status_msgs::Msg` is a re-export of the core's `control::Msg`, with the same fields and methods.
  - `plumcraft_ui::APP_TITLE` is the core's constant, with the same value.
  - `EditorApp` implements `control::Shell` and `control::Driver`.
- **Gone (`pub(crate)` only):** `engine_view::EngineWait`, `EditorApp::engine_waits`, `wait_for_switch` and `engine_inspect`.
- **C ABI core:**
  - New:
    - the `control` config key;
    - the requests `ui.inspect`, `ui.prompt`, `ui.waitRun`, `ui.screenshot` (an ask), `ui.set` and `ui.audio` (errors), and `core.control`.
  - Changed:
    - `ui.invoke` and `ask.answer` reply the full UI state;
    - their param errors use the editor's wording;
    - an unknown method's name is no longer cut at 64 characters;
    - `command.run` errors now also set `lastError`.

## What remains in `crates/ui` that is logic rather than drawing

Rough line counts.

| Item | Where | Lines | For |
|---|---|---|---|
| `resolve` of thumbnails, pictures and peaks, and the `Thumbs` cache with no "not yet" state | `run_view.rs` (`Thumbs`, 304 in the file), `audio.rs` (peaks) | ~150 | the media crate |
| Full values: `Fulls::read`, `full_reader`, `sync_media`, `sync_compare` | `watched.rs` (526), `lib.rs` | ~85 in `lib.rs` plus `Fulls` | the media crate |
| Decoding pictures, SVG and PDF; compare pictures | `media.rs` (1,076), `compare_view.rs` (582) | mostly logic around egui textures | the media crate |
| `pump_runs`, `drain_engine`, `pump_saves` (sending runs, applying events, the save thread, the uploads prompt) | `lib.rs` | 110, 24, 43 | the core's `Document` (N2), as the FFI's `send_outgoing` and `events` duplicate them |
| `try_run`'s follow-ups (document name, problems, fit, watch reruns, bundle and expand notes, Convert offers, the engine-switch wait) | `lib.rs` | 119, plus `offer_for` 11 and `expand_note` 15 | core (contract 6.2) |
| The menu model: `MENUS`, `menu_bar`, `checked`, `parse_shortcut`, `shortcuts`, the Engine menu's choices | `lib.rs`, `engine_view.rs` | 79, 7, 15, 36, 57 | the menu-model slice |
| Appearance commands through `ui_asks` (`window_command`) | `settings_view.rs` | 12 plus `pump_theme` and `use_prefs` (~40) | appearance into the core |
| Startup: `with_link` reads `schema` and the catalogue before the window; the app's `editor_with` | `lib.rs`, `apps/plumcraft-editor` | 15 plus the app's | the startup slice |
| The run animation's plan and pump | `lib.rs` | `pump_run_anim` 23, `run_anim_plan` 18 | core (its model is already there) |
| Selection, focus, hit testing, align and distribute, copy, wire routing, groups | `canvas.rs` (2,167), `edit_view.rs` (727), `wire_route.rs` (240), `groups_view.rs` (1,079) | several hundred | N2 |
| Rename as a `text` ask | `rename_view.rs` (177) | ~40 | the asks follow-up |
| Close handling over unsaved changes | `lib.rs` `handle_close` | 19 | small, with the menu model |

Duplication left between the two shells:

- The FFI's `spike.waitRun` and its own run summary, kept for the N0 measurements and separate from `ui.waitRun`.
- `send_outgoing` and `events` against egui's `pump_runs` and `drain_engine`.

## Lines of code

From `git diff --numstat`, plus the two new files.

| Area | Added | Removed | Notes |
|---|---|---|---|
| `crates/editor-core/src/control.rs` | 1,011 | 0 | New, after rustfmt. 190 lines of tests |
| `crates/editor-core/src/asks.rs`, `lib.rs` | 45 | 2 | The `screenshot` kind and its answer |
| `crates/ui/src/control.rs` | 108 | 262 | Now only the `Shell` impl and the window's block |
| `crates/ui/src/engine_view.rs` | 59 | 177 | The `Driver` impl replaces the loop and the waits |
| `crates/ui/src/lib.rs`, `status_msgs.rs`, `frame_view.rs`, `asks_view.rs` | 19 | 117 | Title, banner, status, messages and frames delegate |
| `crates/ffi/src/core.rs` | 190 | 179 | The dispatcher, the `Shell` and `Driver` impls, the control port |
| `crates/automation/src/server.rs` | 93 | 2 | `Server`, which stops when dropped |
| `crates/ffi/tests/control_port.rs` | 226 | 0 | New |
| `docs/control-protocol.md` | 17 | 2 | |
| **Total** | **1,768** | **741** | Net +1,027. About 415 lines are tests; `crates/ui` is 435 lines smaller |

## What the docs should say

- **`docs/STATUS.md`:** N1 slice 5 is done.
  - The control channel's dispatcher is in the core (`plumcraft_editor_core::control`), the same for the egui editor, `plum_request` and a C ABI core's own control port (config `control: {port, token_file}`).
  - There is one "reply once switched" and one engine-stepping loop.
  - `ui.inspect` is the core's parts plus the shell's block, unchanged for the egui editor (41 replies compared, 0 differences).
  - `ui.screenshot` over the C ABI is the `screenshot` ask.
  - Test counts 1,336 → 1,342, with the same single local failure.
- **`docs/research/24-native-shells.md`:**
  - **Section 16:** a slice 5 bullet with the above:
    - the `Shell` and `Driver` traits;
    - the key split of `ui.inspect`;
    - what the C ABI core lacks (`ui.set`, `ui.audio`, geometry);
    - that the log facade (section 9's logging bullet) is not built, only the status messages moved.
  - **Section 4.1's `ui/control.rs` row:** done, except `ui.set` and `ui.audio`, which need the canvas view and audio in the core.
  - **Section 4.4:** `plumcraft-automation` gained a stoppable `Server`.
  - **Section 9's first two bullets:** built.
- **`docs/shell-interface.md`:**
  - **Section 4.1:** `control` is read by the C ABI core today. Add `core.control` as how a shell learns the bound port.
  - **Section 5.3:**
    - the control channel is in the core;
    - `ui.screenshot` from the channel raises the `screenshot` ask (built);
    - `ui.set` and `ui.audio` from a shell without them answer "isn't available from this shell".
  - **Section 6.2's `ui.inspect` row:** describe the split. The core's keys are `title`, `document`, `path`, `dirty`, `undo`, `redo`, `prompt`, `asks`, `closing`, `graph`, `engine`, `banner`, `engineStatus`, `about`, `catalog`, `lastError`, `run`, `runAnim` (where the shell has an animation), `theme`, `prefs`, `frames`, `docs` and `save`. The shell's are the rest.
  - **Section 11.2:** add `screenshot`, `{path, offscreen}`, answered `{path, width, height, source?}` or `{error}`, with a 2 s timeout.
  - **Section 15:** today's facts. No framework: `eprintln!` and the status messages. The status messages are now `control::status_messages` in the core.
  - **Appendix A's `control.rs` and `status_msgs.rs` rows:** moved, except the window's block, `ui.set`, `ui.audio` and the screenshot capture.
- **`docs/control-protocol.md`:** done in this change.
  - A new subsection, "On a core opened through the C ABI".
  - The handlers' location sentence is corrected to the core.
  - The date line is updated.
- **`docs/BUGS.md`, `AI.md`:** nothing from this slice.
