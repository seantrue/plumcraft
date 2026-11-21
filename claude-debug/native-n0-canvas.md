# N0(a): Swift canvas performance spike

Date: 2026-11-19. Plan: `docs/research/24-native-shells.md`, sections 5.4, 10 and 12 (stage N0, spike a).

The question: can "a CALayer per node, wires in CAShapeLayers, all under one transformed layer" hold every display frame while panning, zooming and dragging a node, on a graph of 500 nodes and 800 wires, on a Mac and an iPad?

**Short answer.**

- **Mac, 500 nodes:** yes, with one wire layer per wire, each sized tight around its wire. At 120 Hz the compositor showed almost no hitches during zoom and drag, and a few one-frame hitches during pan and run progress.
- **Wires batched into a few graph-sized shape layers:** no. The compositor misses 30 to 40% of frames.
- **2,000 nodes and 3,200 wires:** per-wire layers bend when the whole graph is on screen.
- **iPad Pro (M4), the real device:** every mode, at 500 and at 2,000 nodes, held every frame of a 60 Hz display link with main-thread work under 2.3 ms. The display link never ran at 120 Hz although the screen reports 120, so 120 Hz on the iPad is unmeasured. Compositor hitches on the iPad were not recorded.

## 1. What was built and where

Everything is in `native/apple/CanvasSpike/`. Nothing else in the repo was changed.

| Path | What |
|---|---|
| `project.yml` | XcodeGen project. Two apps: `CanvasSpikeMac` (macOS 15) and `CanvasSpikePad` (iPadOS 18, iPad only, automatic signing with the maintainer's team, from `PLUMCRAFT_TEAM_ID`). Swift 6, strict concurrency, no third-party dependencies. |
| `CanvasKit/` | The Swift package both apps share. |
| `CanvasKit/Sources/CanvasKit/FakeScene.swift` | A deterministic scene (SplitMix64) laid out in layered columns (about √N × 1.1 columns). Each node has a title, 2 to 6 port rows with captions and coloured pins, an inline widget row on about 40% of nodes, and a ~128 px generated preview on about 20%. About 30% of nodes carry a run state. Wires are cubic beziers that run 1 to 3 columns forward. |
| `CanvasKit/Sources/CanvasKit/Face.swift` | The shell measures each face (size and pin anchors, plan 5.2) and draws it with Core Graphics and Core Text. It is pure code with no state, so faces can be rasterised on any thread. |
| `CanvasKit/Sources/CanvasKit/CanvasController.swift` | The canvas itself (see the list below). |
| `CanvasKit/Sources/CanvasKit/Bench.swift` | The scripted benchmark. Times each frame with CADisplayLink, times main-thread work (including `CATransaction.flush`), reads `task_info` phys_footprint, emits phase signposts (OSSignposter, Points of Interest) and writes JSON. |
| `CanvasKit/Sources/CanvasKit/Hosts.swift` | The two hosts. AppKit: a layer-hosting NSView. Two-finger scroll pans, pinch zooms, ⌘-scroll zooms, smart zoom fits, dragging a node moves it, dragging empty canvas pans, and keys 1, 2 and 3 switch wire modes. UIKit: one finger pans or drags a node, pinch zooms, double tap fits. The canvas code is the same for both; only input and the owner of the display link differ. |
| `App/Mac/MacApp.swift`, `App/Pad/PadApp.swift` | Thin app entry points (Pad uses the scene lifecycle). |
| `bench.sh` | `mac`, `trace`, `sim`, `device` and `shot` (section 2). |
| `.gitignore` | The generated `.xcodeproj`, `build/` and the generated `App/Pad/Info.plist`. |

What `CanvasController.swift` does:

- Each node is a CALayer whose `contents` is a CGImage of its face, drawn once, plus a wash sublayer and a progress sublayer for run state.
- Every node and wire sits under one world layer, and pan and zoom are that layer's affine transform.
- A node drag re-paths only the attached wires, every frame.
- When a gesture settles (200 ms after the last change), visible faces whose raster scale no longer matches the zoom are redrawn, in parallel with `concurrentPerform`. The raster scale moves in powers of two: screen scale × 2^⌈log2 zoom⌉, clamped to 0.5 to 4.
- There are four wire modes:
  - `perWire`: one CAShapeLayer per wire, its frame tight around the wire's path.
  - `perWireLoose`: one CAShapeLayer per wire with a zero frame, so the path spills outside the bounds.
  - `batched`: one CAShapeLayer per wire colour (8).
  - `hybrid`: batched while still; the dragged node's wires move to their own layers for the drag.

Helper scripts (untracked, in `claude-debug/`):

- `native_n0_summarize.py` turns a bench JSON into the tables below.
- `native_n0_hitches.py` bins an Instruments Animation Hitches export by the bench's phase signposts.

Package tests: `cd native/apple/CanvasSpike/CanvasKit && swift test` (2 tests: the scene is deterministic and full, and a face renders at a given scale).

## 2. How to rerun each measurement

Everything runs locally; nothing touches CI. Run from `native/apple/CanvasSpike/`. Output goes to `claude-debug/native-n0/`.

| Measurement | Command | Output |
|---|---|---|
| Mac, display-link and main-thread view | `MODES=perWire,perWireLoose,batched,hybrid ./bench.sh mac 500 800` | `mac-500.json` |
| Mac, 2,000 nodes | `MODES=perWire,batched ./bench.sh mac 2000 3200` | `mac-2000.json` |
| Mac, compositor hitches (Instruments) | `PHASE_SECONDS=3 MODES=perWire,perWireLoose ./bench.sh trace 500 800` (or `2000 3200`) | `canvas-mac-N-MODE.trace`, `canvas-mac-N-MODE-hitches.md` |
| iPad simulator | `PHASE_SECONDS=3 ./bench.sh sim 500 800` | `sim-500.json` |
| Physical iPad | `DEVICE=00008132-001424640C05801C ./bench.sh device 500 800` (iPad unlocked) | `device-500.json` |
| Screenshots | `./bench.sh shot` | `mac-window-zoomfit.png`, `mac-window-zoom1.png` |
| Tables | `python3 claude-debug/native_n0_summarize.py claude-debug/native-n0/mac-500.json` | Markdown |

The app's arguments are `--bench out.json [--nodes N] [--wires M] [--phase-seconds S] [--modes a,b]`. Each mode builds the scene at zoom 0.5 and then runs these phases:

| Phase | Length | What it does |
|---|---|---|
| warmup | 1 s | Nothing; records the first frames after the build. |
| idle | 1.5 s | Nothing. |
| pan | S | Pans at zoom 0.5 along an ellipse across the graph. |
| pan_fit | S | The whole graph on screen, panning a small loop. |
| zoom | 1.5 S | Zooms 1 → 0.25 → 2 → 1, in log space, about the middle. |
| drag | S | At zoom 1, one node with 8 wires circles once every 2 s. |
| run_progress | S | 50 nodes near the middle change wash colour every frame; the running ones also move a progress bar. |

After the first mode, the bench measures the re-raster cases. It then writes the JSON and exits.

Two notes on the tooling:

- **Traces are huge.** Each recording of about 30 s leaves a raw kernel trace of 5 to 60 GB in `$TMPDIR` before saving it to a 0.4 to 0.9 GB `.trace`. Recordings much longer than a minute stalled while saving, and the killed ones left their raw traces behind. That is what filled the disk once in this session; I removed the orphaned `$TMPDIR/instruments*.ktrace` files, about 140 GB. `trace` mode now makes one short recording per wire mode and deletes its own raw trace.
- **`xctrace --launch` leaves a stopped copy of the app** (state T) after the bench exits. That is the "hung" process; `bench.sh` kills it.

## 3. Results

### 3.1 Machines

| | Mac | iPad simulator | Physical iPad |
|---|---|---|---|
| Hardware | MacBook Pro, Apple M4 Max (Mac16,6), 16 cores, built-in Liquid Retina XDR | iPad Pro 13-inch (M5) simulator on the same Mac | iPad Pro 11-inch (M4), iPad16,3 |
| Refresh measured | 120 Hz (CADisplayLink 8.33 ms; `maximumFramesPerSecond` 120) | 60 Hz (`maximumFramesPerSecond` 60) | 60 Hz actually delivered (16.67 ms) although `maximumFramesPerSecond` is 120 |
| Load during runs | The final numbers below were taken at load 1.5 to 3. The earlier runs were at load 4 to 7, with other agents running an Android emulator and Rust builds. | Same Mac | n/a |

### 3.2 Mac, 500 nodes and 800 wires: what the app's own frame timing sees

Source: `mac-500.json`. Phases of 4 s; dropped frames are counted against 120 Hz. Every phase in every mode had frame-interval p50, p95 and p99 of 8.33 ms.

| Mode | Phase | Frames | FPS | Max interval (ms) | Dropped | Main-thread work p95 (ms) | Work max (ms) |
|---|---|---|---|---|---|---|---|
| perWire | pan | 480 | 120 | 8.33 | 0 | 0.06 | 0.11 |
| perWire | pan_fit | 480 | 120 | 8.33 | 0 | 0.07 | 0.10 |
| perWire | zoom | 718 | 119.7 | 20.36 | 2 | 0.07 | 0.11 |
| perWire | drag | 481 | 120 | 8.33 | 0 | 0.44 | 0.58 |
| perWire | run_progress | 480 | 120 | 8.33 | 0 | 0.58 | 0.92 |
| perWireLoose | pan | 480 | 120 | 8.33 | 0 | 0.06 | 0.10 |
| perWireLoose | zoom | 721 | 120 | 8.33 | 0 | 0.08 | 0.11 |
| perWireLoose | drag | 480 | 120 | 8.33 | 0 | 0.36 | 0.45 |
| batched | pan | 478 | 119.3 | 20.83 | 2 | 0.06 | 0.09 |
| batched | zoom | 719 | 119.8 | 20.82 | 1 | 0.06 | 0.08 |
| batched | drag | 480 | 120 | 8.33 | 0 | 0.55 | 0.66 |
| hybrid | drag | 480 | 120 | 8.33 | 0 | 0.23 | 0.30 |
| hybrid | run_progress | 479 | 119.6 | 20.81 | 1 | 0.42 | 0.49 |

By this measure every mode looks fine, and main-thread work never exceeded about 1 ms a frame. **This measure is not enough.** The display link only shows whether the main thread kept up; it does not show frames that the render server delivered late.

### 3.3 Mac: what the compositor delivered (Instruments, Animation Hitches)

Hitches belong to the app's process and are binned by the phase signposts. A hitch is a frame presented late. The figures are hitch time per second (ms/s). Apple's rule of thumb: under 5 ms/s is good, 5 to 10 a warning, over 10 critical. Phases were 3 s (zoom 4.5 s).

| Scene | Mode | pan | pan_fit | zoom | drag | run_progress | Source |
|---|---|---|---|---|---|---|---|
| 500 / 800 | perWire (tight frame) | 16.7 (6 hitches) | 0 | 1.9 (1) | 0 | 11.1 (4) | `canvas-mac-500-perWire-hitches.md` |
| 500 / 800 | perWireLoose | 13.9 (5) | 0 | 18.5 (10) | 27.8 (10) | 16.7 (6) | `canvas-mac-500-perWireLoose-hitches.md` |
| 500 / 800 | perWireLoose (busy machine) | 13.9 | 0 | 14.8 | 80.6 | 19.4 | `canvas-trace-b-*.xml` |
| 500 / 800 | batched (busy machine) | 280 | 355 | 350 | 495 | 30.5 | `canvas-trace-b-*.xml` |
| 500 / 800 | hybrid (busy machine) | 303 | 393 | 348 | 2.8 | 27.8 | `canvas-trace-b-*.xml` |
| 2000 / 3200 | perWire (tight frame) | 103 (37) | 421 (120) | 107 (57) | 5.6 (2) | 52.8 (19) | `canvas-mac-2000-perWire-hitches.md` |
| 2000 / 3200 | batched | 799 | 862 | 859 | 846 | 737 | `canvas-mac-2000-batched-hitches.md` |

Notes:

- **What the hitches were.** Every hitch at 500 nodes with per-wire layers lasted one frame (8.3 ms). Instruments labelled the batched ones "Potentially expensive render", that is, render-server work, not the app's.
- **Rows marked "busy machine"** were recorded while the other agents were loading the machine. The batched and hybrid traces from the quiet run did not save, but the gap is far too large to come from load.
- **`perWire` builds faster.** It took 29 ms of build plus a 70 ms first commit, against about 100 ms in the other modes, where that commit is counted inside the build.

### 3.4 Mac, 2,000 nodes and 3,200 wires: display-link view

Source: `mac-2000.json`.

| Mode | Phase | FPS | p99 (ms) | Max (ms) | Dropped | Work p95 (ms) |
|---|---|---|---|---|---|---|
| perWire | pan | 120 | 8.33 | 8.33 | 0 | 0.07 |
| perWire | pan_fit | 117.8 | 20.83 | 20.83 | 6 | 0.07 |
| perWire | zoom | 119.8 | 8.33 | 16.89 | 1 | 0.07 |
| perWire | drag | 120 | 8.33 | 8.33 | 0 | 0.90 |
| perWire | run_progress | 120 | 8.33 | 8.33 | 0 | 0.98 |
| batched | pan | 118.1 | 8.33 | 20.83 | 5 | 0.06 |
| batched | drag | 119.3 | 8.33 | 20.83 | 2 | 1.01 |

The display link barely notices what the compositor table above shows.

pan_fit at 2,000 nodes hits the zoom floor of 0.1, so the whole graph is not quite on screen; it is still about 2,000 layers and 3,200 wire layers in view.

### 3.5 Build, memory and re-raster cost (Mac)

| Scene | Build (ms) | Face bitmaps at 1x (MB) | phys_footprint after build (MB) |
|---|---|---|---|
| 500 / 800 | 100 to 116 (perWire 29 + a 70 ms first commit) | 68 | 179 to 196 |
| 2000 / 3200 | 401 (batched); 107 + a 287 ms first commit (perWire) | 277 | 662 to 680 |

The build includes thumbnails, faces drawn in parallel (17 ms for 500, 58 ms for 2,000), layers and wires. At 2,000 nodes the build plus first commit is a main-thread stall of about 0.4 s.

Re-raster cost when a zoom settles. "Render" includes setting the layers' contents and committing; the separate commit time came out as 0 because each re-raster commits its own transaction.

| Scene | Case | Faces | Scale | Parallel | Render (ms) | Per face (ms) | Face MB after | Footprint (MB) |
|---|---|---|---|---|---|---|---|---|
| 500 | Visible at zoom 1 | 26 | 2 | Yes | 13.9 | 0.54 | 79 | 203 |
| 500 | Visible at zoom 1 | 26 | 2 | No | 29.1 | 1.12 | 79 | 202 |
| 500 | Visible at zoom 2 | 10 | 4 | Yes | 20.6 | 2.06 | 96 | 248 |
| 500 | Fitted, all faces | 500 | 0.5 | Yes | 42.5 | 0.08 | 17 | 91 |
| 500 | All at screen scale | 500 | 2 | Yes | 232 | 0.46 | 273 | 608 |
| 500 | All at screen scale | 500 | 2 | No | 333 | 0.67 | 273 | 607 |
| 2000 | Visible at zoom 1 | 27 | 2 | Yes | 17.8 | 0.66 | 288 | 686 |
| 2000 | Visible at zoom 2 | 9 | 4 | Yes | 14.2 | 1.58 | 301 | 719 |
| 2000 | Fitted, all faces | 2000 | 0.1 | Yes | 163 | 0.08 | 69 | 257 |
| 2000 | All at screen scale | 2000 | 2 | Yes | 941 | 0.47 | 1108 | 2352 |

Interactively, settling after a zoom redraws about 10 to 30 visible faces in 14 to 21 ms with parallel rendering. That costs two or three frames, once, after the gesture.

### 3.6 iPad simulator

`sim-500.json`, iPad Pro 13-inch (M5) simulator, 500 / 800, 3 s phases. It runs correctly: all four modes, every phase at 60 frames per second, no dropped frames, main-thread work p95 at most 0.2 ms. Scene, gestures and faces render (screenshot `sim-ipad-fit.png`).

Simulator frame timing is not representative of a device: it renders through the Mac's GPU, caps at 60 Hz here, and does not model iPad memory or thermals. It is reported only as "runs correctly".

### 3.7 Physical iPad (iPad Pro 11-inch, M4, iPad16,3, J717AP)

`./bench.sh device` built and signed the app with the maintainer's team (automatic provisioning worked), installed it, ran it with no human input and copied the JSON back. Results: `device-500.json` (4 s phases) and `device-2000.json` (3 s phases).

**The display link ran at 60 Hz, not 120.** `UIScreen.maximumFramesPerSecond` reported 120 and the link asked for `preferredFrameRateRange` up to 120, but every interval in every phase was 16.67 ms. That is why the "Dropped" column in `device-*.json` counts every frame: it counts against 120 Hz. Measured against 60 Hz, no phase missed a frame: p99 and max were 16.67 to 16.71 ms everywhere, at both sizes.

The likely causes are guesses: Low Power Mode, Settings › Accessibility › Motion › Limit Frame Rate, or iPadOS not granting 120 Hz to a display link that runs with no touches. **120 Hz on the iPad is therefore unmeasured.**

| Scene | Mode | Phases | Interval p99 / max (ms) | Main-thread work p95 (ms) | Build (ms) |
|---|---|---|---|---|---|
| 500 / 800 | perWire | pan, pan_fit, zoom, drag, run_progress | 16.67 / 16.7 | 0.12 to 0.68 | 37 |
| 500 / 800 | batched | same | 16.67 / 16.7 | 0.11 to 0.82 | 55 |
| 500 / 800 | hybrid | same | 16.67 / 16.7 | 0.11 to 0.70 | 49 |
| 500 / 800 | perWireLoose | same | 16.67 / 16.7 | 0.10 to 0.68 | 33 |
| 2000 / 3200 | perWire | same | 16.67 / 16.7 | 0.11 to 1.24 | 124 |
| 2000 / 3200 | batched | same | 16.67 / 16.7 | 0.10 to 2.21 | 156 |
| 2000 / 3200 | hybrid | same | 16.67 / 16.71 | 0.16 to 1.02 | 146 |

Re-raster on the iPad was faster than on the Mac. The likely reason is that it has fewer, smaller faces to fill and less to commit, but that is a guess.

| Scene | Case | Faces | Render (ms) | Footprint (MB) |
|---|---|---|---|---|
| 500 | Visible at zoom 1, 2x, parallel | 25 | 3.0 | 36 |
| 500 | Visible at zoom 1, 2x, serial | 25 | 21.5 | 35 |
| 500 | Visible at zoom 2, 4x, parallel | 10 | 5.7 | 42 |
| 500 | All at 2x, parallel | 500 | 38.5 | 43 |
| 2000 | Visible at zoom 1, 2x, parallel | 25 | 2.6 | 114 |
| 2000 | All at 2x, parallel | 2000 | 170 | 122 |
| 2000 | Fitted, all faces at 0.5x | 2000 | 69 | 140 |

On iPadOS, `phys_footprint` does not include the layer contents (34 MB for 500 nodes whose face bitmaps total 68 MB, 108 MB at the end of the 2,000 run). The bitmaps appear to be charged to the render server, so the footprint is not a good memory measure there; also a guess.

**Compositor hitches on the iPad were not recorded.** Only the display link's view exists, and section 3.3 shows that the display link can miss render-server hitches. To get them, record the device run in Instruments: Animation Hitches with Points of Interest, target "Sean's iPad", app CanvasSpikePad, arguments `--bench bench-500.json --phase-seconds 3`.

## 4. Budgets in plan section 10

| Budget | Status | Evidence |
|---|---|---|
| Pan, zoom and node drag hold every display frame (8.3 ms at 120 Hz) on 500 / 800 | **Mac: nearly met with tight per-wire layers. Missed with batched wires. iPad: met at 60 Hz by the display link; 120 Hz and compositor hitches unmeasured.** | Main thread under 1 ms a frame in every case. Compositor: zoom 1.9 ms/s and drag 0, but pan 16.7 ms/s (6 one-frame hitches in 3 s). Batched and hybrid wires miss 280 to 400 ms/s while panning and zooming. |
| The same at 2,000 / 3,200 (where it bends) | **Bends on the Mac** | Per-wire: pan 103, all on screen 421, zoom 107 ms/s; drag stays at 5.6. |
| Run progress: at most one scene change per frame | **Shell side met on the Mac** (the core's coalescing is not part of this spike) | 50 washes and progress bars changing every frame: main thread under 1 ms, compositor 11 ms/s (4 one-frame hitches). |
| A command (an edit) to its scene change on screen within 16 ms | Unmeasured (no core) | — |
| A preview on screen one frame after decoding | Unmeasured (no store or blob path) | Drawing a face with a thumbnail costs about 0.5 ms at 2x. |
| Launch to a usable window under 1 s | Partly: canvas build only | 500 nodes take about 0.1 s of build and first commit; 2,000 take about 0.4 s. |
| No main-thread stall over 100 ms, ever | **Met interactively; missed by the eager cases** | Re-rastering all 2,000 faces at 2x takes 941 ms. Building 2,000 takes about 0.4 s. Re-rastering only visible faces takes 14 to 21 ms. |
| Memory: decoded caches dropped at once on a warning | Unmeasured | Footprints are in 3.5; there was no memory-pressure test. |

## 5. What was not verified

- **The iPad at 120 Hz, and the iPad compositor.** The device delivered only 60 Hz to the display link (cause not found), and no Instruments trace was recorded on the device.
- **Simulator timing** is not representative and is not used for any conclusion.
- **Hitches on a quiet machine.** The final 500-node Mac traces ran at load 1.5 to 3, but other agents were still active. Each phase was recorded once (3 s), with no repeats, so single-digit hitch counts are within noise.
- **Real gestures under measurement.** Momentum scrolling, an actual pinch and an actual trackpad drag were not measured; the bench drives the same code paths from the display link. I tried the gestures by hand only briefly (the app opens, shows the graph, and the screenshot at zoom 1 is sharp after settling).
- **Memory under pressure:** no memory warning was sent, and the footprint does not include the window server's or render server's copies of layer contents.
- **The external-display and 60 Hz Mac cases**, and macOS Low Power Mode.
- **Hit-testing cost** for wire targets (the core's job per plan 5.3) and accessibility elements.
- **Text against the egui face**: fonts are the system UI font through Core Text; sizes are not matched to egui's.

## 6. Surprises

1. **The display link hides the real cost.** CADisplayLink intervals and main-thread time said every mode held 120 Hz. Instruments showed batched wires missing a third of frames on the render server. Any budget for the real canvas has to be checked with Animation Hitches (or the device's hitch metrics), not with the app's own frame clock.
2. **Batching wires is worse, not better.** Eight CAShapeLayers, each the size of the whole graph, cost the render server far more on every pan and zoom than 800 small layers do. Shape layers are re-rasterised by the render server, and a huge path is expensive to rasterise even when most of it is off screen.
3. **The size of each wire layer's frame matters.** The same per-wire layers with a zero frame, so the path spills outside the bounds, hitched more during zoom and drag than layers whose frame is tight around the path (zoom 18.5 against 1.9 ms/s, drag 27.8 against 0). The guess is that a tight frame lets Core Animation cull off-screen wires and bound what it redraws.
4. **The memory of eager re-rastering.** Re-rastering every face at screen scale took 500 nodes to 608 MB and 2,000 nodes to 2.35 GB. Visible-only re-rastering at a scale that follows the zoom keeps 500 nodes near 200 MB.
5. **Tooling.** `xctrace` hung when saving longer recordings, left a stopped copy of the app behind, and left 5 to 60 GB raw traces in `$TMPDIR` after a stall, which filled the disk once.
6. **`CALayer.render(in:)` is wrong for screenshots.** It drew a geometry-flipped tree with the faces upside down (`mac-canvas-fit.png`). Screenshots must come from the window server (`screencapture -l`) or an equivalent.

## 7. Recommendation

**Decision 1 (native drawing from a shared scene) stands.** At the plan's target size the approach leaves the main thread nearly idle (under 1 ms a frame on the Mac, under 2.3 ms on the iPad, even at 2,000 nodes). With the right wire layers the Mac compositor holds 120 Hz in zoom and drag, with a few one-frame misses in pan. The iPad Pro (M4) held every 60 Hz frame in every mode at both sizes. Two things remain open before the 120 Hz budget can be called met on the iPad: the device's 120 Hz display link, and its compositor hitches.

What the real canvas should do differently from this spike:

1. **One shape layer per wire, its frame tight around the path, never batched.** Re-path only the dragged node's wires. `plum_wire_paths` gives exactly the points needed.
2. **Level of detail and culling for large graphs.** At 2,000 nodes with everything on screen, the per-wire design bends. Possible answers:
   - Below a zoom threshold, swap faces for simple rectangles and wires for a single low-detail layer.
   - Or hide layers that are off screen, rather than relying on Core Animation's culling.
   - Or tile the static wires into bitmaps.

   The fix needs measuring; the guess is that hiding off-screen layers plus simplified faces below zoom 0.3 is enough.
3. **Faces rasterised off the main thread, visible only, at a scale that follows the zoom in powers of two, capped at 2× screen scale.** Never eager for the whole graph. Free faces far off screen to bound memory.
4. **Run state as compositor-only sublayers** (wash, progress), never a face redraw. It cost about 1 ms of main-thread time for 50 nodes a frame.
5. **Build big scenes in slices,** or build the faces on workers first, so a 2,000-node open never stalls the main thread for 0.4 s.
6. **Make "hitch ms per second under Instruments" the gate metric,** per phase, with signposts, and keep the bench (`bench.sh trace`) as the regression check. The display link's frame times are a secondary metric only.
7. **Screenshots from the window server, not `render(in:)`.**

## 7a. Maintainer's decision (2026-11-19)

In the maintainer's words: "Initial target would be < 500 active nodes. Current rendering is adequate."

So the 2,000-node bend (section 3.3) and the level-of-detail work in recommendation 2 are not needed for the first target. They become relevant only if graphs grow past about 500 active nodes. The other recommendations still apply to the real canvas: tight per-wire layers, visible-only re-rastering off the main thread, run state on sublayers, and Instruments hitches as the gate.

## 8. Needs the maintainer

- **The iPad's 120 Hz.** Check that Low Power Mode is off, and Settings › Accessibility › Motion › Limit Frame Rate is off. Then rerun `DEVICE=00008132-001424640C05801C ./bench.sh device 500 800` from `native/apple/CanvasSpike/`, with the iPad unlocked and awake. `refresh_hz_max` in the JSON is the screen's maximum; `link_duration_ms_p50` in each phase is what was actually delivered.
- **iPad compositor hitches.** Record the device run in Instruments: Animation Hitches with Points of Interest, target "Sean's iPad", app CanvasSpikePad, arguments `--bench bench-500.json --phase-seconds 3`.
- **Large trace bundles.** `claude-debug/native-n0/canvas-mac-500.trace` is 15 GB, from a stalled recording, and the other `.trace` bundles are 0.4 to 0.9 GB each. The exported tables (`*-hitches.xml`, `*-os-signpost.xml`, `*-hitches.md`) hold the numbers. I left the bundles in place under the "don't delete in claude-debug" rule; delete them if you want the space.
