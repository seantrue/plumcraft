# B155: a wire's right angles, dragged and converted

*2026-11-24, branch `fix/b155-wire-corners` on `fix/b153-bend-point` (d4650db). Uncommitted.*

## The model chosen, and the alternative

**No new stored state.** A wire still has only its bend points (`via`). What is new is how they are read:

- **Right-angled is a property of the bend points.** A wire whose bend points (at least two) each lie straight across from, or straight above or below, the one before (`wire_path::right_angled`) is drawn as a right-angled route:
  - straight legs and rounded corners (radius 12, as the automatic route's);
  - leaving its output across to its first point's x, then up or down to it;
  - coming from its last point up or down to its input's height, then across into it.

  Moving a node therefore never breaks the angles: the elbows at the pins follow the pins. Any other wire with bend points is drawn as before, a smooth curve through them.
- **An automatic route** (a wire running right to left, `wire_path::backward_corners`) has six corners from pin to pin. Its four inner corners, and the legs between them, can be taken hold of.

**Why not a stored style.** A per-wire flag (`style: "square"`) would be a graph-format change. It would also need rules for what a drag of a single bend point does to a squared wire. The bend points alone already carry the right angles, through node moves too, so nothing visible needed the flag.

**The alternative, which the maintainer would see differently:** an existing wire whose own bend points happen to line up (two or more, exactly) is now drawn right-angled where it was a curve. A stored flag would leave such wires curved until someone chose Make Right-Angled. Hand-placed points rarely line up exactly (whole graph units), so the chosen model takes that risk. If the maintainer wants old wires to stay curved, the flag is the way: additive, defaulting to curved.

## What a person can do

| Action | Where | What it does | Command |
|---|---|---|---|
| Drag an automatic route's corner | any shell that sends drags to the core | the route's corners become the wire's own bend points with that one moved; the corners beside it move with it on the axis they share | one `wire.route`, one undo step |
| Drag an automatic route's leg (not the ones at the pins) | the same | the leg moves across itself: a horizontal one up or down, a vertical one left or right | one `wire.route` |
| Drag a right-angled wire's corner or leg | the same | the same, on its own points | one `wire.route` |
| Make Right-Angled | the wire's menu: a wire whose bend points don't line up, or one with none running right to left | squares the bend points (see below), or takes the automatic route's corners | `wire.square {to}`, one undo step |
| Route Automatically (was Straighten) | the wire's menu, when it has bend points | drops them: the automatic route, or the plain curve | `wire.route {via: []}` |

**Make Right-Angled where it is ambiguous.** It leaves the output across. From each point to the next that is neither straight across nor straight above or below it, it goes across first, then up or down. Into the input it comes up or down first, then across. Repeated points and points on a straight leg are dropped. A wire whose pins are level and whose points lie on the line between them ends with no bend points: a plain wire.

## The core

- `wire_path.rs`: `backward_corners` (the automatic route's corners; `backward_chain` now rounds them), `right_angled`, `orthogonal_corners`, `simplified`, `square_up`, `route_drag`, and `segment_distance` made public.
- `geometry.rs`:
  - `wire_chain` draws a right-angled wire's legs.
  - The snapshot keeps each wire's corners (`SWire.corners`, `WireInfo.corners` and `ends`).
  - The hit test: after bend points, a route's inner corner is a `wire` hit with `part` `corner` (1), and an inner leg one with `part` `segment` (2). `index` counts corners from the output pin. `hit_json` says `part` and `index`.
  - `geometry.locate {wire, corner: k}` and `{wire, segment: k}`.
- `gesture.rs`: a drag that begins on a corner or leg is `route` (code 9). `move` gives `update.via` (the bend points as they would be); `end` runs one `wire.route`.
- `canvas_cmds.rs`: `wire.square {to}`, the core's command (it needs the pins), which runs `wire.route`.
- `menu.rs`: Make Right-Angled, and Route Automatically in place of Straighten.
- `plumcraft.h`: drag code 9. `docs/shell-interface.md`: the gesture row, the `via` note and the wire menu row.

## The shells

- **Apple (Mac, iPad, iPhone):**
  - The drag word `route`. While it is under way, a ring where it was pressed moves with it, as for a bend point. The core makes the route at the end.
  - The iPad and iPhone send a drag, not a pan, for a press on a corner or leg (`dragsItem`).
  - The Mac shows the open hand over a corner or leg.
  - Once a drag makes the corners the wire's own, they get the rings every bend point has.
  - **Not drawn:** a handle on an automatic route's corners before anyone takes hold of them. That would put rings on every right-to-left wire; the cursor (Mac) and the drag are the way in. This is the conservative choice; the alternative is a small dot at each automatic corner.
- **egui:** a right-angled wire is drawn with its legs (square corners), not a curve, from the core's `orthogonal_corners`. Its menu gains the two items from the core. It doesn't gain the drag on automatic corners: its own canvas handles wires itself, not through the core's gesture.
- **Windows, for the Windows agent** (no C# written here):
  1. A wire's `via`: draw a ring of radius 5 at each bend point, as the Apple shells' `LayerCanvas.chain` does. It has none today (`CompositionCanvas.cs:992-1000`, `CompositionCanvas.Tiles.cs:101-106`), so a bend on a straight leg is invisible (B153).
  2. Send a drag, not a pan or marquee, for a press whose hit is `wire` with `part` 1 or 2 (corner, leg); `hit.part` already comes in the hit struct.
  3. Accept drag code 9 (`route`) like a bend point's (code 4): a ring at the press, moved by the update's `dx`, `dy`. The core sends the command at the end; the scene redraws the wire.
  4. Optional: the size-all or hand cursor over a corner or leg.
  5. Nothing to draw differently: the path in the scene is already right-angled.
  6. The conformance case "a wire's right angles…" in `cases.json` holds it to all this.

## Tests

`crates/editor-core/tests/wire_corners.rs` (6 tests, through the gesture path and the menu as shells run them):

- a press on an automatic route's corner or leg is a `wire` hit with its part; the legs at the pins are not;
- dragging a corner makes the corners the wire's own in one undo step (undo, redo);
- a leg moves across itself, and the route stays right-angled (each drag one undo step);
- Make Right-Angled squares free points, the menu stops offering it, and Route Automatically drops them (undo, redo);
- Make Right-Angled on an automatic route takes its corners;
- odd wires: points on top of each other, zero-length legs, a point above the output, out-of-range indexes, huge drags, a wire back into its own node. No panic, and never a result that isn't right-angled.

All of them depend on what this change adds: before it, none of the functions, the hit `part` or the `route` drag exist.

A conformance case, "a wire's right angles: drag an automatic corner, Make Right-Angled, Route Automatically", runs the gesture path and the commands in every shell's runner.

## Launched on the Mac (control port only, no HID)

`launch.sh` and `launch.py` launch the Release build's scripted copy (scratch store) with two wires: one running right to left, and one with free bend points. Over the control port, they send what a mouse drag sends:

| Step | Read back |
|---|---|
| `ui.gesture` drag begin on the automatic route's corner 2 (`geometry.locate {corner: 2}`) | hit `{kind: wire, part: corner, index: 2}`, drag `route` |
| move 60 down | `update.via` `[[794,343],[794,444],[-24,444],[-24,343]]` |
| end | one `wire.route`, ok; the wire's bend points those four; the scene's kind `routed` |
| a drag on its leg 3, 40 right and 15 down | hit `part: segment, index: 3`; via `[[794,343],[794,444],[16,444],[16,343]]` (the upright leg moved sideways only) |
| Make Right-Angled on the free wire (`[[380,10],[440,150]]`), as its menu item runs it | `[[380,43],[380,10],[440,10],[440,43]]`, `routed` |
| three undos | the free wire back to `[[380,10],[440,150]]`; the other back to no bend points (its automatic route) |

The pictures are in `launch-shots/`, upside down (B148). They show the automatic route, the route through the dragged corners (now rings), the moved leg, and the squared wire.

## Gate

| Check | Result |
|---|---|
| The editor core (the new `wire_corners`, 6 tests, and the conformance case on the core's own driver) | 228 of 228 |
| egui (`plumcraft-ui`, its wire menu test following the new label) | 384 of 384 |
| Rust gate (format, clippy, every test, after the last change) | clean except the known `hidden_params_match_the_baseline` (1687 of 1688) |
| swift test | passed |
| The Mac app, every case over the control port (`conformance.sh all`, no person check), after the last change | 64 of 64, 8 skipped, 916 steps; the person's Library and home unchanged |
| The iPad standard gate (simulator; built before the last change, which is in the core's geometry only) | 61 of 61 cases; UI tests 12 passed, 0 failed, 5 skipped |

## Defects in this work

- **A spur.** The first Make Right-Angled kept a point where the route ran out and straight back along its own leg, a spur, which the first Mac picture showed. `simplified` now drops the middle of any three points on one line, whichever way they turn. The test expected the spur too, and was corrected with it.
- **Formatting.** `cargo fmt` flagged my first version.

## Not checked

- A person's real drag, by mouse or finger (no HID; the gestures were sent through the control port).
- A device.
- Windows (the list above).
- The iPad simulator's UI tests don't drag a route.
- egui's drag on an automatic corner: not built, since its canvas handles wires itself.
- Linux (yours).
