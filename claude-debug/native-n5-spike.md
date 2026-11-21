# N5 spike: a native Windows shell (C#, WinUI 3) on the editor core

2026-11-21, branch `native/n5-spike` (not committed). Findings and the slice plan are in `docs/research/24-native-shells.md` section 20; this report holds the measurements, how they were taken, and the screenshots.

## Answer

The core works on Windows ARM64 through the C ABI with no change to any Rust: the DLL builds, and C# runs N0's whole flow through P/Invoke. A WinUI 3 window draws the core's scene at 100 and 500 nodes three ways; Composition visuals (a visual per face, a shape visual per wire, one transformed container) are the one to build on. The unchanged conformance suite passes against the Windows app from the Mac over an SSH-forwarded control port: read half 30 of 30, edit half 21 of 21. The VM renders with WARP (no GPU) at 64 Hz, so its frame numbers compare approaches but cannot judge the frame budget.

## The machine

| What | Value |
|---|---|
| Host | Windows 11 Pro 10.0.26200, ARM64, UTM on the Mac, 4 cores, 8 GB |
| Display | VirtIO GPU display-only driver, 1280 x 909, 96 DPI, 64 Hz (EnumDisplaySettings and DWM agree; WMI says 1 Hz) |
| Adapter | Microsoft Basic Render Driver (0x1414, 0x8c), listed as hardware and as software: WARP |
| Toolchains | Rust 1.99.1 (aarch64-pc-windows-msvc), VS 2022 Build Tools 17.14, Windows SDK 10.0.26100, .NET SDK 10.0.404 |
| Packages (NuGet) | Microsoft.WindowsAppSDK 2.6.5 (2.7.0 was four days old), Microsoft.Graphics.Win2D 1.4.0 |
| Free disk | 15.86 GB before, 12.11 GB after (checkout and target 1.56 GB, NuGet cache 1.70 GB, scratch 0.07 GB) |

Nothing was installed on the VM beyond NuGet packages: no Visual Studio workload, no Windows SDK, no Rust toolchain.

## 1. The core through the C ABI

`cargo build -p plumcraft-ffi --release --target aarch64-pc-windows-msvc` with no debug info: a 25.6 MB `plumcraft_ffi.dll` (and a 168 MB static library), no warnings, about 3 minutes cold. The first attempt failed in `plumcraft-craft`'s build script: Git for Windows' default `core.symlinks false` checked the repository's 45 symlinks out as text files. With `core.symlinks true` (the VM's account is an administrator) and the links checked out again, it built.

`PlumCraft.Probe` (C#, `native/windows/src/PlumCraft.Probe`) printed:

| Check | Result |
|---|---|
| Struct sizes against the header (buf 24, bytes 32, hit 56, face 80, drag 96) | All match |
| `PLUM_ABI_VERSION` | 1 |
| `command.list` | 164 commands, node.add enabled |
| A four-node graph, `graph.get` | 4 nodes, 3 wires |
| Run on the in-process text engine | Both watched nodes done; 15 node.status events and run.reply by the feed; 23 wakes, 24 polls |
| Borrow the 64 MB value | 64,000,000 bytes, starts with the literal; a second borrow shares the pointer; release twice gives missing |
| After close | The lease still reads; no leases left |

Timings (medians): ping 27 us (300); graph.get 4.6 ms at 500 nodes (93 KB) and 18 ms at 2,000 (378 KB); scene.get whole 68 ms at 500 (1.27 MB) and 263 ms at 2,000 (5.1 MB), core and C# parse together; plum_hit_test 0.58 and 2.5 us; borrowing a 64 MiB plain blob 41 us.

## 2. Drawing: three renderers

Each bench run opens a core, builds `spike.scene` (N nodes, 1.6 N wires), follows the scene, measures and reports every face, then drives five 4-second phases from `CompositionTarget.Rendering`: idle, pan at zoom 1, pan with the whole graph in view, zoom between the fit and 1.5, and a face dragged through the core (`ui.gesture`, `plum_gesture_move`, `plum_wire_paths` each frame). Late is an interval over 1.5 periods (23.4 ms). Medians of three runs for Composition and Win2D, one for XAML.

| Renderer, nodes | Idle | Pan | Pan, whole graph | Zoom | Drag |
|---|---|---|---|---|---|
| Composition, 100 | 58 fps, 13 ms/s late | 58, 17 | 57, 22 | 57, 15 | 58, 17 |
| Win2D, 100 | 58, 2 | 57, 8 | 58, 0 | 57, 0 | 57, 5 |
| XAML, 100 | 56, 31 | 13, 808 | 31, 514 | 6, 921 | 20, 724 |
| Composition, 500 | 58, 18 | 56, 47 | 54, 85 | 53, 81 | 56, 51 |
| Win2D, 500 | 58, 16 | 58, 4 | 37, 486 | 56, 95 | 58, 2 |
| XAML, 500 | 46, 1,076 | 2, 1,096 | 1, 1,117 | 1, 1,035 | 14, 1,175 |

| Renderer, 500 nodes | App CPU a second: pan, whole graph, zoom | Working set | First scene to renderer (faces built) |
|---|---|---|---|
| Composition | 0.60, 0.77, 0.66 s | 400 MB | 138 ms |
| Win2D | 0.86, 2.62, 1.35 s | 325 MB | 73 ms |
| XAML | 1.13, 0.99, 1.61 s (at 1 to 2 fps) | 3,106 MB | 126 ms |

The hot calls during a drag took 0.01 ms a frame (p50) in every renderer; the canvas's own UI-thread work per frame was under 0.1 ms for Composition. In WinUI 3 the compositor runs in the app's process, so its WARP rasterising shows as the app's CPU; `dwm.exe` used 8 to 250 ms a second.

## 3. Budgets

| Budget | Composition 100 | Composition 500 | Win2D 500 |
|---|---|---|---|
| Launch to first frame (process start) | 0.42 s | 0.63 s | 0.57 s |
| Core open to first frame | 0.16 s | 0.38 s | 0.32 s |
| One-node change to its face drawn (p50, max) | 17.4, 124 ms | 17.1, 20 ms | 16.4, 36 ms |
| Frames | 57 to 58 fps | 53 to 56 fps | 37 to 58 fps |

The one-node change is `node.set` on the literal, `scene.get {since}`, layout, `plum_geometry_set`, the renderer updated, and the next `Rendering` event: about one frame of a 64 Hz display. DWM's `DwmGetCompositionTimingInfo` answers but its counters did not advance, so present-level numbers need PresentMon on a machine with a GPU.

## 4. Text

DirectWrite through Win2D's `CanvasTextLayout` (Segoe UI Variable Text), cached by string and style. At 500 nodes: 507 strings measured in 6 to 7 ms, every face laid out in 10 to 13 ms, reported in 3 ms (`plum_geometry_set` with header, pins and widget rects; `geometry.parts` with the title and the more-button). The core's paths and hit tests follow: the suite's geometry cases pass against the app.

## 5. Driving and testing

| Question | Answer |
|---|---|
| Launch from SSH | A one-shot Interactive scheduled task on the console session (`ps/launch.ps1`); the app writes a status file (PID, control port); stopped by PID |
| Screenshot | GDI screen copy from an Interactive task (`ps/shot.ps1`); the app takes `--top` to sit above the shared desktop's other windows |
| Control port from the Mac | Loopback port, `ssh -L` from the Mac, the token file copied back; the Swift `plumcraft-conformance` runs unchanged with `--measured` |
| Suite | Read half 30 of 30 and edit half 21 of 21, no skips, no settle timeouts, about 10 s each |
| Packaged or not | Unpackaged and self-contained builds with the .NET SDK alone and runs from its folder (280 MB); MSIX is a packaging step, not a dependency |
| UI tests | UI Automation sees the window, the content bridge and a TextBlock, nothing of the canvas; FlaUI (UIA3) would drive it once each face has an automation peer |

The first read run failed one case ("the more-button opens the node's menu": `face n1 has no menu measured`), because the spike drew no more-button and reported no `menu` part. It now draws one and reports it; both halves then passed.

## 6. Input

Injected inside the console session (`ps/inject.ps1`), recorded by the app's `--input-log`, and what the core made of each from the app's log:

| Input | WinUI delivered | Core's reply |
|---|---|---|
| Mouse click, empty canvas | Mouse pressed and released | selection.clear |
| Mouse drag of face n1's header by (120, 60) | Pressed and released; the SetCursorPos moves in between did not arrive as PointerMoved | edit.move n1 to (120, 60), drag "gesture:2" |
| Wheel, Ctrl+wheel, horizontal wheel | Delta -120, 120 with Control, 120 horizontal | None (pan and zoom are the view's) |
| Touch tap, one-finger drag | Touch, pressure 0.5, 10 moves in contact | selection.clear; a marquee |
| Pen hover, press, drag, lift | Pen, 6 hover moves, pressure up to 0.90, tilt (20, -10) | A marquee |

Not probed: a precision touchpad and pinch (nothing synthesises them and UTM has none), the touch keyboard, ink.

## 7. Screenshots

All in `claude-debug/native-n5/shots/`, taken inside the VM.

| File | What |
|---|---|
| `shot-1-win2d-500.png` | Win2D, 500 nodes, fit |
| `shot-2-composition-500.png` | Composition, 500 nodes, fit |
| `shot-3-xaml-500.png` | XAML, 500 nodes, fit |
| `shot-4-composition-500.png` | Composition, 500 nodes, zoom 1 |
| `shot-5-win2d-500.png` | Win2D, 500 nodes, zoom 1 |
| `input-before-composition-100.png`, `input-after-composition-100.png` | Before and after the input probe |
| `first-win2d-100.png` | The first window (before the face drew a more-button) |

![Composition, 500 nodes, zoom 1](native-n5/shots/shot-4-composition-500.png)

![XAML, 500 nodes, fit](native-n5/shots/shot-3-xaml-500.png)

## 8. Defects introduced and caught

| Defect | Caught by | Fixed |
|---|---|---|
| `await` inside an `unsafe` class (C# CS4004) | The first build | Unsafe narrowed to the methods with pointers |
| A reply could arrive before its request was registered (race between `plum_request` and the pump) | Review before the first run | The pump looks replies up under the lock the request holds |
| Win2D draw times counted once per frame instead of once per draw | Reading the first bench | Counted by draw |
| The first-scene timing record overwritten by later refreshes | Reading the XAML bench (it showed 0 faces) | Recorded once |
| No more-button part reported | The conformance read half (29 of 30) | Drawn and reported; 30 of 30 |
| A stray app from the input probe left running locked the build | The next build | Stopped by PID; scripts stop by the status file's PID |
| A dangling scheduled-task console window over the app | The first screenshot | The launcher runs hidden |

## 9. Scripts

`claude-debug/native-n5/`: `win.sh`, `push.sh`, `pull.sh` (SSH with time limits), `conform.sh` (the suite from the Mac), `summarize.py`, and `ps/` (survey, clone and build, probe, build, launch, stop, shot, shots, bench, inject, uia). Bench JSON in `bench/`, logs `conform-read.log` and `conform-edit.log`, the input log `input-log.jsonl`.
