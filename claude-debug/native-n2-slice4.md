---
title: "N2 slice 4: gestures, the schema and the conformance suite"
date: 2026-11-20
---

# N2 slice 4: gestures, the schema and the conformance suite

Branch `native/n2-gesture`, cut from `origin/feat/native-n0`. Nothing is committed; the changes are in the working tree for review.

## Summary

All six items were done. Item 2 (egui) is done in part, for the reason given under it.

**The N2 gate is met.** The suite has 19 cases and 197 steps, kept as data. It passes on:

- the C ABI headless, through the exported functions, with geometry reported by `plum_geometry_set` and drag moves through `plum_gesture_move`;
- the C ABI's own control port, as JSON lines;
- the egui editor through its control dispatcher, with its window drawn and its own measurements;
- the core's own document.

egui draws its faces from the scene, as it has since slice 2.

**The gate's checks passed, except the known local failure.** nextest (b3, CI's environment) ran 1,446 tests and passed 1,445. The one failure is `plumcraft-craft::params_coverage hidden_params_match_the_baseline`. Before this slice it was 1,433 of 1,434: the slice adds 12 tests. Clippy, fmt, both scripts, ruff and pytest (224 of 224) are clean.

**The recorded probes are unchanged except where expected:**

- **The 41 control replies** are identical to slice 3's except for run timings, run ids and scratch paths. `command.list` is unchanged.
- **The 124 menu dumps** are identical.
- **The offscreen pictures** show the same graph, moved about 5 pixels. The reason is under item 2.
- **The maintainer's `prefs.json`** is unchanged (`9221fa66…`).

**The slice 3 numbers have not regressed beyond noise.** A one-node delta at 2,000 nodes over the C ABI took 6.36 to 6.96 ms; slice 3 measured 6.24 to 6.79.

## 1. `ui.gesture`

The module is `crates/editor-core/src/gesture.rs` (contract 6.3 and 8.13).

**What a shell sends.** It reports gestures it has already recognised, in graph units:

- `tap`, `doubleTap`, `context` (also accepted as `longPress`) and `marquee` (`at` and `to`);
- `drag`, with a `phase` of `begin`, `move`, `end` or `cancel`;
- `mods` (`shift`, `primary`, `alt`, `ctrl`) and an optional `slop`.

**What the core does with it:**

1. It publishes the scene.
2. It hit-tests the gesture against the snapshot.
3. It runs the existing commands the gesture means. The full table is in contract 8.13.

**The reply** is `{commands: [{command, params, ok, result | error}], hit?, drag?, update?, context?, menu?, cancelled?}`. A context gesture returns `menu.get`'s context and the menu model itself, so a shell needs one round trip.

**Drags keep state.**

- `begin` decides what the drag is: a move of faces, a wire, a marquee, a bend point, a frame, a frame's size or an open group. It keeps that in the document's `DragCell`.
- `move` changes nothing.
- `end` runs the drag's commands.
- `cancel` forgets the drag. The suite checks that `graph.get`, `dirty` and the undo label are the same after a cancel as before it.

**Undo.** A drag runs at most one undoable command (`wire.connect`, `wire.reroute`, `wire.disconnect` or `wire.route`). Moves are not undo steps in this editor and never were, so a face drag adds none. Plan 5.3 assumed it would add one; this is listed under decisions.

**The typed hot call for drag moves** is `plum_gesture_move(core, doc, x, y, slop) → plum_drag`. It returns `{status, kind, serial, dx, dy, rect[4], target: plum_hit}`. It reads the document's drag cell and its snapshot on the caller's thread. The ends of a drag go through JSON; its moves never do.

| Median of 400, release, M4 Max | 500 nodes | 2,000 nodes |
|---|---|---|
| `plum_gesture_move`, wire drag (finds the target) | 0.33 µs (p99 0.46) | 1.17 µs (p99 2.1) |
| `plum_gesture_move`, face drag | 0.04 µs | 0.04 µs |

**Hostile input** is refused: an unknown kind, phase, modifier or field; a point that isn't two finite numbers within ±1e7; a `move` or `end` with no drag under way. Unit tests and the suite's hostile case check this.

**New method: `geometry.locate`.** It returns where the core has a face (its centre, header or body), a pin, a point on a wire, or a frame's tab or handle. Its reply includes `measured`. Scripts and the suite use it to aim gestures at whatever sizes a shell measured.

## 2. egui uses it

**Done:**

- **Clicks are `ui.gesture` taps,** hit-tested by the core against the faces egui measured.
- **The snarl selection bridge is retired.** `Picked::gained`, `absorb` and the `raw` state are gone; snarl's private selection is no longer read.
- **egui's own click hit tests are gone:**
  - `click_command` and `node_at` for selection;
  - the port-click watch and the wire-click watch, now the core's tap on a pin or a wire.
- **A Shift-drag that begins on empty canvas is recognised by the canvas.** On release it is sent as `ui.gesture {kind: "marquee"}`. egui-snarl still draws the rectangle while it is dragged.

**Not done, and why.** egui-snarl owns these, and slice 4 does not fork it:

- **Node drags.** Snarl moves its nodes and its own stale selection while the drag runs. `drag_moves` still works out which faces move from snarl's positions; its rules match the core's `begin`.
- **Wire drags and the ⌘-drag reroute.** Snarl draws the wire, picks the target pin and calls `connect`.
- **Double and secondary clicks** on nodes and wires. Snarl's frame responses carry them.

Routing these would mean disabling snarl's input and recognising the gestures in the shell. That is possible, but it is a rewrite of the canvas's input, not a bridge.

**Found while doing it: egui's faces were about 7 graph units off.** egui-snarl draws its frame margin around the node's position and puts its pins on the frame's edges. Slice 3 placed every egui face at the node's position, so the core's face rects, pins and wire chains were offset from what egui drew. Routing clicks through the core made `kittest_moves` fail (a Shift-click on a group's top edge hit nothing), and that exposed the offset.

- **Fix:** `geometry.set` takes `offset: [dx, dy]`, where the face is drawn relative to its position, default `[0, 0]`. egui reports its frame's offset.
- **Visible effect:** the core's fit rect now includes the margin, so View › Fit frames about 5 pixels differently. The offscreen pictures differ from slice 3's by that shift only; they were looked at side by side.

**Also found: egui's control port didn't serve the geometry queries.** It answered `geometry.hit`, `geometry.wireTarget` and `geometry.wirePaths` with "isn't available from this shell", because its `Shell` lent no document. It lends one now, so those queries and `ui.gesture` work there.

## 3. The scene's open points

### Colours (question 13)

The maintainer already decided this question (13A), so this was a technical change, not a new decision.

**The evidence:**

- Engine-supplied colours (wire-type colours) and other computed colours (family tints, a frame's own colour) can't be names. They must be sent as `[r, g, b, a]`.
- Every theme colour can be a name, if a shell can resolve it.

**What changed:**

- A `Paint` is now `{token}` or `{color}`, exactly one.
- `theme.get` returns every token a scene can name: the Settings table's, plus `pin_hidden`, `group` and `group_body`.
- egui resolves tokens against the theme it draws.
- The suite checks that no paint carries both, and that `theme.get` has every token the scene names.

**Size:** the shared fixture went from 120 KB to 107 KB. The full 2,000-node measurement scene went from 5.19 MB to 5.08 MB, because most of its colours are computed.

### `nid`s

Since slice 3, nids have come from one counter per document. A face and a wire can no longer share one, which contradicts what the contract said.

- A suite case now checks that every nid in a full scene is distinct across kinds.
- `plum_wire_paths` now carries the whole nid: low word, high word, then the count. Slice 3 sent the low 32 bits only.

### Measured geometry

- **Measured tags, frame tabs and group bands:** decided, not built. They are drawn from the scene's own data, the core's estimates are at most one line of text off, and no hit test there has been wrong. The contract (8.11) says how they would be added.
- **A fit after `graph.open`:** built. `graph.new` and `graph.open` with something to show leave a fit owed. The geometry report that measures the last face asks for it once, in `view.fit`.
  - egui doesn't report through `geometry.set` and still fits itself.
  - A unit test covers this.
- **Snapshot handover:** the hot calls now get a new snapshot the moment it is made, through a cell per document. Before, the C ABI shared it at the end of the core thread's turn, after the replies had gone out.

## 4. The schema

The schema is `crates/editor-core/schema/shell1.json`: 64 KB, 59 definitions, 11 messages.

**What it covers:**

- **From the scene's own serde types:** `Scene` and `Delta`, with every face, row, widget and paint type.
- **From schema-only types** (`src/shell_schema.rs`), for messages the core parses by hand or builds as JSON:
  - `geometry.set` and its reply;
  - `ui.gesture` and its reply;
  - `geometry.hit` and `geometry.wireTarget` params, and the `Hit` they return;
  - `geometry.locate`'s reply;
  - `theme.get`'s reply.

**How it is kept current:**

- Regenerate it with `cargo run -p plumcraft-editor-core --example export_shell_schema --features schema`.
- A test fails while the checked-in copy is stale.
- A second test reads the scene fixtures and sample messages through the schema types.

## 5. The conformance suite

**The runner** is `plumcraft_editor_core::conformance`, written against an abstract `Driver` with these methods:

- `call` sends one control request;
- `reports` says whether the suite supplies the driver's measurements;
- `report_geometry` sends them;
- `settle` waits on a condition;
- `hot_move` makes the typed drag move, where the driver has one.

**The cases** are in `crates/editor-core/fixtures/conformance/cases.json`. The step language and value matchers are documented in the module and in contract 16.1. Every point is aimed with `geometry.locate`.

**The 19 cases cover:**

- hits on reported geometry;
- taps;
- face drags, alone and with the selection;
- a cancelled drag;
- wire drags: fitting, refused, dropped on nothing, rerouted;
- marquees;
- double taps and context menus;
- collapsed groups;
- frames and bend points;
- colours and nids;
- a hostile sweep.

| Driver | Where | Result |
|---|---|---|
| C ABI, headless, through the exports (`plum_request`, `plum_poll` after a wake, typed `plum_geometry_set`, `plum_gesture_move`) | `crates/ffi/tests/conformance.rs` | 19 of 19 |
| The C ABI core's control port (TCP, `auth` first) | same file | 19 of 19 |
| The egui editor (`handle_control`, window drawn, its own measurements) | `crates/ui/tests/conformance.rs` | 19 of 19 |
| The core's document | `conformance::tests` | 19 of 19 |

**The runner is not vacuous.** A test feeds it six wrong cases (a wrong value, a wrong `ok`, a missing pointer, a bad locate, a bad save), and each one fails.

**Waiting is on conditions only:**

- the egui driver draws until every face is measured, with a 5 s limit;
- the C ABI driver polls `geometry.locate` until the core reports the faces measured.

**How a Swift or Kotlin shell runs it.** Its core opens a control port. The `Port` driver, or any program that implements the step language, runs the same cases. The contract (16.1) says so.

## 6. Draft 2 of the contract

`docs/shell-interface.md` is now **Draft 2**. Its header lists 11 changes from Draft 1, and the "As built" sections were updated, including:

- 5.2: `plum_drag`, `plum_wire_paths`' layout, the snapshot handover, the ABI version;
- 5.3: the control channel's table;
- 6.3: `ui.gesture`, `geometry.locate`, `theme.get`;
- 8: colours and nids;
- 8.9: the owed fit;
- 8.11: `offset`, and the decision on measured tags;
- 8.13: the gesture table as built;
- 9.1, 9.5 and a new 9.6 (`plum_gesture_move`);
- 16: the suite, and how a shell runs it;
- 17: questions 7, 13 and 17.

`docs/control-protocol.md` documents `geometry.locate`, `ui.gesture`, `theme.get` and `geometry.set`'s `offset`.

**`PLUM_ABI_VERSION` stays 1.** N2 added exports and changed one layout (`plum_wire_paths`). Nothing outside this repository links the library, and the N0 spikes under `native/` call none of the calls that N2 added or changed. The version rises with the first breaking change after a shell is released (N3). Additions never raise it.

## Every behaviour change on the C ABI and the control port

On the egui editor's control port, the C ABI's `plum_request` and the C ABI's control port, unless a change says otherwise.

**New:**

1. `ui.gesture`. Before, it was "unknown method".
2. `geometry.locate`. Before, it was "unknown method".
3. `theme.get`. Before, it was "unknown method".
4. **C ABI only:** the export `plum_gesture_move` and the type `plum_drag`.

**Changed:**

5. **The scene's colours** are `{token}` or `{color}`, no longer both. This affects every `scene.get` reply and the fixtures.
6. **`plum_wire_paths`** writes three header floats a wire (nid low, nid high, count); it wrote two.
7. **`geometry.set`** accepts `offset`. Other unknown fields are still refused.
8. **egui's port only:** `geometry.hit`, `geometry.wireTarget` and `geometry.wirePaths` now answer. Before, they said "isn't available from this shell".
9. **egui only, positions:** egui's faces carry their frame offset. The scene's wire paths, open groups' rects, marks' places, hit results and `canvas.fit`'s rect move by about 7 graph units, to where egui draws.
10. **C ABI only, snapshot timing:** the hot calls see a new snapshot as soon as it is made, before the reply after it. `doc` 0 now means the first open document even before its scene is published (`PLUM_ERR_MISSING` until then). Before, it meant the first document that had a snapshot.
11. **C ABI, owed fit:** after `graph.new` or `graph.open` with content, the `geometry.set` or `plum_geometry_set` that measures the last face sets `view.fit`. The scene changes, and the feed says so.
12. **A drag under way is forgotten** by `graph.new` and `graph.open`.

**egui only, no reply change:**

13. **A click on a pin** watches its value and no longer also selects the node. Before, a click inside the node's rect did both.
14. **Clicks on nodes, wires and empty canvas** select, watch and clear as before, now decided by the core.
15. **Shift-drag marquees** select by the core's rects, with the same intersect rule as snarl.
16. **Nothing else changed:** `command.list`, all 41 replies and all 124 menus are unchanged.

## Fixtures and schema

**`crates/editor-core/fixtures/scene/full.json` and `delta.json`** were rewritten with `PLUMCRAFT_WRITE_FIXTURES=1`.

- The only change: the 109 paints with a token lost their `color`.
- The 150 computed colours are unchanged.
- The fixture's 36 nids are all distinct.
- The copies from before this slice are `claude-debug/native-n2/s4-full-before.json` and `s4-delta-before.json`.

**New files:**

- `crates/editor-core/fixtures/conformance/cases.json`
- `crates/editor-core/schema/shell1.json`
- `crates/editor-core/examples/export_shell_schema.rs`
- the `schema` feature on `plumcraft-editor-core`, with `schemars` optional

## The gate

| Check | Result |
|---|---|
| `cargo nextest run --all-features --no-fail-fast`, b3, fresh folders (`claude-debug/native-n2/s4-final1-b3.log`) | 1,445 of 1,446; failed: `plumcraft-craft::params_coverage hidden_params_match_the_baseline` (the known local-only failure) |
| `cargo clippy --all-targets --all-features -- -D warnings` | clean |
| `cargo fmt --all --check` | clean |
| `scripts/check-hash-gates.sh` / `check-core-toolkit-free.sh` | ok / ok |
| `uv run ruff check .` / `uv run pytest -q` (b3) | clean / 224 of 224 |
| 41 control replies (`s5_probe.py`, `s4-probe-after` against `s3-probe-after2`) | identical but run timings, run ids and scratch paths |
| 124 menu dumps (`s4-menus-after` against `s3-menus-after`) | identical |
| Offscreen pictures (`s2-shot.py`, `s4-shot-after-a` and `-b`) | between the two runs of this slice: 5,099 pixels (unrun), 8,858 (ran); against slice 3's: about 360,000, because the view moved about 5 pixels with the fit's offset; looked at side by side, the same graph |
| `prefs.json` SHA-256 | `9221fa66…` before and after every editor run |
| Free disk | 107 GB |

**New tests (12):**

- `gesture::tests`: 2.
- `conformance::tests`: 4. The cases parse; values and matchers; a wrong case fails; the suite passes on the document.
- `shell_schema::tests`: 2.
- `canvas_cmds::tests::a_fit_owed_is_asked_once_every_face_is_measured`: 1.
- `crates/ffi/tests/conformance.rs`: 2.
- `crates/ui/tests/conformance.rs`: 1.

**Existing tests edited, because this slice changes what they check:**

- **`crates/ui/src/edit_view.rs`, `picked_takes_only_what_snarl_adds`** is now `picked_is_the_documents_selection`. Its subject, the snarl bridge (`absorb`), is gone. Its `set`, `clear` and `retain_existing` checks are kept.
- **`crates/ui/src/edit_view.rs`, `a_click_is_a_selection_command`** is now `a_click_is_a_tap_gesture`. `click_command` is gone: its rules (Shift adds, ⌘ takes away, an empty click clears) are the core's now, checked by the suite's tap case.
- **`plum_wire_paths`' readers** read the new three-float header: `geometry::tests` (two tests) and `crates/ffi/tests/geometry.rs`.
- **`scene::tests`' colour assertion** reads `Some([..])` for an engine colour, and now also checks that a token is sent alone.
- **`measure_scene.rs`** (ignored) also measures `plum_gesture_move`.

No test failed intermittently, and none was weakened.

## Measurements (release, M4 Max)

Logs: `claude-debug/native-n2/s4-measure-ffi-run1..3.txt` and `s4-measure-ffi-gesture.txt`.

| What | Slice 3 | Slice 4 |
|---|---|---|
| C ABI one-node delta, 2,000 nodes | 6.24 to 6.79 ms | 6.36 to 6.96 ms |
| C ABI one-node delta, 500 nodes | 2.30 to 2.63 ms | 2.50 to 2.67 ms |
| Full `scene.get`, 2,000 nodes (median of 24) | 5.19 MB, 75.7 ms | 5.08 MB, 75.3 to 76.7 ms |
| `plum_geometry_set` of every face until the snapshot has it, 2,000 | 14.3 ms | 15.6 to 15.7 ms |
| `plum_hit_test` / `plum_wire_target` / `plum_wire_paths` (20 faces), 2,000 | 1.63 / 1.38 / 5.3 µs | 1.67 / 1.75 / 5.9 µs |

The delta is within run-to-run noise; its upper end is 0.2 ms above slice 3's. Geometry reaching the snapshot is about 9% slower at 2,000 nodes. That is probably each snapshot pin now keeping its port's name (needed to turn a hit into a command); it was not profiled.

## Defects caught

**Pre-existing (from slice 3), found by this slice's tests:**

- **egui's faces were offset** by snarl's frame margin. The kittests caught it once clicks went through the core.
- **egui's control port had no geometry queries,** because its `Shell` lent no document. The egui conformance driver caught it on its first run: 17 of 19 cases failed with "isn't available".

**Introduced and caught:**

- **A JSON case had a duplicate key** (`/result/undo` twice). I caught it on review; serde would have kept only the second.
- **A test helper was left unused** after the bridge tests were rewritten. Clippy caught it.
- **A hot call right after a reply could read the previous snapshot.** This was designed out (the cell handover) before any test saw it. In slice 3's design the window was real but narrow.

## Decisions the maintainer may want to revisit

1. **Shells send gestures, not commands.** Draft 1 said only a script sends `ui.gesture` and a shell sends commands. That would put the gesture's meaning in each shell, against rule 2. Draft 2 has every shell send gestures.
2. **Moves are not undo steps**, as before. A face drag is no undo step; a wire drag is one. Plan 5.3 assumed a drop is one undo step.
3. **A tap on a pin watches and does not select** its node.
4. **A plain marquee replaces the selection;** Shift adds and ⌘ takes away. On egui a marquee needs Shift, so there it always adds.
5. **New meanings that egui doesn't send yet:**
   - a double tap on empty canvas opens the palette;
   - a drag that begins on a wire, a tag or a frame's body is a marquee;
   - a new wire dropped on nothing does nothing (question 8's filtered palette is not built).
6. **`offset` is JSON-only.** The typed `plum_face` has none, because a native shell draws faces at their positions.
7. **Measured tags, tabs and bands are deferred.**
8. **`PLUM_ABI_VERSION` stays 1** despite `plum_wire_paths`' new layout, because no shell calls it.
9. **`theme.get` is minimal** (`{shell, name, tokens}`), not Draft 1's 14.1 with fruits, text sizes and spectrogram.

## For N3 (the first macOS shell)

- **The canvas:**
  - Send `ui.gesture` for everything a person does on the canvas. While dragging, call `plum_gesture_move`, then `plum_wire_paths` with the `faces` and `dx, dy` that `begin` gave.
  - Never decide meaning in Swift: the reply's `commands` say what happened.
- **Geometry:**
  - Report it with `plum_geometry_set` whenever a face's `rev` changes, faces drawn at their positions.
  - Add `parts` (title, chevron, enter, menu) through `geometry.set`'s JSON form if taps on them should do what they are; the typed form has no parts.
- **Colours and ids:**
  - Resolve colour tokens with `theme.get`, and redraw on a theme change.
  - Key everything by nid, which is unique within a document.
- **The conformance suite:** run it against the app through its core's control port, using the `Port` driver's protocol; `reports()` is false, since the app measures its own faces.
- **The feed:** after `graph.open`, expect a `view.fit` once every face is measured.

## What I did not verify

- **No native shell** (Swift or Kotlin) called the new export or the changed layout; they are tested from Rust only.
- **Nothing ran on Linux.** The yellowfin run before merging was not done here.
- **The egui conformance driver goes through the control dispatcher,** not simulated pointer input. Pointer input reaching `ui.gesture` is covered only by the existing kittests that click (they pass) and by `a_click_is_a_tap_gesture`. No kittest drives a Shift-drag marquee end to end. I did not add one.
- **Measurements are from one machine** (M4 Max), three runs.
- **The 9% slower geometry-to-snapshot** at 2,000 nodes was not profiled.
