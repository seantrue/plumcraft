# B153: Add Bend Point from a wire's right-click menu put the point in the wrong place

*2026-11-24, branch `fix/b153-bend-point` from `origin/feat/native-n0` (921fd65). Uncommitted. No device, no HID, no person check.*

## Cause

**The press point never reached the menu.**

- The core's `context` gesture put the point (`at`) only into its reply's `menu`, not its `context` (`crates/editor-core/src/gesture.rs:469-477` before the fix).
- The Apple shells don't use the reply's `menu`. They pass the reply's `context` to `menu.get` themselves:
  - Mac: `CanvasView+AppKit.swift:339-340`;
  - iPad and iPhone: `CanvasView+UIKit.swift`, the same `link.context`, then `model.contextMenu(ctx)`, `DocumentModel.swift:521`.
- Windows does the same with `r.Context` (`CanvasView.cs:651, 684`).

**So Add Bend Point took the menu's no-point branch** (`crates/editor-core/src/menu.rs:1337-1344`). That branch puts the point at the midpoint of the two nodes' top-left corners plus (110, 40), wherever the person clicked:

- On an ordinary wire, the point lands on or near the wire's own line, or under a face where the wire and its ring are hidden. The wire looks unchanged, though the core recorded an edit, so Undo had something to take back.
- On a wire routed right to left round its faces, the wire became a "routed" one through that point. The detour was gone ("removes auto routing"), but the point sat between or under the faces, not where the person clicked.

**Reproduced in the core along the shells' exact path** (`crates/editor-core/tests/wire_bend.rs`): a `context` gesture on the wire, `menu.get` with the reply's `context`, then the item's command as the menu runs it. Before the fix:

| Wire | Pressed at | Bend point added at |
|---|---|---|
| ordinary (source left of target) | (418, 43) | (360, 40) |
| routed right to left | (430, 104), on the detour below the faces | (410, 40) |

## Fix (the core, so every shell gets it)

`crates/editor-core/src/gesture.rs`: a wire's `context` in the gesture reply now carries `at`, the press in graph units, as the reply's `menu` already did. `menu.get` already read `at` from a wire's context (`menu.rs` `Context::parse`), so no shell changes. About 10 lines.

## Tests

- `crates/editor-core/tests/wire_bend.rs` (new, about 90 lines), two tests:
  - an ordinary wire: the bend point is where the press was, and the wire is drawn through it (`routed`);
  - a wire routed right to left: the point is where the press was, and not under either face.
  - Both **failed before the fix** and pass after.
- The conformance case for a wire's context (`cases.json`, the `context` gesture on `$b.text`) checked the reply's `context` as exactly `{kind, to}`. It now checks `kind`, `to` and that `at` is there. Every runner uses it, the Mac's, the iPad's and Windows'.

## Contract

`docs/shell-interface.md`, the gesture table's Context row: a wire's `context` carries `at`. This is an additive field; nothing that read the context breaks.

## The other shells

- **iPad and iPhone:** the same fault, the same path as the Mac. Fixed by the core change; no shell code changed.
- **Windows:** the same fault (`CanvasView.cs` passes the reply's `Context` to `menu.get`). Fixed by the core change; no Windows code changes. For the Windows agent: run the conformance suite with this branch's core, where the wire context case now also checks `at`, and right-click a wire › Add Bend Point.
- **egui:** it adds the bend itself at the click (`crates/ui/src/wire_route.rs`), so it never had the fault.

## Not changed

The menu's no-point branch is still what a `menu.get` without `at` gets (a script asking for a wire's menu with no place): the nodes' corner midpoint plus (110, 40). No shell's right-click reaches it now. A better place there would be the middle of the wire's drawn path, which `menu.get` doesn't have; I left it as it was.

## Launched on the Mac (control port only, no HID)

`claude-debug/b153/launch.sh` launches the Release build's scripted copy (`.scripted` id, scratch store) with two wires: an ordinary one, and one whose source is to the right of its target. `launch.py` then does what the right-click does, over the control port:

1. `geometry.locate` a point on the wire;
2. the `context` gesture there;
3. `menu.get` with the reply's `context`;
4. Add Bend Point's command;
5. the wire's bend points read back (`graph.get`, `scene.get`), and a `ui.screenshot`.

| Wire | Pressed at | Reply's `context` | Bend points after | Scene |
|---|---|---|---|---|
| ordinary | (365.0, 43.0) | `{kind: wire, to: b1.text, at: [365.0, 43.0]}` | `[[365, 43]]` | `routed` |
| right to left | (742.1, 404.0) | `{kind: wire, to: b2.text, at: [742.1, 404.0]}` | `[[742, 404]]` | `routed` |

The picture taken 3 s later (`launch-shots/after-3s.png`) shows both bend points as rings where the press was. One sits on the straight ordinary wire, which keeps its shape. The right-to-left wire is now drawn through its bend instead of its automatic detour. A picture taken at once (`ordinary.png`) still showed the old scene: the canvas draws the scene that follows the command a moment later. Both pictures are upside down: `ui.screenshot`'s offscreen picture on the Mac is flipped, a separate fault, not this one.

So **a bend added to a straight segment is visible on the Mac**, a ring at the point (`LayerCanvas.chain` draws one at every `via`). The iPad and iPhone draw with the same code.

## The two reports

- **The second report,** "places the point mid point of the selected segment", is this fault exactly: the no-point branch puts the point near the middle, between the two nodes' corners.
- **The first report,** "does not add one" (on a build without #192), is most likely the same fault. #192 changed only the curve editor's own window (`Popouts.swift`) and the person check, not the graph window's menu path. Before the fix the core always added a point (the tests' "before" values), at the no-point place. Whether that place was visible depends on the graph: on the wire's old line, under a face, or between faces on a right-to-left wire, which also drops its detour ("removes auto routing"). This is not proven: I have no copy of the maintainer's graph.

## Windows

- **The same fault in what it sends:** `CanvasView.cs:651, 684` passes the reply's `Context` to `menu.get`. The core fix covers it; no Windows change.
- **A gap of its own:** the Windows canvas draws only a wire's path (`CompositionCanvas.cs:992-1000`, `CompositionCanvas.Tiles.cs:101-106`) and no handle at a bend point. A bend on a straight segment is invisible there, and there is no ring to drag. Not fixed here (I can't build Windows); for the Windows agent: draw a ring of radius 5 at each `via`, as the Apple shells' `LayerCanvas.chain` does.

## B155, the automatic right-angle routes (not fixed)

They are the core's `wire_path::backward_chain`, drawn by every shell, egui's included (its own backward wires, `crates/ui/src/canvas.rs:184-187`). Their corners are not bend points, and no shell gives them handles. **egui had no interaction with them either.** As now, the only way to shape such a wire was Add Bend Point, which replaces the automatic route with a route through the point.

## Gate

| Check | Result |
|---|---|
| Rust gate (before the merge) | clean except the known `hidden_params_match_the_baseline` (1678 of 1679) |
| The editor core after the merge with `487a40d` | 222 of 222, `wire_bend` included |
| swift test | passed |
| The Mac app, every case over the control port (`conformance.sh all`; no person check) | 63 of 63 cases, 8 skipped, 902 steps; the person's Library and home unchanged |
| The iPad's standard gate (simulator, after the merge) | conformance 60 of 60 cases, 11 skipped (a second run: the first stopped at a build time limit while other agents' builds loaded the machine); UI tests 12 passed, 5 skipped, 0 failed |
