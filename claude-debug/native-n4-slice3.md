# N4 slice 3: parameters, groups, frames, nets, names in place, and pop-out windows

Branch `native/n4-windows` (from `feat/native-n0` with N3 and N4 slices 1 and 2 merged), 2026-11-20. Uncommitted, for the controller to review. The maintainer's own edit in `docs/BUGS.md` is untouched.

## On the maintainer's screen

Nothing modal appeared, and nothing of mine took the keyboard. No alert, sheet, share picker, colour panel or menu was opened on the Mac by an automated run. This slice opens real windows: every automated Mac launch was a scripted one (`--scratch-state`), which never activates the app; its pop-out windows are ordered just above its own document window (`order(.above, relativeTo:)`), never in front of another app's, and the parameters inspector is a popover on that window. The Mac pictures were taken with `screencapture -l` of the app's own windows, the app in the background throughout. The document windows of the picture driver and of `conformance.sh` were on screen behind the maintainer's windows while they ran, as in slices 1 and 2. The maintainer's apps (PIDs 5861, 58303 and 60555) were never signalled; nothing was built into `native/apple/PlumCraftMac/build` or touched under `claude-debug/native-n3/s6-try/` (the Mac builds went to `claude-debug/native-n4/s3-build/mac`).

## The rule: every action is a command

The maintainer: "No exceptions to the rule." Slice 2's exemption of Select mode and Share… in contract 12.13 is withdrawn, and 12.13 now says what a shell may keep: the view's pan and zoom, and platform chrome with no document meaning.

### The audit of both apps and the kit

| Action | Command id | Where it was | What I did |
|---|---|---|---|
| Select mode (iPad toolbar) | `canvas.selectMode` | iPad shell state (`CanvasView.selectMode`) | Core view state (`view.select`), View › Select Mode, canvas menu, toolbar check from the core |
| Share… (iPad title menu) | `graph.share` | iPad's own share sheet | Core command raising the `share` ask; iPad answers with the share sheet, the Mac with `NSSharingServicePicker`; File › Share… |
| Toolbar buttons' label, enabled, reason, check | `menu.get {context: toolbar}` | Hard-coded labels and symbols in both apps; Fit and Add always enabled | The core's toolbar model; the apps show its items with their SF Symbol for the core's icon |
| Show Watched Panel, ⌥⌘I, Watched button | (none) | Shell window state | Removed with the column |
| Watched viewer Fit, 1:1, Monospaced, Show All | `viewer.zoom`, `viewer.text` (new) | Shell-local | The pop-outs' controls dispatch the core's commands; their state is the core's (`popout.get`) |
| Rename… (node, group, frame), Rename Net…, Name This Port…, Rename Port…, Slice…, Label All Uses…, a promoted parameter's label | `name.edit`, `name.set` (new) | Menu items with no command: the egui editor drew its own windows; the Apple apps did nothing | Core in-place name editor; one native text field over the core's rect on both apps; egui shows a small window for it |
| Go to Driver, Go to Next Use | `canvas.reveal {node}`, `{net}` | Items with no command; the Apple apps did nothing; `canvas.reveal` was shell-only and refused by the C ABI | Commands; reveal is core view state (`view.reveal`) every shell follows |
| Add Bend Point (from a context menu) | `wire.route` | Item with no command when the menu had no point | The gesture's menu carries `at`; the item routes through the press point |
| Draw All as Wires | `wire.unlabel {to: [..]}` | Item with no command | Command, one undo step |
| Parameters (double tap, ⋮, Node menu) | `node.parameters`, `parameters.get`, `row.set`, `node.face` | "Not built" banner | Built (below) |
| Pop Out, double tap on a value | `view.popout`, `view.close`, `popout.get` | "Not built" banner | Built (below) |
| Leave a group (breadcrumb) | `canvas.focus` | Not shown in the apps | Path control (Mac), capsule of crumbs (iPad) |
| Collapse an open group from its band | `group.collapse` | Not reachable by a tap | The core's tap on the band's chevron |
| Zoom In, Zoom Out, Actual Size, smart zoom, scroll, pinch | (none) | Shell | Kept: the contract gives the view to the shell |
| Window menu, Minimize, Zoom, full screen, window close and resize, Documents (iPad back), the document browser | (none, or `document.close`) | Shell | Kept: platform chrome |
| A pop-out's window frame remembered, the iPad's sheet or scene choice, "Open in New Window" | (none) | Shell | Kept: presentation with no document meaning |
| The palette's search field's typing | `palette.items {query}` | Shell field filtering through a core method | Kept (listed for the maintainer: the core keeps the query it opened with, not each keystroke) |

Nothing else the person can do in either app is shell-side. The menu model now has no choosable item without a command (a test checks the node menu).

## What was done, item by item

| Item | State | Notes |
|---|---|---|
| 1. Rule fixes and the audit | Done | Above |
| 2. Parameters | Done, both apps | `parameters.get`; Show on Face (`node.face`, new); reset (`row.set {reset}`); Mac popover on the node; iPad popover (regular) or sheet (compact); double tap, ⋮ and context menu, Node › Edit Parameters… ⌘I. Not built: egui's filter field past 8 rows, a promoted parameter's doc and range form |
| 3. Groups | Done | Make, ungroup, collapse and expand, enter (double tap) and the way back, rename in place, group ports through the existing menus |
| 4. Frames | Done | Make round the selection, rename in place (double tap the tab), resize, move, colour, remove; shown grown round a pushed member |
| 5. Nets and named ports | Done | Make, join (`net.connect`), leave, rename in place; driver tag with its count; named ports through the input's menu, named in place |
| 6. Pop-out windows | Done | `picture`, `text.plain`, `scalar`; "no viewer yet" for other kinds |
| 7. Unbuilt banner | Shrunk | Still says "isn't available yet": Settings (`app.settings`), PlumCraft's own About, value editors (`node.editor`), the Messages window, the Cache Report, the graph's Parameters window (`view.params`), Attach…, Install Engine…, Compare. Removed this slice: the parameters sheet and pop-out viewers |
| 8. Conformance and tests | Done | Six new cases, 36 of 36 on every driver, no skips but the core's own document's three engine cases; new kit tests; the iPad UI test extended |

## Pop-outs on each platform, and what persists

- **The core** keeps them in the scene's `view.windows.popouts` (`{id, node, port, index, title, viewer, raised, stamp}`); `view.popout` of a port already out raises it; `stamp` changes when the value or the viewer's state does; `popout.get` reads it; `viewer.text {wrap, mono, all}` (new) and `viewer.zoom` are its controls.
- **Mac:** a real window per pop-out, titled "node · port", listed in the Window menu, its frame remembered per value (`frameAutosaveName`), closed with the close button or ⌘W (`view.close`). A person at the active app gets it in front; a scripted run orders it just above its own document window, never in front of another app's, never taking the keyboard.
- **iPad:** a scene of its own (activity `com.plumcraft.pad.popout`) when the graph's scene isn't full screen (Stage Manager, iPadOS 26's windowed apps, Split View, an external display), with a close button and the window's own controls; a sheet with medium and large detents when it is full screen, one value at a time (another replaces it, closing the one replaced in the core), with Done, a swipe down, and Open in New Window. The simulator here runs iPadOS 26 in windowed mode, so pop-outs opened as windows of their own (shown maximised by the system).
- **Persistence:** none. Pop-outs and the inspector are the document's view state and close with it; a reopened graph has none (as in the egui editor). A decision below. Pop-out scenes are never restored by the system; one restored from an earlier build is closed and a document window takes its place.
- **The Watched column is gone** from both apps. A tap still watches (the core's `run.watch`: the face's border, what a run watches by default); a double tap on a value, Pop Out, or the parameters open windows.

## Frames and the push

The core keeps pushing overlapping faces apart in the scene (slice 2). Since this slice a frame is shown grown, right and down, to hold every member the push moved, with 16 of room (`scene::grown_frame`); the frame's size in the graph is untouched (the conformance case checks `graph.get` keeps 300 × 150 while the scene shows it taller), and what it holds is still what its own rect holds, so moving it moves the same things. The growth goes when the push does. Alternative not built: the push respecting frames (faces pushed only within their frame), which can't always find room.

## Core, C ABI, kit and contract changes

- **Graph:** `Node.face` (a per-input map, saved only when set; old files unchanged) and `Graph::set_face`.
- **Session:** `node.face`; `viewer.text`; pop-outs keep `raised`, `text` and their whole-view zoom; `view.popout` of one open raises it; `wire.unlabel` takes a list; a new graph closes the inspector, a removed node closes its inspector.
- **Editor core:** modules `windows` (`view.windows`, `parameters.get`, `popout.get`) and `naming` (`name.edit`, `name.set`); canvas commands `canvas.selectMode`, `graph.share`, `name.edit`, `name.set`; `canvas.reveal` now core view state (`view.reveal`) and takes `{net}`; `row.set` takes any input and `{reset}`; scene `view.select`, `view.windows`, `view.name`, `view.reveal`, net `driver.text`, frames grown with the push; menu `icon` on items, the `toolbar` context, File › Share…, View › Select Mode, Node › Edit Parameters… ⌘I, every former shell item a command, a wire's menu taking `at`; gestures: double taps open what they are on (values pop out, names edit), a tap on an open group's chevron collapses it; `rows::shown_rows` honours `face`; `values::value_json`.
- **egui editor:** presents the core's name editor with its own windows (the node and frame rename window, the group's, the net and port name field, the slice field), opened when the core opens a name and closing the core's editor when they close; a small window of its own for a promoted parameter's label (`name.set`, Escape `name.edit {open: false}`). Its old `(None, "rename.…")` menu branches are now unreachable (cleanup for later). Passes Select mode in its menu facts.
- **C ABI:** no change to `crates/ffi`'s code or header; **`PLUM_ABI_VERSION` unchanged (1)**.
- **Kit:** `NameEditor`, `ParametersModel`, `PopoutModel`, `PopoutDiff` (PlumCraftKit `Windows.swift`); `DocumentModel` `toolbar`, `onToolbar`, `onWindows`, `onName`, `parameters(node:)`, `popout(id:)`, `poke()`; models `Windows`, `PopoutWindow`, `NameEdit`, `Reveal`, `PopoutValue`, `ParametersReply`, `MenuItem.icon`, `NetDriver.text`; canvas `NameFieldHost` (AppKit, UIKit), the reveal, the driver tag's count, Select mode read from the core; `ToolbarWords`; `fieldText` public; the unbuilt list shrunk.
- **Apps:** Mac `Popouts.swift` (was `Watched.swift`), `Parameters.swift`, the core's toolbar, the path control, the share ask; iPad `Popouts.swift` (was `Watched.swift`) with `PopoutSceneDelegate`, `Parameters.swift`, the core's toolbar, the crumbs capsule, the share ask's `file`; `--hide-watched` accepted and ignored; sample "Windows and Names".
- **Contract:** `docs/shell-interface.md` 8.9, 8.13, 12.5, 12.11, 12.13 (rewritten), 12.14 to 12.18 (new), 13.2, 13.3 note, 16.1; `docs/control-protocol.md` (`parameters.get`, `popout.get`, `canvas.selectMode`, `graph.share`, `name.edit`, `name.set`, `canvas.reveal`, `node.face`, `row.set {reset}`, `viewer.text`, `menu.get`'s toolbar and items); `schema/shell1.json` regenerated; `fixtures/scene/full.json` rewritten (`driver.text`).

## Conformance

| Driver | Result |
|---|---|
| The core's own document (`conformance::tests`) | passes: 33 run, 3 skipped (they need an engine it doesn't have), as before |
| C ABI, headless (`plumcraft-ffi` `conformance`) | 36 of 36 (the picture case on a core with the array engine) |
| C ABI's control port | all pass |
| egui editor (`plumcraft-ui` `conformance`) | all pass (steps with `only: "rows"` don't run there, as in slice 2) |
| Swift kit (`swift test`, `PlumCraftConformance`) | passes |
| Mac app (`conformance.sh all`) | 36 of 36, 0 skipped |
| iPad app in the simulator (`conformance.sh all`) | 36 of 36, 0 skipped, three runs in a row after the palette fix below |
| iPad UI tests (`conformance.sh gestures`) | `testTouchGestures`, `testInlineEdits` and the new `testWindowsAndNames` pass |

New cases (six, 36 in all): the parameters inspector (a hidden input set, shown and reset); a group made, entered, edited inside, renamed, left, collapsed, ungrouped and undone; a frame made, renamed in place, resized and grown with a pushed member; a net made, joined, renamed in place and left; a pop-out opened by a double tap, raised, viewed and closed; Select mode, Share and the toolbar as the core's.

## Tests

| Run | Result |
|---|---|
| `cargo fmt --all --check` | clean |
| `cargo clippy --all-targets --all-features -- -D warnings` | clean (one `sort_by_key` lint in my code fixed) |
| `cargo nextest run --all-features` (b3, scratch) | 1,488 of 1,489; the one failure is the known local-only `plumcraft-craft::params_coverage::hidden_params_match_the_baseline`; none of the known intermittents failed |
| `check-hash-gates.sh`, `check-core-toolkit-free.sh` | pass |
| Python (`uv run pytest -q`, `ruff check .`) | 224 passed; ruff clean |
| `build-xcframework.sh` | built |
| `swift build`, `swift test` | 76 of 76 (73 before; 3 new in `WindowsTests.swift`, and `ShellTests` extended), zero warnings |
| Mac app Release; iPad simulator Debug and Release; device arm64 Release (`s1-ipad-device.sh build`) | build, zero warnings |
| New Rust tests | `canvas_cmds` (Select mode and the share ask, names in place); changed: `menu`, `canvas_cmds` reveal, `ffi` `selection` |

## Changed expectations, deliberately

- `menu::tests::context_menus_name_their_commands`: Rename… was a shell item (`rename.node`, no command); it is `name.edit {node}` now, and the test also checks every choosable node-menu item has a command.
- `canvas_cmds::tests::the_views_commands_are_checked_before_the_shell_is_asked` and `crates/ffi/tests/selection.rs`: `canvas.reveal` was refused by a core with no canvas; it is core view state now.
- The egui kittests for renaming, frames, named ports, net labels and slices failed in the first gate run (12 tests): egui showed a plain window for the core's name editor instead of its own. Fixed in egui (above), not in the tests: they pass unchanged.
- `scene/full.json`: the net driver's `text`.
- The new frame case's bound was first 200 (the core's test sizes); the Mac's smaller faces grow the frame to 192, so the case asks for more than the frame's own 150 (`$min 160`).

## Store, processes, simulator

- The real store, two levels, before and after (`claude-debug/native-n4/s3-store-before.txt`, `s3-store-after.txt`): identical. The listing before was taken before any command ran; every build, test and launch ran with the store, marks, test run, craft root and preferences in scratch (`claude-debug/native-n4/s3-env.sh`), b3, 2 GB minimum.
- Final `pgrep -lx`: `5861 PlumCraft`, `58303 PlumCraft`, `60555 PlumCraft` (the maintainer's, never signalled); no `PlumCraftPad`, `CanvasSpike`, `CanvasSpikeMac` or `xctrace`. Every app I launched was quit by its PID (the picture driver and `conformance.sh`), or terminated by `simctl` in the simulator.
- Simulator "iPad Pro 13-inch (M5)" still booted; `com.plumcraft.pad` and `com.plumcraft.pad.uitests.xctrunner` uninstalled; `com.plumcraft.spike.canvas` (not mine) left; nothing erased. The physical iPad was not touched; `s1-ipad-device.sh build` still builds.
- Disk: 99 GB free at the end.

## Evidence

Under `claude-debug/native-n4/s3-shots/`:

- **Mac** (`s3-mac-shots.py`, the app driven over its control port in the background, each window captured by number with `screencapture -l`), light and dark: `mac-canvas` (the core's toolbar, the net tag "said ×2", the frame, the collapsed group), `mac-parameters` (the popover on Every widget, count hidden: its eye struck through), `mac-hidden-shown` (count set to 7 and shown on the face), `mac-inside-group` (the path control Graph › shout then hush), `mac-frame-renaming` (the field over the tab), `mac-frame-renamed-resized`, `mac-net-renaming`, `mac-select-mode-on`, `mac-popout-text.plain`, `mac-popout-text-mono` (Wrap off, Monospaced on), `mac-popout-scalar`, `mac-popout-picture`.
- **iPad** (`s3-pad-shots.py`, the simulator over the port, `simctl io screenshot`), light and dark: `pad-canvas`, `pad-parameters` (the popover), `pad-hidden-shown`, `pad-inside-group` (the crumbs capsule), `pad-frame-renaming`, `pad-frame-renamed-resized`, `pad-select-mode-on` and `-off` (the toolbar's Select button checked by the core), `pad-popout-text`, `-scalar`, `-picture` (each a window of its own); compact width: `pad-parameters-light-compact` (the sheet), `pad-popout-sheet-light-compact`.
- **iPad UI test** (`n4s3-ui-*`): the parameters by a double tap, the hidden count set and shown on the face, inside the group, the frame renamed, the pop-out.
- **Recording:** `claude-debug/native-n4/s3-shots/pad-windows-and-names.mp4` (0.57 MB): the UI test double-tapping a node to open its parameters, setting the hidden count and showing it on the face, entering the group and coming back, renaming the frame in place, and popping a value out.
- The remaining banner: no picture; it appears only for the commands listed under item 7 (Settings, About and the rest), which this slice didn't touch.

## Defects introduced and caught

- The iPad app came up windowless after a run that left a pop-out scene: the system restored it, my scene delegate closed it (its value was gone) and nothing brought a document window back. Fixed: a stale pop-out scene hands over to a document window, and pop-out scenes are never restored.
- The Mac picture pop-out's Fit ran before its window was laid out and never enlarged a small picture (a 160 × 120 picture shown at about 32 pixels). Fixed: fit again on layout, up to 8×.
- The Mac's path control was white on white in dark mode (its background resolved once). Fixed: on a vibrancy material.
- `canvas.reveal` stayed a shell-only command: once Go to Driver and Next Use became commands, the Apple apps could only say "isn't available". Fixed: core view state.
- A stuck read-half case on the C ABI (a parameters window left open across cases by a graph whose new node reused the id). Fixed: a new graph closes the inspector.
- **A crash in slice 1's iPad palette** (index out of range in `PaletteController.tableView(_:cellForRowAt:)` when the palette's items changed before the table reloaded), hit in one iPad suite run (the app died at the sixteenth case). Golden rule 1: the cell lookup is guarded on both platforms (an empty cell until the reload); the suite then passed three runs in a row.
- In the UI test: the status file's rows hold `null` for a socket row, which my first check didn't allow; typing appended to the field's "3" (the core then held "35" to 10, correctly); the sheet's Done was found by label. Test fixes; the inspector's fields now select what they hold when editing begins.

## Decisions for the maintainer

1. **A tap on a pin with no Watched column.** Built: a tap still watches (the core's `run.watch`: the face's border, what a run watches); a value opens in a window only by a double tap, Pop Out, or the inspector. Alternative: a tap pops the value out at once (one window per value, a second tap brings it forward).
2. **Pop-outs and the inspector don't persist** with the graph (the egui editor's behaviour; contract 12.14). Alternative: save open pop-outs in the workflow and reopen them.
3. **The iPad's presentation:** a window of its own wherever the graph's scene isn't full screen (in this simulator's iPadOS 26 windowed mode that is always, and the system shows the new window maximised over the graph), a sheet when it is. Alternative: always a sheet unless the person chooses Open in New Window.
4. **Frames and the push:** the frame is shown grown round a pushed member (built), or the push keeps faces inside their frame.
5. **Select mode on the Mac:** in View and the canvas menu, left out of the Mac's toolbar (a drag on empty canvas is already a marquee there). Keep, or show it.
6. **Show on Face is not an undo step** (a view change, as `node.fields` and `node.view`), but it is saved and dirties the graph. Alternative: an undo step.
7. **The palette's typed search** stays in the shell's field, filtering through `palette.items {query}`; the core keeps only the query it opened with. Make each keystroke core state, or keep.
8. **A promoted parameter's doc and range** (`group.editPromoted`'s other fields) have no form yet; only its label is edited in place. A form ask now, or later.
9. **The egui editor's dead rename branches** (unreachable since the items became `name.edit`): remove now, or with egui's next change.

## What only a person can verify

- On the Mac with the app active: a pop-out window coming to the front, its frame remembered across launches, the Window menu listing it, ⌘W closing it; the parameters popover's keyboard focus and Tab order; the share picker; the in-place name field's caret and Escape.
- On a real iPad: a pop-out as a window in Stage Manager and as a sheet in full screen; the inspector popover and compact sheet with the keyboard; the crumbs capsule; renaming with a hardware keyboard.
- Whether "grown frames" and pop-outs (rather than a column) feel right on the maintainer's own graphs.

## For slice 4

- Settings and themes: `app.settings` is still on the unbuilt banner; the toolbar model and `ToolbarWords` are the pattern for its controls. `view.theme {name: "system"}` and `lifecycle.appearance` are still not built.
- Autosave on the iPad: the windows are view state and need nothing from it; the per-input `face` map is saved with the graph and dirties it like `node.fields` (not an undo step), which autosave must treat as a save-worthy change.
- The network control port on a device: the conformance suite now opens real windows (the inspector, pop-outs); on a device they will be scenes or sheets as in the simulator.
- The suite on the device: `claude-debug/native-n4/s3-pad-shots.py` drives the app over the port and can serve as the device's picture script once the port listens there.
- egui: the dead rename branches, and egui drawing its faces at the scene's pushed positions and grown frames (it still draws the graph's).
