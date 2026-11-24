# B158: the wire follows a bend drag; quiet bend points

Branch `fix/b158-wire-drag` off the dev branch at `0147266`.

## Causes, confirmed in the code

**Part 1 (the wire does not drag).** In `CanvasDrag.swift`, `.bend` and `.route` showed only a 12-point ghost ring moved by the drag's offset. The wire was redrawn only when the end's `wire.route` changed the scene. The typed move (`plum_gesture_move`) carries no path. This was the same for an ordinary bend point's drag before B155: B155 only added `.route` to the same branch.

**Part 2 (large, always-visible points).** `LayerCanvas.chain(_:via:)` added a radius-5 ellipse at every bend point into the wire's own path. So every routed wire carried its rings in its tiles, all the time.

egui does the same: `wire_route.rs` `route_overlay` paints a radius-5 circle (`HANDLE`) at every bend point of every routed wire. It is not changed here (see below). egui's wire does follow its drag (`shown_via`).

## What changed

| File | Change | Lines |
| --- | --- | --- |
| `crates/editor-core/src/gesture.rs` | `Bend` and `Route` carry the wire's nid. `Drag::bent_wire` gives the wire as it would be. The move reply adds `update.wire {nid, via, path}`. `bend_moved` is shared with the end. | +40 / -14 |
| `crates/editor-core/src/geometry.rs` | `Snapshot::chain_with(nid, kind, via)`: the scene's `wire_chain` with given bend points | +12 |
| `crates/ffi/src/lib.rs`, `include/plumcraft.h` | `plum_drag_wire`: the same, typed (two records in `plum_wire_paths` form: the path, then the bend points) | +54 |
| `docs/shell-interface.md` | Rows for `update.wire` and for when bend points are drawn; `plum_drag_wire` in 9.6 | +4 |
| `cases.json` | Case: "a wire follows a drag on its bend point, and a cancel leaves it" | +21 |
| `crates/editor-core/tests/wire_drag_preview.rs` (new) | 4 tests | 153 |
| `crates/ffi/tests/geometry.rs` | 1 test of `plum_drag_wire`, including hostile arguments | +60 |
| `PlumCraft/HotCalls.swift`, `PlumCraftKit/CanvasLink.swift` | `dragWire` wrappers | +28 |
| `PlumCraftCanvas/BendRings.swift` (new) | `showBentWire` (live layer; kept until the scene repaths on end; restored on cancel); dots only for the dragged, hovered and end-selected wires; read-backs | 110 |
| `CanvasDrag.swift`, `LayerCanvas.swift`, `WireTiles.swift` | The drag draws the core's path; the ring layer; hover feeds it; `chain` loses its rings | +45 / -20 |
| `PlumCraftCanvasTests/BendDragTests.swift` (new) | 2 tests | 117 |

## Choices

- **Rule:** dots show while the wire or one of its bend points is under the pointer, while the wire has an end on a selected face, or while it is being bent. egui's rule is "always", which is the fault, so I did not copy it.
- **What "selected" means:** the scene has no wire selection (a tap on a wire clears the selection and watches its source). The canvas therefore takes "an end on a selected face" from the scene's selection.
- **Size:** a dot of radius 3 graph units in the wire's colour, with a 1-unit outline in the canvas colour. The core's hit radius stays 5 plus the slop. A test checks a hit 4.2 units from an undrawn bend point.

## Checks

| Check | Result |
| --- | --- |
| fmt, clippy `-D warnings`, schema, hash gates, core toolkit-free, private strings | pass |
| nextest, all features | 1698 of 1700. The B158 FFI test failed first (its bend point lay under a face) and was fixed and rerun with the conformance suites: 13 of 13. `plumcraft-craft::params_coverage` fails on this machine's installed packs (newly hidden parameters in another app's pack), which is not touched here |
| pytest, ruff | 249 passed, clean |
| swift test (kit) | all pass, including BendDrag 2 of 2 |
| Mac conformance, scripted copy, scratch store, `all` (no person check) | 75 of 75, including the new case |
| iPad Pro 11-inch simulator (79D3825B, booted and shut down here), `all` | 63 of 63 (12 skipped as before), including the new case |

## Launched and read back

Scripted Mac copy, control port only (`launch.sh`, `launch.py`, `launch.out`; pictures in `launch-shots/`, flipped as B148):

- `1-unselected.png`: no dots.
- `2-selected-a1.png`: two small dots on the selected face's wire.
- `4-after-unselected.png`: the moved wire, no dots.

The move's `update.wire` (nid 5, via [[380,10],[480,230]], 10 points) equals the scene's path after the end. One `wire.route`; undo restores the bend points.

The port cannot show the canvas's live preview. `ui.gesture` over the port goes to the core and never through the canvas's `CanvasDrag`, so `3-mid-drag-port.png` shows the stored wire. The live preview was read mid-drag in-process instead (`BendDragTests`, real core, real `LayerCanvas` and `CanvasDrag`): during the drag, the wire's own layer holds the core's path through the moved point, its dot moves with it, and the document is unchanged. After the end, the scene path matches; after a cancel, the wire's own is back.

Pictures from that test are in `shots/b158-*.png`. Its tile pictures are unreliable (a headless snapshot draws the tiles misplaced), but `b158-4-mid-drag.png` shows the live bent wire.

## Not checked

- No person check: no hand drag on the Mac or iPad.
- The live preview has not been seen in the running app.

## For the Windows canvas

1. During a bend (kind 4) or route (kind 9) drag, draw the move's `update.wire.path` (or `plum_drag_wire`) in place of the wire's own path, with dots at `via`. Keep it after the end until the scene brings the wire's new path; on a cancel, drop it at once. Compute no route in the shell.
2. Stop drawing rings into the wire's path. Draw small dots (radius about 3 graph units) only for:
   - the hovered wire, or the wire whose bend point is hovered;
   - wires with an end on a selected face;
   - the wire being dragged.

   Leave the hit test to the core.
