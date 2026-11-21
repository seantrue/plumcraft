# N4: the 500-node frame hitches on the iPad (the wires)

Branch `native/n4-wire-hitches`, from the dev branch `feat/native-n0` (`6f2d2c6`, merged forward to `8820a35`). 2026-11-21. Nothing was run on a device; nothing was sent to Apple. Uncommitted.

## Answer first

The hitches come from the wires' length, not from how a wire is stroked. The canvas drew one CAShapeLayer per wire with its frame tight around the wire, as the N0 spike did. The spike's scene wired each node forward one to three columns, so its wire layers were small. The core's bench scene (`spike.scene`, `crates/ffi/src/core.rs`) wires node `i` to node `(i*7 + k*13) % i`, any earlier node, so almost every wire crosses much of the graph (37 of the 800 fit inside 800 by 400 units) and its layer is nearly as big. A shape layer is rasterised and blended over its bounds, so every frame the view moved, the wires cost 43 to 160 views' worth of blending at 500 nodes.

The fix draws wires the way faces are drawn: into tile pictures on worker threads, drawn again at the zoom's own scale once a view change settles. On this Mac, measured with Core Animation's own renderer, the moving phases at 500 nodes went from 148 to 690 late ms a second to 0 in every phase and every run, the same as with wires hidden. Only the iPad can confirm it.

## The evidence the work started from

Measured on the maintainer's iPad: iPad Pro 11-inch M4, 120 Hz, portrait 834 by 1210 points, 500 nodes and 800 wires, Release builds, two traces a build, the same view and the same 120.7 MB of face pictures. Figures are ms of hitch a second from Instruments' Animation Hitches.

| Build | idle | pan | pan_fit | zoom | drag |
|---|---|---|---|---|---|
| N0 spike (control, earlier run) | 0 | 0 | 0 | 0 | 0 |
| N3 slice 3 | 0 | 161 | 86 to 115 | 142 to 150 | 74 to 87 |
| N4 slices 4, 5, 6 | 0 to 8 | 157 to 167 | 96 to 118 | 141 to 159 | 57 to 93 |
| the dev branch before the fix | 0 | 154 to 155 | 106 to 107 | 146 to 162 | 54 to 75 |
| the same, `--hide-wires` | 0 | 0 | 0 | 0 | 0 |

Instruments gave "Potentially expensive render" and "Potentially expensive GPU work" as the reasons, and the app's own late-frame counter read 0. Together these point at the render server.

## Measuring without the iPad and without Instruments

No Instruments traces were recorded on this Mac. The Mac bench has a new `--offscreen` mode (`PlumCraftShell/OffscreenRenderer.swift`), which:

- opens no window and never activates the app;
- renders the canvas's layer tree with `CARenderer` (Core Animation's own renderer) into a Metal texture of the iPad's view, 834 by 1210 points at scale 2;
- steps each phase frame by frame, 120 frames to a second of phase time;
- times each frame from `beginFrame` to the end of a command buffer queued after it;
- counts the time past 8.33 ms as the hitch proxy, "late ms a second".

The bench also samples the wire layers' load every 15th frame (`LayerCanvas.wireStats`): the layers, those in view, their path elements, and their view-clipped area over the view's ("coverage", in views). It adds `os_signpost` intervals around a drag's wire updates (`wires.move`) and each tile batch (`wires.tiles`). The `--wire-*` switches each isolate one suspect.

One caveat about the proxy. `CARenderer` redraws the whole frame every time, while the render server redraws only what changed, so idle reads high here and 0 on the device. Only the phases that move the view compare.

### Suspects, on the canvas before the fix (500 nodes, one run each)

| run | idle | pan | pan_fit | zoom | drag | coverage while panning (views) | wire layers in view |
|---|---|---|---|---|---|---|---|
| as it was | 272.1 | 240.1 | 123.6 | 145.2 | 648.4 | 116.17 | 357.31 |
| 2-point wires, as the spike (`--wire-thin`) | 273.7 | 246.8 | 127.4 | 143.0 | 658.3 | 116.17 | 357.31 |
| mitred joins, as the spike (`--wire-miter`) | 267.1 | 239.7 | 144.7 | 153.1 | 644.2 | 116.17 | 357.31 |
| only wires as short as the spike's (`--wire-short-only`, 37 wires) | 0.0 | 0.0 | 0.0 | 0.0 | 0.0 | 0.06 | 4.47 |
| wires hidden (`--hide-wires`) | 0.0 | 0.2 | 0.0 | 0.0 | 0.0 | 0 | 0 |

Each difference between the spike's wires and the canvas's was a hypothesis, tested here or ruled out in the code:

- **Stroke width (2 against 2.5) and joins (mitre against round):** no change.
- **Dashes, shadows, halos, `shouldRasterize`, `drawsAsynchronously` and implicit animations:** none of these are on wire layers. Every update runs with actions disabled.
- **Paths rebuilt each frame during pan and zoom:** no. Pan and zoom change one transform, and paths are rebuilt only for a drag's wires.
- **Line width under zoom:** constant in graph units, as in the spike.
- **Culling:** the canvas hides wire layers outside the view and its margin, but a long wire's layer reaches into the view from far away and stays shown.
- **The wires' length:** this is what matters. Short wires cost nothing even as separate shape layers, and the same number of long wires costs as the device showed.

## The fix

`native/apple/PlumCraftFFI/Sources/PlumCraftCanvas/WireTiles.swift`, shared by the Mac and iPad canvases:

- **Tiles.** The graph is cut into 512-unit tiles. A tile's picture holds every wire that crosses it, in the scene's order, stroked as the shape layers stroked them: 2.5 units, round caps and joins, rings at bend points. Pictures are drawn on workers, one batch at a time.
- **No seams.** Each tile is drawn with two texels of margin beyond its edge, and its layer shows only the tile (`contentsRect`), so neighbouring tiles meet without a seam. The first version had visible gaps where tiles met; the tests' pixel comparison caught them.
- **The settled zoom's scale.** When a view change settles (200 ms, as for faces), visible tiles are drawn again at the screen's pixels per graph unit at that zoom, from 0.25 to twice the screen's scale. Faces use a power of two above the zoom instead; for wires that drew a 2.5-unit stroke visibly heavier when shrunk, which the pixel comparison caught too. Tiles within the cull margin are drawn at no more than 1, the rest at 0.5, and a settle puts far tiles back to 0.5. Without that last step, 80 MB of large pictures stayed behind after zooming in; now it is 34 MB at 500 nodes.
- **Pan and zoom move pictures only,** as for faces. Tiles outside the view and its margin are culled.
- **Live wires.** A wire being moved (a face drag) or picked up off its input leaves the tiles for a shape layer of its own. That layer is clipped to the view and its margin, so it is never graph-sized. A drag's wires leave at `beginMove`, before the pointer has gone far. A kept move's wires stay on their own layers, where they were dropped, until the scene brings their new paths. They go back once the tiles that draw them again are up, so a wire never vanishes or shows twice for longer than a batch.
- **Hover.** The wire under the pointer is drawn wider (4.5) on a layer above the tiles, rather than its own layer being widened.
- **The old drawing in the same build.** `--wire-layers` makes every wire live and unclipped, which is exactly the old drawing. The iPad can then compare both in one install.

The apps' API (`hover`, `pickUp`, `beginMove`, `move`, `endMove`) is unchanged. The test accessor `wireLayer(nid:)` is replaced by `wireBox(nid:)`, `wireShown(nid:)` and `liveWireLayer(nid:)`. `wireLayerCount` now counts drawn wires.

## What changed on the Mac

The Mac bench was run offscreen, three runs each at 100 and 500 nodes. "Before" is `--wire-layers`, the old drawing in the same build; "after" is the canvas as it is now (`claude-debug/native-n4/s9-gate-table.md` and `s9-bench/`).

| nodes | wires drawn as | phase | late ms/s (3 runs) | render p95 ms | coverage (views) | wire layers in view |
|---|---|---|---|---|---|---|
| 100 | a layer each (before) | pan | 0, 0, 0 | 2.48 | 5.91 | 88.63 |
| 100 | a layer each (before) | zoom | 0, 0, 0 | 2.55 | 15.13 | 102.13 |
| 100 | a layer each (before) | drag | 0, 0, 0 | 2.98 | 13.77 | 115 |
| 100 | tiles (after) | pan | 0, 0, 0 | 0.67 | 0.42 | 8.63 |
| 100 | tiles (after) | zoom | 0, 0, 0 | 0.29 | 0.62 | 7.33 |
| 100 | tiles (after) | drag | 0, 0, 0 | 0.42 | 0.7 | 13 |
| 500 | a layer each (before) | idle | 302.8, 287.3, 288.2 | 11.12 | 160.35 | 522 |
| 500 | a layer each (before) | pan | 249.6, 246.1, 253 | 17.95 | 116.17 | 357.31 |
| 500 | a layer each (before) | pan_fit | 148, 152.3, 164.7 | 9.98 | 43.02 | 800 |
| 500 | a layer each (before) | zoom | 155.8, 163.9, 167.2 | 13.44 | 147.12 | 401.27 |
| 500 | a layer each (before) | drag | 690, 680.8, 663.3 | 14.34 | 152.25 | 553 |
| 500 | tiles (after) | idle | 0, 0, 0 | 0.88 | 1 | 30 |
| 500 | tiles (after) | pan | 0, 0, 0 | 0.94 | 0.93 | 22.19 |
| 500 | tiles (after) | pan_fit | 0, 0, 0 | 1.1 | 0.37 | 94.72 |
| 500 | tiles (after) | zoom | 0, 0, 0 | 0.6 | 0.97 | 18.81 |
| 500 | tiles (after) | drag | 0, 0, 0 | 0.62 | 0.7 | 13 |
| 500 | hidden | every phase | 0 in all 15 | 0.44 to 0.95 | 0 | 0 |

What else the runs showed:

- **Memory.** Wire pictures take 28 MB at 100 nodes and 34 MB at 500, beside the faces' 43 and 121 MB. The process footprint during the phases was the same with tiles as with layers: 269 to 292 MB at 100 nodes.
- **A one-node change** on screen is unchanged: 5.6 to 7.0 ms p50 at 500 nodes, against 5.6 to 7.4 before.
- **The first frame after open** is 150 to 158 ms at 500 nodes, against 131 to 136, because the tiles' first drawing shares the workers with the faces.
- **Single slow frames.** An earlier tiles run had one 55 ms frame in its drag phase, and a 100-node run had one frame just over budget. Neither recurred in the six final runs.

## Pictures: does it look the same?

`WireShotsTests` (run with `WIRE_SHOTS=DIR`) draws both ways in one build and compares them pixel by pixel. A pixel differs if a channel is off by more than 48 of 255. The pictures are in `claude-debug/native-n4/s9-shots/`.

| picture | pixels drawn | pixels differing | % of drawn |
|---|---|---|---|
| 500 nodes fitted, light | 967,276 | 5,526 | 0.57 |
| 500 nodes at zoom 0.5, light | 2,748,109 | 2,521 | 0.09 |
| 500 nodes at zoom 1.5, light | 4,035,682 | 0 | 0.00 |
| 500 nodes fitted, dark | 967,162 | 5,317 | 0.55 |
| 500 nodes at zoom 0.5, dark | 2,746,681 | 2,465 | 0.09 |
| 500 nodes at zoom 1.5, dark | 4,035,675 | 0 | 0.00 |
| a backward wire's route and net tags, light and dark | 281,755 and 281,555 | 0 | 0.00 |
| a selected net, light and dark | 281,755 and 281,555 | 0 | 0.00 |
| the wire under the pointer, light and dark | 285,776 and 285,580 | 0 | 0.00 |
| a face being moved (its wires live) and a new wire being dragged from a pin, light and dark | 283,775 and 283,602 | 0 | 0.00 |

What differs, and why:

- **At the fitted zoom (about 0.13),** where 800 wires cross in a small space, the tiled wires read slightly heavier: the tile is drawn at the lowest scale (0.25) and shrunk a little to the view.
- **At zoom 0.5, a few antialiased edge pixels** differ where tile pictures are resampled.
- **What the comparison can't see:** both sides are drawn by `CALayer.render(in:)`, the CPU's drawing of the layer tree, not by the render server. On the iPad a shape layer's own rasteriser could antialias slightly differently from Core Graphics.
- **Between settles,** while a zoom is under way, tiles are scaled pictures and look softer, as faces do. They are drawn again at the right scale 200 ms after the view stops.
- **Selection.** The scene has no selected-wire state, so "a selected wire" means a selected net (its tags filled) and the wire under the pointer. Both draw as before.

## What only the iPad can confirm, and how

Before the run, the iPad needs to be connected and unlocked, with Auto-Lock set to Never, kept on power, and UI Automation enabled. The Mac needs 15 GB free for each trace; `bench.sh` gives each trace its own temporary folder and deletes it afterwards.

```sh
cd native/apple/PlumCraftPad
DEVICE=<the iPad's id> TEAM=<team id> OUT=<results folder> PREFIX=s9 ./bench.sh trace-wires 500 800
```

This builds and installs once. It then records a hitch trace for tiles (as shipped), layers (`--wire-layers`, the old drawing) and hidden (`--hide-wires`), alternating, two rounds, and writes `OUT/s9-wires-500-summary.md` with each phase's ms of hitch a second.

**What would confirm the fix:**

- the tiles rows read 0 to low single digits in every phase, as the hidden rows and the N0 spike do;
- the layers rows still show roughly the day's numbers (pan about 150, zoom about 150, drag 50 to 90), which shows the comparison is fair.

To confirm the cause on the device as well, add `WIRES="short thin miter" RUNS=1`. Short should read like hidden; thin and miter should read like layers.

Not yet shown on the device: the memory the wire pictures take there, which `./bench.sh device 500` reads into its JSON as `wire_pictures_mb`.

## What the Windows canvas should take

`native/windows/src/PlumCraft.App/Canvas/CompositionCanvas.cs` draws a ShapeVisual per wire with its bounds tight round it, the same design, so it should have the same cost with long wires. Once a Windows GPU measurement shows that cost (PresentMon, note 24 section 20.3), it should take the same scheme. I did not edit the Windows code.

- one CompositionDrawingSurface per 512-unit tile, drawn by Win2D off the UI thread, with a margin of a couple of pixels and a surface brush that shows only the tile;
- tiles drawn again at the settled zoom's own scale, with far tiles at a low scale;
- moved or picked-up wires as their own ShapeVisuals, clipped to the view;
- a hover visual above the tiles.

## Defects introduced and caught on the way

- **Seams between tiles:** a tile picture's edge was sampled against nothing. Caught by the pixel comparison test; fixed with a two-texel margin and `contentsRect`.
- **Heavier wires when zoomed out:** tiles were drawn at a power of two above the zoom, as faces are, and shrunk. Caught by the same test; fixed by drawing at the zoom's own scale.
- **80 MB of wire pictures left after zooming in:** large tiles were never drawn again once out of sight. Fixed: a settle puts tiles far from the view back to 0.5. Now 34 MB.
- **Test-only problems:** the first comparison test used an unflipped host layer, so every picture came out upside down; a theme change raced the first snapshot. Both are fixed in the tests.

## The other job: B60

The B60 investigation is recorded in `docs/BUGS.md` and STATUS. Briefly: it was not reproduced in an iPad Pro 11-inch (M4) simulator I created, booted headless by UDID, then shut down and deleted. That simulator ran in portrait with the software keyboard showing (screenshot `s9-b60/n4s3-ui-count-typing.png`) and the status read from the accessibility element, at the default text size, the three largest standard sizes, and Accessibility Large. I found no app defect. The test now waits for `face.count` as its other steps wait, looks for it in the list both ways, and attaches a screenshot and the element tree if it is still missing. To confirm, run `DEVICE=... TEAM=... native/apple/PlumCraftPad/device.sh ui`: a pass means the cause was the tap's timing; a failure means the attachments show what the popover held.

## Tests and checks

- `swift test` in `native/apple/PlumCraftFFI`: 87 passed, none failed, in five groups (31, 9, 14, 5, 28). This includes four new tile tests: a wire is never a layer bigger than a tile; a moved face's wires leave the tiles and come back; a picked-up wire is faint on its own and comes back; tiles draw what the shape layers drew at three zooms. The two screenshot tests are skipped unless `WIRE_SHOTS` is set.
- **The Mac conformance suite:** 53 of 53, before and after the merge.
- **The Mac Release build and the iOS Release device-SDK build** (unsigned, not installed): both succeed, with no warnings other than the system's App Intents note, which both builds always print.
- **The UI test in the simulator** (`testWindowsAndNames`): passes, with the status read from the element and from the file.

## Files

**Changed:**

- `native/apple/PlumCraftFFI/Sources/PlumCraftCanvas/LayerCanvas.swift`
- `native/apple/PlumCraftFFI/Sources/PlumCraftShell/CanvasBench.swift`
- `native/apple/PlumCraftFFI/Tests/PlumCraftCanvasTests/CanvasTests.swift`
- `native/apple/PlumCraftMac/App/Bench.swift`: bench switches; `--offscreen`; a scripted bench window no longer activates the app.
- `native/apple/PlumCraftPad/App/Bench.swift`
- `native/apple/PlumCraftPad/bench.sh`: `trace-wires`.
- `native/apple/PlumCraftPad/UITests/GestureTests.swift`: B60.
- `docs/BUGS.md`, `docs/STATUS.md`, `docs/research/24-native-shells.md`

**New:**

- `native/apple/PlumCraftFFI/Sources/PlumCraftCanvas/WireTiles.swift`
- `native/apple/PlumCraftFFI/Sources/PlumCraftShell/OffscreenRenderer.swift`
- `native/apple/PlumCraftFFI/Tests/PlumCraftCanvasTests/WireTilesTests.swift`
- `native/apple/PlumCraftFFI/Tests/PlumCraftCanvasTests/WireShotsTests.swift`
- `native/apple/PlumCraftPad/hitch_row.py`
- this report

**Scratch** (`claude-debug/native-n4/s9-*`): the bench, test, simulator and gate scripts, the results, the pictures and the logs.
