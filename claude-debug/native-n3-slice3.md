---
title: "N3 slice 3: the layer canvas and the Mac app"
date: 2026-11-20
---

# N3 slice 3: the layer canvas and the Mac app

Branch `native/n3-canvas`, cut from `origin/feat/native-n0`. Nothing is committed; the changes are in the working tree for review. `docs/BUGS.md` (the controller's edit) and `AI.md` were not touched.

## Summary

All seven items were done; item 7's hitch budget is neither shown met nor shown missed (below).

1. **The layer canvas** is a kit target, `PlumCraftCanvas`: a CALayer per face drawn once on a worker, a CAShapeLayer per wire with its frame tight around the core's cubic chain, frames, open groups, net tags and marks, all under one transformed layer. It follows `DocumentModel.onChange` by nid, so a one-node change lays out, reports and draws one face (tested). AppKit and UIKit hosts behind `#if`; the target compiles for macOS, iOS device and iOS simulator with zero warnings.
2. **Measure and report:** a face whose `rev` changed is laid out with Core Text on a worker and reported typed (`plum_geometry_set`) from that worker; nothing is measured on the main thread or per frame. A test checks that the core's rects, pins and wire ends are what the canvas drew.
3. **Pan and zoom** are the canvas's own transform. Two-finger scroll keeps the system's momentum and meets a rubber band past the content. Pinch and ⌘-scroll zoom about the pointer, and ⌘+, ⌘−, ⌘0 and Fit (⌘9) zoom from the menu. Smart zoom fits the node under the pointer, and a second one goes back. Space-drag and middle drag pan. The core's `view.fit` is taken once per `seq`.
4. **Selection:** click, Shift-click and ⌘-click are `ui.gesture` taps; a double click is a double tap. A drag goes through `ui.gesture`'s drag and the core decides what it is: a marquee on empty canvas, a face move whose wires are re-pathed each frame from `plum_wire_paths`, or a new wire drawn to the pointer. Its moves are typed. Hover rings the pin and widens the wire under the pointer, from the typed hit test.
5. **The Mac app**, `native/apple/PlumCraftMac`, is document-based: `NSDocument` per graph file; File › Open, Open Recent, the Dock and Finder, a drop on a window, path arguments; one window per document, several at once (checked: two files opened through LaunchServices, two windows). The title bar shows the file's name, its proxy icon and the zoom. It has a minimal menu, follows light and dark, quits on ⌘Q or TERM, and is signed ad hoc.
6. **The read half of the conformance suite against the running app**, which measures its own faces: 11 of 11 passed and 1 skipped on the text engine; the skipped picture case passed on an app opened on the array engine. The edit half passes too (12 of 12, informational).
7. **Budgets** were measured at 500 and 2,000 nodes on real scenes (below). The hitch numbers vary up to tenfold run to run on today's loaded machine, and the N0 spike, run beside the app as a control, varied the same way.

**One core change** (with tests and the contract): over the C ABI, `view.theme {name}` now sets the theme the document's scenes are built in, and `theme.get` follows. Before, the C ABI core accepted the command and ignored it. This is how the Mac app follows the system's appearance.

## How the canvas is built

| Piece | What | Redrawn when |
|---|---|---|
| Face layer (`CALayer`, one per face nid) | A picture of the whole face (header, rows, pins on its edges, widgets at rest, the preview's box, the footer, the border), drawn by `FaceRenderer` with Core Graphics and Core Text into a bitmap reaching 8 graph units past the face for pins and border | Its `rev` changed (layout, report and picture); the theme changed (picture only); the view settled at a zoom whose raster scale differs, for visible faces only |
| Selection | A `CAShapeLayer` sublayer of the face, an accent outline | Never redrawn; shown or hidden when the selection changes |
| Run wash | A sublayer over the body in the shown state's colour at contract 8.6's amount; running pulses with a Core Animation opacity animation | When the face changes; the pulse is the compositor's. Over the C ABI `anim.on` is false (no run animation in that core), so washes don't show yet; the border, status and footer show run state |
| Wire (`CAShapeLayer`, one per wire nid) | The scene's cubic chain (start, then three points a segment), its frame tight around it (15.2), stroked 2.5 in the wire's colour; a routed wire's bend points as rings; a net-drawn wire has no path | The wire's `rev` changed (the core recomputes paths after a report); during a face drag, the moving faces' wires from `plum_wire_paths` each frame |
| Frames, open groups | A layer each: tinted body and border, a tab or band picture with the title | When the item changes, or the theme does |
| Net tags, marks | Small pictures placed where the core's hit test estimates them (6.6 a character plus 17 wide, 17 high) | When nets or faces change |
| Overlay | Hover ring, marquee, the wire being dragged | Per pointer move, as shape paths only |

**Workers and threads.** `LayerCanvas` is main-actor. When an apply names faces, it sends one batch to a detached task. There `DispatchQueue.concurrentPerform` lays out (`FaceLayout.make`) and draws (`FaceRenderer.render`) each face, and the measurements go to the core from the worker (`PlumCore` is `Sendable`, and `plum_geometry_set` copies before it returns). The pictures come back to the main actor and are set in one `CATransaction`. One batch is out at a time; faces that change meanwhile wait for the next. A face that changed again while drawn is redone.

**Text.** Core Text (`CTLine`, `CTTypesetter`) with the system UI font at the contract's sizes (title 14 semibold, port 13, small 12, mono 12). It is measured only when a face's `rev` changes, on the worker, never per frame. Labels, tags and tabs are cut with an ellipsis rather than growing the face; faces are 140 to 440 graph units wide.

**Raster scale.** The screen's scale times a power of two chosen from the zoom, from half the screen's to twice it, as the spike did. A face out of sight is drawn once at a lower scale, so a pan shows it at once. 200 ms after the view last changed, only the visible faces at the wrong scale are drawn again.

**Pan and zoom: the canvas's own transform, not an NSScrollView.** The canvas is unbounded, and the contract's view is `screen = graph · zoom + pan`. Zoom must keep the point under the pointer, and the same controller must serve the iPad, where the UIKit host already drives it. Momentum comes from the system: a trackpad keeps sending scroll events (`momentumPhase`) after the fingers lift, and the canvas pans by each. A rubber band holds the view's centre over the content. Past that, it gives `(1 − 1/(0.55·d/D + 1))·D` of an overshoot `d` (a view of size `D`), as a scroll view does. When the scroll ends, or its momentum does, it springs back with a 0.28 s ease-out animation of the transform. A mouse wheel pans without the band. Zoom is held to 0.1 to 4; the core's fit to 0.2 to 1, as the contract says.

**Decided from 15.2, not changed:** a shape layer per wire, tight frames, never batched; only visible faces redrawn after a zoom settles, off the main thread, in power-of-two steps; run state on sublayers. No measured reason to depart from any of it.

## The app

- **Project:** XcodeGen `project.yml`. The generated `PlumCraftMac.xcodeproj`, `App/Info.plist` and `build/` are ignored (`native/apple/PlumCraftMac/.gitignore`), as the spike did. The kit is a local package dependency (`../PlumCraftFFI`, products `PlumCraftKit` and `PlumCraftCanvas`); there is no third-party code. **Why XcodeGen rather than a SwiftPM executable:** a document-based app needs a real bundle with `CFBundleDocumentTypes` (for the Dock, Finder and Open Recent) and an icon slot, and Xcode's signing. XcodeGen keeps that in one small, reviewable file, with the same tool and layout the spike used.
- **Documents:** `GraphDocument` (an `NSDocument`; `read` only checks that the file is a JSON object with `nodes`, and refuses anything else with the reason, the Mac way). The core opens the file itself with `document.open {path}`, one core document per window. The core's own first, untitled document is closed at launch. Closing a window sends `document.close`.
- **Menu:** PlumCraft (About, Services, Hide, Hide Others, Show All, Quit), File (Open…, Open Recent, Close), View (Fit ⌘9, Zoom In ⌘+, Zoom Out ⌘−, Actual Size ⌘0, Enter Full Screen ⌃⌘F), Window (Minimize, Zoom, Bring All to Front, and the window list), and Help (empty).
- **Appearance:** the canvas follows the system: Paper in light, Dusk in dark, through `view.theme` when the view's effective appearance changes, then `theme.get`. `--appearance light|dark` forces one.
- **For scripts:**
  - `--status-file F` writes the pid, control port and each window's number, document, readiness and first-frame time.
  - `--control-token-file F` opens the core's control port.
  - `--store-root D` sets the store's root.
  - `--scratch-state` keeps saved window state away from the Mac's own.
  - `--engine text|array` picks the engine.
  - `--zoom Z --center ID` and `--select a,b` set up screenshots.
  - `--no-open-panel` suppresses the Open panel.
  - `--bench …` runs the budget run.
- **Build and launch by hand:**

```sh
cd native/apple/PlumCraftMac && xcodegen generate && xcodebuild -project PlumCraftMac.xcodeproj -scheme PlumCraftMac -configuration Release -derivedDataPath build -destination 'platform=macOS,arch=arm64' build
open build/Build/Products/Release/PlumCraft.app --args /path/to/graph.json
```

(`native/apple/build-xcframework.sh` first, if the xcframework isn't built.)

## Screenshots

All are of the running app, taken with `screencapture -l <window>` (that window only), on scratch preferences and store. The graph is `claude-debug/native-n3/s3-showcase.json`: 10 text nodes, a frame, a collapsed group (`to json`), an open group (`echoes`), plain wires in two types (TEXT and TABLE), a routed wire with a bend point, and a net-labelled wire (`Loud`).

| File | What |
|---|---|
| `claude-debug/native-n3/s3-shots/s3-fit-light.png` | At the core's fit, light |
| `claude-debug/native-n3/s3-shots/s3-fit-dark.png` | The same, dark (Dusk) |
| `claude-debug/native-n3/s3-shots/s3-zoom2-light.png` | Zoom 2 on Join: text, pins, widgets, a net tag legible |
| `claude-debug/native-n3/s3-shots/s3-select-light.png` | Upper case, Join and Text to Table selected |
| `claude-debug/native-n3/s3-shots/s3-ran-zoom1-dark.png` | After a run, zoom 1, dark: run states, times, excerpts, Join watched (wider border) |
| `claude-debug/native-n3/s3-shots/s3-two-windows-a.png`, `-b.png` | Two documents opened together through LaunchServices (`open -a`), a window each |

**Against the egui editor.** The same file was opened in the egui editor (debug build, scratch preferences and store, `claude-debug/native-n3/s3-egui-shot.py`) and fitted, then drawn offscreen: `claude-debug/native-n3/s3-egui/s3-egui-fit-light.png`, and dark after a run, `s3-egui-fit-dark-ran.png`. Compared with `s3-fit-light.png`, the visible differences are these:

1. **Rows.** egui stacks a face's outputs below its inputs, one row each (Upper case: `text` in, then `text` out on the next row). The Mac canvas puts inputs and outputs side by side from the top, so its faces are shorter and its output pins sit higher. Both report their own pins, so wires and hits agree with what each draws.
2. **Header.** egui has a Lucide icon (a cube; a group's icon for a proxy) after the chevron, the status ("not run") just after the title, and a ⋮ menu button at the right. The Mac canvas has no icon and no ⋮ button, and right-aligns the status in a slot kept at least 88 wide, so its faces are wider.
3. **Net tags.** egui draws a pointed tag with a use count (`Loud ×1`); the Mac canvas draws a rounded pill with the name.
4. **Frame and group chrome.** egui's frame tab has a frame icon and is as wide as its title; the Mac tab is a wide bar. egui's group band has a group icon, a pencil and ⋮; the Mac band has the chevron and name only.
5. **Bend points.** egui draws a filled handle; the Mac canvas draws a ring.
6. **Widgets.** egui's number field is a small pill; the Mac's is a rectangle as wide as the value. The Mac's text field sits after its label rather than at the face's right.
7. **Text.** egui uses Inter; the Mac uses the system font (SF), as contract 14.3 says a native shell does. egui's faces carry larger, softer shadows.
8. **The window.** egui has its own menu bar row, the Watched panel and a status bar; the Mac window has the canvas alone (the menu is the system's; the Watched panel is slice 4's).
9. **Same:** the layout from the file's positions, the fit, colours and tints (tokens resolved from the same theme), pin shapes and colours, wire colours and paths (the core's chains), the frame, the open group's rect from the core, the collapsed group's face and its rows' text, and the routed wire's bend.

## Conformance against the app

`native/apple/PlumCraftMac/conformance.sh` (log `claude-debug/native-n3/s3-conformance-read.txt`) does the following:

1. Builds the app (release) and the `plumcraft-conformance` command.
2. Launches the app with an empty graph, scratch preferences, store and saved state, and its control port open.
3. Waits (60 s at most, on the status file) for the port and the canvas.
4. Runs the read half naming the app's document, measuring its own faces.
5. Quits the app by its PID (TERM; it quit cleanly each time).
6. Repeats on an app opened on the array engine for the cases that need it.

| App's engine | Run | Passed | Skipped, and why |
|---|---|---|---|
| text | 11 | 11 | "a watched picture arrives as picture": needs `engine:array` |
| array (cases needing it) | 1 | 1 | none |
| text, the edit half (informational, `conformance.sh edit`) | 12 | 12 | none; the typed drag-move steps are skipped by every port driver (it has no hot call), as before |

Settles that ran out of time: 0. The read half takes about 0.6 s. The run and watch cases pass because the app's core has the text engine in process; they were not skipped.

## Budgets

Release builds on the M4 Max (120 Hz). The scene is `spike.scene`, real text nodes built by the core (n nodes, 1.6n wires). Results: `claude-debug/native-n3/s3-bench-500.json` and `s3-bench-2000.json`; hitches in `s3-trace-*-hitches*.md`. Frames are timed by Instruments' Animation Hitches with a "phase" signpost per phase (15.2's gate), and by the display link only for its interval counts.

| What | 500 nodes, 800 wires | 2,000 nodes, 3,200 wires | Budget | Met? |
|---|---|---|---|---|
| `document.open` to first frame (scene built, every face laid out, reported, visible ones drawn) | 169 ms (of it the core's `spike.scene` 22 ms) | 793 ms (`spike.scene` 282 ms) | Launch to a usable window under 1 s | Yes |
| Process launch to first frame | 0.50 s | 0.91 s | under 1 s | Yes, barely at 2,000 |
| One-node change (`node.set`) to its face on its layer and committed, through the kit's `run` (fetches when answered) | p50 5.9 ms, p95 12.8 | p50 35.7, p95 38.3 | 16 ms | Yes at 500; no at 2,000 (the core's own `node.set` costs 18 to 27 ms there, slice 2) |
| The same, learnt from the feed alone | p50 18.4, p95 19.6 | p50 34.1 | 16 ms | No (the core's 16 ms scene throttle plus a build, slice 2) |
| Memory (physical footprint) after the run | 427 MB (face pictures 150 MB) | 811 MB (pictures 220 MB) | none set | n/a |
| Display link, late frames (over 1.5 frames) per phase, untraced run | pan 0, pan at fit 0, zoom 2, drag 2 | pan 0, pan at fit 0, zoom 4, drag 1 | | The display link is the wrong clock (15.2) |

**Hitches (Instruments), ms of late frames a second, by phase.** The app and the spike's perWire bench were alternated as a control on the same machine, whose load average was about 5.

| Run | pan | pan at fit | zoom | drag |
|---|---|---|---|---|
| App 500, run 1 | 20.8 | 14.6 | 61.1 | 6.3 |
| App 500, run 2 | 20.8 | 367.6 | 267.4 | 131.4 |
| App 500, run 3 | 72.8 | 6.2 | 72.2 | 12.5 |
| App 500, run 4 | 27.1 | 4.2 | 28.5 | 38.5 |
| Spike 500, today, run 1 | 12.5 | 33.3 | 63.8 | 29.2 |
| Spike 500, today, run 3 | 56.3 | 31.2 | 62.4 | 27.0 |
| Spike 500, today, run 4 | 10.4 | 2.1 | 27.8 | 75.0 |
| Spike 500 at N0 (15.2) | 16.7 | 0 | 1.9 | 0 |
| App 2,000 | 371.4 | 594.0 | 328.5 | 20.9 |
| Spike 2,000 at N0 | 102.8 | 421.1 | 107.3 | 5.6 |

**Reading it.** Today the spike itself is three to thirty times worse than its own N0 numbers in some phases, and it moves as much between runs as the app does. So this machine's state, not the canvas, sets the floor today. The app's best runs (pan at fit 4.2, zoom 28.5, drag 6.3) are at the spike's best of today. The budget (under 5 good, over 10 poor) is **not shown met** at 500 nodes, and not shown missed as a fault of the canvas. It needs a quiet machine; I didn't have one. At 2,000 nodes the app bends as N0 predicted, and worse than N0's spike: no culling, larger faces with real text, and a loaded machine. The 2,000-node spike control trace failed to export (its recording outran the 60 s limit), so there is no same-day spike number at 2,000. The main thread's work per frame was not traced separately this time.

## Every change outside the new code

**Rust (one behaviour, no export):**

- `crates/editor-core/src/document.rs`: `Document::scene_theme()` (the theme `view.theme` last named, else Paper; old names read as their successors, unknown names as Paper). `Shell::scene` builds in it, and `Shell::theme_tokens` (new override) answers `theme.get` in it. Test `view_theme_picks_the_scenes_theme_and_theme_get_follows`.
- `crates/ffi/tests/scene.rs`: `view_theme_changes_the_scenes_theme` (the C ABI: `theme.get` follows, and only the face whose computed tint changed comes in the delta). It fails without the change (`theme.get` stays `paper`).

**C ABI and control-port behaviour:** `command.run view.theme {name}` over the C ABI and its control port now changes the document's scene theme and `theme.get`'s reply. Before, both stayed Paper. No export, struct, JSON shape or `PLUM_ABI_VERSION` changed, and the egui editor is untouched (its own `Shell` builds its scene).

**Kit (`native/apple/PlumCraftFFI`):**

- New targets: `PlumCraftCanvas` (library) and `plumcraft-conformance` (executable), plus the test target `PlumCraftCanvasTests`.
- `SceneChange` gained a public initializer, so a canvas can replay the model as if all of it changed.
- `CanvasLink.tap`, `doubleTap`, `context`, `marquee` and `dragBegin` are `@discardableResult`.
- `PortDriver` gained `reports`, `doc` and `engines`, a `settle` that waits (on the condition, at most 10 s) until every face is measured, and a count of settles that ran out. Its defaults are unchanged.

**Contract:**

- `docs/shell-interface.md` 14.2 gains an "as built" paragraph: `view.theme` over the C ABI.
- `docs/shell-interface.md` 16.1 gains "Against a running app": the driver, the command, the script and results, plus a note on header `parts` and the header-centre aim.
- `docs/control-protocol.md` `theme.get`'s row now says which theme over the C ABI.

**Ledger:** `docs/research/24-native-shells.md` section 18 gains slice 3, and `docs/STATUS.md` gains an entry.

## Tests

| Suite | Result |
|---|---|
| `swift test` in `native/apple/PlumCraftFFI` | 44 run and passed (33 before plus 11 new), 7 measurements opt-in and skipped as before |
| `PlumCraftCanvasTests` alone, 20 runs | 20 of 20 passed |
| New canvas tests (11) | Layout (3): every fixture face lays out and draws, pins inside, widgets inside, the measurement is the layout; hostile text is cut, not grown; widgets show their values. View (3): zoom keeps the point under the pointer and clamps; the raster scale steps; the rubber band gives less and springs back. On a core (5): what is drawn is what the core hits (rects, pins, wire ends, a typed hit on the drawn pin); one node's change lays out and draws one face and leaves the other's picture the same object; selection is a sublayer, draws no face, and removal takes the layers; the core's fit is taken once per `seq`; net tags are where the core hits them |
| iOS compile of `PlumCraftCanvas` (generic device, generic simulator) | Built, zero warnings |
| Mac app, release, clean build | Zero compiler warnings (`SWIFT_TREAT_WARNINGS_AS_ERRORS`). Xcode prints its own tool notices (an AppIntents metadata skip, a provisioning-profile lookup); they are not compiler warnings |
| `cargo fmt --all --check` | Clean |
| `cargo clippy --all-targets --all-features -- -D warnings` | Clean (after fixing one finding in the new test; logs `claude-debug/native-n3/s3-gate-clippy.log`, `s3-gate-clippy2.log`) |
| `cargo nextest run --all-features --no-fail-fast` (b3, fresh folders, from a clean `target/`) | 1,462 of 1,463 passed, 16 skipped; the one failure is the known local-only `plumcraft-craft::params_coverage hidden_params_match_the_baseline`. 1,461 before plus 2 new (`claude-debug/native-n3/s3-gate-nextest.log`) |
| `scripts/check-hash-gates.sh`, `scripts/check-core-toolkit-free.sh` | ok, ok |
| `native/apple/build-xcframework.sh` (all four slices, after the Rust change) | Built |
| Final run of `conformance.sh` with the final build | Read half 11 of 11 and 1 skipped (text), 1 of 1 (array); the app quit cleanly both times (`claude-debug/native-n3/s3-conformance-read-final.txt`) |
| The maintainer's `prefs.json` and `trusted.json` | Unchanged before and after (SHA-256 `9221fa66…`, `a10d6fc8…`) |

## Defects introduced and caught

1. **AppKit opened the `YES` of `-ApplePersistenceIgnoreState YES` as a document.** It raised a modal "The document "YES" could not be opened" on the maintainer's screen and stalled two early launches (the first took 19.8 s to its first frame; the maintainer dismissed the dialog). Caught by the maintainer. Replaced by the app's own `--scratch-state`, which sets the same default from inside.
2. **One early launch used the maintainer's commonkit store** (`/Volumes/CommonkitStore/store`): `COMMONKIT_STORE_ROOT` in the environment wins over the config's `store_root`. Seen in the app's log line "policy file /Volumes/CommonkitStore/store/.policy.json". That launch opened a graph and ran nothing, so I expect nothing was written there, but I didn't check. Every script now sets `COMMONKIT_STORE_ROOT` and `COMMONKIT_MARK_DIR` to scratch.
3. **Net tags, frame tabs and group bands were invisible:** their layers had no bounds. Caught by the first screenshot.
4. **Tag text was cut ("Lo…"):** a monospaced 12-point font is wider than the core's 6.6-a-character estimate. Now proportional, with a smaller inset. Caught by a screenshot.
5. **Translucent tags let a wire show through.** Now filled opaque. Caught by a screenshot.
6. **An empty graph never reported ready** (the status was written only after a batch of faces). Caught by the conformance script.
7. **The canvas tests waited vacuously** (`allMeasured` is true with no faces) and failed or raced. Caught by the tests; they now wait for the faces they made.
8. **`--zoom` was overridden by the core's owed fit**, which arrives after the last face is measured. Caught by a screenshot at 68% where 100% was asked.
9. **The app's product name `PlumCraft` clashed with the kit's module `PlumCraft`.** The module is now `PlumCraftMac`. Caught by the build.
10. **A clippy finding in the new Rust test** (`[lit.clone()]`). Caught by the gate.

## Decisions the maintainer may want to revisit (the look and behaviour are yours to judge)

1. **Own transform, not an NSScrollView,** with a rubber band that keeps the view's centre over the content (the content's edge may come to the middle of the view).
2. **Shortcuts:** Fit is ⌘9 (as Preview's Zoom to Fit) and Full Screen is ⌃⌘F. The contract's F key for Fit is not bound in this slice (slice 4's keys).
3. **Smart zoom toggles:** the first fits the node under the pointer (up to zoom 2), the second goes back. With no node there, it fits everything.
4. **A drag on a face moves it,** and a drag from a pin draws a new wire, in a read-only slice. Both go through the core and are undoable. The alternative was to ignore drags on faces until slice 4.
5. **Window tabs follow the user's setting** (`.automatic`); with no document to open at launch, the Open panel shows (Preview's behaviour).
6. **The face's look:** egui-snarl's arrangement (inputs left, outputs right) with the core's colours, a baked soft shadow, a 6-unit corner, the header's chevron and a pencil for "enter". There is no Lucide icon yet (shells are to ship that set, contract 8.3). The selection is an accent outline that crosses the pins.
7. **Following the system's appearance** sends `view.theme` (Paper or Dusk) per document. It is not kept between runs, and Pewter isn't reachable from the app yet.
8. **Header parts are not reported** (the typed call has none). So a click on a title selects and doesn't also watch, unlike 8.13's table for shells that report parts. Reporting a `title` part over the header's centre would make "a tap selects" fail, because the cases aim there (contract 16.1's new note).
9. **Labels, tags and tabs are drawn at twice the screen's scale, fixed,** so they soften past zoom 2.
10. **The document type is `public.json` at rank Alternate.** Opening the app once through LaunchServices registered it as an alternate JSON viewer on this Mac; it doesn't take over the default handler.

## What slice 4 should know

- `CanvasView` takes a `DocumentModel` and owns its `onChange`; one canvas per model. `LayerCanvas` exposes `viewport`, `fit`, `zoom`, `pan`, `requestFit`, `faceLayout(nid:)`, `pinPoint`, `nid(of:)`, `snapshot` and `counts`.
- Context menus are not built: a right click does nothing yet. The reply of a `context` gesture already carries `menu`; `DocumentModel.contextMenu` and `activate` are there.
- Inline controls: `FaceLayout.widgets` holds each widget's rect in face coordinates (reported as `rows`), for the field-editor pattern.
- `ui.viewport` doesn't exist in the core; the canvas has an `onViewport` hook ready to report it.
- The run wash needs `anim` from the C ABI's core (`run_anim` is `None` there today), or the shell's own run animation.
- Previews (`rgba8`) are drawn as their box only; the picture via `media.get` and `plum_blob` into a sublayer is not built.
- Faces are drawn whole. At 2,000 nodes, culling and a simpler zoomed-out face are what 15.2 said; nothing is built for them.
- The app's control port opens only with `--control-token-file`. It is never on by default, in debug or release.

## What I did not verify

- **Real hands on the trackpad.** Momentum, the rubber band's feel, pinch, smart zoom, Space-drag, middle drag, hover, the marquee and face drags as the recognisers see them were not exercised by a person or a UI test. The canvas arithmetic behind them is unit-tested; the conformance suite drives the core over the port, not the recognisers. File › Open's panel, Open Recent, a drop on a window and the Dock icon were not exercised; LaunchServices opening two files was.
- **The hitch budget on a quiet machine** (above).
- **iOS:** the canvas target compiles for device and simulator; nothing ran there.
- **Linux:** the Rust change ran on the Mac only; the controller runs it on yellowfin.
- **Whether the one early launch wrote anything** to the maintainer's commonkit store (defect 2).
