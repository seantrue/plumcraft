---
title: "N3 budget stage: engines by graph, the 130 ms reading, and the budgets at 100 and 500 nodes"
date: 2026-11-20
---

# N3 budget stage: engines by graph, the 130 ms reading, and the budgets at 100 and 500 nodes

Branch `native/n3-budgets`, cut from `origin/feat/native-n0`. Nothing is committed; the controller's edit to `docs/research/24-native-shells.md` ("Decisions at N3's exit") is kept. `AI.md` and `docs/BUGS.md` were not touched.

## Summary

| Item | State |
|---|---|
| 1. Start only the engines a graph uses (decision 8C) | Built in the core (session, editor core, C ABI), used by both apps; contract and tests in the same change. Conformance 12 of 12 on each half on both apps, no skips, no flag forcing engines |
| 2. The Mac's 130 to 170 ms reading at 2,000 nodes | Not a regression: the same build and config read 21 to 23 ms when the window is on screen; it reads 115 to 162 ms when the window can't be seen (the screen locked, or covered) or the process is held at background QoS. The bench now records whether its window was visible. 500 nodes did not regress (5.4 to 5.6 ms) |
| 3. Re-measure at 100 and 500 nodes | Mac: launch, one-node change and hitches, three or more runs each with the N0 spike as the control, in two sessions: one loaded (load to 17) whose screen locked at 15:06, one with the screen on while the maintainer worked (below). iPad: **not measured**: the iPad went "unavailable" (asleep or off the network) before the first run; the build from this branch is made and signed, not installed. One-command recipe below |
| 4. N3's gate against 100 and 500 nodes | Restated below; note 24 section 18 ("After the budget stage") and `docs/STATUS.md` updated |

Also found and fixed on the way: the hitch reader dropped every phase when Instruments' export put a table's schema and its rows in separate nodes. That is the cause of slice 5's 2,000-node device trace "with no phases", and of three Mac spike traces this afternoon showing none. Fixed in `claude-debug/native-n3/s6-hitches.py`, which both bench scripts now use.

## 1. Start only the engines a graph uses

**The rule as built.** It is the egui editor's own rule (Pick Engines Automatically, `engine.auto`, `engine_cmds::start_needed` after `graph.open` and `graph.insert`), extended in the core so it works for files that don't name their engines, and used by the C ABI for every document. No rule lives in Swift.

- **Which engines a graph uses** (`plumcraft_session::engine_cmds::engines_used`, `graph_engines`): a pinned node uses its engine; a kind exactly one engine offers uses that one; a kind several offer uses one already used, else the first the workflow's `engines` names, else the host's own engine; a kind no known engine offers uses the engines the workflow names (as `missing_engines` always read them).
- **Which engine offers a kind** comes from the file, and for engines compiled in (text, array, an application's packs) from their own catalogues, read once per process without starting them (`plumcraft_editor_core::engines::offered`, held as `Engines::offered`). A craft, a manifest's engine or the audio engine is a process, so only a file that names it says it is used.
- **`Session::missing_engines`** now also names the engine the host knows offers a missing kind, so `start_needed` starts it. The egui editor gets the index too (`EditorApp::with_engines`); its own default (picking off unless the preference is on) is unchanged.
- **The C ABI** (`crates/ffi/src/core.rs`): a new config field `engines.auto` (default true; anything but a boolean is the config's error). With it, `document.open {path}` reads the file first (`peek_workflow`, at most 64 MiB, a workflow read whole) and starts the engines it uses, `engines.default` first when it is one of them, else the first used; a graph that uses none, or a file that isn't a plain workflow (a bundle, an application's own file), opens on `engines.default`. `engines.also` still starts its engines in every document. The document picks automatically from then on (`engine.auto` on). False keeps the old way.
- **A node of another engine's kind added later** (the path N4's palette will take): `node.add` or `node.addWired` through `command.run` of a kind the catalogue lacks and a startable engine offers asks for that engine (once, so one that failed isn't retried on every add) and the request waits for it, at most 30 s, then runs; later requests for the document wait behind it in order. A kind nothing offers is refused at once, as before. The session's own `node.add` (the egui editor, which doesn't hold requests) refuses it naming the engine: "array.mandelbrot is the array engine's, which isn't connected; it is starting: add the node again once it has connected" (picking on), "...; the Engine menu starts it" (off).
- **Engines stay once started:** a graph that stops using one keeps it until the document closes or `engine.clear`.
- **Both apps** now open their core with no `engines.also` and picking on (`ShellConfig.pickEngines`). `--engines-also array` and `--engines-pick off` remain, for comparisons.

**A visible change in both apps' Engine menu:** with picking on, the core's Engine menu lists one item per engine ("Text", "Array", each hosted the fastest way the platform can, a craft or the audio engine disabled with the reason), as the egui editor's does with Pick Engines Automatically, instead of one submenu of hostings per engine. Turning picking off in the menu brings the hostings back.

**What it saves.** At 2,000 nodes launch to first frame is 0.88 s with the text engine alone, against 1.17 s with both (this afternoon, same build); at 500 it is 0.28 s, slice 4's "text alone" figure, against slice 4's 0.78 s with both. A Showcase-like graph (text and array nodes, no engines named) starts both, as before.

**Tests (all pass):**

| Test | What it checks |
|---|---|
| `crates/ffi/tests/engines_used.rs`, `a_document_starts_only_the_engines_its_graph_uses` | Text-only file: text alone. Text and array, no engines named: both, the graph whole (0 problems). Array only: array alone. Untitled: the config's engine |
| `adding_a_node_of_another_engines_kind_starts_that_engine_and_waits_for_it` | On a text-only document `node.add array.mandelbrot` starts the array engine, waits, adds; the next array kind is immediate; an unknown kind is refused at once |
| `engines_auto_off_keeps_the_configs_engines_in_every_document` | `auto: false`: text only and the add refused naming "array engine"; `also` still starts its engine; hostile `auto` values are config errors |
| `crates/session/src/engine_cmds.rs`, `the_engines_a_graph_uses_come_from_pins_the_file_and_what_engines_offer` | The rule's cases: single, shared kinds, pins, the file's order, crafts named only by the file, nothing |
| `a_missing_kind_an_engine_offers_names_that_engine_and_picking_starts_it` | `missing_engines` names the engine; `engine_for_kind`; `start_for_kind` off and on, once only; an engine the host can't start isn't named |
| `crates/ffi/tests/menus_log.rs`, `a_platform_that_cant_start_programs_lists_process_engines_disabled_with_why` | Changed: hostings with `auto: false` (as before); with picking on, one item an engine, the audio engine disabled with "this device can't start programs" |
| Kit, `ShellTests.aDocumentStartsOnlyTheEnginesItsGraphUses` | The same through `ShellCore` as the apps open it, and `engines.auto` in the config's JSON |

**Conformance, both apps, no override:** each app opens an empty graph on the text engine alone; the picture case's first `node.add array.mandelbrot` starts the array engine by the rule above and waits for it. No test-only flag was needed.

| Run | Passed | Skipped | Log |
|---|---|---|---|
| Mac app, read half | 12 of 12 | 0 | `claude-debug/native-n3/s6-mac-conformance-read.txt` |
| Mac app, edit half | 12 of 12 | 0 | `s6-mac-conformance-edit.txt` |
| iPad app (simulator), read half | 12 of 12 | 0 | `s6-pad-conformance-read.txt` |
| iPad app (simulator), edit half | 12 of 12 | 0 | `s6-pad-conformance-edit.txt` |

## 2. The Mac's 130 to 170 ms reading

**Not a regression.** The bench's one-node code is unchanged since slice 4 (it moved into the kit as it was), and the reading follows whether the system is showing and scheduling the app, not the code:

| Run (2,000 nodes, 3,200 wires) | Engines | Window | Kit p50 ms | Feed p50 ms |
|---|---|---|---|---|
| Slice 4 | both | | 35.5 | 36.1 |
| Slice 5 (load 8 to 11) | both | not recorded | 130.6, 168 | 131.5 |
| This stage, 14:38, three runs | both (`--engines-pick off --engines-also array`, slice 5's config) | visible | 22.2, 22.6, 22.8 | 33.4, 34.2, 34.6 |
| The same, run from a background shell | both | visible | 22.1 | 34.3 |
| The same under `taskpolicy -c utility`, two runs | both | visible | 23.2, 21.8 | 42.9, 43.4 |
| The same under `taskpolicy -c background` | both | visible | **145.6** | **124.5** |
| This stage, 15:17 to 15:19, three runs, the screen locked since 15:06 | text alone | **not visible** | **131.6, 125.0, 115.2** | **109.1, 107.4, 119.2** |
| This stage, 15:34 to 15:36, three runs, the screen unlocked, the maintainer using the Mac | text alone | covered at the end of each run | 21.5, **161.7**, 20.9 | 32.6, **140.7**, 22.5 |

So the 130 ms is what the bench reads when its app is demoted: its window off screen (here the lock screen, and in one of three later runs, most likely the maintainer's windows over it), or its threads held at background QoS. With the window on screen the same build reads 21 to 23 ms, faster than slice 4's 35 ms because of slice 5's validation fix. A bisect was not needed, since the current build doesn't show it. I did not trace which of the system's throttles applies (the window's display cycle, or the threads' QoS): both reproduce it. At 500 nodes the lock changes nothing (5.4 ms locked, 5.5 to 5.6 ms visible), and 500 did not regress (slice 4 6.2, slice 5 5.7).

**What changed so a run says whether it can be trusted:** the Mac bench writes `app_active` and `window_visible` into its JSON; `s6-quiet.sh` waits for an unlocked screen as well as a load under 3. Every run this afternoon had `app_active` false (the maintainer's own window was in front), which does not by itself slow the reading. `window_visible` is read once, at the end of a run, so a window covered for part of the run can show either.

## 3. The budgets at 100 and 500 nodes

### The Mac (M4 Max, 120 Hz)

**The machine, two sessions.** The maintainer's PlumCraft (PID 5861) ran throughout both, and every run had `app_active` false (another app in front).

- **First session, 14:50 to 15:19** (`s6-mac-machine.txt`): the load was 3.0 at the start (14:37: 2.8, `mediaanalysisd` 21 %) and rose: 13.9 at 14:53 (`mediaanalysisd` 228 %, `biomesyncd` 83 %), 17.0 at 15:00 (`mediaanalysisd` 285 %), 7 to 9 from 15:04 (`spotlightknowledged` 65 to 106 %). **The screen locked at 15:06:09**, after which a window shows nothing: a 500-node app trace and two spike traces recorded no frames at all (zero hitches, which is not a pass) and are left out, and the scripted runs (15:14 to 15:19) ran with the window not visible.
- **Second session, 15:31 to 15:46** (`s6-mac2-machine.txt`), once the screen was unlocked (the quiet wait waited 121 s): load 2.9 at the start, 3.0 to 3.4 during the scripted runs, 4 to 5.5 during the traces (WindowServer 10 to 25 %, Safari, `backupd` 8 %: the maintainer at work). Three scripted runs at each size, two trace pairs at 500.

**Launch and the one-node change** (`bench.sh run`, the second session's three runs each with the screen on, median and spread; the first session's locked runs in brackets; release build in `claude-debug/native-n3/s6-build/mac`; text engine alone, as the rule gives a text graph):

| Row | 100 nodes | 500 nodes | 2,000 (information) | Budget |
|---|---|---|---|---|
| Launch to first frame | 166 ms (163 to 173) [159] | 292 ms (277 to 297) [277] | 885 ms (884 to 924) [881] | under 1 s |
| `document.open` to first frame | 46 ms (45 to 47) [44] | 164 ms (161 to 171) [162] | 780 ms (765 to 789) [764] | |
| One-node change, kit, p50 | 2.6 ms (2.4 to 2.7) [2.4] | 5.6 ms (5.5 to 5.7) [5.4] | 21.5 ms (20.9 to 161.7) [125] | 16 ms |
| One-node change, kit, p95 | 6.5 ms (6.1 to 6.5) [3.4] | 8.2 ms (7.2 to 17.1) [8.0] | 25.2 ms (22.7 to 190.0) | |
| One-node change, feed, p50 | 18.9 ms (18.4 to 19.4) [20.8] | 17.8 ms (17.7 to 18.5) [19.5] | 32.6 ms (22.5 to 140.7) [109] | 16 ms |
| Footprint at the end | 205 MB | 428 MB | 785 MB | |

**Can these be trusted?** At 100 and 500 nodes, yes: the two sessions, locked and unlocked, and the traced runs (kit 2.34 to 2.39 and 5.43 to 5.58 ms) agree within a few tenths of a millisecond, so the screen and the load didn't move these rows. Launch is not under the tracer (which adds about 0.8 s to launch). At 2,000 nodes one run of three was throttled (161.7 ms), as section 2 explains.

The feed path is about 20 ms at any size: it waits for the core's next look at the scene (the 16 ms throttle, `SCENE_EVERY`), where the kit's path fetches the scene as soon as its command is answered.

**Hitches** (Instruments' Animation Hitches, ms of late frames a second by phase, median and spread; the app and the spike's perWire bench alternated in the same minutes; only runs whose window was on screen: at 100 nodes the first session's three app and two spike runs; at 500 two app runs and one spike run from the first session and two of each from the second):

| Phase | App, 100 (3 runs) | Spike, 100 (2 runs) | App, 500 (4 runs) | Spike, 500 (3 runs) | Budget |
|---|---|---|---|---|---|
| idle | 0.0 (0.0 to 5.6) | 0.0, 0.0 | 8.4 (5.6 to 16.7) | 0.0 (0.0 to 0.0) | |
| pan | 4.2 (2.1 to 4.2) | 4.2, 0.0 | **132.3 (93.8 to 135.6)** | 12.5 (8.3 to 20.8) | under 5 good, over 10 poor |
| pan at fit | 0.0 (0.0 to 2.1) | 2.1, 6.3 | **160.5 (4.2 to 447.3)** | 0.0 (0.0 to 4.2) | |
| zoom | 19.5 (13.9 to 25.0) | 31.9, 23.6 | **79.1 (16.7 to 242.3)** | 15.3 (6.9 to 19.4) | |
| drag | 4.2 (2.1 to 4.2) | 14.6, 0.0 | **51.1 (25.0 to 77.0)** | 16.7 (10.4 to 41.7) | |
| longest hitch | 8.3 ms | 8.3 ms | 29.2 ms | 16.7 ms | |

Files: `s6-r{1,2,3}-trace-{100,500}-hitches.md`, `s6-v{1,2}-trace-500-hitches.md`, `s6-spike-trace-{100,500}-r{1,2,3}-hitches.md`, `s6-spike-trace-500-rv{1,2}-hitches.md`. Left out: spike 100 run 2 (its phases were lost to the reader's defect before its exports were kept), and the first session's app 500 run 3 and spike 500 runs 2 and 3 (screen locked, no frames). Instruments' reasons for the app's hitches at 500: "Potentially expensive render" and, in the second session, "Potentially expensive app update(s)".

**Can these be trusted?** At 100 nodes, yes as a reading of this machine: three runs agree, the screen was on, the control reads the same or worse (zoom 19.5 against the spike's 24 to 32), though run 2 was under a load of 14. At 500 nodes the direction can be trusted, the size only roughly: four app runs at loads from 4 to 17 agree that pan (94 to 136) and drag (25 to 77) miss by far, while zoom and pan at fit swing tenfold between runs (pan at fit 4 in two runs, 315 and 447 in the two runs while the maintainer was working); the spike beside them in the same minutes stayed between 0 and 42 in every phase. So the app's canvas misses at 500 nodes by more than this machine's state explains. Slice 4's best 500-node run read pan 31, zoom 28, drag 4.

**What a person would notice at 500 nodes on this Mac:** about 130 ms of late frames a second while panning is about 16 of every 120 frames shown a frame late (each 8.3 ms late, the longest 16.7 ms): a fast pan judders slightly, about every seventh frame, visible on a 120 Hz display to someone watching for it; nothing stalls. Dragging a face (25 to 77 ms a second) shows a late frame three to nine times a second. At the worst pan-at-fit run (447 ms a second, longest 29 ms) the whole graph moving at fit would look visibly rough, about half the frames late. No remedy was built (as decided).

### The iPad

**Not measured.** At 14:45 the iPad (UDID `00008132-001424640C05801C`) was "available (paired)" over the local network; by 15:21, when the first run tried to install, it was "unavailable" and `devicectl` refused ("The device is not able to fulfill the requested usage assertion requirements", CoreDeviceError 4016), so nothing was installed or launched and the display link's rate was not read. Low Power Mode's state is unknown. The app was built from this branch for the device (release, signed with the existing "Apple Development" identity and the team's profile, automatic signing without provisioning updates, nothing requested from Apple): `native/apple/PlumCraftPad/build-device/Release-iphoneos/PlumCraftPad.app`, and a fresh build with zero warnings (`s6-pad-clean-build-device.log`; its folder removed afterwards). The installed `com.plumcraft.pad` on the iPad is still slice 5's build.

**The one-command recipe** (with the iPad unlocked, awake, Low Power Mode off):

```sh
claude-debug/native-n3/s6-ipad.sh
```

It builds and installs this checkout's app, runs the 500-node bench once and reads the display link's rate from its pan phase; at 60 Hz it stops and says Low Power Mode (or Settings, Accessibility, Motion, Limit Frame Rate) is still on. Otherwise it runs three launches each at 100 and 500 nodes and a hitch trace of the app and of the N0 spike (`com.plumcraft.spike.canvas`, already installed) at each size. Every bench quits by itself; `bench.sh`'s cleanup quits `PlumCraftPad` or `CanvasSpikePad` on the device by PID. Results: `claude-debug/native-n3/s6-pad-device-{100,500}-r{1,2,3}.json`, `s6-pad-device-trace-{100,500}-hitches.md`, `s6-spike-device-trace-{100,500}-hitches.md`; `python3 claude-debug/native-n3/s6-summarize.py pad` tables them.

**The trace fixes for the device** (`native/apple/PlumCraftPad/bench.sh`):

- **No phases** (slice 5's 2,000-node trace): the reader's defect above, found on the Mac this afternoon in three spike traces whose exports put the `os-signpost` schema in one node and its 28 rows in the next. The same reader read the device trace, and its symptom was the same (214 hitches, none in a phase). `s6-hitches.py` reads every table and gives schema-less rows the schema before them; it read those three exports' phases correctly. Not yet seen on a device trace.
- **The spike's control trace that failed to save:** its output went to `/dev/null` and the trace was deleted, so slice 5 couldn't say why; its table file was empty, which is what the old reader gives when the export has no table. `bench.sh` now keeps xctrace's output (`<name>-xctrace.log`) and both exports beside the table, has a `trace-spike-device` mode for the control, and records for up to 120 s. The cause itself is not established.

### Slice 5's device numbers, for reference

Slice 5 at 500 nodes, both engines, 60 Hz (Low Power Mode on): launch 0.49 s, one-node change 12.5 ms (kit) and 17.7 ms (feed), hitches pan 137, pan at fit 341, zoom 117, drag 30 ms a second. With the text engine alone a 500-node launch should be shorter (on the Mac the second engine cost 0.5 s at 500), not measured.

## 4. N3's gate, at 100 and 500 nodes

| Part | Mac | iPad |
|---|---|---|
| Read half of the suite, no skips | **Met:** 12 of 12 (edit half 12 of 12), the array engine started by the rule | **Met** in the simulator, 12 of 12 (edit 12 of 12); not on the device (no control port there) |
| Launch to a usable window, under 1 s | **Met:** 0.17 s (100), 0.29 s (500) | **Not measured** this stage (slice 5: 0.49 s at 500 with both engines) |
| A command to its scene change on screen, 16 ms | **Met through the kit:** 2.6 and 5.6 ms p50 (p95 6.5 and 8.2); not through the feed alone (18 to 19 ms at 100 and 500) | **Not measured** (slice 5 at 500: 12.5 ms kit, met; 17.7 feed) |
| Every frame of pan, zoom, drag at 120 Hz (under 5 ms of hitches a second) | **100: met for pan, pan at fit and drag** (4.2, 0.0, 4.2); zoom 19.5, with the spike control at 24 to 32 on the same machine. **500: not met**: pan 132 (94 to 136), pan at fit 161 (4 to 447), zoom 79 (17 to 242), drag 51 (25 to 77), medians of four runs, the spike control 0 to 21 | **Not measured**; 120 Hz on an iPad still unmeasured |
| Run progress, one change a frame | Coalesced in the core (unchanged) | The same |
| Main-thread stall over 100 ms | None seen | Not traced |

**So:** on the Mac the gate is met at 100 nodes (zoom aside, where the control is worse) and missed at 500 on the canvas's frame budget, by about 25 times the budget while panning (132 against 5 ms a second), with the N0 spike in the same minutes at 12.5. The iPad's device budgets are unmeasured this stage; the recipe above takes them.

## Every Rust, kit, C ABI and contract change

**Rust:**

- `crates/session/src/engine_cmds.rs`: `Engines::offered` (`Offered`, kinds by engine); `engines_used` and `graph_engines` (the rule); `engine_for_kind` and `start_for_kind`; two tests.
- `crates/session/src/lib.rs`: `missing_engines` also names the engine the host knows offers a missing kind; docs of `MissingEngines::engines`.
- `crates/session/src/graph_cmds.rs`: `node.add` and `node.addWired` refuse a kind only an engine not connected offers, naming it, and ask for it with picking on (`not_started`).
- `crates/editor-core/src/engines.rs`: `offered(choices)` from `compiled_kinds()`, the compiled-in engines' catalogues read once per process.
- `crates/ui/src/engine_view.rs`: the egui editor sets `Engines::offered` with its choices.
- `crates/ffi/src/core.rs`: `Config::auto` (`engines.auto`), `Worker::auto`; `Worker::engines_for` and `peek_workflow` (a document's engines from its file); `Doc::waiting`, `added_kind`, `waits_for_engine`, `release` (a node add waits for its engine; `hold_or` keeps order behind it; `ENGINE_WAIT` 30 s).
- Tests: `crates/ffi/tests/engines_used.rs` (new, 3), `crates/ffi/tests/menus_log.rs` (one test now covers both menu forms).

**C ABI:** no export, struct or `PLUM_ABI_VERSION` change. Behaviour: the config's new `engines.auto` (default true) changes which engines a document starts, a `node.add` request can now wait up to 30 s for an engine, and the Engine menu model lists engines rather than hostings while picking is on.

**Kit (`native/apple/PlumCraftFFI`):** `ShellConfig.pickEngines` (`engines.auto`); test `aDocumentStartsOnlyTheEnginesItsGraphUses` and two expectations in `theConfigIsTheContractsShape`.

**Apps and scripts:** both apps' `also` defaults to none and `--engines-pick on|off` is new (`AppMain.swift`); the Mac bench writes `app_active` and `window_visible`; `PlumCraftMac/bench.sh` and `conformance.sh` take `DERIVED` (a build folder other than `build/`, so a running copy is left alone) and `bench.sh` takes `WRAP` (a command to run the app under) and keeps its trace exports; `PlumCraftPad/bench.sh` keeps xctrace's output and exports, has `trace-spike-device`, quits the spike too, and reads with `s6-hitches.py`; both `conformance.sh` no longer pass `--engines-also`.

**Contract:** `docs/shell-interface.md` 4.1, "As built (the N3 budget stage): `engines.auto`", and the slice 4 and 5 notes brought into line. `docs/control-protocol.md` and `schema/shell1.json` don't change (no message changed). Ledger: note 24 section 18 gains "After the budget stage"; `docs/STATUS.md` an entry.

## Tests

| Suite | Result |
|---|---|
| `cargo fmt --all --check` | Clean |
| `cargo clippy --all-targets --all-features -- -D warnings` | Clean |
| `cargo nextest run --all-features --no-fail-fast` (b3, scratch) | **1,472 of 1,473 passed**, 17 skipped; the one failure is the known `plumcraft-craft::params_coverage hidden_params_match_the_baseline`. Five new tests (`s6-gate-nextest.log`). A first run failed `menus_log a_platform_that_cant_start_programs_…` (below), fixed |
| `scripts/check-hash-gates.sh`, `scripts/check-core-toolkit-free.sh` | ok, ok |
| `native/apple/build-xcframework.sh` | Built, all slices (`s6-xcframework.log`) |
| `swift build`, `swift test` (scratch) | Zero warnings; **65 tests passed** (64 before, one new) (`s6-swift-test.log`) |
| Mac app, clean release build | Zero warnings (`s6-mac-clean-build.log`; the clean builds' folders under `s6-build/` were removed afterwards, `s6-build/mac`, the bench build, is kept) |
| iPad app, clean release build, simulator and device (arm64) | Zero warnings (`s6-pad-clean-build-sim.log`, `s6-pad-clean-build-device.log`; the device log's one error is Xcode refusing to "clean" a folder it hadn't made, before a fresh build that succeeded) |
| Conformance | Section 1's table |

Python (`python/`) was not run: nothing there changed.

## The maintainer's store, preferences, simulator

- **The store** (`/Volumes/CommonkitStore/store`, top two levels, names, sizes and mtimes, `s5-store-list.sh`): I took no listing at the very start of this stage. Compared with slice 5's last listing (13:32, `s5-store-after.txt`): **no difference** at 15:24 (`s6-store-mid.txt`) and at the end (`s6-store-after.txt`).
- **Preferences and trusted files** of the editor, the research software's app and PlumPipe: the same SHA-256 as at slice 5's end, at 15:24 and at the end (`s6-prefs-mid.txt`, `s6-prefs-after.txt`).
- Every launch, test and script ran with the store, marks, test run, craft root and preferences in scratch (`s4-env.sh`, or the scripts' own), `COMMONKIT_HASH=b3`, `COMMONKIT_STORE_MIN_FREE_GB=2`. `claude-debug/native-n3/s6-try/` was not touched.
- **The simulator** (iPad Pro 13-inch (M5)): booted as found; `com.plumcraft.pad` was installed by `conformance.sh` and uninstalled at the end, leaving 40 apps as before.

## Final process check

```
$ pgrep -lx PlumCraft
5861 PlumCraft
$ pgrep -lx PlumCraftPad
$ pgrep -lx CanvasSpike
$ pgrep -lx CanvasSpikeMac
81062 CanvasSpikeMac
81859 CanvasSpikeMac
82854 CanvasSpikeMac
84007 CanvasSpikeMac
$ pgrep -lx xctrace
```

PID 5861 (`PlumCraft`, the maintainer's) was never signalled; none of the listed processes is mine. The first listing also showed two of mine, stopped tracer copies (PIDs 20852 and 22824, state T, started 15:36 and 15:40), which I quit by PID: see defect 5. Four stopped `CanvasSpikeMac` copies (PIDs 81062, 81859, 82854, 84007, state T, started 10:58 to 11:10 today, before this stage) are left by earlier slices' spike traces (`s3-spike-trace.sh` doesn't clear them, and slice 5's check looked for `CanvasSpike`, which doesn't match `CanvasSpikeMac`); not mine, not touched. Every copy the tracer left this stage (fourteen, app and spike, each stopped before it ran) was cleared by exact name and binary path as it happened.

## Defects I introduced and caught

1. **The Engine menu test failed** with picking on by default: the core's menu lists engines, not hostings, when engines are picked automatically, which the test (and the apps' menu) assumed. Caught by the gate's first nextest run. The test now covers both forms; the behaviour change is reported above.
2. **A slice 5 reader defect, not mine, found here:** phases dropped when an export splits schema and rows. Fixed in a copy (`s6-hitches.py`).
3. **`bench.sh`'s `WRAP` under macOS's bash 3.2** would have failed on an empty array with `set -u`; caught before use (`${wrap[@]+...}`).
4. **The first trace session's spike script** still used slice 5's reader for the first five runs; its results were re-read from the kept exports where they existed (500-node runs), and spike 100 run 2's phases are lost.

5. **Two stopped tracer copies outlived the second session's traces.** xctrace's extra launch resolved to the newest `PlumCraft.app` the system knew, the clean build's in `s6-build/mac-clean`, not the bench's own binary, so `bench.sh`'s cleanup (which matched this build's path only) left them. Caught by the final listing; quit by PID. `bench.sh` now also clears a copy that is stopped and runs any build inside this checkout (a copy someone starts meanwhile runs, so isn't stopped). The second session's two traced runs may have run that clean build's binary: the same sources.

## Disk

Free space went from about 83 GB at the start to 37 GB at the end. Most of it is not this stage's scratch: `target/` is 40 GB (the four xcframework triples and the test builds), three Instruments kernel traces of 12 GB in `$TMPDIR` are from slice 5's device traces at 13:22 to 13:31 (`instruments*.ktrace`, left as they are), and Time Machine took three local snapshots during the afternoon. This stage's scratch: `claude-debug/native-n3/s6-build/mac` (0.5 GB, the bench build, kept); the clean builds' folders and the two gate runs' scratch were removed.

## What I did not verify

- **Everything on the iPad device** this stage: the display link's rate, Low Power Mode, launch, the one-node change, hitches with phases, the spike control saving. The recipe is ready.
- **The Mac's 500-node hitches on a quiet machine:** the first session's load reached 17, and the second ran while the maintainer worked; four valid app runs and three valid controls. Whether the app's window was covered during a trace is known only at its end (`window_visible`).
- Which throttle (display cycle or QoS) makes a hidden window's 2,000-node change slow; why the app pans ten times worse than the spike at 500 (not looked into, as decided).
- **Linux:** the Rust changes ran on the Mac only; the controller runs the gate on the Linux test host.
