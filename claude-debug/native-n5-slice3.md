# N5 slice 3: editing (Plummer for Windows)

2026-11-21, branch `native/n5-editing` from `feat/native-n0` at `5046bd1`, merged with the dev branch at `50b8aec` and then `e0cf7eb` (not committed). Note 24 section 21 has the summary; this report has the gate, the measurements, the input probe, the screenshots and what looked wrong.

## Answer

The Windows app edits. Every gesture goes to the core as `ui.gesture`, and the app draws what the core replies. That covers mouse, pen and touch drags of faces, wires from pins, a wire's end picked up, marquees and scrubs, plus double clicks, context menus, the palette, field editors, names in place, nudges, Delete and Escape.

The last shell-side actions are now the core's. File › New, File › Open, Exit, the close button and the zoom keys each go through a core command or method:

- New, Open and Exit use a new config flag, `platform.document_windows`. The feed's `window` change tells the app which documents to show.
- The close button sends `document.close {by}`. Its prompt now reads "Discard them and close the window?".
- The zoom keys run a new `canvas.zoom` command.

Mouse clicks had never worked in the app (B51, fixed). With that fixed, real input was driven on the VM: mouse and keys through `SendInput`, pen and touch through synthetic pointer devices.

The whole suite passes, 53 of 53 with no skips:

- from the C# runner on the VM, against the ARM64 app and against the x64 app under emulation;
- from the Swift runner on the Mac;
- against the Mac app.

`dotnet test` passes 41 of 41. The app is renamed **Plummer**.

## Gate

| Check | Where | Result |
|---|---|---|
| Suite, read half, C# runner | VM, ARM64 app | 32 of 32, no skips |
| Suite, edit half, C# runner | VM, ARM64 app | 21 of 21 |
| Suite, both halves, C# runner | VM, x64 app (emulated) | 32 of 32, 21 of 21 |
| Suite, both halves, Swift runner | Mac over a forwarded port | 32 of 32, 21 of 21 |
| Suite on the Mac app (`conformance.sh all`) | Mac | 53 of 53 |
| `dotnet test` | VM | 41 of 41 (34 before, 7 new); one of four runs had 40 (B52) |
| `cargo nextest run --all-features` | Mac | 1,569 of 1,570; the known `hidden_params_match_the_baseline` |
| clippy `-D warnings`, fmt, hash gates, core gate | Mac | clean |
| Python (`pytest`, `ruff`) | Mac | 224 passed, clean |
| `swift test` (`PlumCraftFFI`) | Mac | every run passes (31, 9, 14, 5 and 22 tests) |
| x64 build | VM | the core DLL and the app build |

The case counts are 53, not the 52 expected. The dev branch has 52 cases (31 read, 21 edit), and this slice adds one read case, "Zoom In, Zoom Out and Actual Size are the core's commands". The C# runner reads `cases.json` from the checkout it was built from. `r3-conform.sh` now points the Swift runner at this worktree's file, not the main checkout's. Every run above came after the final merge and on the renamed build.

## Budgets (500 nodes, 800 wires; WARP, 64 Hz, so compare, don't judge)

Each drag moves through the canvas's own entry points (`Press`, `MoveTo`, `Release`), one move per display frame for 4 s. "Move to frame" is the time from a move to the next composition frame.

| Run | Launch to first frame | One-node change p50 | Face drag fps, late frames, late ms/s | Face drag CPU | Wire drag fps, late, ms/s | Move to frame p50/p95/max |
|---|---|---|---|---|---|---|
| a | 2,456 ms (first after a build) | 16.4 ms | 59.2, 7, 24.9 | 0.55 s/s | 57.2, 4, 15.8 | 16.6 / 22.7 / 38.8 ms |
| b | 830 ms | 16.2 ms | 57.5, 7, 32.2 | 0.52 s/s | 58.2, 0, 0 | 16.6 / 20.3 / 36.1 ms |
| c | 764 ms | 16.0 ms | 58.2, 8, 28.1 | 0.54 s/s | 58.8, 1, 2.6 | 16.6 / 21.8 / 36.7 ms |

The face drag moves the face's visual and re-paths its wires from `plum_wire_paths` on each move. The wire drag rebuilds one overlay path and the target ring. Both stay at about the display's rate on this software renderer. Pan in the same runs: 55.5 to 56.8 fps (slice 2: 55.5). The JSON files are in `bench-r3/`.

## What remains the shell's, and why each is allowed

- **The view's pan and zoom:** the wheel, Shift and horizontal wheel, Ctrl+wheel, middle drag, one-finger pan and two-finger pinch. Contract 12.13 leaves the view's transform to the shell. The zoom keys are not on this list: they are now `canvas.zoom`.
- **Which device does what before the core decides:** a finger on empty canvas pans instead of starting a drag (contract 5.5's touch rule; Select mode makes it a marquee). A finger, the mouse and the pen all send the same drag otherwise.
- **The platform's chrome:** window placement and cascading, minimise and maximise, the title bar, a field's own text editing, the system's double-click time and rectangle.
- **Opening the files named on the command line:** `document.open`, the platform's way of opening files.
- **Test hooks:** `--then` steps and `--bench`.

## Rust and contract changes

| Change | Where | The Apple apps | egui |
|---|---|---|---|
| `platform.document_windows` (config): New and Open make documents the feed announces (`changed {doc, domain: "window", open, from, by, path?}`); Open stays in an untouched document; Quit closes every document in turn, a cancel stopping it | `crates/ffi/src/core.rs`, `crates/editor-core/src/invoke.rs` (`Host::document_windows`, `open_document`, `quit_all`, with defaults), `document.rs` | Don't set it, and never send New, Open or Quit to the core: unchanged | Its `Host` keeps the defaults: unchanged |
| `document.close {doc, discard?, by?}`; its prompt's `then: "close"` and "Discard them and close the window?" (`AfterDiscard::Close`) | `core.rs`, `invoke.rs` | The Mac closes with `discard: true`, unchanged; the iPad's close now says "close the window" | Not used |
| `canvas.zoom {by} \| {to}` and the scene's `view.zoom {seq, by?, to?}`, with a conformance case | `canvas_cmds.rs`, `scene.rs`, `document.rs`, `cases.json`, `schema/shell1.json` | The Swift models decode `view.zoom` (`ZoomAsk`) so the strict schema test passes; the apps ignore it and keep ⌘+ ⌘− ⌘0 | Ignores the command (its `canvas_command` falls through); passes the new case |
| Tests | `crates/ffi/tests/documents.rs` (two), `canvas_cmds.rs` (one) | | |
| Docs | `docs/shell-interface.md` 4.3, 8.9, 12.13; `docs/control-protocol.md` (`canvas.zoom`) | | |

## The Windows app and kit

- **`PlumCraft.Kit/Editing.cs`** holds the platform-free editing pieces:
  - `DragDriver`;
  - `PaletteModel` (Enter waits for the latest search);
  - `RowEditor` (with `StartText`, the core's number text);
  - `NameEditor`;
  - `EditorPlacement`;
  - `WindowWanted`.
- **`EditingTests.cs`** has seven tests against a real core.
- **The app:**
  - `Canvas/CanvasView.cs` was rewritten. Mouse, pen and touch input feeds the driver, with autoscroll, the system's Holding gesture and the system's double-click rule.
  - `Canvas/CompositionCanvas.Drag.cs` draws a drag's previews.
  - `Editors.cs` holds the native controls.
  - `DocWindow` and `App` dispatch through the core and follow its windows. The canvas takes focus on activation.
  - Pop-outs are placed beside their window in the display's work area. New windows cascade.
  - `Bench` gains the two drag phases.
- **`unsafe` code:** still only in `Interop.cs` in the app, plus the kit's `Native.cs` and `PlumCore.cs`. `GetSystemMetrics` was added there, without `unsafe`.

## Input probe (real input on the VM's desktop)

The injector, `r3-send.ps1`, is a one-shot Interactive scheduled task. It logs:

- its session and integrity level, and the app's: both session 1, both High (0x3000);
- the foreground window before and after each step;
- `SendInput`'s count and error;
- `GetAsyncKeyState` after each button.

The mouse uses absolute moves over the virtual desktop with buttons as separate events; keys carry scan codes.

**Why slice 2's clicks "didn't arrive":** they did arrive. The app's status listed `pressed Mouse 892,474 L` and `released Mouse 892,474`, but nothing ran. The cause was the canvas's own pointer-capture handling (B51; `r3-probe-run1.txt`). After the fix (`r3-probe-run2.txt` to `run5.txt`):

| Input | The core ran |
|---|---|
| Mouse click on empty canvas | `selection.clear` |
| Mouse drag of a face | `edit.move` ("Move Node") |
| Ctrl+Z | the move undone |
| Mouse drag from an output to an input | `wire.connect` |
| Mouse drag from an output to empty canvas | `canvas.palette` with the wire, the wire held |
| Right click on a face, then Escape | the context menu (12 items), closed |
| Double click on empty canvas, type "upper", Enter | `canvas.palette`, then an Upper case node added ("Add Node") |
| Double click on a face | `node.parameters` (slice 4's inspector: the banner says so) |
| Click on a text row, type, Enter | `selection.set,row.edit`, then "Set Value" |
| Click on a row, type, Escape | the editor closed, nothing set |
| Click a face, arrows | `selection.nudge` ("Move Node") |
| Delete | "Remove Node"; Ctrl+Z put it back |
| Mouse drag of a face, Escape before letting go | cancelled: no step |
| Tab on the canvas, then Escape | the palette opened, then closed |
| Pen drag on empty canvas | the core's marquee, `selection.set` |
| Touch drag on a face | `edit.move` |
| Touch drag on empty canvas | the view panned, nothing sent |
| Touch hold on empty canvas | the core's long press: the palette |
| Ctrl+=, Ctrl+0 | `canvas.zoom`: 1.25, then 1 |

Two observations:

- A step that brought the main window forward between opening the palette and typing sent the keys past the palette, which is a popup of its own. Typing in the same step as the click worked.
- The VM stopped nothing this time. The `SendInput` mouse caused no trouble, and right clicks were sent several times.

## Screenshots

All are in `claude-debug/native-n5/shots-r3/`, light and dark, taken inside the VM. I looked at every one.

| File | What |
|---|---|
| `move-*` | A face being dragged: its visual offset, its wire re-pathed as the core routes it (right to left) |
| `wire-ok-*` | A wire dragged from Upper case's output over Every widget's `text` input, which takes it: the accent wire and ring |
| `wire-bad-*` | The same over the `label` input, which the core says doesn't fit: the wire in the error colour, the pin crossed |
| `palette-*` | The palette after a drop on empty canvas: "22 kinds take this wire", sections, a kind marked "starts the array engine", the held wire drawn to the drop |
| `edit-text-*` | The field editor over `label`, "corgi" typed |
| `edit-number-*` | The field editor over `amount` with the core's text "0.50" |
| `edit-choice-*` | The ComboBox over `mode`, its list open |
| `marquee-*` | A marquee on empty canvas |
| `context-*` | The core's context menu for a node (12 items) |
| `close-*` | The close prompt with two windows open: "Discard them and close the window?" |
| `popout-*` | A pop-out placed over its window's right edge, the display having no room to its right |

`move`, `close`, `popout` and `wire-ok` were retaken after the rename: their titles read "Untitled - Plummer". The others predate the rename and read "PlumCraft".

What looks wrong:

1. The InfoBar, when it opens (a banner such as "the parameters inspector isn't in Plummer for Windows yet"), pushes the canvas down by its height. What was under the pointer moves.
2. The palette flyout is placed where it fits, not exactly at the drop: above it near the bottom of the window. Its section headers look like rows with an underline.
3. During a marquee, the faces it touches aren't marked. The core reports `touches`; the app doesn't draw them yet.
4. Every widget's `label` input doesn't take a text wire (the core's `fits` 0). That is the core's answer, and I didn't look into why.
5. The Cancel button's icon is a square, which reads as a checkbox (slice 2's glyph mapping).
6. Windows shows "Activate Windows" on the VM's desktop. That is the VM's licence, not the app.
7. A wire drawn from an output to a pin left of it loops out to the right first. The Apple canvas draws the same curve.

## Defects introduced and caught

| Defect | Caught by | Fixed |
|---|---|---|
| Every mouse click and drag did nothing (slice 2's code; B51) | The input probe | The release is handled before the capture is let go |
| A script step `wire:a>b` was a redirection to cmd.exe | The first shots | `/` separates the ports |
| A script `add` step went on before the scene had the face (a null reference, faces 0) | The shots | It waits for the face to be measured |
| The shots aimed at row 0, the socket-only `text` input; `flag` was flipped | The edit-choice shot | Rows counted from the widget rows |
| A field committed and closed when the window lost activation (the screenshot task takes the foreground) | The edit shots (no field open) | Only focus lost inside an active window commits |
| The status file was stale after a menu or the palette closed | The Escape probes | Written on close |
| Enter right after typing in the palette picked from the list before the search | The probe ("upper" added nothing) | Enter waits for the latest `palette.items` |
| Delete did nothing (the model's shortcut is Backspace) | The probe | Delete and Backspace are both `edit.delete` |
| A Rust test answered a stale prompt | Reading the test | The feed cleared before the second Quit |
| C# build errors (a missing `using`, reference comparisons, a parameter name, a shadowed local); two wrong test expectations | The build, the kit tests | Fixed |
| The Swift strict schema test failed on `view.zoom` | `swift test` | `ZoomAsk` in the Swift models |
| The VM's display turned off after 5 idle minutes, and captures were stale (black or white windows, a frozen clock) | The renamed shots | Each shot first wakes the display with a pointer move |

## Unverified

- **Hardware input:** real mouse, pen and touch on a physical machine. Everything was injected in a VM: `SendInput` for the mouse and keys, synthetic devices for the pen and touch. Also not checked: a precision touchpad, a hardware pinch, the touch keyboard.
- **App gestures not driven:**
  - frame, group and bend-point drags (no frames in the test graph);
  - a scrub in the app (the kit's driver handles kind `scrub`);
  - autoscroll;
  - the colour picker and several-line editors;
  - a name edited in place through the app's UI (the kit test covers the model);
  - Shift+Tab order beyond the canvas.
- **Documents:**
  - Quit with Save on an untitled document (the `file.save` ask). The quit sequence waits on modal asks; if the core's save finishes later than its turn, Quit may stop there, as a cancel does.
  - Pop-out placement on a display wide enough to put the pop-out beside the window.
- **Platforms:** the x64 bench; Windows 10.

## On the VM

- **Free space:** 65.94 GB before, 65.88 GB after.
- **Checkout:** brought to the branch's content by `git checkout -- .` and `git clean` of slice 2's copies, a fetch of `feat/native-n0`, `git checkout --detach 5046bd1`, then `r3-sync.sh`. That script copies every file this worktree changed or added since `5046bd1`, the merges included, and its result was checked by hashing the Rust sources on both sides.
- **Left running:** no scheduled task, and no Plummer, `dotnet`, `cargo` or runner process.
- **Scratch:** about 10 MB of `r3-*`.
- **The Mac:**
  - Its disk filled from another agent's traces at about 13:40. Every Mac-side result in this report was rerun after space returned at 16:20.
  - The real store, `~/Library/Caches/com.plumcraft.mac`, `~/Library/Logs/PlumCraft` and the preferences (`com.plumcraft.mac.plist`, and `com.swapwizard.plummer.mac.plist` after the Apple rename) were not touched by this slice's Mac app runs. Their times predate the runs, and no file changed since 16:45. Files: `r3-mac-before*.txt`, `r3-mac-after*.txt`.
  - Local snapshots were thinned once while the disk was full: `tmutil thinlocalsnapshots / 200000000000 4`.

## Scripts (`claude-debug/native-n5/`, untracked)

Copied from slice 2's `r2-*` under new names, with:

- `r3-sync.sh`: the VM's checkout from the worktree;
- `r3-ffi.ps1`: the core DLL for either target;
- `r3-cargo.sh`: Mac commands under the scratch env, refusing the real store;
- `r3-send.ps1` and `r3-probe.sh`: real input, with what the injector can see;
- `r3-conform-vm.sh`: the C# runner, both halves, ARM64 or x64;
- `r3-mac-snap.sh`: the Mac's real state;
- `r3-shots.sh`: the slice's shots, each waking the display first.

Logs: `r3-probe-run*.txt`, `r3-shots-run*.log`, `r3-nextest*.log`, `r3-mac-conform*.log`, `r3-swift-test.txt`, `bench-r3/`.

## Decisions for the maintainer

1. **The zoom keys as a core command (`canvas.zoom`).** Contract 12.13 had left Zoom In, Zoom Out and Actual Size to the shell; this slice made the keys a command, following the rule. Keep it, or return them to the view?
2. **`platform.document_windows`.** The flag's name, and Open's rule of using an untouched document's window rather than a new one. Should the Mac adopt it too, dispatching New, Open and Quit to the core instead of NSDocumentController?
3. **The banner pushing the canvas down.** Float it over the canvas, or keep it in the layout?
4. **B52.** Make the kit test wait for the engine, or raise or reshape the core's 256 held requests?
5. **The VM's display turning off after five minutes.** Set it to never on the test VM (a setting I didn't change), or keep waking it per shot?
6. **The old `%LOCALAPPDATA%\PlumCraft` folder** that scripted runs used. Leave it, or remove it on the machine?
7. **The palette's look.** Category headers as list rows, and placement where the flyout fits rather than exactly at the drop.
