# N4 slice 1: the editing gestures and the palette

Branch `native/n4-gestures` (from `feat/native-n0` with N3 merged), 2026-11-20. Uncommitted, for the controller to review.

## What was done, item by item

| Item | State | Notes |
|---|---|---|
| 1. Drag nodes | Done, both apps | The kit's `CanvasDrag` (PlumCraftCanvas) is the one driver both hosts feed. Faces (and the selection with them) follow every frame through `plum_gesture_move` and `plum_wire_paths`; one undo step ("Move Node"); frames, frame sizes, open groups and bend points drag as the core decides, shown as a dashed outline; autoscroll within 36 points of the edge; Escape (both), a second finger and the system (iPad) cancel and restore. Arrow nudges were already `selection.nudge` on both. |
| 2. Wires | Done, both apps | Drag from a pin: the target pin is ringed and filled when it fits, dashed in the warning colour through a Convert, crossed in the error colour when it doesn't; a wired input's end is picked up (its wire fades while dragged) and moved or dropped off; a new wire dropped on nothing opens the palette filtered to what takes it, the wire stays drawn while it is open, a pick is "Add and Connect" (one step). Touch uses the core's per-gesture slop at 22 points (44 across): no contract change was needed. A haptic on the iPad when the target changes to a pin that fits. |
| 3. Marquee | Done | Mac: drag on empty canvas, a wire, a tag or a frame's body; Shift adds, ⌘ takes away (core). iPad: Select mode (toolbar toggle), the Pencil, or a pointer; one finger otherwise pans. A decision below. |
| 4. Palette | Done, both apps | `PaletteModel` (PlumCraftKit) over `view.palette`, `palette.items`, `palette.choose`, `canvas.palette {open: false}`. Mac: NSPopover, search field, source-list table by category, ↑ ↓ Return Escape, click away dismisses. iPad: popover in regular width, page sheet with medium and large detents in compact width, search field, inset-grouped list, a keyboard's ↑ ↓ Return Escape. Opened by double tap or click on empty canvas, long press or secondary click there, Tab, Node › Search…, and an Add Node toolbar button. Kinds of an engine not started are listed with "starts the array engine", and a pick of one shows a spinner and "Starting the array engine… X appears when it is ready" until the core answers. |
| 5. Editing commands | Mostly as before, plus touch | Delete, Duplicate, Cut, Copy, Paste, Select All, Group, Ungroup, Tidy, Undo, Redo were already the core's items in both apps' menus and context menus (N3 slices 4 and 5); this slice adds the touch taps (two-finger Undo, three-finger Redo, two-finger double tap Tidy), Tab, and removes `canvas.palette` from the unbuilt banner list. Only Undo, Redo and Tidy were exercised end to end in an app this slice (the iPad UI test); the rest were not re-walked. |
| 6. Fit | Done | A toolbar button on the Mac (new) and the iPad (it had one), F, and View › Fit (the core's `canvas.fit`). Fit is bound to no other gesture: the iPad's double tap went to the palette and three-finger pinch is the system's copy. The Mac keeps smart zoom (a trackpad two-finger double tap) fitting the node under the pointer. |
| 7. Conformance | Done | The whole suite, 24 of 24, no skips, on both apps. The iPad UI test drives the new gestures through UIKit's recognisers. The Mac: no UI test (it would need the app frontmost and take the keyboard); its gestures are covered by the suite's `ui.gesture` cases over the port, the kit tests and the screenshot driver (the same `CanvasDrag` and `PaletteModel`, not NSEvent recognition). |
| 8. Device recipe | Written, not run | `claude-debug/native-n4/s1-ipad-device.sh [build or test]`. The device arm64 build passes with zero warnings; the test step is unverified (see below). |

## Touch gestures and system clashes

| Gesture | Meaning | iPadOS clash, and what was done |
|---|---|---|
| One finger on a face, pin, bend point, frame tab or handle, group band | The core's drag | None |
| One finger on empty canvas, a wire, a tag, a frame's body | Pan (marquee in Select mode, with the Pencil or a pointer) | None |
| Two fingers | Pan and pinch | None |
| Tap | Select, watch a pin | None |
| Double tap | Palette on empty canvas; on a face the core's `node.parameters`, which says it isn't built yet | None (slice 5's fit-on-double-tap removed) |
| Long press (the system's context-menu timing, about 0.5 s) | Context menu; on empty canvas the palette, no menu | None |
| Two-finger tap | Undo | None found |
| Three-finger tap | Redo | Clash: iPadOS shows its edit shortcut bar on a three-finger tap, and three-finger swipes and pinches undo, redo, copy and paste. Turned off on the graph's view controller (`editingInteractionConfiguration = .none`), so the tap is Redo and nothing else shows. ⌘Z, ⇧⌘Z and the Edit menu still undo and redo. |
| Two-finger double tap | Tidy | None found |
| Pointer (mouse or trackpad) | As on the Mac: drag on empty canvas a marquee, scroll pans | Needed `UIApplicationSupportsIndirectInputEvents` so a click is an indirect-pointer touch the canvas can tell from a finger |

## How a touch marquee starts (a decision)

Section 5.5 already names it: "a Select mode in the toolbar, then tap or drag a marquee; Pencil drags a marquee directly". Built as that: a Select toggle (`rectangle.dashed`) in the iPad's toolbar makes a one-finger drag on empty canvas a marquee, two fingers still pan; the Pencil and a pointer always marquee there. A long-press-then-drag was not used, since a long press is the context menu.

## Core, C ABI, kit and contract changes

- **Core: the palette lists kinds of engines not started.** `palette.items` adds the kinds of compiled-in engines this document hasn't started (`Engines::offered_defs`, `engine_cmds::startable_defs`, `editor_core::engines::offered_defs`, `palette::with_starts`), each with `starts: <engine>`; `canvas.palette`'s `offered` counts them. Without this the palette could never offer an array kind on a text graph since decision 8C. Test: `crates/ffi/tests/engines_used.rs::the_palette_lists_an_engine_not_started_and_choosing_from_it_starts_that_engine`.
- **C ABI: `palette.choose` is held** while the engine its kind needs starts, as `node.add` and `node.addWired` were (`added_kind` in `crates/ffi/src/core.rs`).
- **Core: `run.selected`** (Run Selected), a canvas command: `run.start {targets}` with the selected nodes and selected collapsed groups' members; refused with "select nodes to run what they need"; in the menu model's Run menu after Run, disabled with the reason; `command.list` lists it. Test: `crates/ffi/tests/run_selected.rs`. Both apps dispatch it by id; their own Run Selected code is gone.
- **Contract:** `docs/shell-interface.md` 12.7 (kinds whose engine isn't started), 12.8 (Run Selected), 12.9 (the Apple shells' editing gestures); `docs/control-protocol.md` (`palette.items`' `starts`, `run.selected`). `shell1.json` is unchanged (`palette.items` isn't in it). `PLUM_ABI_VERSION` unchanged; no new C function.
- **Kit:** `CanvasDrag`, `Palette+AppKit` (`PalettePopover`), `Palette+UIKit` (`PaletteController`) in PlumCraftCanvas; `PaletteModel` in PlumCraftKit; `CanvasSteps` (shared `--then` steps: press, moveto, release, release-cancel, palette, type, pick, dismiss) in PlumCraftShell; `DocumentModel.onView`; `PaletteItem.starts`; LayerCanvas `showDragTarget`, `showGhost`, `pickUp(wire:)`, `anchorPoint`, `wireNid(into:)`; `ShellCommands.notBuilt(ran:)` and `canvas.palette` off the unbuilt list. Both hosts now build their drags on `CanvasDrag` (the Mac's own drag code went into it).
- **Apps:** Mac toolbar Add Node and Fit; a scripted launch (`--scratch-state`) no longer activates the app (it no longer takes the keyboard); the `quit` script step clears the edited state for a scripted launch, as TERM did. iPad toolbar Add Node and Select; Tab; Escape cancels a drag; the status file carries palette, positions, face and pin points for UI tests.

## Tests

| Run | Result |
|---|---|
| `cargo fmt --all --check` | Clean |
| `cargo clippy --all-targets --all-features -- -D warnings` | Clean (after fixing one `manual_contains` in my own code) |
| `cargo nextest run --all-features` (b3, scratch store) | 1,474 of 1,475; the known local failure `plumcraft-craft::params_coverage::hidden_params_match_the_baseline`; 2 new |
| `check-hash-gates.sh`, `check-core-toolkit-free.sh` | Pass |
| `build-xcframework.sh` | Built |
| `swift build`, `swift test` | 69 of 69 (65 before, 4 new in `EditingTests.swift`), zero warnings |
| Mac app, Release | Builds, zero warnings |
| iPad app, simulator Debug and Release; device arm64 Release | Builds, zero warnings |
| Mac `conformance.sh all` | 24 of 24, no skips |
| iPad `conformance.sh all` | 24 of 24, no skips |
| iPad `conformance.sh gestures` (XCUITest) | Pass (tap, long press, pinch, pan, Fit button, double tap palette and dismiss, node drag, two-finger Undo, three-finger Redo, wire dropped into the filtered palette and a row tapped, Select mode marquee, two-finger double tap Tidy, rotations) |
| Python tests | Not run (not in this slice's gate) |

## Defects introduced and caught

- **Palette closing race:** the Mac popover's close animates, so its did-close callback came after the next palette had opened, and dismissed that one in the core. Found by the screenshot driver (a dropped wire's palette never showed); fixed by holding the "the core closed it" flag until the close finishes, and the model's `dismiss` now tells the view to close.
- **A modal alert on the Mac's screen, about 70 seconds, from an automated run.** The screenshot driver's `quit` step quit an edited scratch document; NSDocument's review alert appeared (the app was in the background and not activated, but the alert was on screen) and TERM could not get past it, so the driver killed the app by its PID after 70 s. Fixed in `Script.swift` (a scripted `quit` clears the edited state, as TERM always did) and the driver no longer sends `quit`; later runs exit 0. Flagging it because it broke the rule against modal dialogs.
- **Shot set-up mistakes:** pins off screen or near the edge triggered autoscroll in early shots. That was autoscroll working, not a bug; the targets were moved.
- **A status the UI test read was stale:** Select mode was not in the window's status until the next report. Fixed.

## The audit of shell-side actions (the controller's item 4)

| Action | Where | Verdict |
|---|---|---|
| Run Selected | Both apps | Was shell-side; **fixed** (`run.selected`) |
| Fit | Both | The core's `canvas.fit` already; toolbar, F and menu all dispatch it |
| Add Node, Tab, Node › Search… | Both | `canvas.palette` (the core's); the shell only supplies the point |
| Zoom In, Zoom Out, Actual Size; smart zoom | Both | The contract leaves pan and zoom to the shell (5.3; control protocol: "zoom and pan are the window's alone"): fine |
| Show Watched Panel | Both | Shell window state; contract 12.5 makes windows the core's `windows` domain. Not fixed: the panel is being replaced by pop-outs (N4 slice 3). For the maintainer |
| Watched viewers' Fit, 1:1, Monospaced, Show All | Both | Shell-local; contract 13.2 says viewer zoom is `viewer.zoom` and text wrapping `viewer.text`. Not fixed (goes with the pop-outs). For the maintainer |
| Select mode (new) | iPad | A recognition mode, like Space-drag on the Mac; not a document action. For the maintainer whether it should be a command |
| Share… (the graph file) | iPad title menu | Shell-side share sheet; `share` asks and `shell.share_targets` are open in the contract. For the maintainer |
| Documents (close the graph) | iPad | Closes through `document.close`; the browser is the system's. Fine |
| Escape | Both | Dispatches `run.cancel` or `selection.clear` (or cancels a drag): fine |

## Store, processes, simulator

- The real store, two levels, before and after: identical (`claude-debug/native-n4/s1-store-before.txt`, `s1-store-after.txt`). Everything this slice ran used scratch stores.
- Final `pgrep -lx`: `5861 PlumCraft` only (the maintainer's, untouched, never signalled; nothing was built into its `build` folder; my Mac builds went to `claude-debug/native-n4/s1-build/mac`). No `PlumCraftPad`, `CanvasSpike`, `CanvasSpikeMac` or `xctrace`.
- Simulator "iPad Pro 13-inch (M5)" still booted; `com.plumcraft.pad` and `com.plumcraft.pad.uitests.xctrunner` uninstalled; `com.plumcraft.spike.canvas` (not mine) left; nothing erased.

## Evidence

Under `claude-debug/native-n4/s1-shots/`:

- Mac (driven through the kit by `claude-debug/native-n4/s1-mac-shots.py`, the app in the background): `mac-node-mid-drag`, `mac-wire-valid-target`, `mac-wire-convert-target`, `mac-wire-invalid-target`, `mac-marquee`, `mac-palette-light(-popover)`, `mac-palette-dark(-popover)`, `mac-palette-dropped-wire(-popover)`, `mac-after-add-and-connect`; the Fit and Add Node buttons are in every window's toolbar.
- iPad simulator (`claude-debug/native-n4/s1-pad-shots.py`): `pad-node-mid-drag`, `pad-wire-valid-target`, `pad-wire-invalid-target`, `pad-after-cancel`, `pad-marquee`, `pad-palette-light`, `pad-palette-dark`, `pad-palette-compact` (the sheet), `pad-palette-dropped-wire`, `pad-after-add-and-connect`; the UI test's `n4s1-ui-*` (palette by double tap, after a drag, the dropped-wire palette, after add and connect, after Tidy, Fit, the context menu, rotations).
- Recording: `pad-drags.mp4` (1.6 MB): a node drag, a wire to a valid target, to an invalid one, and a cancel.

## Decisions for the maintainer

1. The touch marquee: Select mode in the toolbar, plus the Pencil and a pointer directly (built, as section 5.5 had it), or something else.
2. The system's three-finger editing gestures are off on the graph so three fingers tapped are Redo (built). The alternative keeps the system's (its shortcut bar, swipe undo) and gives Redo another gesture.
3. A double tap on a face runs the core's `node.parameters`; with no parameters sheet yet the apps say so in the banner. Keep, or have the core's rule be something else until slice 3.
4. The audit's open rows: the Watched panel toggle and its viewers' tools (likely moot with pop-outs), Select mode as a command or not, Share….
5. A pointer on the iPad behaves like the Mac's (a drag on empty canvas is a marquee, not a pan): built that way, from "a mouse or trackpad is untouched".

## What only a person can check

- On the Mac: the feel of drags, wire snapping and autoscroll with a real mouse and trackpad; the popover's keyboard focus when the app is frontmost (all runs here were in the background); Escape cancelling a drag.
- On the iPad (not available this slice): the 22-point pin slop in the hand, the haptic on snap, the two- and three-finger taps and their freedom from system gestures on a device, the palette with and without a hardware keyboard, the Pencil marquee, and whether the on-screen keyboard should come up when the palette opens by touch (it does not; Tab focuses the field). `claude-debug/native-n4/s1-ipad-device.sh build` works; `test` is written but unverified, and likely needs the app's status file in an app group on a device.

## For slice 2 (inline controls)

- A press on a face's row is the core's drag today (a move). Editing a value on the face needs the hit test's `row` part to become a control: tap to edit, and a drag on a slider row to scrub rather than move. That should be the core's rule in `gesture.rs` (a drag beginning on a row of a widget kind decides "scrub"), not each shell's, so `CanvasDrag` can grow a scrub kind.
- `CanvasDrag.onBegan` and `onEnded` report the core's kind; the palette is a reusable pattern (a model in the kit, a popover and sheet per platform) for the more-button's menu and value editors.
- The status file's `points` gives UI tests face and pin positions; rows' rects could join it for tests of inline editing.
