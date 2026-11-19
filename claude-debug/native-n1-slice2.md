# N1 slice 2: starting and connecting engines belong to the core

Branch `native/n1-launcher`, working tree only (nothing committed, staged or stashed). 2026-11-19.

## Summary

- **Moved to `plumcraft-editor-core`:**
  - `engines`: the kinds, modes, hosting rules, packs registry, manifests, `choose`, `choices`, the `Launcher` and the connect paths. The connect paths return the core's `Link` with a `Waker`.
  - `switchboard`: the engine-switching state machine (switch, add, remove, attach, reconnect, clear, `engine.use`, reload, install, problems and warnings).
  - `install`: the install job off the UI thread, and `uv tool install`.
  - `headless`: the windowless `run`, `check`, `tidy` and `nodes`.
- **New `Capabilities` value**, consulted by the hosting rules. The default is a desktop's, so nothing changes. Unit tests show that without `subprocess`, only in-process and already-running HTTP hostings are offered, and every other hosting says why.
- **What stays in the shells:**
  - `crates/ui` keeps only drawing and egui-context code: the egui `Connector` type, the Engine menu, the Attach and Install windows, and applying the core's `Effects` to the canvas and the Watched panel.
  - `apps/plumcraft-editor` keeps the window, demos, `doc_apps` (it needs the viewer registry), the CLI flags for the window, and thin egui wrappers (`LauncherExt`).
- **`crates/ffi` starts its engine through the core's `Launcher`.** Its config can now name an engine and hosting (`engines.default`, `engines.mode`) and a `platform`. The C ABI's function list is unchanged.
- **Core stays toolkit-free.** `scripts/check-core-toolkit-free.sh` prints ok. The core has no dependency cycle.
- **Tests:** the same single expected failure before and after, under b3 and md5. 1,306 tests before, 1,314 after: 2 moved, 8 new. No existing test file was edited.
- **Downstream:** a scratch copy of PlumImpress compiles unchanged against this tree.

## The cut, and why

- **What moved:** everything that decides or performs engine starting and switching, but neither draws nor needs an `egui::Context`. A native shell needs all of it.
- **The state the switch acts on stays in the shell's struct.**
  - `EditorApp.link`, `.coordinator`, `.engine`, `.installing` and `.last_error` are public fields that the kittests (and PlumImpress) read.
  - So the core's `Switchboard<'a>` borrows them for one operation: session, link, coordinator, state, last error, reloader, installer and installing.
  - It returns `Effects { runs_forgotten, catalog_set: Option<first>, redraw }`, which the shell applies to what it keeps beside the session (the Watched panel's value, thumbnails, the rerun flag, canvas invalidate and fit).
  - This keeps every field path the tests use and moves all of the logic. When N2 gives the core a `Document` owning these parts, `Switchboard` becomes a method set on it.
- **The connect function is given per call** (`&mut dyn FnMut(&str, &str) -> Result<Link, String>`), not stored in the core.
  - The egui `Connector` takes an `Option<&egui::Context>`, and PlumImpress builds such closures. So the ui binds its context into a plain closure at each call, and a native shell passes `launcher.connect_ids` with its waker.
- **The built-in engines are hard-wired; packs and manifests stay injected.**
  - The built-ins (text, array, craft packs; audio is process-only) do not make the core heavy in toolkit terms, and they cause no cycle.
  - Hard-wiring keeps the `EngineKind::Text`/`Audio`/`Array`/`Craft` enum and its exact rules. The kittests depend on both.
  - The application's own engines are still injected: `engines::set_packs`, called by `run()` from `AppConfig.packs`. Manifests are injected through `load_manifests`.
  - The cost is the array engine's decoders (`image`, `hayro`, `skrifa`, `tiny-skia`) in every core build, the FFI's included. A cargo feature per built-in engine would remove that if a phone build needs to shrink.
- **The registries keep their old scope:**
  - packs: process-wide `OnceLock` (an engine reads its pack on its own thread);
  - manifests: thread-local (`EXTERNALS`), read on the thread that chooses and starts engines.
  - Making the manifests a process-wide lock would let parallel `cargo test` threads overwrite each other's manifest lists. Several kittests load different manifests.

## What moved where

| From | To | Notes |
|---|---|---|
| `apps/plumcraft-editor/src/lib.rs` `PackEngine`, `WORKER_FLAG`, `serve_worker`, `PACKS` | `crates/editor-core/src/engines.rs` | `set_packs()` replaces `PACKS.set`. `plumcraft_editor::PackEngine` is a `pub use` |
| `lib.rs` `Mode`, `EngineKind` (all methods), `External`, `EXTERNALS`, `manifest_folder`, `load_manifests`, `AUDIO_HINT`, `EMPTY_WORKFLOW`, `flag_value` | `engines.rs` | Methods now `pub`. `EngineKind::demo` split: `own_demo()` in core (`None` for audio); the app's `demo()` adds `demos/audio.json` |
| `lib.rs` `choose`, `choices`, `engine_binary` | `engines.rs` | Plus `choose_for(args, caps)`, `choices_for(caps)`, `offers(caps)` |
| `lib.rs` `Launcher` (fields `binaries`, `manifests`), `connect_core`, `connect_core_with`, `connect_external` | `engines.rs` `Launcher` (+ `caps`, `store_root`), `connect_core`, `connect_ids`, `connector_core`, `start`, `connect_external` | `Launcher::connector` (egui) and `connect` (egui) stay as `LauncherExt` in the app. `connect_with` stays, test-only |
| `lib.rs` `installed_executables`, `tail`, `install_engine` | `crates/editor-core/src/install.rs` | Unchanged |
| `lib.rs` `file_engines`, `manifest_args`, `with_file_engines`, `with_engine_arg` | `engines.rs` | Unchanged |
| `apps/plumcraft-editor/src/headless.rs` | `crates/editor-core/src/headless.rs` | `run`, `check` and `tidy` take `usage: &str` (was `crate::usage(program)`). Its 2 unit tests moved with it |
| `crates/ui/src/engine_view.rs` `Reloader`, `Installer`, `KNOWN_ENGINES`, `Installing` | `switchboard.rs`, `install.rs` | Re-exported at the old paths. `Installing` gains `start()` and `poll()` |
| `engine_view.rs` `pump_engine`, `start_install`, `poll_install`, `reload_engines`, `clear_engines`, `add_engine`, `remove_engine`, `relink`, `forget_runs`, `switch_engine`, `attach` | `switchboard.rs` `Switchboard::{pump, start_install, poll_install, reload, clear, add, remove, relink, forget_runs, switch}`, `engines::attach` | ui keeps the `EditorApp` methods as one-line delegations through `with_board` |
| `engine_view.rs` `target_label`, `engine_title`, `engine_inspect` | `switchboard.rs` `target_label`, `engine_title`, `inspect` | ui delegates. `ui.inspect`'s JSON is built by the same code |
| `crates/ui/src/lib.rs` `engine_label`, `engine_short_label`, `share_connection`, `sync_engine`, `set_catalog` (session part) | `switchboard.rs` | ui delegates; `catalog_changed` keeps the canvas part |

**Stayed, and why:**

| Item | Where | Why |
|---|---|---|
| `Connector` (`FnMut(&str, &str, Option<&egui::Context>)`) | `crates/ui/src/engine_view.rs` | It names egui, and PlumImpress constructs it |
| `engine_choices_menu`, `attach_window`, `install_window`, `by_engine`, `group_engine`, `scrolling`, `AttachForm`, `InstallForm` | `engine_view.rs` | Drawing, and the forms' widget state |
| `with_engines`, `with_installer`, `with_reloader`, `with_link` | ui | Builder API on `EditorApp`. `with_link` takes ui's `Link` |
| `doc_apps`, `has_own_view` | app | Uses `plumcraft_ui::viewers::kinds_for` (the viewer registry is ui's) |
| `demo()` (audio), `MULTI_DEMOS`, `multi_demo`, `read_workflow` | app | `demos/*.json` are the app's files. `editor-core` is publishable, so it can't `include_str!` outside its crate (only the unpublished `plumcraft-audio` does) |
| `workflow_arg`, `control_args`, `start_control`, `VALUE_FLAGS`, `usage`, `version_text`, `about`, `AppConfig`, `run`, `editor_with` | app | The window's CLI and builder |
| `drain_engine`, `pump_runs`, `pump_saves` | ui | Runs off the UI thread is the next slice |

## The core's new dependencies, and evidence that it is still toolkit-free

`cargo tree -p plumcraft-editor-core -e normal --depth 1` now lists:

- plumcraft-array, -coordinator, -craft, -engine, -graph, -host, -proto, -session, -store and -text;
- serde, serde_json.

New this slice: array, coordinator, craft, engine, graph and text. `serde` stays (Capabilities no longer derives `Deserialize`; other core modules use it).

- **Each one is toolkit-free,** checked before adding:

  | Crate | Its normal dependencies |
  |---|---|
  | `plumcraft-text` | engine, proto, serde_json |
  | `plumcraft-array` | base64, flate2, hayro, image, npexact, engine, proto, svg, serde_json, skrifa, tiny-skia |
  | `plumcraft-craft` | commonkit, engine, proto, store, png, serde, serde_json, toml |
  | `plumcraft-coordinator` | commonkit, host, proto, store, serde_json |

- **The gate passes.** `scripts/check-core-toolkit-free.sh` prints "core gate: ok". The gate's tree (`--all-features --target all -e normal,build`) is 268 unique lines, up from 204 in slice 1. A grep of it for `egui|eframe|epaint|winit|wgpu|rfd|plumcraft-ui|cpal|accesskit|glow|tokio` finds nothing.
- **No cycle.** `cargo tree --workspace -i plumcraft-editor-core --depth 1` lists only plumcraft-editor, plumcraft-ffi and plumcraft-ui.
- **The FFI is toolkit-free too:** the same grep over `-p plumcraft-ffi` finds nothing. It dropped its direct `plumcraft-text` and `plumcraft-engine` dependencies, which now come through the core.
- **`apps/plumcraft-editor` dependencies shrank.**
  - Removed: `plumcraft-text`, `-engine`, `-host`, `-array` and `-coordinator` as normal dependencies.
  - `plumcraft-text`, `plumcraft-engine` and `plumcraft-host` became dev-dependencies, since its in-file tests name them.

**Correction to the brief:** `plumcraft-text` does not depend on coordinator, host or store: its normal dependencies are `plumcraft-engine`, `-proto` and `serde_json`. The audio engine is not a dependency of the editor at all; it is only ever a process.

## `Capabilities`

```rust
pub struct Capabilities { pub subprocess: bool, pub craft_apps: bool, pub python: bool }
Capabilities::DESKTOP   // all true; Default
Capabilities::NONE      // all false
Capabilities::declared(Option<&Value>) -> Capabilities   // a shell config's `platform`; unsaid = absent, non-bool = absent
Capabilities::refuses(kind, mode) -> Option<String>      // the reason, or None
```

- **Rules,** in the order checked (the most specific reason wins):
  1. A craft kind without `craft_apps`: "VectorCraft: this device has no craft apps".
  2. A Python mode without `python`: "this device has no Python".
  3. `EngineKind::spawns(mode)` without `subprocess`: "this device can't start programs".
- **`spawns` is new.** It is true for every hosting except:
  - an engine compiled in (`InProc`);
  - an HTTP engine already running (a manifest's `url` attach).

  The craft adapter starts the craft's server even when compiled in. HTTP mode for a built-in engine spawns it (`HttpHost::spawn`).
- **Interpretation of the brief:** "only in-process and HTTP hostings" is taken as only in-process engines and HTTP engines that are already running. A spawned HTTP engine is a program, as `docs/shell-interface.md` 4.2 says. `engine.attach` needs no capability.
- **Who consults it:**
  - `Launcher.caps`, checked in `connect_core` and `connect_ids` before anything starts;
  - `choose_for`, `choices_for`, `Launcher::choices`;
  - `offers(caps) -> Vec<Offer { choice, why_not }>`, which lists every hosting with its reason. This is section 4.2's "listed and disabled with the reason".
- **The egui menu still shows only the startable hostings,** which on a desktop is all of them. Showing disabled items needs a field on `plumcraft_session::EngineChoice`. Adding one would break downstream struct literals (PlumImpress builds `EngineChoice { … }`), so that is left for the menu-model slice.
- **No `target_os` cfg** was added anywhere.
- **Tests** (`engines::tests`):
  - `a_desktop_offers_every_hosting_as_before`: desktop offers equal the old `choices()`.
  - `without_programs_only_in_process_and_running_http_engines_are_offered`, with `subprocess: false` and `Capabilities::NONE`:
    - the offered set is exactly `text/inproc`, `array/inproc` and a running manifest engine over HTTP;
    - each other hosting is listed with its reason;
    - the launcher and `choose_for` refuse before starting anything;
    - in-process still connects.
  - `a_platform_declares_what_it_has_and_nothing_else`: parsing, including hostile values.

## The headless commands

- **`run`, `check`, `tidy` and `nodes` now live in `plumcraft_editor_core::headless`,** which links no egui. `plumcraft-editor run …` delegates to it with the same arguments and its own `usage` text. The editor's `tests/headless.rs` integration tests pass unchanged, through the executable.
- **The `plumcraft-editor` executable still links egui,** since one binary holds both the window and the commands. The library code no longer needs a GUI, so any windowless program can call `headless::run(program, usage, args, kind, mode, extras, manifests)`. Section 4.4's "small change" row is now met at the library level.
- **`apps/plumcraft-cli` was not changed.** It has its own `run` and `nodes` with different semantics: protocol-level, a workflow inline to one engine. Switching them to the core's would change that tool's behaviour. Adding `check` and `tidy` to it is now a one-line call each, if wanted.

## `crates/ffi`

- **`Core::open` starts the engine with `Launcher::connect_core`** (with the drain `Waker`), checks the link is `Connected`, and keeps `Arc<dyn Connection>`. It used to hard-code `Host::start(plumcraft_text::engine())` and `Arc<InProc>`.
- **The store** for `plum_blob`'s fallback and `spike.publish` is now `Store::new(store_root or the commonkit default)`. That is the same tree the in-process engine publishes to, since `Launcher.store_root` is applied with `with_store` and processes get it through `Manifest.store`.
- **The config reads `docs/shell-interface.md` 4.1's fields:**
  - `engines.default` and `engines.mode` (the spike's top-level `engine` and `mode` still work);
  - `platform` (`subprocess`, `craft_apps`, `python`).
- **Without `platform`, the FFI assumes nothing** (`Capabilities::NONE`): only engines compiled in, or already running. `Config::default()` is the same.
  - This keeps `c_abi.rs`'s hostile-input case refusing `{"engine": "vectorcraft"}`, now with the reason "VectorCraft: this device has no craft apps".
  - It also makes the shells declare their platform. A Mac shell must send `platform` to get process engines.
- **`demo: true` opens the named engine's own demo** (the text engine's by default, as before). `spike.demo` does the same.
- **Unchanged:** the C ABI function list and the header.
- **New unit tests in `crates/ffi/src/core.rs`:**
  - config naming and refusals (hostile shapes included);
  - a core opening on the array engine named in its config.
- **Still two `Waker`s:** the FFI's coalescing C wake beside the core's `link::Waker`, as slice 1 noted.

## Public API effects on downstream apps

- **`plumcraft_editor::{AppConfig, PackEngine, run}`:** unchanged. `PackEngine` is now a `pub use` of the core's struct, with the same fields.
- **`plumcraft_ui::engine_view`:**
  - `Connector` is unchanged.
  - `Reloader`, `Installer`, `KNOWN_ENGINES` and `Installing` are re-exports of identical types.
  - `Installing`'s `rx` was private and still is; it gains `pub fn start` and `poll`.
  - `AttachForm` and `InstallForm` are unchanged.
- **`EditorApp`:**
  - Every public method keeps its signature: `with_engines`, `with_installer`, `with_reloader`, `with_link(plumcraft_ui::link::Link)`, `pump_engine`, `poll_install`, `clear_engines`, `add_engine`, `remove_engine`, `switch_engine`, `engine_title`, `engine_label`, `engine_short_label` and `set_catalog`.
  - **One type change:** `EditorApp.link` is now `Option<plumcraft_editor_core::link::Link>` (was the ui newtype). `.conn`, `.label`, `.state()` and `.drain()` read the same. Code that names the field's type, or assigns a ui `Link` to it, needs `.into_core()`.
- **`plumcraft-editor` no longer depends on `plumcraft-engine` and friends.** An application naming `plumcraft_engine::Engine` for `PackEngine.engine` must depend on `plumcraft-engine` itself (PlumImpress already does).
- **PlumImpress was checked against a scratch copy.**
  - Its sources were copied to `claude-debug/native-n1/s2-plumimpress-check/`, with slice 1's `.cargo/config.toml` patching every `plumcraft-*` crate to this tree.
  - `cargo check --tests` exits 0, with no source change (log `check.log` there).
  - It uses `engine_view::Connector`, `with_engines`, `with_link` and `h.state().link.as_ref().unwrap().conn`.
  - `/Users/seant/workspace/plumimpress` was not touched.

## Tests

The CI command, with `COMMONKIT_STORE_MIN_FREE_GB=2` and a fresh run folder each, logs beside them in `claude-debug/native-n1/`.

| Run | Ran | Passed | Failed | Skipped | Failing |
|---|---|---|---|---|---|
| Baseline b3 (`s2-base-b3`) | 1,306 | 1,305 | 1 | 9 | `plumcraft-craft::params_coverage hidden_params_match_the_baseline` |
| Baseline md5 (slice 1's `final-md5`, the same commit) | 1,306 | 1,305 | 1 | 9 | the same |
| Final b3 (`s2-last-b3`) | 1,314 | 1,313 | 1 | 9 | the same |
| Final md5 (`s2-last-md5`) | 1,314 | 1,313 | 1 | 9 | the same |

- **Test count.** A diff of the test names (`s2-base-b3.tests` against `s2-final2-b3.tests`) shows:
  - **Moved (2):** `plumcraft-editor headless::tests::{a_file_needing_an_engine_says_how_to_ask_for_it, run_reads_its_arguments_and_leaves_the_engines_to_the_engine_flags}`, now `plumcraft-editor-core headless::tests::…`.
  - **New in the core (6):**
    - `engines::tests::{a_desktop_offers_every_hosting_as_before, without_programs_only_in_process_and_running_http_engines_are_offered, a_platform_declares_what_it_has_and_nothing_else, an_engine_starts_publishing_where_it_is_told}`;
    - `install::tests::an_install_runs_off_the_callers_thread_and_wakes_it_when_done`;
    - `switchboard::tests::switching_adding_and_clearing_without_a_window`.
  - **New in the FFI (2):** `core::tests::{the_config_names_an_engine_and_its_hosting_on_the_platform_declared, a_core_opens_on_the_array_engine_named_in_its_config}`.
  - Nothing else differs: every engine-switching kittest in `apps/plumcraft-editor` (`engine_kittest`, `vectorcraft_kittest`, `audio_kittest`, `install_tests`) and in `crates/ui/tests` (`kittest_engine_menu`, `kittest_install`, …) passes as before.
- **Marks:** no `shm_foreign_digest` or `shm_bad_key` mark in any run (only `*.started`).
- **`cargo test` (threads, not nextest)** of the core and the FFI passes too. That matters for the thread-local manifests.
- **Gates:**
  - `cargo fmt --all --check`: clean. `cargo clippy --all-targets --all-features -- -D warnings`: clean.
  - `scripts/check-hash-gates.sh`: ok. It prints `awk: can't open file apps/plumcraft-editor/src/headless.rs` because the deletion is unstaged, as slice 1 noted.
  - The hash gate skips untracked files, so its Rust pattern was run by hand over the four new core files: no hits.
  - `scripts/check-core-toolkit-free.sh`: ok.
- **Python, as CI runs it:** `uv run pytest`, 224 passed under md5 and under b3, no bad marks. `uv run ruff check .` passes. Untouched.

## Existing test files touched

- **None** in `crates/*/tests`, `apps/*/tests` or the in-file test modules of `apps/plumcraft-editor/src/lib.rs`. A diff of that file's `mod tests { … }` to its end, before and after, is empty (`claude-debug/native-n1/s2-old-editor-tests.rs` against `s2-new-editor-tests.rs`).
  - Those tests reach the moved items through `use super::*`. `lib.rs` now imports them from the core, with `#[cfg(test)]` imports for the three only tests use: `engine_binary`, `installed_executables` and `connect_with`.
  - The `connector()` and `connect(kind, mode, Some(&ctx))` calls that take an egui context resolve to the app's `LauncherExt` trait. The core's own methods are named `connector_core` and `connect_core`, so no inherent method shadows them.
- **One test module moved with its file:** `headless.rs`'s two tests, which are unchanged apart from now living in `plumcraft-editor-core`.

## Not moved, and why

- **The Engine menu's drawing,** the Attach and Install windows, and the egui `Connector` type: drawing, egui types, and API that PlumImpress builds.
- **Demos:** `apps/plumcraft-editor/demos/audio.json` and the several-engine demos stay the app's files (see the table). The core gives each engine's own demo (`EngineKind::own_demo`).
- **`doc_apps` and `has_own_view`:** they read ui's viewer registry. Moving them needs the viewer registry as data (section 13.1, a later slice).
- **`drain_engine`, `pump_runs`, `pump_saves` and `with_link`'s first `schema` call:** the runs-off-the-UI-thread slice.
- **`apps/plumcraft-cli`'s own `run` and `nodes`:** different commands (see the headless section).
- **Engine switching in the FFI's core thread:** `engine.*` commands sent through the C ABI record a request that nothing performs. Wiring `Switchboard::pump` into the FFI worker is small, but the FFI's `Shared.conn` (used by `plum_blob`) would then have to follow switches. That belongs with the `Document` of section 4.3.
- **The Linux CI lookups ("no engine binary at …/out/plumcraft-text-engine"):** not fixed by this move.
  - `engines::engine_binary` is unchanged: beside the executable, or one folder up from `deps/`.
  - The core is now the one place a fix would go: for example, `Launcher.binaries` filled from an environment variable naming the build's `bin` folder, or a search of `CARGO_TARGET_DIR`. It was not done.

## For later slices

**Runs off the UI thread is next.** Everything in `EditorApp` (`crates/ui/src/lib.rs`) that still calls an engine synchronously on the UI thread:

| Where | Call | When |
|---|---|---|
| `pump_runs` | `conn.uploads_pending(&body)`, then `conn.call("run", body)` | Every run sent |
| `pump_runs` | `conn.call("graph.release", …)`, `conn.call("run.cancel", …)` | Outbox releases and cancels |
| `pump_saves` | `conn.uploads_pending` (the save itself is already on a thread, `plumcraft-save`) | Every save |
| `answer_prompt` (Uploads) | `conn.allow_uploads(&files)` | Trusting uploads |
| `pump_engine`, through `Switchboard` | The whole connect: process spawn and `wait_ready` (10 s; 60 s for crafts, manifests and packs), `HttpHost::spawn` or `attach`, then `conn.call("schema")`. Also `run.cancel` to the old engine in `forget_runs`, and `Coordinator::add` | Every `engine.switch`, `use`, `add`, `remove`, `reconnect`, `attach`, and the reload after an install |
| `with_link` | `conn.call("schema")` | Startup (before the window shows) |
| `drain_engine` | `link.state()` (the host's cached state) | Every frame |
| `resolver()`, `full_reader()`, used by `pump_runs`, `sync_compare`, `wire_view`, `popouts`, the media views | `conn.resolve(r)` (a store read in-process, an HTTP fetch for a remote engine) | Whenever a watched, compared, hovered or popped-out value's bytes are first read |
| `session.docs.resolver`, used by `doc.open` and `open_cmds` in the session | `conn.resolve(r)` | Open in an app; open a value |
| `Session::connection()`, used by applications' commands (PlumImpress) | Any call the application makes | Its commands |
| `editor_with` (app) | `launcher.connect` + `schema` | Startup |
| Control requests (`ui.*`, `command.run`) | Whatever the command runs, synchronously in `drain_control` | Per request |

- **The switchboard is ready to go off-thread.** It takes a connect closure and returns `Effects`, so the long part (`connect` plus `schema`) can run on a worker. The shell would then apply the link, catalogue and effects when it is woken.
- **The two-sessions audit (section 4.3) gains two rows:**
  - `engines::PACKS`: process-wide, set once; fine to share.
  - `engines::EXTERNALS`: per thread. Two documents on one core thread share it; documents on different threads each need their own `load_manifests`.
- **`Launcher.store_root` is the hook for `Dirs.store`** (section 15.5 #3): in-process engines get `with_store`, and processes get `Manifest.store` (`COMMONKIT_STORE_ROOT`).
- **A waker is built per switch operation** (an `Arc`) in the egui shell. `pump_engine` and `poll_install` return early when nothing is pending, so the per-frame cost is one check.

## Lines of code

From `git diff --numstat`, plus the untracked files.

| Area | Added | Removed | Notes |
|---|---|---|---|
| `crates/editor-core/src/engines.rs` | 805 | 0 | New. 100 of them tests |
| `crates/editor-core/src/switchboard.rs` | 507 | 0 | New. 78 of them tests |
| `crates/editor-core/src/install.rs` | 139 | 0 | New. 34 of them tests |
| `crates/editor-core/src/headless.rs` | 528 | 0 | Moved (+4 lines of docs and the `usage` parameter). 45 of them tests |
| `crates/editor-core/src/lib.rs`, `Cargo.toml` | 23 | 3 | |
| `apps/plumcraft-editor/src/headless.rs` | 0 | 524 | Moved out |
| `apps/plumcraft-editor/src/lib.rs` | 39 | 591 | |
| `apps/plumcraft-editor/Cargo.toml` | 5 | 6 | |
| `crates/ui/src/engine_view.rs` | 88 | 328 | |
| `crates/ui/src/lib.rs` | 15 | 27 | |
| `crates/ffi/src/core.rs` | 117 | 35 | 38 of the added are tests |
| `crates/ffi/Cargo.toml` | 1 | 3 | |
| `Cargo.lock` | 6 | 4 | |
| **Total** | **2,273** | **1,521** | Net +752, of which about 340 are new tests and 92 new docs in the core's three new modules |

## What the docs should say

- **`docs/STATUS.md`:** N1 slice 2 done.
  - Engine starting and switching, installing, and the windowless commands are in `plumcraft-editor-core`.
  - `Capabilities` gates the hostings.
  - The FFI starts its engine through the core's `Launcher` from `engines.default`, `engines.mode` and `platform`.
  - Test counts 1,306 → 1,314, with the same single local failure.
- **`docs/research/24-native-shells.md`:**
  - **Section 16:** add a slice 2 bullet with the above. Note the switchboard design (borrowed parts plus `Effects`), the registries' scope (packs per process, manifests per thread), and that the built-ins are compiled into the core.
  - **Section 4.4's `apps/plumcraft-editor` row:** the windowless commands are a core library module now. The `plumcraft-editor` binary still links egui, being one executable with the window.
  - **Section 4.3:** add `engines::PACKS` (process-wide, set once) and `EXTERNALS` (per thread) to the process-wide state list.
  - **Section 6, Capabilities paragraph:** only `subprocess`, `craft_apps` and `python` exist in code so far. "Listed and disabled" is available as `engines::offers(caps)`, but the Engine menu still lists only what can start, because `EngineChoice` has no disabled field yet.
- **`docs/shell-interface.md`:**
  - **Section 4.1:** the FFI now reads `engines.default`, `engines.mode` and `platform`. A config without `platform` declares nothing (no programs, no Python, no craft apps). Say so in the field table. `engines.manifests` is not read yet.
  - **Section 4.2:**
    - `subprocess` false also refuses spawned HTTP engines (all built-in engines' HTTP mode) and the craft adapter even in-process. Only compiled-in engines, running HTTP engines (manifest `url`) and `engine.attach` remain.
    - When several capabilities are missing, the reason given is the most specific one: craft apps, then Python, then programs.
    - The reasons are worded "this device can't start programs", "this device has no Python" and "‹Craft›: this device has no craft apps".
  - **Section 5.1:** the sketch gains what exists: `engines::Launcher` and `switchboard::Switchboard` with a per-call connect.
  - **Appendix A, the `engine_view.rs` row:** Connector, the menus and the two windows stay in the shell. Reloader, Installer, the install job, `pump_engine` (the switch state machine) and the `ui.inspect` engine block are in the core.
