# N4 slice 2: inline controls, growing nodes, backward wires, icons and the more-button

Branch `native/n4-inline` (from `feat/native-n0` with N3 and N4 slice 1 merged), 2026-11-20. Uncommitted, for the controller to review.

## On the maintainer's screen

Nothing modal appeared. No alert, sheet, open panel, colour panel or menu was opened on the Mac by an automated run: the Mac's choice pop-up and colour well open their menu or panel only for a person's click, and the Mac shots typed into the field through the kit. The Mac app windows of the shot driver and of `conformance.sh` (each launched with `--scratch-state`, never activated) were on screen in the background while they ran, as in slice 1. The maintainer's apps (PIDs 5861 and 58303) were never signalled; nothing was built into `native/apple/PlumCraftMac/build` or `claude-debug/native-n3/s6-try/`.

## What was done, item by item

| Item | State | Notes |
|---|---|---|
| 1. Edit every inline widget kind | Done, both apps | text, multiline, number (INT and FLOAT, ranges), toggle, choice, multichoice, path, colour; scrub by drag; Tab, Shift-Tab, Escape, Return or Done, focus loss; one undo step "Set Value"; validation and clamping by the core with its words shown by the row |
| 2. Faces stay cheap | Done | one control at a time, made when `view.edit` opens and removed when it closes; a value edit redraws the one face whose `rev` changed (plus faces whose stale state changes); a scrub's value is a label layer, not a redrawn face |
| 3. Growing nodes | Done (core) | the scene shows overlapping faces pushed apart; the graph is untouched; a decision below |
| 4. Backward wires | Done (core, both apps) | routed round both faces; same path format; hit test and drag follow; egui still draws snarl's curve (a gap, below) |
| 5. Icons and the more-button | Done | Lucide icons from the core (`icon.get`) drawn in the header; ⋮ opens the core's node menu; the disclosure triangle already collapsed a node (slice 4's `node.view`), unchanged |
| 6. Slice 1's audit rows | Done (contract text) | Select mode and Share… written down as the shell's (contract 12.13); nothing else found |
| 7. Conformance and tests | Done | 6 new cases, 30 of 30 on all drivers, no skips; iPad XCUITest `testInlineEdits`; kit tests for placement at several zooms |

## The widget kinds and how each is edited

| Kind | Mac | iPad |
|---|---|---|
| text, path, multichoice | NSTextField over the row: Return commits, Escape cancels, Tab and Shift-Tab commit and move, focus lost commits; spelling, quote, dash and text substitution off | UITextField: autocorrection, capitals, smart quotes and dashes off; URL keyboard for a path; a bar with Previous, Next, Done; hardware Escape, Tab, Shift-Tab as on the Mac; the canvas scrolls the row above the keyboard |
| number | The same, right-aligned monospaced digits; the whole value shown (not the face's rounding) | The same with the numbers-and-punctuation keyboard (minus and point on it) |
| multiline | NSTextView in a scroll view, monospaced; Return is a new line, ⌘Return commits | UITextView, monospaced; Done or ⌘Return commits |
| choice | NSPopUpButton with the core's choices, disabled ones greyed with the reason as tooltip; opens at once for a person's click, waits for click or Space after Tab or a script | the system pull-down menu: a touch on the row is routed (in `hitTest`) to a button over it whose menu is the choices, the current one ticked, disabled ones greyed with the reason; a pick is `row.set` |
| toggle | drawn as a checkbox; a click flips it in the core (`ui.gesture` tap → `row.set`) | drawn as a switch; a tap flips it in the core |
| colour | NSColorWell; the colour panel opens for a person's click; committed when the panel closes | the system colour picker in a popover; committed when it closes |
| scrub | a mouse drag on a number's row (the core says `scrub`); the value shown over the row; one step at release | a one-finger drag on the row, the same |

## The far-zoom rule (a decision)

Built: the control sits over the row's widget at the canvas's zoom, its text the face's size (11 graph units × zoom). When that text would be under 10 points on the Mac or 11 on the iPad, the control is drawn at that legible size instead, centred on the row and floating over the face with a shadow (at least 170 or 220 points wide), so the view never moves to edit; a control near the view's edge is moved in. Rejected alternatives: zooming the view to the node first (moves the user's view on every edit at moderate zoom), a popover (a second surface for every edit). Pictures: `mac-edit-far-zoom.png` (35%), `pad-edit-far-zoom.png` (40%).

## The growing-node rule (a decision)

Built in the core (`scene::make_room`): where two measured faces overlap, the later one (by top, then left, then id) is shown pushed out of the way, to the right when it stands to the right and that is shorter, else below, 16 graph units clear; pushes cascade (at most 8 passes). The push is the view's: the scene's `pos` is where a face is shown; the graph's positions, the undo history and the dirty state are untouched (a run never dirties a clean document), and the push goes when the face shrinks. A drag moves a pushed face from where it is shown, so it doesn't jump; hit tests, wire paths, open groups, fit and locate use the shown places.

Trade-offs: the rule has no memory, so two faces a person deliberately overlaps are pushed apart too (the canvas keeps faces apart); a saved file keeps the unpushed positions, so the egui editor (which positions faces itself through egui-snarl) still shows the overlap; frames don't take part (a pushed face may be drawn past its frame's edge). The alternative is to push only faces that grew since they were placed (a size remembered per face), which would let deliberate overlaps stand but forgets across a reopen. Pictures: `mac-grow-before-after-run.png` (slice 1's build: Lower case jammed against Upper case after a run) and `mac-grow-after-after-run.png` (this slice: pushed clear).

## The routing rule

A plain wire whose input is left of its output, with both faces measured, leaves its output 24 to the right, runs along a channel (midway between the faces when one is wholly above the other, else below both or above both, whichever is the shorter way, 24 clear) and enters its input from the left, corners rounded at 12. It stays within the path format: a cubic chain of up to nine cubics (28 points) where a forward wire has four (13 points); `plum_wire_paths` already gives each wire's point count, so the C ABI is unchanged. The hit test, a wire's middle (marks, locate) and the dragged faces' wire paths use the same chain. Trade-off: the route is orthogonal, not a curve, and egui-snarl in the egui editor still draws its S-curve while the core hit-tests the routed one (egui's clicks on such a wire can miss). Pictures: `mac-routes-before-view.png` (slice 1 build) and `mac-routes-after-view.png`: a left-to-right wire, vertically stacked nodes, a node directly behind, and a target up and to the left; `pad-routes-after.png`.

## Core, C ABI, kit and contract changes

- **Core, new:** `inline` (editability, value parsing and clamping, scrub speeds, Tab order), `icons` (the Lucide set moved here from `plumcraft-ui`, which re-exports it), canvas commands `row.edit`, `row.set`, `geometry.parts`; method `icon.get`; scene `view.edit`; `scene::face_of_node`; `scene::make_room`; `wire_path::backward_chain` and `rounded`; snapshot row widgets and shown positions; `geometry.locate` takes `row` and measured part names, and `part: "header"` avoids measured parts (needed once the apps reported a title part); `Measured` keeps parts across a same-size typed report.
- **Gestures (`gesture.rs`):** tap on an editable row selects and opens it (`row.edit`) or flips a switch; double tap there edits it; tap on the `menu` part replies with the node's menu; a drag on a number's row is `scrub` (kind 8); `ui.gesture` takes `zoom`; moves start from shown positions.
- **C ABI:** `plum_drag` kind 8 with the value bit-cast into `rect[0..1]` (and as a float in `rect[2]`). Struct and functions unchanged; **`PLUM_ABI_VERSION` unchanged (1)**. Header comments updated.
- **Text engine:** `text.widgets` ("Every widget", Text › Testing) in the Rust and Python engines (identical catalogue, the parity test passes), with BOOL and ENUM declared; Python's golden schema regenerated.
- **Schema:** `shell1.json` regenerated (`view.edit`).
- **Kit:** `RowEditor` (PlumCraftKit), `EditorPlacement`, `FieldEditing`, `IconStore` and `SVGIcon` (a Lucide SVG subset parser, arcs included), `FieldEditorHost` for AppKit and UIKit, scrub and note labels in `LayerCanvas`, `rowRect`, `moreRect`, `settlePending`; `CanvasDrag` scrubs; `CanvasLink.reportParts`; models `View.edit`, `RowEdit`, `DragWord.scrub`, `Gesture.zoom`, `HotDrag.scrubValue`; `CanvasSteps` steps `tap`, `edit`, `field`, `return`, `tab`, `backtab`, `escape`, `choose`, `color`, `zoom` and targets `row=`, `more=`; the face draws the icon, the ⋮, a switch on iOS, and keeps a multiline value's own lines.
- **Apps:** iPad status file has `rows`, `editor`, `points.more`; the colour picker's popover; sample `Inline Editing`.
- **Contract:** `docs/shell-interface.md` 8.8 (backward routing), 8.11 (parts, locate), 8.13 (rows, more-button, scrub), 9.6 (kind 8), 12.10 to 12.13 (inline editing and the Apple controls, faces never overlap, icons, what the shells keep), 16.1 (`rows`); `docs/control-protocol.md` (`icon.get`, `geometry.parts`, `row.edit`, `row.set`, `ui.gesture` `zoom` and its row meanings, `geometry.locate` `row`).

## Conformance

| Driver | Result |
|---|---|
| C ABI, headless (`plumcraft-ffi::conformance`) | 30 of 30 run but the picture case on the text engine (as before) |
| C ABI's control port | all passed |
| egui editor | all passed (steps with `only: "rows"` don't run there: egui's own widgets take clicks on its rows) |
| Swift kit driver (`swift test`) | passed |
| Mac app (`conformance.sh all`) | 30 of 30, 0 skipped |
| iPad app in the simulator (`conformance.sh all`) | 30 of 30, 0 skipped |
| iPad XCUITest (`conformance.sh gestures`) | `testInlineEdits` and `testTouchGestures` pass |

New cases: a value typed into a row (checked, held, refused, undo, redo, Tab, Escape); every widget kind set through its row; a switch's tap and a number's scrub (the typed move's value checked); the more-button's menu; a backward wire routed and hit; overlapping faces shown apart with the graph's positions, undo name and dirty state unchanged.

## Tests

| Run | Result |
|---|---|
| `cargo fmt --all --check` | clean |
| `cargo clippy --all-targets --all-features -- -D warnings` | clean |
| `cargo nextest run --all-features` (b3, scratch) | 1,485 of 1,487: the known `plumcraft-craft::params_coverage::hidden_params_match_the_baseline`, and `plumcraft-ui::kittest_viewer_tools scrolling_in_past_the_picture_asks_for_detail_once` (intermittent under load: alone 20 of 20 pass; this slice changed nothing it uses but the icon data's location). An earlier full run: 1,484 of 1,486 with only the known failure and the stale schema |
| `check-hash-gates.sh`, `check-core-toolkit-free.sh` | pass |
| Python (`uv run pytest -q`, `ruff`) | in the gate run, 223 of 224 (`test_persist.py::test_fast_never_session_too_big_and_setup_bound_results_are_not_persisted`, a timing case, under the parallel cargo load); alone 10 of 10, and the whole suite again 224 of 224; ruff clean |
| `build-xcframework.sh` | built |
| `swift build`, `swift test` | 73 of 73 (69 before, 4 new in `InlineTests.swift`), zero warnings; `selectionIsASublayerAndRemovalTakesTheLayers` (made to wait for icons and the settle redraw) 20 of 20 |
| Mac app Release; iPad simulator Debug and Release; device arm64 Release (`s1-ipad-device.sh build`) | build, zero warnings |
| New Rust tests | `inline` unit tests (4), `wire_path` (2), `icons` (1), `tests/inline_edits.rs` (5); one existing geometry test changed deliberately (below) |

## Changed expectations, deliberately

- `geometry::tests::degenerate_and_hostile_points_never_panic`: a face placed over another was "the last drawn is on top"; faces now never overlap, so it checks the pushed face is hit where it is shown and the other stays.
- `CanvasTests.selectionIsASublayerAndRemovalTakesTheLayers`: the baseline draw count is read once it holds (icons arrive after the first draw and redraw the faces once); `settled` also waits for the pending settle redraw.
- `cases.json`: no existing case's expectation changed; `locate part: header` now avoids measured parts, which the apps now report (without that, two read cases hit the title).

## Store, processes, simulator

- The real store, two levels, before and after (`claude-debug/native-n4/s2-store-before.txt`, `s2-store-after.txt`): identical. One early probe (a Python one-liner dumping the new node's catalogue) ran before I sourced the scratch environment and read the store's `.policy.json`; it wrote nothing (the listing matches slice 1's after-listing).
- Final `pgrep -lx`: `5861 PlumCraft`, `58303 PlumCraft` (both the maintainer's, untouched); no `PlumCraftPad`, `CanvasSpike`, `CanvasSpikeMac` or `xctrace`.
- Simulator "iPad Pro 13-inch (M5)" still booted; `com.plumcraft.pad` and `com.plumcraft.pad.uitests.xctrunner` uninstalled; `com.plumcraft.spike.canvas` (not mine) left; nothing erased.

## Evidence

Under `claude-debug/native-n4/s2-shots/`:

- Mac, light and dark (`mac-…-light`, `-dark`): `edit-text`, `text-set`, `edit-number`, `number-held` (12 held to 10), `number-refused` (the core's words by the row), `edit-choice`, `choice-set`, `edit-multiline`, `edit-multichoice`, `edit-color`, `color-set`, `toggle-flipped`, `scrub-mid`, `scrub-set`; `mac-edit-far-zoom`; `mac-routes-before-view`, `mac-routes-after-view`; `mac-grow-before-before-run`, `-after-run` and `mac-grow-after-before-run`, `-after-run`.
- iPad, light and dark with the on-screen keyboard up and the row above it: `pad-edit-text`, `-number`, `-multiline`, `-multichoice`, `-choice`, `-color`, `pad-toggle-flipped`, `pad-scrub-mid`, `pad-number-refused`; `pad-edit-far-zoom`; `pad-routes-after`; from the UI test `n4s2-ui-text-keyboard`, `-text-set`, `-number-keyboard`, `-choice-menu` (the system menu open), `-after-scrub`, `-more-menu` (the node menu from ⋮).
- Recording: `pad-inline-edits.mp4` (0.55 MB, re-encoded from 22 MB): the UI test tapping a text value, typing, Done, a number, a choice, a switch, a scrub and the more-button.
- Drivers: `claude-debug/native-n4/s2-mac-shots.py`, `s2-pad-shots.py`, graphs in `s2-graphs/`; gate `s2-gate.sh` and its logs.

## Defects introduced and caught

- Header parts reported for the first time made `locate part: header` land on the title in two existing cases on both apps (28 of 30): fixed in the core's locate, and the title part tightened to its text.
- Icons arriving after a face's first draw made a canvas test's draw count racy: the canvas now counts icon fetches as not idle and tests wait for the settle redraw.
- The Mac pop-up's and well's bezels let the face's own drawing of the value show through ("tttle" in dark): given an opaque background.
- A multiline value's own lines were flattened on the face: kept.
- A first canvas test of the placement thresholds (15 points on the iPad) floated the editor at zoom 1: thresholds lowered to 10 and 11 points.

## Decisions for the maintainer

1. Growing nodes: the view-only, history-free push (built), or push only faces that grew since placed (deliberate overlaps stand, forgotten across a reopen), or real moves joined to the person's next edit.
2. The far-zoom rule: a legible floating editor (built), or zoom the view to the node first.
3. Backward wires: orthogonal with rounded corners (built), or a smoother curve through the same channel; and whether the egui editor should draw the core's route (today it draws snarl's curve and hit-tests the core's).
4. The test node `text.widgets` ("Every widget") now ships in both text engines, in Text › Testing beside Slow: keep it there, or hide Testing kinds from the palette.
5. Autocorrection is off for every text kind (values are often code). Turn it on for prose-like `text` ports, if the catalogue ever says which are prose.
6. On the iPad a choice's row and the more-button take the touch from the canvas (a drag starting on them doesn't move the node); on the Mac a click on a choice opens its menu at once. Keep.

## What only a person can check

- Real typing on the Mac: every Mac picture is the kit filling the field in a background window; focus rings, the caret, Return, Tab and Escape from a keyboard, the pop-up's menu and the colour panel opened by a click, and focus going back to the canvas are unverified.
- On a real iPad: the keyboard's bar and the canvas lifting above it at every orientation, Stage Manager sizes and with a hardware keyboard (its Tab and Escape), the switch and menu touch targets, scrubbing feel, and the colour picker popover.
- Whether the push of overlapping faces and the routed backward wires read well on the maintainer's own graphs.

## For slice 3

- `RowEditor` and `FieldEditorHost` are the editor pattern for the parameters sheet: the same `row.edit`/`row.set` take any port, shown or not (`row.set` reads the node's own face), so the sheet can reuse the core's checks; sliders for spans are not built on the face (egui's face uses drag fields too).
- The `view` domain now carries `edit`; pop-out windows replacing the Watched column can follow the same pattern (`view.*` state, a kit model following it).
- `make_room` ignores frames; groups' open rects follow the pushed faces. Frames and named ports may want to take part.
- The egui editor does not draw backward routes or pushed positions; if it stays the Linux and Windows editor, it should draw faces at the scene's `pos` and route wires through the core's chain.
