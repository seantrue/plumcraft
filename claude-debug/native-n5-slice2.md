# N5 slice 2: read and run (PlumCraft for Windows)

2026-11-21, branch `native/n5-read-run` from `feat/native-n0` at `057ed2d` (not committed). Note 24 section 21 has the summary; this report has the measurements, how they were taken, the screenshots and what looked wrong.

## Answer

PlumCraft for Windows is now a WinUI 3 app on the Windows kit, replacing the spike's window. It has a window per document, the core's menus, toolbar and context menus, the Composition canvas, the asks answered natively, run and stop, pop-out windows for `picture`, `text.plain` and `scalar`, and light and dark following the system. The read half of the conformance suite passes against the running app on the Windows 11 ARM64 VM: 30 of 30 from the C# runner on the VM, and 31 of 31 from the Swift runner on the Mac over a forwarded port. The edit half also passes, 21 of 21 from each runner, and `dotnet test` passes 34 of 34. At 500 nodes, launch to first frame is 0.78 to 0.79 s and a one-node change reaches the screen in 16.2 to 16.4 ms (p50), about one frame of the VM's 64 Hz display. The VM draws in software (WARP), so frame rates compare approaches and don't judge the budget.

## What was built

| Part | Where | Notes |
|---|---|---|
| The app | `native/windows/src/PlumCraft.App` | WinUI 3, unpackaged, self-contained, `PlumCraft.exe`; one core per process, a `document.open` per window |
| Window chrome | `DocWindow.cs` | Content extended into the title bar over Mica; MenuBar, CommandBar toolbar, InfoBar, status line |
| Menus | `Menus.cs` | From `menu.get`, updated in place while the shape is unchanged; checks, radios, reasons as tooltips; the core's shortcuts shown and matched; unique Alt keys |
| Canvas | `Canvas/CompositionCanvas.cs` | A SpriteVisual per face, a ShapeVisual per wire, frames and open groups, one transformed container; redraws at the zoom's scale when a zoom settles; culling; run wash with pulse and fade as compositor animations; selection ring; previews; marquee |
| Faces | `Canvas/FaceLayout.cs`, `FaceRenderer.cs`, `FaceText.cs` | The Swift kit's layout and constants, DirectWrite text, Lucide icons through Direct2D's SVG renderer |
| Input | `Canvas/CanvasView.cs` | Tap, double tap, context and marquee go to the core as `ui.gesture`; pan and zoom stay in the shell |
| Asks | `Asks.cs` | Pickers bound to the window, ContentDialog (prompt, text, form), clipboard, Launcher, offscreen canvas for `screenshot`; `share` answered with its reason |
| Pop-outs | `Popouts.cs` | A window per `view.windows.popouts` entry; `picture`, `text.plain`, `scalar` |
| Appearance | `Appearance.cs` | `lifecycle.appearance` from the system's dark mode, high contrast, animations and transparency |
| Test hooks | `Script.cs`, `Bench.cs`, `Interop.cs` | `--then` steps for screenshots; the spike's budget run (Composition only); display and adapter queries |
| C# runner | `native/windows/src/PlumCraft.ConformanceCli` | `plumcraft-conformance` with the Swift command's flags, plus `call` |

Removed: `PlumCraft.CanvasSpike`, with its Win2D and XAML renderers, its own scene model and face layout, and its input log. Its bench now lives in the app as `--bench`, with Composition only. Its display and adapter queries moved to `Interop.cs`. The probe remains.

`unsafe` code in the app is limited to `Interop.cs`, the DXGI adapter query copied from the spike. The canvas has none. In the kit it stays in `Native.cs` and `PlumCore.cs`'s pointer methods.

## Gate results

| Check | Where | Result |
|---|---|---|
| Read half, C# runner | VM, against the app (ARM64) | 30 of 30, no skips, no settle timeouts, 28.6 s (21.6 s in a run before the VM's restart) |
| Read half, Swift runner | Mac, over an SSH-forwarded port | 31 of 31 (the main checkout's `cases.json` has one case newer than this branch's), 11.4 s (11.0 s before the restart) |
| Edit half, C# runner | VM | 21 of 21, 15.8 s |
| Edit half, Swift runner | Mac | 21 of 21, 7.5 s |
| `dotnet test` | VM | 34 of 34, 34 s including the build |
| Read half, x64 app under emulation | VM, C# runner | 30 of 30, 34.2 s |

## Budgets

Measured the way the spike measured them (`Bench.cs`). Launch to first frame runs from the process's start to the first `CompositionTarget.Rendering` after every face is laid out and every visible face is drawn. A one-node change runs from `node.set` on a literal's text, through its delta, layout, report and worker draw, to the next frame. The machine draws with WARP at 64 Hz.

| Run (ARM64) | Launch to first frame | One-node change p50 / max | Pan fps, late ms/s | Pan with all in view | Zoom | Working set |
|---|---|---|---|---|---|---|
| 500 nodes, b | 783 ms | 16.3 / 16.7 ms | 55.5, 52 | 56.0, 55 | 53.8, 70 | 438 MB |
| 500 nodes, c | 779 ms | 16.2 / 35.5 ms | 55.5, 53 | 55.5, 72 | 52.8, 88 | 426 MB |
| 500 nodes, d | 786 ms | 16.4 / 16.7 ms | 55.5, 57 | 57.8, 18 | 54.5, 60 | 438 MB |
| 100 nodes, b to d | 625 to 675 ms | 16.5 to 17.1 / 18.6 ms | 57.8 to 59, 0 | 57.5 to 58.2, 0 | 58 to 58.5, 0 | 230 to 240 MB |
| 500 nodes, x64 emulated, two runs | 1,897 and 1,990 ms | 36.5 and 37.5 ms | 51.8 to 53, 61 to 100 | 50.5 to 53.2, 70 to 116 | 51.5 to 51.8, 57 to 73 | |

How launch time was brought under 1 s: before the changes below it measured 1.05 to 1.14 s on a warm disk, and 2.35 s for run `a`, the first launch after a build. Three changes brought it to 0.78 s:

- The scene is requested before the window's controls are built.
- The header parts are reported while the faces are drawn.
- Faces out of sight are laid out at once but drawn after the first frame.

The marks in the status file at 500 nodes, in ms from the process's start:

| Step | ms |
|---|---|
| App constructor | 120 |
| Core open | 183 |
| Document open | 227 |
| Window created | 380 |
| First scene applied | 580 |
| Faces drawn | 736 |
| First frame | 774 |

The first launch after a build, with a cold disk, takes 1.6 to 2.3 s, most of it before the app's constructor runs. MSIX and ReadyToRun at N5's gate should help with that.

App CPU while panning and zooming is 0.64 to 0.96 s of CPU per second. The compositor rasterises on WARP inside the app's process.

## Input probe (injected on the VM's desktop)

| Input | Result |
|---|---|
| Wheel −120 | Pan y −120 |
| Ctrl+wheel +120 | Zoom 0.717 → 0.858, about the pointer |
| Touch drag on empty canvas, (−100, −50) | Pan by (−100, −50); no marquee, nothing selected |
| Two-finger pinch, 100 → 200 px | Zoom × 2 (0.858 → 1.715) |
| Touch tap on empty canvas | `ui.gesture` tap ran `selection.clear` |
| Mouse click, mouse drag (marquee) | **Not delivered**: hover moves reached the app (the File item highlighted) but the buttons from `mouse_event` didn't. The spike's mouse clicks did arrive on this VM before its restart. Not explained |

Right-click, the more-button and File › Open were driven through the app's own handlers instead (`--then context:`, `more:`, `open`).

## Screenshots

All are in `claude-debug/native-n5/shots-r2/`, taken inside the VM. I looked at each one.

| File | What |
|---|---|
| `small-light.png`, `small-dark.png` | The showcase graph (9 nodes: text, a slow node, a failing node, a picture) |
| `500-out-light.png`, `500-out-dark.png` | 500 nodes and 800 wires at the core's fit (zoom 0.2, its floor) |
| `500-in-light.png`, `500-in-dark.png` | 500 nodes at zoom 1.6: faces sharp (drawn again at 2× after the zoom settled) |
| `run-light.png`, `run-dark.png` | Light: the run just finished, washes held (done green, failed red, skipped grey). Dark: a run in progress (Slow running 90% with its progress bar, others queued, Cancel enabled) |
| `watched-light.png`, `watched-dark.png` | Three pop-outs: the picture (Mandelbrot PNG, Fit and Actual Size), the text (Wrap, Monospaced, Show All), the scalar (15) |
| `menu-light.png`, `menu-dark.png` | The File menu open (Alt, F) with the core's items and shortcuts |
| `context-light.png`, `context-dark.png` | The core's context menu for a node, with the hover text of its first item |
| `picker-light.png`, `picker-dark.png` | File › Open's system picker over the window |
| `dialog-light.png`, `dialog-dark.png` | The core's unsaved-changes prompt (raised by the close button after an edit) as a ContentDialog: Save, Don't Save, Cancel |

The pop-outs are tiled over the document's window, so they cover part of the graph. The `dev-*.png` files are working pictures from along the way.

What looks wrong:

1. In some shots the File menu item has a keyboard focus ring, because focus lands on the MenuBar when the window is activated. The canvas can't take focus yet.
2. The menu screenshots show the Alt key tips over every menu (the keyboard opened them). Before the fix, Edit and Engine shared E.
3. The prompt's text is the core's "Discard them and quit?" even though only the window is closing.
4. At zoom 1.6 the 500-node graph's wires merge into a solid band. That comes from the spike scene's fan of wires from one output and its routed backward wires, not from drawing.
5. Every node shows the core's fallback icon (a box): the core answers `as: box` for these kinds.
6. The light theme's Mica shows the desktop's tint only faintly. On Windows 10 there is no Mica (not tested).
7. The pop-out window has no icon, and neither does the app (no MSIX yet).

## Shell-side behaviour (not core commands)

- **The view's pan and zoom:** wheel, Shift and horizontal wheel, Ctrl+wheel, middle drag, one-finger pan, pinch, and Ctrl+= Ctrl+- Ctrl+0 when the core binds those keys to nothing.
- **The platform's document windows:**
  - File › New opens a new window with `document.open`.
  - File › Open shows the system's picker. It opens the graph in a new window, or in this window with the core's `graph.open` while this window holds an untouched new graph. This is the Mac's `NSDocumentController` pattern.
  - Exit runs the core's `app.quit` for each window in turn.
  - The close button runs `app.quit` for its document.
  - About runs the core's `app.about` and shows its words in a dialog.
- **Window placement**, including how pop-outs are tiled.
- **Test-only hooks:** the `--then` script steps and `--bench`. One step (`set:`) runs a scripted `node.set` so a close has something to ask about.

## Defects introduced and caught

| Defect | Caught by | Fixed |
|---|---|---|
| Subscribing to `AccessibilitySettings.HighContrastChanged` throws 0x80070490 in a desktop app, so the core never opened | The first launch | Each subscription is guarded and logged |
| `UISettings.AnimationsEnabledChanged` needs Windows 10 2004 or later (CA1416 against the 1809 floor) | The build | Version-guarded |
| A lambda parameter `_` swallowed `_ = …`; `Border` was ambiguous; `Str()` was called on a non-nullable value; the DirectX pixel format came from the wrong namespace | The build | Fixed |
| An XML comment in the csproj contained `--` | The next build | Reworded |
| Drawing into a CompositionDrawingSurface on a worker fails with a COMException | The status file (`drawOnUi`) and the log | Faces are drawn on workers into Win2D render targets and copied in on the UI thread |
| Icons that arrived during the first batch were never drawn | The first screenshot | All faces are redrawn when icons arrive |
| Icons drawn at 24 units instead of 14 | The dark screenshot | Scaled into the icon's box |
| The canvas's child visual wasn't clipped and drew over the menus and toolbar above zoom 1 | The zoomed-in screenshot | `InsetClip` |
| An empty graph never reached "first frame", so `--then` never ran on it | The status file | Fixed |
| Launch to first frame 1.05 to 1.14 s at 500 nodes | The marks | Reordered as above; 0.78 s |
| A fit that arrived before the view had a size was lost | The 500-node shot | The fit waits for the size (the spike scene itself asks for no fit; the shot uses View › Fit) |
| Pop-outs stacked exactly on top of each other | The watched screenshot | Tiled |
| Duplicate Alt keys (Edit and Engine) | The menu screenshot | Unique letters |
| The title doubled "PlumCraft" (the core's long title) | The first screenshot | Now "name - PlumCraft", `*` when edited |
| The probe script left the app running, which locked the next build | The build | The script stops the app |

## Interrupted and rerun

The VM stopped uncleanly at about 12:05 Mac time, during the first input probe (around an injected right click). It was restarted at 12:25, not by me. The QEMU process was gone in between. The cause is unknown.

- **Lost:** the probe's later steps. Its output had been cut by a `tail`, so its earlier lines were lost too.
- **Rerun:** `dotnet test`, the app build, the probe (without the right click), every screenshot, the three ARM64 budget runs at 500 nodes and three at 100, and both halves of the suite from both runners. All of these postdate the restart.
- **Before the restart:** the two earlier read-half passes (C# 30 of 30, Swift 31 of 31) and the 500-node bench `a`. They are superseded by the reruns.

## On the VM

- **Free space:** 67.87 GB before, 65.95 GB after.
- **Added:**
  - NuGet: the .NET x64 runtime packs and the Windows App SDK's x64 parts, +323 MB (cache 1.81 → 2.13 GB). The ARM64 packages were already cached.
  - `target\x86_64-pc-windows-msvc`: 1.11 GB, the x64 core DLL. The Rust target was already installed.
  - Build output under `native\windows`.
  - the scratch folder's `r2-*`: 0.15 GB of scratch in total.
- **Checkout:** moved to `057ed2d` (detached) with this branch's `native/windows` copied over it. The spike's tracked `.cs` and `.csproj` files were removed from that working tree.
- **Left running:** no scheduled task, no PlumCraft process, no `dotnet` or `cargo` process.

## Scripts

These are in `claude-debug/native-n5/` (untracked):

- `r2-win.sh`, `r2-pull.sh`, `r2-push.sh`: SSH and copying, with time limits.
- `r2-build.ps1`: build the app.
- `r2-test.ps1`: `dotnet test` with scratch roots.
- `r2-launch.ps1`, `r2-stop.ps1`: an Interactive scheduled task, stopped by PID.
- `r2-shot.ps1`, `r2-shots.ps1`, `r2-shots.sh`: screenshots.
- `r2-input.ps1`, `r2-probe.sh`: input injection.
- `r2-call.ps1`, `r2-call.sh`: one control request.
- `r2-conform-vm.ps1`: the C# runner on the VM.
- `r2-conform.sh`: the Swift runner from the Mac.
- `r2-bench.ps1`: the budget run.
- `r2-ffi-x64.ps1`: the x64 core DLL.

Output lives beside them: `bench-r2/` (JSON), `r2-conform-mac-read.log`, `r2-conform-mac-edit.log`, `r2-probe-run2.txt`.

## Not done

- **Editing (slice 3):** dragging a face, pin or wire does nothing yet.
- **The core's other windows and the remaining viewers (slice 4):** parameters, Settings, Messages, the Cache Report, Compare and names in place show a banner instead.
- **Canvas details:**
  - Net tags, application marks, the hover ring on pins and wires, and the reveal's flash.
  - Inertia and a rubber band when panning (plain pointer events, no InteractionTracker).
  - The `share` ask is refused.
  - The `screenshot` ask's offscreen picture leaves out washes and selection, and was not exercised by a case.
- **Testing gaps:**
  - No automation peers and no UI-test runner (slice 5).
  - Mouse clicks and marquee drags were not verified by injection (see the input probe).
  - Windows 10 was not tested (no machine).
- **Packaging:** no MSIX and no app icon.
