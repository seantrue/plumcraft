---
title: "N3 slice 1: moves as undo steps, the dropped wire's palette, the suite's halves"
date: 2026-11-20
---

# N3 slice 1: moves as undo steps, the dropped wire's palette, the suite's halves

Branch `native/n3-start`, cut from `origin/feat/native-n0`. Nothing is committed; the changes are in the working tree for review. The controller's two edits (`docs/research/24-native-shells.md` section 17, `Cargo.lock`) are kept; section 18 was added to the note.

## Summary

All five items were done.

1. **A move is one undo step, in the core, for every shell** (decision 3). Done, including egui's own drags, without forking egui-snarl.
2. **A new wire dropped on empty canvas opens the palette for it** (decision 8, question 8A). Done in the core, on the control port, the C ABI and egui (which originates it from egui-snarl's own hook).
3. **The conformance suite has a read-only and an editing half.** Done: 24 cases, 300 steps; 12 read-only. New read-only cases run a graph and watch text, scalar and picture values.
4. **The 9% regression** was profiled and fixed: the hot calls' snapshot is 40% cheaper at 2,000 nodes.
5. **The N0 Swift wrapper** builds against today's header and its 6 tests pass, unchanged.

**The gate passed, except the known local failure.** nextest (b3, CI's environment) ran 1,456 tests and passed 1,455 (twice: before and after the `mapped_nodes` fix). The one failure is `plumcraft-craft::params_coverage hidden_params_match_the_baseline`. Before this slice it was 1,445 of 1,446: the slice adds 10 tests. Clippy, fmt, both scripts, ruff and pytest (224 of 224) are clean. No test failed intermittently.

## 1. Moves are undo steps

**The rule** (`Session::edit_move`, `crates/session/src/lib.rs`; contract 12.6):

- `node.move`, `group.move`, `frame.move`, `frame.resize` and a new session command `edit.move {nodes?: {id: [x, y]}, groups?: {id: [x, y]}, drag?}` each make one undo step.
- Labels: "Move Node", "Move 3 Nodes", "Move Group", "Move 2 Groups", "Move 4 Items" (mixed), "Move Frame", "Resize Frame". Existing labels have no counts ("Paste Nodes"); the brief's "Move 3 Nodes" was followed.
- A move **joins** the step on top of the stack when:
  - that step was made by a move of exactly the same things;
  - nothing has made, undone or redone a step since (each step now has a serial; the mark is valid only while its step is on top and redo is empty);
  - and either both name the same `drag` (a string of 1 to 64 characters or a whole number), or neither names one and the last joining move was at most 1 s ago (`MOVE_COALESCE`).
- A move to where something already is changes nothing: no step, and the document doesn't become dirty (it did before).
- Moves are still **not journaled** (a drag would fill the journal), and make nothing stale.

**Who names what:**

- `ui.gesture`'s face drag sends one `edit.move` naming `gesture:<serial>`; frame, resize and open-group drags name it too.
- egui sends one `edit.move` (or `frame.move`, `frame.resize`, `group.move`) a frame while it drags, naming `egui:<press>` (a counter per primary press). So **egui's own drags are one undo step each**, at the point where it writes snarl's positions back; egui-snarl is not forked.
- Arrow keys: new canvas command `selection.nudge {by, drag?}`. egui binds the arrows (1 unit, 10 with Shift) when something is selected and nothing has the keyboard, and names each press `key:<n>`; repeats join it, so a held key is one step and two taps are two. A shell that can't tell repeats sends no `drag`, and the one-second rule holds a held key together.

**Tidy, Align and Distribute** were already undo steps ("Tidy", "Align", "Distribute") and are unchanged; a move after one is a step of its own.

**Found while doing it: drag serials restarted at 1.** `begin` took the serial from the drag under way, of which there is none once a drag ends, so every drag had serial 1 (contract said they rise). Fixed (`DragCell` keeps the highest); a unit test and a conformance case check it. It mattered here because a drag's moves are named by it.

## 2. A new wire dropped on empty canvas

Contract 12.7. In the core:

- **The palette is the core's state** (question 4A): `Document::palette`, shown in the scene as `view.palette {at, query?, wire?}` (left out while closed, so the shared fixtures are unchanged).
- `canvas.palette {open?, at?, query?, wire?}`: `wire` is `{from: {node, port}}` (the output the wire leaves) or `{to: {node, port}}` (the input it enters), checked against the graph. Reply adds `wire` and `offered`. It is no longer a shell-only command, so the C ABI serves it.
- `palette.items {query?, limit?}` (a control method): the kinds offered, in category order, each with the `port` a pick connects. The fit is `wire.connect`'s (types, list-ness, wireability, named ports) for a node not yet made (`Graph::input_for_new`, `Graph::output_for_new`). Core's Convert node is neither offered nor looked through.
- `palette.choose {kind, port?, at?}` (a canvas command): with a wire, `node.addWired`; without, `node.add`. The palette closes. A kind that doesn't take the wire is refused and the palette stays.
- `node.addWired {kind, pos?, from | to, port?}` (a session command, journaled): the node and the wire in **one undo step**, "Add and Connect". Question 8 named it.
- `ui.gesture`: a new wire's drag ending on nothing (having moved) runs `canvas.palette` with the wire at the drop point.

**egui:**

- It **originates** the drop: egui-snarl calls `SnarlViewer::has_dropped_wire_menu` when a new wire is let go on empty canvas; the canvas records the pin there, returns false (so snarl shows no menu), and runs `canvas.palette {wire}` at the pointer.
- It **honours** the core's state: its palette shows only the kinds `palette::fitting` offers ("No node takes this wire." when none).
- Its palette's pick is now `palette.choose`, and Escape or a click away is `canvas.palette {open: false}`, so the core's state stays in step.
- A kittest drags a real wire to empty canvas, checks the filter, Escapes, drags again, picks with Enter, and undoes once.

## 3. The suite's two halves

**The line** (written in contract 16.1): a case is read-only when nothing it asks of the shell after its setup changes the graph. Setup builds the graph with `command.run`. After that a read-only case only hit-tests, locates, taps, double-taps and long-presses (select, watch, open windows, menus, the palette), marquees, fits, reads the scene and theme, runs, watches, and sends refused hostile input. Every case that moves, wires, reroutes, routes, resizes, adds from the palette, nudges, undoes or redoes is editing; so is a cancelled drag.

**Runner changes** (`crates/editor-core/src/conformance.rs`):

- every case has `half`; `run_half(driver, Half::Read)`;
- `needs: [capability]` on a case (`engine:text`, `engine:array`, `media`, `reports`), `only` on a step takes the same; `Driver::has`; skipped cases are reported as skipped, never as passed;
- `until` and `timeout` on a call: sent again until it matches (`Driver::pause` between: a wake on the C ABI, a frame in egui), a condition with a deadline;
- matchers `$find` and `$min`;
- after each case the palette is closed as well as any drag.

**Cases:** 24 (19 before), 300 steps (197). Read: 12 cases, 141 steps. Edit: 12.

| Case | Half | New or changed |
|---|---|---|
| A drag moves an unselected face alone | edit | changed: one `edit.move`, undo label "Move Node", one undo puts it back |
| A drag on a selected face moves the selection | edit | changed: one command, "Move 2 Nodes", undone |
| A collapsed group's face is its group | edit | changed: `edit.move` with the group, "Move Group" |
| A frame is moved by its tab and sized by its handle | edit | changed: "Move Frame", "Resize Frame" |
| A wire dropped on nothing makes nothing | | removed (decision 8 changes what it checked) |
| A new wire dropped on nothing opens the palette for what takes it; dismissing leaves nothing | edit | new |
| A kind chosen from a dropped wire's palette is added with the wire, as one undo step | edit | new: from an output and into an input |
| Two drags are two undo steps; a held arrow key is one | edit | new |
| A run's state arrives in the scene | read | new, needs `engine:text` |
| A watched text value arrives as text.plain, and a scalar as scalar | read | new, needs `engine:text` |
| A watched picture arrives as picture | read | new, needs `engine:array`; `media.get` decodes it where served |
| Hostile gestures and queries | read | 13 more hostile steps (`palette.*`, `canvas.palette`'s wire, `edit.move`, `selection.nudge`) |

**Drivers:**

| Driver | Cases run | Result |
|---|---|---|
| C ABI headless (text engine; `media`) | 23 (picture skipped) | all pass |
| C ABI headless, read half alone | 11 (picture skipped) | all pass |
| C ABI headless on the array engine (cases needing it) | 1 | passes |
| C ABI's control port (text engine) | 23 | all pass |
| egui editor, linked to the text engine | 23 | all pass |
| egui editor, linked to the array engine (cases needing it) | 1 | passes |
| The core's document (no engine) | 21 (3 skipped) | all pass |

**`ui.inspect`'s `watched` comes from the core** (contract 13.1; `values::watched_json`): `{node, port, summary, media, viewer, value?, data?, picture?: {ref, w, h}, excerpt?, error?}`. The C ABI had no `watched` at all before. `viewer` is the core's first choice by media (`values::default_viewer`); a test in `crates/ui` holds egui's registry to the same first choice for the nine media it decides. egui's block is the core's with its own fields over it.

## 4. The 9% regression

Profiled with `sample` on a loop of 2,000-node snapshots (a new opt-in knob, `PLUMCRAFT_MEASURE_SNAPSHOT_SECS`, in the ignored `measure_scene_sizes`). Over half the snapshot's time was `Graph::mapped_nodes`: ordered maps keyed by node id, every lookup a string compare. Rewritten to walk nodes by index (a hash from id to index once, then vectors); `Graph::mapped_nodes`' test that it agrees with `output_is_list` passes.

| Release, M4 Max | Slice 3 | Slice 4 / before this fix | After |
|---|---|---|---|
| Snapshot build, 2,000 nodes (core, median of 5, three runs) | 2.93 ms | 3.08 to 3.32 ms | 1.87 to 1.90 ms |
| Snapshot build, 500 nodes | 0.81 ms | 0.83 to 0.86 ms | 0.56 ms |
| `plum_geometry_set` until the snapshot has it, 2,000 (C ABI, three runs) | 14.3 ms | 15.6 to 15.8 ms (one run 31 ms) | 14.3 to 14.6 ms |
| Same, 500 | 5.7 ms | 5.5 to 5.7 ms | 5.3 to 6.1 ms |
| One-node delta, 2,000 (C ABI) | 6.24 to 6.79 ms | 6.45 to 6.81 ms | 6.22 to 6.33 ms |
| `plum_hit_test` / `plum_wire_target` / `plum_wire_paths` (20), 2,000 | 1.63 / 1.38 / 5.3 µs | 1.67 to 1.71 / – / – µs | 1.67 / 1.38 to 1.46 / 5.8 µs |
| First publish with every face measured, 2,000 (core) | 16.8 ms | 16.8 to 17.6 ms | 16.8 to 17.0 ms |

**What the C ABI number is.** It is mostly the core's scene throttle, not work: the core looks at a scene at most every 16 ms while busy, and the `scene.get` just before `plum_geometry_set` in the measurement was a look. With `PLUMCRAFT_MEASURE_IDLE=1` the measurement also times a report sent after 100 ms of quiet: 18.5 ms at 2,000 nodes, 5.0 ms at 500. That is the work (apply, rebuild every wire path and face, publish, snapshot) on a core whose CPU has slowed; the hot calls timed after it are slower too, which is why it is opt-in. The full first publish at 2,000 (16.8 ms) dominates it, not the snapshot.

Logs: `claude-debug/native-n3/s1-measure-before-fix-run*.txt`, `s1-measure-after-fix3-run*.txt`, `s1-measure-idle.txt`, `s1-measure-core-run*.txt` (before), `s1-measure-core-fix-run*.txt` (after), the profile `s1-snapshot-sample.txt`.

## 5. The Swift package

- **xcframework:** `native/apple/build-xcframework.sh` (all four slices: macOS arm64 and x86_64, iOS, iOS simulator). The macOS arm64 slice alone first: 38 s. Then all slices: 113 s more. Cold, about 2.5 minutes in all; about 3.6 GB of target directories (`target/aarch64-apple-darwin` 827 MB, `x86_64-apple-darwin` 889 MB, `aarch64-apple-ios` 822 MB, `aarch64-apple-ios-sim` 822 MB, the universal library 215 MB). The static libraries are 105 to 215 MB.
- **`swift build`:** clean, 10 s. **`swift test`:** 6 of 6 pass (open, list, build, run and borrow; a 64 MB blob; graph.get's cost; hostile input and a second close; request latency; wakes and events during a run). No change to the package was needed.
- **Its state:** `PlumCore` (a `@MainActor` class) wraps `plum_open`, `plum_request` (async, by id), `plum_poll` after the wake (an `AsyncStream`), `plum_blob` and `plum_release`, `plum_last_error`, `plum_leases`. It calls none of N2's exports.

**What the Swift kit slice will have to add:**

- the scene feed: `scene.get {since}` on the `changed {domain: "scene"}` item, decoded into observable models (`JSONSerialization`; `Codable` was 3 to 4 times slower in N0);
- `plum_geometry_set` (typed `plum_face`, pins as float arrays) after a face's `rev` changes, and `geometry.set`'s JSON form for `parts`;
- the hot calls: `plum_hit_test`, `plum_wire_target`, `plum_wire_paths` (three header floats a wire: nid low, nid high, count), `plum_gesture_move` (`plum_drag`);
- `ui.gesture` for every canvas gesture, with `slop` from its target size over its zoom;
- `theme.get` for colour tokens; `menu.get` for the menu bar; asks;
- the palette from `view.palette` and `palette.items`, sending `palette.choose` and `canvas.palette {open: false}`;
- arrows as `selection.nudge` naming each key press (`NSEvent.isARepeat`);
- `ui.inspect`'s `watched` for the floor's three viewers (`excerpt`, `value`, `picture.ref` with `plum_blob`);
- a conformance driver: the read half over the app's control port (`Port`'s protocol; `has("engine:text")`, `reports()` false), as contract 16.1 says.

## Every behaviour change on the C ABI and the control port

On the egui editor's control port, the C ABI's `plum_request` and the C ABI's control port, unless a change says otherwise. No C export was added or changed; `PLUM_ABI_VERSION` stays 1.

**New:**

1. Session commands `edit.move` and `node.addWired`; canvas commands `palette.choose` and `selection.nudge`. `command.list` has 140 rows (136); the shared rows keep their order.
2. Method `palette.items`. Before: "unknown method".
3. `canvas.palette` takes `wire`; its reply adds `wire` and `offered`.
4. The scene's `view` has `palette` while the palette is open.
5. `ui.inspect` has `watched` from the core. **C ABI:** new field (it had none). **egui:** the block gains `excerpt` (seen in the recorded replies), and `value`, `data`, `picture`, `error` where the value has them.

**Changed:**

6. `node.move`, `group.move`, `frame.move`, `frame.resize` are undo steps (`ui.inspect`'s `undo` and `edit.undo` see them), take `drag`, and a move to the same place no longer makes the document dirty. Their `command.list` params text says so; `edit.undo`'s too.
7. `ui.gesture`: a face drag's `end` runs one `edit.move` (was a `node.move` or `group.move` a face); frame, resize and band drags name the drag; a new wire's drag ending on nothing runs `canvas.palette` (it ran nothing).
8. `ui.gesture`: a drag's `serial` rises with every drag (it was 1 for every drag begun with none under way).
9. **C ABI only:** `canvas.palette` is served (it answered "isn't available from this shell").
10. `graph.new` and `graph.open` close the palette.

**Unchanged:** the 124 menu dumps are identical; all 41 recorded replies are identical but for `command.list` (above), `watched.excerpt` (above) and the scratch paths.

## What a user of the egui editor sees

1. **Undo now takes back moves:** a node drag (one step however long), a frame's move or resize, a group's band drag. Undo's label in the Edit menu names them ("Undo Move 2 Nodes").
2. **Undo after an edit no longer silently undoes later moves.** Before, undoing a step restored the whole graph as it was, so moves made since were lost too; now they are their own steps.
3. **The arrow keys nudge the selection** (1 unit, 10 with Shift) when something is selected and no field has the keyboard; a held key is one undo step.
4. **A wire dropped on empty canvas opens the palette there,** offering only nodes that take it; a pick adds the node connected, one undo step; Escape leaves nothing. Before, the wire vanished.
5. Moving a node to where it already is no longer marks the document unsaved.

## Fixtures and schema

- `crates/editor-core/fixtures/conformance/cases.json`: rewritten (above). The copy before is `claude-debug/native-n3/s1-cases-before.json`; the script that rewrote it, `claude-debug/native-n3/s1_cases.py` (two later hand fixes: reply pointers and the picture case's ports).
- `crates/editor-core/schema/shell1.json`: regenerated; it gains `Palette`, `PaletteWire`, `PortAt` and `View.palette` (90 lines). `palette.items`, `edit.move` and `selection.nudge` are not in the schema (the core builds and parses them by hand, like most commands).
- The scene fixtures (`full.json`, `delta.json`) are unchanged (`view.palette` is left out when closed).

## The gate

| Check | Result |
|---|---|
| `cargo nextest run --all-features --no-fail-fast`, b3, fresh folders (`claude-debug/native-n3/s1-gate2-b3.log`; the first run `s1-gate1-b3.log`) | 1,455 of 1,456; failed: `plumcraft-craft::params_coverage hidden_params_match_the_baseline` (the known local-only failure), in both runs |
| `cargo clippy --all-targets --all-features -- -D warnings` | clean |
| `cargo fmt --all --check` | clean |
| `scripts/check-hash-gates.sh` / `check-core-toolkit-free.sh` | ok / ok |
| `uv run ruff check .` / `uv run pytest -q` (b3) | clean / 224 of 224 |
| `swift build` / `swift test` in `native/apple/PlumCraftFFI` | clean / 6 of 6 |

**New tests (10):** session 4 (`moves_are_undo_steps_and_a_drag_is_one`, `a_move_joins_only_within_the_rule`, `edit_move_names_what_moved_and_refuses_hostile_params`, `a_node_added_wired_is_one_undo_step`); `crates/ffi/tests/conformance.rs` 2 (the read half alone, the array cases); `crates/ui/tests/conformance.rs` 1 (the array cases); `crates/ui/tests/kittest_n3_edits.rs` 2 (the dropped wire, the arrows); `crates/ui` viewers 1 (the core's default viewer). Also extended: `gesture::tests` (serials), the frame and files tests below.

**Existing tests edited because this slice changes what they check:**

- `plumcraft-session` `group_cmds::tests::group_move_is_not_an_undo_step` is now `group_move_is_an_undo_step`; `a_group_moves_with_its_members_and_collapses_in_place` counts the move as a step and walks back over it.
- `frame_cmds::tests::a_frame_around_nodes_moves_them_and_undoes` asserted "moving isn't an undo step"; it now checks "Move Frame", "Resize Frame" and the undo.
- `crates/session/tests/files.rs` `moves_and_views_are_saved_but_not_undo_steps` is `moves_are_saved_and_undo_steps_and_views_are_saved_but_not`.
- `crates/ui/tests/kittest_moves.rs`: the paste and insert-as-group drags are one undo step each ("Move 2 Nodes", "Move Group"), undone before the paste or insert.
- `crates/ui/tests/kittest_frames.rs` `dragging_the_tab_moves_the_frame_and_what_it_holds`: "a move isn't an undo step" became "Move Frame", undone in one.
- `crates/ffi/tests/selection.rs`: `canvas.palette` is served over the C ABI now; `canvas.reveal` is still refused.
- The C ABI and egui conformance drivers gained `has` and `pause`; the egui one is linked to an engine.
- `crates/ffi/tests/measure_scene.rs` (ignored): an opt-in idle measurement.

## Regression probes

| Probe | Result |
|---|---|
| 41 control replies (`claude-debug/native-n1/s5_probe.py`, `s1-probe-after` against `native-n2/s4-probe-after`; `s1_probe_diff.py`) | Differences: `command.list` (4 new rows, 6 params texts, order kept), `watched.excerpt` added in 6 replies after a run, scratch paths. Nothing else |
| 124 menu dumps (`s1-menus-after` against `s4-menus-after`) | Identical. No dump state follows a move, so Undo's label is the same |
| Offscreen pictures (`s2-shot.py` twice) | This slice's two runs: 3,186 pixels (unrun), 6,889 (ran). Against slice 4's: 1,930 and 21 (unrun), 18,107 and 23,851 (ran, timings drawn). Looked at: the same graph |
| The maintainer's `prefs.json` (SHA-256) | `9221fa662e648228…` before and after every editor run |
| Free disk | 91 GB at the start, 45 GB at the end: `target/` grew to about 59 GB of debug build, plus 3.6 GB for the Apple targets and 1 GB release |

## Defects caught

**Pre-existing, found by this slice:**

- **Drag serials restarted at 1** after every drag (slice 4). Found while naming a drag's moves; fixed; tested.
- **`Graph::mapped_nodes` was slow** at 2,000 nodes (string compares in ordered maps): the bulk of the snapshot. Fixed.

**Introduced and caught:**

- **A session test's "an edit between" step did nothing** (`node.set` on a socket-only input fails, and I'd swallowed the error), so the moves after it joined the earlier step. Caught by the test failing; replaced with `node.add`.
- **Shift-arrow nudged by 1, not 10:** a plain key's match in egui ignores Shift, so the plain pass took the Shift press. Caught by the new kittest; Shift is consumed first.
- **Conformance cases pointed one level too deep** (`/result/result/...`) and used a saved value in the same step's `expect` before `save` ran. Caught by the first run on the core's document.
- **The watched text case expected `data` or `value`;** a text value carries neither, only an excerpt rendition. Caught by every driver; the core's block gained `excerpt`.
- **`selection.nudge` refused `drag`** at first. Caught by the conformance case.
- **Clippy:** a test helper left after a test module, and a one-element loop. Caught by clippy.
- **My measurement change made the hot calls look slower** (they were timed after a second geometry and a 100 ms pause). Caught by comparing with the earlier runs; the idle measurement is now opt-in and restores the first sizes.

## Decisions the maintainer may want to revisit

1. **The coalescing rule:** same things plus the same `drag`, or no drag and at most 1 s apart. A time window is the fallback only; shells name their drags and key presses.
2. **Two taps of an arrow key are two undo steps in egui** (each press is named); only a held key is one. The alternative is the time rule for taps too.
3. **Labels with counts** ("Move 3 Nodes", "Move 4 Items" when mixed), unlike existing labels ("Paste Nodes").
4. **Moves are still not journaled.** The journal can't replay an undo of a move faithfully; journaling would add an entry a frame during egui drags.
5. **A move to the same place is a no-op**, so it no longer marks the document dirty.
6. **egui binds the arrow keys** to nudge when something is selected. They did nothing on the canvas before.
7. **The palette is core state, and egui's pick is `palette.choose`**, even without a wire (it was `node.add`); the journal still shows `node.add`.
8. **Convert is not offered for a dropped wire**, and a wire from or into a Convert opens an empty palette.
9. **The palette's fit is the first port that fits** in the kind's definition order (variadic slots and default sub-ports included); `port` overrides.
10. **The read/edit line** is as written in contract 16.1; a cancelled drag counts as editing.
11. **`values::default_viewer` is a by-media table in the core**, with egui's registry held to it by a test, rather than moving the registry (13.1) now.
12. **Text values are read from `excerpt`** (first 4 KiB) in `watched`; larger text needs `data` and `media.get`, which a small in-process value doesn't have.
13. **egui's `ui.inspect` `watched` gains the core's fields** (`excerpt` showed up in the recorded replies).
14. **The geometry-to-snapshot number** is mostly the 16 ms scene throttle; I left the throttle alone.

## For the Swift kit slice

- Everything under section 5 above.
- Run the read half with `conformance::run_half(.., Half::Read)` through the `Port` protocol; the app's driver should say `has("engine:text")` (and `media` only if it serves `media.get`, which the control port doesn't).
- On a drag, send `ui.gesture` begin and end; between them call `plum_gesture_move` and `plum_wire_paths`. A drag's moves are one undo step without the shell doing anything.
- A new wire let go on nothing: the reply's `commands` contain `canvas.palette`; draw the palette from `view.palette` and `palette.items`.
- The picture case needs the array engine; a Mac app opened on the text engine will skip it, as the C ABI's default driver does.

## What I did not verify

- **Nothing ran on Linux** (the Linux test host) in this slice.
- **No native shell** called the new methods or commands; they are tested from Rust and through egui.
- **The Swift package's tests** cover N0's calls only; it calls none of N2's or N3's surface.
- **No iOS build was run on a device or simulator;** only the iOS static libraries were built.
- **The egui conformance driver** goes through the control dispatcher. Real pointer and key input is covered for the dropped wire and the arrows by the new kittests, and for drags by the existing ones.
- **The time rule** (1 s) is unit-tested with explicit times, never waited out in a test.
- **Measurements** are from one machine (M4 Max), three runs, with run-to-run noise of about 0.3 ms on the C ABI numbers.
- **The undo-after-edit loss of moves** (user-visible change 2) is inferred from the code (undo restores a whole-graph snapshot); no test reproduced the old behaviour.
