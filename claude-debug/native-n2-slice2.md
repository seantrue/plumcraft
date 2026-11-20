---
title: "N2 slice 2: the core publishes the scene"
date: 2026-11-19
---

# N2 slice 2: the core publishes the scene

Branch `native/n2-scene`, cut from `native/n2-selection` at `d99e2bf`. Nothing is committed; the changes are in the working tree for review.

## Summary

The editor core builds the canvas's scene as data (`crates/editor-core/src/scene.rs`): every face with its header, input and output rows, widgets, preview, footer and run state, every wire with the faces and pins it joins, nets, frames, groups, the application's wire marks, the selection, the group entered and the run animation's switches. `scene::Published` keeps it with a revision per item; `scene.get {since?}` serves the whole scene or what changed, through the one dispatcher, so it works on the egui editor's control port, the C ABI and the C ABI's control port. Over the C ABI the feed says `changed {doc, domain: "scene", gen}`, at most one per document per poll. Shared fixtures (`crates/editor-core/fixtures/scene/full.json`, `delta.json`) cover every row source, widget kind, footer kind and run state, and a Rust test decodes and checks them.

The egui editor draws its faces from the scene: titles, header (icon, state dot or spinner, badges, engine mark, status slot, the icon's hover card), border and tints, every input row (all five sources, captions, dim, named tags, the bundle badge, the widget's value), output rows, pins, the footer (every item), and the preview's size, faded state, summary and note. What it still computes itself is listed below; mostly what needs snarl's structure or measured geometry.

**Nothing in the gate failed beyond the known local-only failure.** Suite as CI runs it (b3): 1,407 of 1,408 before, 1,417 of 1,418 after (the one failure is `hidden_params_match_the_baseline`, both times; 10 new tests, 2 new ignored measurements). The brief said the baseline is 1,406 of 1,407; on this checkout it ran 1,408 tests. The 41 control replies: 0 differences. 124 menu dumps: identical. Pictures: the same but for run timings. No existing test was edited.

**What did not go as well:** the scene is large and its rebuild is the cost. At 500 nodes and 800 wires a full `scene.get` is 1.1 MB and 25 ms (vs 96 KB and 1.5 ms for `graph.get`); a delta after one run-state change is 2 KB and 4.5 ms, nearly all rebuild and hashing, not JSON. At 2,000 nodes the delta takes 36 ms, over the 16 ms budget. The first run of the gate also caught a race in my own C ABI test (it read the feed through a helper that drops `changed`); the test was rewritten before the second run, and 8 back-to-back runs of the C ABI suite then passed.

## The scene's shape, as built

Top level: `{shell: 1, doc, gen, full, nodes, wires, nets, frames, groups, marks, selection: {nodes, groups, net}, focus: {group, path: [{id, name}]} | null, anim: {on, replay, generation, holdMs, fadeMs, pulseMs}}`. Each item has `id`, `nid` (interned per kind, never reused in a document's life) and `rev` (the generation it last changed at). A delta is `{shell, doc, gen, full: false, since, upsert: {nodes, …}, remove: {nodes: [id], …}, selection, focus, anim}`.

Colours are `{token?, color: [r, g, b, a]}` (premultiplied, the core's `Color`). One face, cut down from the fixture (`Every widget`, not run, one row of each of three kinds):

```json
{"id": "n1", "nid": 1, "rev": 1, "kind": "t.all", "proxy": null, "pos": [0.0, 0.0], "view": "full",
 "watched": false, "menu": "node", "shown": null,
 "tint": {"header": {"color": [134, 207, 218, 255]}, "body": {"color": [239, 246, 243, 255]}},
 "border": null,
 "run": {"state": null, "fraction": null, "stale": false, "notRun": true},
 "header": {"open": true, "icon": "sliders", "enter": false,
   "about": {"name": "Every widget", "description": "One input of each widget kind",
             "family": "Effects: Change a picture", "path": "Test › Widgets", "id": "t.all", "known": true},
   "state": {"kind": "none", "paint": null, "hover": null},
   "title": {"text": "Every widget", "watch": true}, "badges": [], "engine": null,
   "status": [{"text": "not run", "paint": {"token": "header_text", "color": [34, 32, 28, 255]},
               "strong": false, "hover": "No run has completed this node yet. Run to compute it."}]},
 "inputs": [
   {"key": "gain", "node": "n1", "label": "gain", "list": false, "ty": "FLOAT", "source": "own", "dim": false,
    "pin": {"shape": "circle", "fill": {"token": "pin_default", "color": [127, 121, 111, 255]},
            "wire": {"token": "wire", "color": [125, 120, 111, 255]}, "wireable": true},
    "text": null, "hover": "How loud", "from": null, "named": null, "badge": null, "inactive": null,
    "widget": {"kind": "number", "value": 1.25, "default": 0.375, "blank": false, "clearable": false,
               "min": 0.0, "max": 4.0, "softMin": 0.0, "softMax": 2.0, "step": null, "decimals": 3,
               "int": false, "choices": [], "pathMode": null, "colorFormat": null, "hover": "How loud"}},
   {"key": "pic", "node": "n1", "label": "pic", "ty": "x.t.pic", "source": "socket",
    "text": "pic · x.t.pic", "hover": "Type: x.t.pic", "widget": null, "…": "…"},
   {"key": "curve", "node": "n1", "label": "curve", "ty": "x.t.curve", "source": "readonly",
    "text": "curve: 2 points", "widget": null, "…": "…"}],
 "outputs": [{"key": "out", "node": "n1", "label": "out · x.t.pic", "list": false, "ty": "x.t.pic",
              "pin": {"shape": "square", "fill": {"color": [204, 68, 102, 255]}, "wire": {"color": [204, 68, 102, 255]}, "wireable": true},
              "hover": "Type: x.t.pic"}],
 "preview": null,
 "footer": [{"kind": "fields", "text": "1 more field", "hover": "Show the fields this choice doesn't use", "all": true},
            {"kind": "editor", "label": "Edit Dial…", "hover": "Edit the dial in its window"}]}
```

(Values from `full.json`; `"…": "…"` marks fields left out here.) Row sources: `wired`, `socket`, `inherited`, `own`, `readonly`. Widget kinds: `number` (`int` for INT), `toggle`, `choice`, `multichoice`, `text`, `multiline`, `path`, `color`. Previews: `rgba8 {handle, w, h, size, hoverSize, items, summary, note?, faded}`, `peaks {handle, channels, size, note?, faded}`. Footer kinds, in order: `fields`, `convert`, `editor`, `progress`, `error`, `save`, `excerpt`, `engineText`, `appText`. Wires: `{id: "node.port" of the input, from, to, fromFace, fromPin, toFace, toPin, paint, kind: plain | routed | net, via, slice?, net?}`. Nets: `{id, key, paint, selected, driver: {from, face, pin, at}, uses: [{to, face, pin, at}]}`. Groups `{id, name, collapsed, parent?, depth, proxy}`, frames `{id, title, pos, size, paint}`, marks `{id, label, tone, paint, tooltip?}`.

## Methods and feed items

| What | Where | Behaviour |
|---|---|---|
| `scene.get {}` | every shell, one dispatcher (`control::now`) | Rebuilds, publishes (revisions), returns the whole scene, `full: true` |
| `scene.get {since: g}` | the same | `full: false`; `upsert` holds items with `rev > g`, `remove` the ids gone since; `full: true` when `g` is older than the 256 generations kept, newer than the scene, or nothing was published |
| `changed {doc, domain: "scene", gen}` | C ABI `plum_poll` | Once a shell has called `scene.get` for that document; the core looks at the scene when idle and at most every 16 ms while busy; a newer change replaces one not yet polled, so a poll has at most one per document |

Two new `Shell` trait methods, both defaulted to none: `scene()` (build the scene now) and `scene_published()` (where revisions live). `Document` implements them in the default theme with no run animation; `EditorApp` with its own theme, run animation, application lines and value editors.

## What moved, and where

Measured with `git diff --numstat` (tracked files) and `wc -l` (new files). Tracked: 524 lines added, 641 removed. New: 2,527 Rust lines (of which 497 are `scene.rs`'s tests and 307 the two new C ABI test files) and 5,289 lines of JSON fixtures.

| File | Added | Removed | What |
|---|---|---|---|
| `crates/editor-core/src/scene.rs` (new) | 2,142 | 0 | The scene's types, `build`, `faces`, `Published` (revisions, removals, deltas), `parse_since`; 1,645 lines of code and 497 of tests (8 tests and 1 ignored measurement) |
| `crates/editor-core/src/widget_text.rs` (new) | 100 | 0 | `value_text`, `choice_text`, `BLANK`, `decimals_of`, `float_decimals`, `has_editor`, `disabled_reason`, `inactive_note`, `typed_label`, `typed_tip`, moved from `crates/ui` |
| `crates/editor-core/src/bundle.rs` (new) | 52 | 0 | `Source`, `provenance`, `source`, `bundle_word`, moved from `crates/ui/src/bundle_view.rs` |
| `crates/editor-core/src/rows.rs` | 100 | 3 | `proxy_inputs` (a collapsed group's rows) and `proxy_node_id`, moved from `groups_view.rs`; `input_rows_with`, `shown_rows_with` (a wire index instead of scanning the wires per row) |
| `crates/editor-core/src/document.rs` | 51 | 0 | `Document::scene` (the `Published`), `build_scene`, `scene_inputs`, the `Shell` methods |
| `crates/editor-core/src/control.rs` | 24 | 0 | `scene.get`; the two `Shell` methods |
| `crates/editor-core/src/lib.rs` | 3 | 0 | Three modules |
| `crates/ffi/src/core.rs` | 40 | 2 | `note_scene`, `SCENE_EVERY`, the coalesced `changed` item |
| `crates/ffi/tests/scene.rs` (new) | 224 | 0 | Read the scene, change things, read only what changed; a run at most one change a poll; removal; a raw hostile sweep |
| `crates/ffi/tests/measure_scene.rs` (new) | 83 | 0 | The C ABI measurement (ignored) |
| `crates/editor-core/fixtures/scene/` (new) | 5,289 | 0 | `full.json`, `delta.json` |
| `crates/ui/src/canvas.rs` | 226 | 413 | Faces drawn from the scene (below); `engine_of`, `pin`, `thumb_note`, `thumb_faded_now`, `own_editor`, `savable_output`, `node_title`, `icon_name`, `view_of`, `def`, `thumb_items`, `NODE_TEXT_CHARS` gone; `thumb_faded` re-exported from the core; `show` no longer takes `watched` |
| `crates/ui/src/groups_view.rs` | 9 | 99 | `proxy_node` uses the core's rows; `header_badges` delegates to `scene::badges_of` |
| `crates/ui/src/widgets.rs` | 3 | 77 | Re-exports the core's widget text |
| `crates/ui/src/bundle_view.rs` | 5 | 41 | Re-exports the core's bundle source |
| `crates/ui/src/lib.rs` | 25 | 3 | `scene_now`, `scene_faces`, `shown_anim`; the faces built each frame and lent to the canvas |
| `crates/ui/src/control.rs` | 6 | 0 | `EditorApp`'s `Shell::scene` and `scene_published` |
| `crates/ui/src/value_editors.rs` | 8 | 0 | `scene_editors` (the registry as the scene takes it, with each summary) |
| `docs/shell-interface.md` | 23 | 3 | 6.3, 7.1, an "as built" block at the top of 8, 8.12, 16.2, question 13 |
| `docs/control-protocol.md` | 1 | 0 | `scene.get` |

## What egui draws from the scene, and what it still computes

From the scene (`RunView::faces`, built once a frame by `scene::faces`):

- The title, the header icon and its hover card (and the node's hover popup), the state spot (spinner or dot, colour, hover), badges (export mark, group output), the engine mark, the status slot's words, colours, weight and hovers.
- The body and header tints, the border (except a revealed node's highlight, which stays the canvas's).
- Every input row: the five sources and their words, captions, dim captions, hovers, named-port tags, the bundle's override badge (its Reset button too), the widget's current value. Pins: shape, fill, wire colour, hidden.
- Output rows: label, hover, pin.
- The footer, every item, and whether there is one (`has_footer`).
- The preview's size (`has_body`, the outputs' indent), faded, summary, note, hover size.

Still computed in `crates/ui`, and why:

- **The snarl's structure** (`Canvas::sync`): which nodes, rows (`CanvasNode.inputs` with their `Port`, which `port_widget` needs to draw and edit) and wires snarl holds, and the routed and net-label wires' classification and colour. The scene has all of it (`wires[].kind`, `paint`, pins), but `sync` runs on the session's generation with no scene to hand, and swapping it means building the snarl from scene items; best done with slice 3, which replaces the wire drawing anyway.
- **Net tags** (`net_view`): drivers and uses come from `sync`'s own list, not the scene's `nets` (same reason).
- **The run fill** (`run_fill`, the pulse and fade): per frame, the shell's by contract (8.6), from the scene's tint and the run animation.
- **Widgets themselves** (`widgets::port_widget`): drawn from the catalogue's `Port` and the scene's value; the widget spec (`Widget`) is not yet read by egui (egui would draw the same from it, but the edit buffers are keyed to the `Port`).
- **Texture and waveform data**: read from the run state's `Rendition` (the scene's `handle` is the same `DataRef`).
- **Menus** on rows and nodes: from the menu model, as before.
- **Open groups' frames, frames, slices, marks**: painted from the graph and measured rects (slice 3).

## Measurements

Release build, Apple M4 Max, Mac otherwise idle. Logs: `claude-debug/native-n2/s2-measure-core2.txt`, `s2-measure-ffi2.txt` (first runs, before the wire index: `s2-measure-core.txt`, `s2-measure-ffi.txt`).

| What | 500 nodes, 800 wires | 2,000 nodes, 3,200 wires | Budget (note 24, 10) |
|---|---|---|---|
| `graph.get` request to decoded reply, for comparison (N0's test, rerun) | 1.45 ms, 96 KB | 5.98 ms, 388 KB | |
| `scene.get {}` request to decoded reply, median of 24 | 24.8 ms, 1.1 MB | 101 ms, 4.4 MB | none stated; at open only |
| first `scene.get` (nids, every item serialised) | 33.9 ms | 131 ms | |
| `scene.get {since}` after one node's run changed (1 face, 2 KB) | 4.5 ms | 36 ms | 16 ms command to change on screen |
| `scene.get {since}` with nothing changed | 4.5 ms | 36 ms | |
| Core only (test catalogue, every node run; `scene.core.500`): `build` | 1.5 ms | not measured | |
| Core only: first publish (hash and serialise every item) | 6.1 ms | | |
| Core only: rebuild and publish after one state change | 4.2 ms | | |
| Core only: delta to bytes | 0.02 ms, 1.9 KB | | |
| Core only: the whole scene to bytes | 5.1 ms, 1.34 MB | | |
| egui's per-frame cost of the faces (`faces`, release) | 1.5 ms | | 8.3 ms a frame at 120 Hz |

Against the budgets:

- **A command to its scene change: met at 500 nodes** (4.5 ms core side, a 2 KB delta), **missed at 2,000** by 2.25 times (36 ms). The cost is the rebuild and hashing every item, not JSON: the delta's JSON is 0.02 ms. It grows faster than the graph (8 times the time for 4 times the nodes), which I did not track down; a first cause, every row looking up its wire in the graph's list, is fixed (the build halved, 3.1 to 1.5 ms).
- **Run progress, one scene change a frame: met as a rate** (the core looks at most every 16 ms and a poll holds one change), but at 500 nodes each look costs about 4.2 ms of the core thread, about a quarter of a frame while a run streams; at 2,000 it would be more than a frame.
- **The whole scene in JSON misses a frame** at 500 nodes: 25 ms request to reply and 1.1 MB, 11.6 times `graph.get`'s bytes, against N0's "JSON is fast enough for the scene, sent by revision". By revision it is (2 KB a change); the full scene, sent at open, is 17 times N0's 1.45 ms. Per the brief I did not switch format. Cheaper ways, all JSON: leave out null and empty fields (most of each face), send token names without the colour where there is one (question 13), and keep each published item's text so a full reply concatenates instead of cloning and re-serialising.
- **egui's frame**: building the faces adds about 1.5 ms a frame at 500 nodes in release (the canvas no longer computes the same words itself, so the net cost is less; not measured separately). In a debug build it is several times that.

## Every change in what the C ABI or the control channel does

On the egui editor's control port, the C ABI and the C ABI's control port, unless said:

1. New method `scene.get {since?}` (above). Before: `unknown method "scene.get"`.
2. **C ABI only:** `plum_poll`'s `changed` can hold `{doc, domain: "scene", gen}`, once the shell has called `scene.get` for that document; at most one a document a poll. A shell that never calls `scene.get` sees no change in the feed and costs the core nothing.
3. Nothing else: the 41 recorded replies are identical (0 differences with the run timings masked), `command.list` unchanged, 124 menu dumps unchanged.

## The gate

| Check | Before | After |
|---|---|---|
| `cargo nextest run --all-features --no-fail-fast`, CI's b3 environment, fresh folders | 1,407 of 1,408; failed: `plumcraft-craft::params_coverage hidden_params_match_the_baseline` (`s2-base-b3.log`) | First run: 1,416 of 1,418, the known failure and my `ffi::scene` test (a race in the test, fixed, below) (`s2-final-b3.log`); second: 1,417 of 1,418, the known failure only (`s2-final2-b3.log`) |
| `cargo clippy --all-targets --all-features -- -D warnings` | | clean |
| `cargo fmt --all --check` | | clean |
| `scripts/check-core-toolkit-free.sh` | | ok |
| `scripts/check-hash-gates.sh` | | ok |
| `uv run pytest` (b3) and `uv run ruff check .` | | 224 of 224; clean |
| `s5_probe.py`, 41 replies | two baseline runs: 0 differences | 0 differences (a first run from a copied binary differed because the engine binary is looked for beside the editor; rerun from `target/debug`) |
| Menu dumps, 124 files | identical to slice 1's `s1-menus-after2` | identical |
| Picture (`s2-shot.py`: 10 nodes of 9 kinds, a collapsed group, Tidy, fit, offscreen, before and after a run) | two baseline runs differ by 4,615 pixels (unrun) and 37,476 (ran): the offscreen picture is not deterministic run to run | after differs from baseline by 10 and 1,472 pixels (unrun) and 22,474 and 42,213 (ran), within the noise; looked at side by side, the only visible differences are run timings (0.08 against 0.09 ms) |
| `prefs.json` SHA-256 | `9221fa66…a727` | `9221fa66…a727` |
| Free disk | 47 GB | 16 GB (a release build for the measurements) |

**The race in my test:** `crates/ffi/tests/scene.rs` first waited for the feed's scene change after calls made through the shared `Client`, whose poll drops `changed`; under the full suite the change came in the same poll as a reply and was lost. Every call after the first `scene.get` now polls through the test's own `Feed`, which notes every scene change. No core change. Eight back-to-back runs of the C ABI suite passed, and the second full suite.

New tests: `scene::tests` (8: every row and widget kind; every run state as drawn; revisions move only for what changed, nids kept, removals; deltas older than kept give everything; focus and its breadcrumb; the application's line and the watched border; hostile `since`; the shared fixtures decode and are what is built) and `crates/ffi/tests/scene.rs` (2: the flow above over the C ABI, including at most one scene change a poll during a run; a raw hostile sweep of bytes and shapes). No millisecond limits.

## What I did not do

- **Geometry**: wire paths, open groups' frames, marks' places, the fit rect, hit testing (slice 3).
- **egui's `sync` and net tags from the scene** (above).
- **`ui.inspect` from the scene**: unchanged; the contract maps 7.2's `scene` domain to `ui.inspect`'s `graph`, `groups`, `frames`, which still come from where they did.
- **`state.get`, `core.hello`, the `view` and `selection` domains** as separate feed domains: the selection and focus ride in the scene.
- **Question 13 as decided** (tokens alone where there is one): every colour carries its value too.
- **The hostile sweep in `crates/proto/tests/hostile.rs`**: the new parser is the core's; it has its own hostile tests (`scene::tests::hostile_since_is_refused`, the C ABI raw sweep).
- **Swift and Kotlin decoders of the fixtures**: the files are there for them.
- **Text excerpt as a preview**: it is a footer item, as egui draws it.
- **Per-face `selected`, `highlight`, `buttons`, the scene's `clock` and `grid`.**

## What I am unsure of

- **The cost model.** Rebuilding and hashing the whole scene on every look is simple and correct, but it is what costs 4.5 ms at 500 and 36 ms at 2,000. Dirty tracking from the session (which nodes an edit or event touched) would make a delta proportional to the change; the run state has no change counter to key on.
- **egui building faces every frame** (1.5 ms release at 500 nodes): it keeps egui exactly in step with the run animation's time (faded previews ahead of a replay), but it is new work per frame. Caching per session generation would miss run events.
- **`nid` per kind**: a face and a wire can share a number. The contract said "an interner"; hot calls name the kind anyway.
- **`Paint` on every colour** makes the scene large and every face change with the theme.
- **`proxy` faces' header `about`** is a sentence pair, not the node type card; a native shell may want structure.

## Where the contract disagreed with the code

- 8.3's header `chevron`, `title.tap`, `title.doubleTap`, `buttons` are `open`, `title.watch`, `enter` (the commands are implied); `about` is the header's.
- 8.3's `shown: {state, since}` is a plain string, and the face gains `run` (the real state).
- 8.4's `member` is `node`; `named` carries its paint and hover; `inherited` rows have `from`.
- 8.7: an excerpt is a footer item, not a preview `kind`; `preview.tap` is implied.
- 8.8's wire has faces and pin indices, `kind`, and no `path`, `width`, `hidden` (a `net` wire is drawn through its tags).
- 8.12: a delta also has `since`, and the small top-level parts come every time.
- 7.3 asks scene changes coalesced to `display.frame_ms`; built as a fixed 16 ms (`SCENE_EVERY`).
- 16.2 named `scene-*.json`; built as `scene/full.json` and `scene/delta.json`.

## For slices 3 and 4

- **Slice 3 (geometry):** `geometry.set` should key by face `id` and check `rev` as 6.3 says; the scene's `rev` is the generation the face last changed, so a shell measures again exactly when it moves. Wire paths go on `Wire` (add `path`), open group rects on `Group`, mark places on `Mark`. Hot calls want a snapshot of faces' sizes and pins by `nid`; `Published` already interns. Then egui's `sync` can build the snarl from the scene's faces and wires and its own net tags can go.
- **Slice 3, performance:** the rebuild cost above is the thing to fix before more goes into the scene: look for the remaining superlinear part (2,000 nodes cost 8 times 500), and consider leaving out nulls and keeping published items as text.
- **Slice 4 (`ui.gesture`, schema, conformance):** the schema can be generated from `scene.rs`'s serde types (they are the wire shape); the conformance suite can read `scene.get` instead of `ui.inspect` for faces; Draft 2 should settle question 13 and `nid` per kind.
