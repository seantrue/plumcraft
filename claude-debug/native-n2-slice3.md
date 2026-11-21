---
title: "N2 slice 3: geometry in the core"
date: 2026-11-20
---

# N2 slice 3: geometry in the core

Branch `native/n2-geometry`, cut from `origin/feat/native-n0`. Nothing is committed; the changes are in the working tree for review. `docs/BUGS.md` (B30) was already modified and is left as it was.

## Summary

All six items were done, three of them with parts left, listed under each item.

1. **The rebuild cost was found and fixed.** A one-node delta through the C ABI takes 6.3 ms at 2,000 nodes (36.2 ms before) and 2.4 ms at 500 (4.4 ms), wire paths included.
2. **`geometry.set`** is a canvas command on the control port, and `plum_geometry_set` is its typed C form.
3. **Hit testing** is in the core (`geometry::Snapshot::hit`).
4. **Wire paths** are published in the scene as cubic chains. The **wire target** query is built.
5. **Typed hot calls** on the C ABI: `plum_hit_test`, `plum_wire_target`, `plum_wire_paths` and `plum_geometry_set`, plus JSON forms of each.
6. **egui consumes it:**
   - egui reports its measurements every frame.
   - It builds its snarl from the scene's structure.
   - It fits to the core's rect.
   - Align and Distribute are commands, and Frame Selection and Tidy take the core's sizes.
   - The snarl selection bridge stays, for the reason given under item 6.

**The gate passed but for the known local failure.** nextest (b3, CI's environment) ran 1,433 of 1,434; the one failure is `plumcraft-craft::params_coverage hidden_params_match_the_baseline`. The count was 1,417 of 1,418 before; this slice adds 16 tests and 2 ignored measurements. Clippy, fmt, both scripts, ruff and pytest (224 of 224) are clean.

**The control replies and menus changed only where intended.** Of the 41 control replies, only `command.list` differs (3 rows appended, and `canvas.fit`'s params text). The 124 menu dumps are identical. The offscreen pictures are within run-to-run noise and look the same. The maintainer's `prefs.json` is unchanged (`9221fa66…`).

**Two existing tests were edited**, both because this slice legitimately changes what they check (below).

## 1. The rebuild cost

Profiled with macOS `sample` on a release loop rebuilding and publishing the 2,000-node scene (`claude-debug/native-n2/s3-profile.sh`, `s3-profile-before.txt`).

**Titles: 1,049 of the build's 1,527 samples.** `plumcraft_session::node_title` looks at every other node for an untitled one sharing its type's name. For each node it calls `display_name`, a linear search of the catalogue plus a `String` clone. The build asks for it once a face and once a wired row.

- **Fix:** `Graph::node_titles` works out every title in one pass. A test checks that it equals `node_title` for every node, including shared names, hand titles and missing serials.
- The build also indexes the catalogue's node types once instead of calling `SchemaReply::node`, a list search, per node and per row.

**Publish: SipHash over each item's JSON.** This was most of the remaining cost.

- **Fix:** each published item keeps its typed value, and the next build is compared with it using the derived `PartialEq`. No serialising happens unless the item changed.
- Repeats and removals are found with a per-publish counter instead of a `HashSet` of cloned ids.

**The "super-linear" growth was a threshold, not a curve.** The C ABI's `scene.get` publishes, and then the core loop's `note_scene` built the scene again before the reply's wake whenever 16 ms (`SCENE_EVERY`) had passed since the last look. At 2,000 nodes one build took longer than 16 ms, so every request was built twice; at 500 nodes it never was.

- **Fix:** `Published::take_fresh` stops a scene being built twice in a turn.

**Found while measuring: an owed look waited for the idle tick.** A look put off by the 16 ms throttle waited for the 50 ms tick, so geometry reached the hot calls 15 to 54 ms late.

- **Fix:** the core thread now sleeps only until the look is due.

| What (release, Apple M4 Max) | 500 nodes, 800 wires, before | after | 2,000 nodes, 3,200 wires, before | after |
|---|---|---|---|---|
| C ABI one-node delta, request to decoded reply | 4.44 ms | 2.30 to 2.63 ms | 36.2 ms | 6.24 to 6.79 ms |
| C ABI `scene.get {since}`, nothing changed (median of 25) | 4.32 ms | 1.43 ms | 35.8 ms | 6.14 ms |
| C ABI full `scene.get` (median of 24) | 23.8 ms, 1.11 MB | 18.0 ms, 1.29 MB | 98.7 ms, 4.44 MB | 75.7 ms, 5.19 MB |
| Core: rebuild and publish after one state change | 4.61 ms | 1.66 ms | 18.95 ms | 7.08 ms |
| Core: build | 2.74 ms | 1.24 ms | 11.0 ms | 5.15 ms |
| Core: first publish | 4.95 ms | 3.03 ms | 19.4 ms | 12.2 ms |
| Core: snapshot for the hot calls | | 0.81 ms | | 2.93 ms |

How these were measured:

- The "after" rows include this slice's geometry: every wire's path, and group rects.
- The core rows use the C ABI's graph shape (`scene::tests::measure_scene_sizes`) with every node done.
- The C ABI rows come from `crates/ffi/tests/measure_scene.rs`. The one-node delta is one sample a run; the range is three runs.
- Logs are in `claude-debug/native-n2/`: `s3-measure-core-before.txt`, `s3-measure-core-step1.txt` (titles and comparison, no geometry yet), `s3-measure-core-after.txt`, `s3-measure-ffi-before.txt`, `s3-measure-ffi-step1.txt`, `s3-measure-ffi-final.txt` and `s3-measure-ffi-hot.txt`.

**What remains.**

- The full `scene.get` at 2,000 nodes is 5.2 MB in 76 ms. It is sent once, when a shell opens the document. Keeping each item's JSON text, and token-only colours (question 13), would cut it.
- A look with nothing changed still rebuilds the scene (6 ms at 2,000 nodes, every 16 ms while a run streams). Skipping the rebuild would need a change counter on the run state, which has public fields mutated in many places.
- Wire paths grew the full scene by about 15%.

## 2. `geometry.set`

`geometry.set` is a canvas command: it is listed by `command.list`, runs through `command.run`, the control port and the C ABI, is not journaled, is not an undo step, and leaves `lastError` alone.

Its params:

```
{faces: [{node, rev?, size: [w, h], pins?: {in?: [y | null], out?: [y | null]}, header?, rows?: [[x0, y0, x1, y1] | null],
          parts?: {title?, chevron?, icon?, menu?, enter?, preview?: rect, footer?: [rect]}}]}
```

It replies `{applied, stale, unknown}`. The core keeps what it is told (`Measured`), forgets faces gone from the scene, and clears everything on `graph.new` and `graph.open`.

**Decisions the plan left open:**

- **Pins and rows are lists in row order**, the order of the face's `inputs` and `outputs`, not maps by key. A collapsed group's rows can share a key, and a wire's `fromPin` and `toPin` already index them.
- **A stale report is kept and listed, not ignored.** The draft said a report with an older `rev` is ignored. During a run a face's `rev` changes every 16 ms, so its measurement might never land; an older measurement beats the default.
- **`rev` is optional.** The egui shell measures without revisions.
- **`unknown`** lists faces the graph doesn't have (at most 64), and they are skipped.
- **Unknown fields are refused**, as everywhere. Numbers are clamped to ±1e7, sizes to 0 to 1e6, and lists to 4,096 pins or rows a face and 100,000 faces a call.
- **The control channel may call it.** The draft's 5.3 said a script may not, and the brief overrode that. The egui shell reports every frame, so a script's report lasts until the next frame there.
- **An unmeasured face** is 220 × 160, with its pins estimated: header 28, rows 22.

## 3. Hit testing

`Snapshot::hit(x, y, slop)` checks in section 9.2's order. Each check uses graph units.

1. A pin: the nearest wireable one within `slop`, the topmost face winning a tie. The hit snaps to the pin's point.
2. A face: the topmost. Its part is a reported named part if one contains the point, else a reported row rect (`row` with its index), else the header (above `header`, default 28), else the body.
3. A bend point, within `max(slop, 5)`.
4. A net tag.
5. A frame's tab or resize handle.
6. The nearest wire within `slop`, prefiltered by its chain's bounds.
7. An open group's band, innermost first.
8. A frame's body.
9. Nothing.

**Decisions:**

- The topmost face is the last in the scene's order, which is published by id.
- One `slop` serves pins, wires and tags.
- A point that isn't a number hits nothing, and a slop that isn't one is 0.
- Tags, frames' tabs (at most 180 × 22) and groups' bands are estimated by the core, because their text is the shell's to measure and `geometry.set` doesn't carry them.
- Only a group's band is a part; its chevron and buttons are the shell's own controls.

The snapshot is made after a publish, only when the session's generation, the graph's revision, the geometry or the group entered changed, so a run's progress doesn't rebuild it.

**Tests** (`geometry::tests`):

- Every kind and part, each in order.
- Overlapping faces: the topmost wins.
- A zero-size face.
- NaN, infinite and ±1e30 points and slops, and a face at 1e30 sized 1e30.
- Unknown faces and pins.
- None panics, and every coordinate returned is finite.

## 4. Wire paths and the wire target

**Paths.** Every drawn wire has `path` in the scene: the start point, then three points a segment.

- A **routed** wire's chain is exactly the Catmull-Rom curve, which is cubic already. `smooth_path` now samples it, and its test is unchanged.
- A **plain** wire's chain is fitted to egui-snarl's degree-5 curve. The curve is ported in full from egui-snarl 0.12 (`wire_path::snarl_quintic`); snarl's two `unreachable!` branches become the last case, and NaN and infinities are tamed.
- **Four cubics, not the two that question 7 recommended.** Two Hermite-matched pieces strayed up to 2.2 graph units on steep wires. Four stay under 0.35 at usual lengths and 0.72 on a 5,000-unit wire (`claude-debug/native-n2/s3-fit-check.py`). A least-squares fit of two was worse. The test checks every case under 0.8 (forward) and 1.25 (backward).
- A wire drawn through a net has an empty path; its tags are drawn instead.
- Geometry is sent as `f64` rounded to a thousandth of a graph unit (reason under "Defects").

Open groups get `rect`: the egui algorithm, moved, with pad 14 and band 28. Marks get `at`, their wire's middle. The top level gets `view: {fit: {seq, rect} | null}`.

**The wire target.** `Snapshot::wire_target(face, output, pin, x, y, slop)` returns the nearest pin on the other side within `slop`.

- A pin that fits beats a nearer one that doesn't. `fits` is 1 when `wire.connect` takes the wire, 2 when only a Convert can, and 0 otherwise.
- `fits` is 0 for a hidden pin or a named port, for the same node, for a cycle (the snapshot keeps each node's downstream), and for a single value into a list input. List-ness comes from the new `Graph::mapped_nodes`, one pass upstream first; it is tested against `output_is_list` node by node.
- The type table is computed per distinct type pair from `accepts` and `convert_paths_where`, not per pin pair as the draft said (that would be n² pairs).
- **A Convert node's own rules are not precomputed**, so a wire to or from one shows as fitting; `wire.connect` has the last word at the drop.

## 5. Typed hot calls

```c
plum_hit plum_hit_test(plum_core, uint64_t doc, float x, float y, float slop);
plum_hit plum_wire_target(plum_core, uint64_t doc, uint64_t face, uint32_t side, uint32_t pin, float x, float y, float slop);
size_t   plum_wire_paths(plum_core, uint64_t doc, const uint64_t *faces, size_t n, float dx, float dy, float *out, size_t cap);
int32_t  plum_geometry_set(plum_core, uint64_t doc, const plum_face *faces, size_t n);
```

- **Reading the snapshot.** Each document's snapshot sits behind a mutex that is held only to clone or swap the `Arc`, so a reader never waits for the core's work. It is not literally lock-free.
- **`doc` 0** means the first document.
- **Unwinds.** Every entry's whole body is inside `catch_unwind`.
- **`SAFETY:` comments.** Each of the 6 new `unsafe` blocks has its own.
- **Pointer checks.** Every pointer is checked for null and alignment, and every count is bounded before any read.
- **JSON forms.** `geometry.hit`, `geometry.wireTarget` and `geometry.wirePaths` are control methods; `geometry.set` is the command.

| Median of 400, release, M4 Max | 500 nodes | 2,000 nodes |
|---|---|---|
| `plum_hit_test` | 0.50 µs | 1.63 µs |
| `plum_wire_target` | 0.33 µs | 1.38 µs |
| `plum_wire_paths`, 20 faces dragged | 5.9 µs (p99 70 µs) | 5.3 µs (p99 143 µs) |
| `plum_geometry_set` of every face, the call | 25 µs | 77 µs |
| ...until the snapshot has it | 5.7 ms | 14.3 ms |

**Tests** (`crates/ffi/tests/geometry.rs`, 2 tests):

- A shell measures typed, and the measured pin, header, body, nothing and the wire are hit.
- The JSON forms agree with the typed calls.
- The wire target fits, and `plum_wire_paths` gives the size first, the floats after, and writes nothing past `cap`.
- The command form, and `canvas.fit` from the measured sizes.
- Hostile calls: no core, no document, a closed core, sides 0, 3 and `u32::MAX`, NaN and huge points, null and `usize::MAX` counts, a face claiming 2^40 pins, NaN and negative sizes, and malformed JSON forms.

**Not built:** JNI forms of the hot calls (N5).

## 6. egui consumes it

**Done:**

- **Geometry is reported every frame.** `EditorApp::report_geometry` hands the core each drawn face's size and pin heights (`Canvas::measured_faces`), in process. Only what changed is kept, and nothing reports headers, rows or parts yet.
- **The snarl is built from the scene's structure.** `Canvas::sync` takes the faces (order, positions, collapsed views) and the wires (face and pin indices; plain, routed or through a net, with the nets' tag offsets) from `scene::build` (`canvas::structure`). The net tags come from it too.
  - Each face's rows are still the catalogue's (`input_rows`), because the widgets edit through the catalogue's `Port`, which the scene doesn't carry.
  - `groups_view::wire_ends` and the proxy index helpers are gone.
- **Align and Distribute are core commands**, `selection.align {edge}` and `selection.distribute {axis}`. They take the selection and the sizes the core has, and the menu model's items carry them. `EditorApp::selection_params`, `arrange`, `frame_selection` and `Canvas::selection_sizes` are gone.
- **Frame Selection** is `Host::frame_selection`'s new default: the selection plus `Host::sizes`.
- **Tidy** reads the core's measurements (`Host::measured`), for egui and the C ABI.
- **Fit** is `canvas.fit`'s rect. The core computes it, and egui fits its view to it (`Canvas::fit_to`), falling back to its own waiting fit while some face is still unmeasured.

**Not done: the snarl selection bridge stays.** egui-snarl owns the canvas's pointer input. A drag on empty canvas starts its own marquee and keeps its selection privately, and it offers no hook to hand the marquee to the shell. Replacing the bridge means turning snarl's selection input off and recognising the marquee and clicks in the shell against the core's hit test. That is gesture recognition, which belongs with slice 4's `ui.gesture`. The core's hit test now makes it possible.

**Also not done:** egui draws plain wires with snarl, not the core's chain (up to 0.72 units apart). Clicks are still hit-tested by egui's own code.

## Every behaviour change on the C ABI and the control port

On the egui editor's control port, the C ABI and the C ABI's control port unless said:

**New commands:**

1. `geometry.set`, `selection.align` and `selection.distribute`. `command.list` gains three rows, appended after `canvas.reveal`; the first 133 rows are unchanged except `canvas.fit`'s params text.

**New methods:**

2. `geometry.hit`, `geometry.wireTarget` and `geometry.wirePaths`. Before, they were `unknown method`.

**Changed replies:**

3. `canvas.fit` replies `{fit: true, seq, rect}`, where it replied `{fit: true}`, and sets the scene's `view.fit`.
4. **C ABI only:** `canvas.fit` is no longer refused. It used to say "canvas.fit isn't available from this shell".
5. The scene changes shape:
   - Wires have `path`, groups `rect`, marks `at`, and the top level `view`.
   - A node moved now also upserts its wires in a delta.
   - Geometry changes bump the wires' `rev`.
6. `menu.get`: Edit › Align › and Distribute › items gain `command` (`selection.align` and `selection.distribute`), with the same ids, labels, params and enabled states. In egui the menu dumps are identical.

**C ABI only, changed behaviour:**

7. `ui.invoke frame.add` frames the selection at the core's sizes. Before, it always said "select nodes to frame first".
8. `ui.invoke graph.tidy` uses the sizes and pins the shell reported. Before, it estimated them.
9. The feed's scene `changed` and the snapshot arrive within 16 ms of the core going quiet. Before, they could take up to 50 ms.
10. A `scene.get` reply no longer waits for a second build. Its `changed` item is no longer pushed redundantly in the same turn.

**New C exports:**

11. `plum_hit_test`, `plum_wire_target`, `plum_wire_paths` and `plum_geometry_set`, with the types `plum_hit` and `plum_face` (`include/plumcraft.h`). `PLUM_ABI_VERSION` stays 1.

**Egui only, no reply change:**

12. Align, Distribute and Frame Selection run through the core with the sizes egui reported. The arrangement should be the same, because the sizes are the same.
13. Nothing else: the other 39 of the 41 recorded replies are identical (`claude-debug/native-n2/s3-probe-after2`, compared with `s2-probe-after`).

## Fixtures

`crates/editor-core/fixtures/scene/full.json` and `delta.json` were rewritten with `PLUMCRAFT_WRITE_FIXTURES=1`.

**Shape:**

- Every wire has `path`: 13 points if plain, 7 if routed, empty through a net.
- Every group has `rect`, null when collapsed.
- Every mark has `at`; the fixture has no marks, since the hooks are process-global.
- The top level has `view`.

**Content:**

- Two more nodes in an open group, "open pair", with its frame.
- Two faces measured: the first node, whose wires start at its measured pins, and the open pair's source.
- A fit request, `seq` 1.

**Test:** the decode test checks every drawn wire's path shape, that the open group has a rect and the collapsed one none, and the fit. On a mismatch it now names the first differing path. The pre-slice copies are `claude-debug/native-n2/s3-full-before.json` and `s3-delta-before.json`.

## The gate

| Check | Result |
|---|---|
| `cargo nextest run --all-features --no-fail-fast`, b3, fresh folders (`s3-final1-b3.log`) | 1,433 of 1,434; failed: `plumcraft-craft::params_coverage hidden_params_match_the_baseline` (the known local-only failure) |
| `cargo clippy --all-targets --all-features -- -D warnings` | clean |
| `cargo fmt --all --check` | clean |
| `scripts/check-hash-gates.sh` / `check-core-toolkit-free.sh` | ok / ok |
| `uv run ruff check .` / `uv run pytest -q` (b3) | clean / 224 of 224 |
| 41 control replies (`s5_probe.py`, compared with slice 2's capture) | only `command.list` differs, as listed |
| 124 menu dumps | identical to slice 2's |
| Offscreen pictures (`s2-shot.py`) | unrun: two baseline runs differ by 1,474 pixels, after against baseline 4,511 and 4,824; ran: 25,092 between baselines, after 25,768 and 2,933. Within noise; looked at side by side, the same |
| `prefs.json` SHA-256 | `9221fa66…` before and after every editor run |
| Free disk | 122 GB |

**New tests (16):**

- `geometry::tests`: 10.
- `wire_path::chain_tests`: 3.
- `graph::every_title_at_once_is_each_title`: 1. `lists_feed_lists_and_map_singles` was extended to cover `mapped_nodes`.
- `crates/ffi/tests/geometry.rs`: 2.

**Ignored measurements:** `scene::tests::measure_scene_sizes` and `measure_scene::hot_calls_cost`.

**Existing tests edited, because this slice changes what they check:**

- `crates/ffi/tests/selection.rs`: `canvas.fit` was asserted refused over the C ABI. It now asserts the fit's reply; the palette and reveal are still refused.
- `crates/ffi/tests/scene.rs`: "a node moved: the delta has no wires" now asserts its one wire is in the delta and its path moved.

No test failed intermittently.

## Defects I introduced and caught

- **Two of my geometry tests read `nid` from an unpublished build**, where it is always 0. The first run caught it; the tests now read the published ids.
- **The fixture didn't read back equal to itself.** A widened `f32` prints 17 digits, and serde_json's default parser isn't correctly rounded, so it read back one bit off. The fixture test caught it. Geometry is now `f64` rounded to a thousandth, which reads back exactly and is shorter.
- **The hot-call measurement's wait passed before the geometry landed.** The default 220 × 160 face contains the point too. An implausible 2.5 µs gave it away; it now waits on the measured pin's x.
- **The 50 ms tick delay** was found by that measurement once it was right.
- **A vacuous assertion** (`to_bits() <= u32::MAX`) was caught by clippy and rewritten to check that every point float is finite.
- **New commands inserted mid-list shifted `canvas.reveal`'s row.** The control reply diff caught it; they are now appended.

## Open questions for slice 4

1. **`ui.gesture`** can be built on `Snapshot::hit` and `wire_target`. Doing it would also let egui drop the selection bridge and its own click hit test.
2. **egui drawing the core's chains:** hide snarl's plain wires and paint the chains, keeping snarl's wire hover and click.
3. **Measured tags, tabs and bands:** should `geometry.set` carry `tags` and frame or group header rects, instead of the core's estimates?
4. **A fit after `graph.open`:** the core can't size unmeasured faces. Should it ask for a fit later, once they are measured?
5. **Draft 2 items:**
   - Stale reports applied; pins as lists.
   - `plum_hit`'s `status`, `item` and `x, y`.
   - `plum_wire_target`'s (face, side, pin).
   - Four cubics.
   - nids cut to 32 bits in `plum_wire_paths`.
   - Question 13, token-only colours, and a cached full scene to cut the 5.2 MB.
6. **Schema:** `geometry.rs`'s and `scene.rs`'s serde types are the wire shape for `schema/shell1.json`.

## What I did not verify

- No native shell (Swift or Kotlin) called the new exports. They are tested from Rust through the C ABI only.
- Nothing ran on Linux. The brief asks for a run on the Linux test host before merging; it wasn't done here.
- The four-cubic fit was checked on 10 wire shapes, not exhaustively.
- Measurements are from one machine (M4 Max), mostly single runs of medians.
