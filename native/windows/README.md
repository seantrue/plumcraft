# PlumCraft on Windows (stage N5): the app and the kit

The native Windows shell, C# and WinUI 3, on the same editor core as the Apple apps, through the C ABI in `crates/ffi` (`crates/ffi/include/plumcraft.h`, `PLUM_ABI_VERSION` 1). Stage N5's spike (2026-11-21) is in `docs/research/24-native-shells.md` section 20 and `claude-debug/native-n5-spike.md`; its slices are in section 21 there.

The maintainer, 2026-11-21: "Let's swap a windows implementation for the Kotlin implementation." The toolkit is "C#, WinUI 3". egui stays the Windows editor until this shell passes the conformance suite.

## What is here

Slice 1 (2026-11-21) grew the spike's `PlumCraft.Core` into the kit: everything a Windows shell needs below its windows, mirroring the Swift kit (`native/apple/PlumCraftFFI`). Slice 2 (2026-11-21, "read and run") replaced the spike's window with the app on the kit; slice 3 (2026-11-21, "editing") made it edit; slice 4 (2026-11-21, "panels and viewers") gave it every window and viewer the Apple shells have, each drawn from the core's model for it. The app is named **Plummer** (the maintainer, 2026-11-21: "Plummer will be the new base app/platform"): `Plummer.exe`, its window titles, About and its data folder; the projects, namespaces, kit, core and protocol stay PlumCraft. What remains of the spike is the probe and, inside the app, its bench (Composition only) and its display and adapter queries; the spike's Win2D and XAML renderers, its own scene model and face layout, and its input logging are gone (the canvas decision is made, and the kit's model replaced them).

| Path | What | Kit or spike |
|---|---|---|
| `src/PlumCraft.Models/` | The contract's types (`docs/shell-interface.md`, `crates/editor-core/schema/shell1.json`), hand-written records with `System.Text.Json` source generation: the scene and its deltas, view, selection, run state, theme, menus, asks, watched values, palette, hits, locate, gestures, `geometry.set`, the feed, pop-outs, parameters. A word a newer core adds is kept as its string (`State`) beside its typed reading (`StateWord`, null when unknown). `Shell.Options` skips unknown fields; `Shell.Strict` (tests) refuses them. `Jsonv`: JSON values as `JsonElement`, pointers, the suite's equality. Pure C#, no pointers | Kit |
| `src/PlumCraft.Core/` | `Native.cs`, the C ABI as P/Invoke, struct for struct; `PlumCore.cs`, a core: the wake as an `[UnmanagedCallersOnly]` function that only sets an event, one pump thread that polls, completes requests and fans each poll out to subscribers as a `FeedBatch` (`Subscribe`), typed requests decoded on the thread pool, refusals as `PlumException` with the core's words and `code`, borrowed blobs (`Lease`), the hot calls; `HotCalls.cs`, their results read in words; `ShellConfig.cs`, the core's config and the environment set before it opens. The only `unsafe` code in the kit: `Native.cs` and `PlumCore.cs`'s pointer methods (open, request, post, blob, wire paths, geometry) | Kit (grown from the spike's) |
| `src/PlumCraft.Kit/` | `DocumentModel`: one document as views see it, living on one `SynchronizationContext` (a window's UI thread; headless, a `SerialContext`): the scene by nid with an `ItemModel` per item replaced only when its `rev` changes (`INotifyPropertyChanged`), drawing order, selection, focus, view, theme, menu bar, toolbar, asks, the watched value, engine and document state, media notes; `SceneChanged` with the nids changed and removed for a Composition canvas; read-your-writes after its own commands; `WaitAsync` on conditions, never fixed sleeps. `CanvasLink`: typed and JSON `geometry.set`, `geometry.parts`, hits, wire targets, every `ui.gesture`, a drag's moves through `plum_gesture_move` and the dragged faces' wire paths, arrow nudges. `Editing.cs` (slice 3): `DragDriver` (a drag's four phases, what it shows each move), `PaletteModel`, `RowEditor`, `NameEditor`, `EditorPlacement`, `WindowWanted` (the feed's word that the core opened a document for New or Open). `Windows.cs` (slice 4): `ParametersModel`, `PopoutModel` (every viewer command, `viewer.data` pages), `PopoutDiff`, `SettingsModel`, `ReportModel`, `CompareModel`, `MessagesModel`, `CoreWindowKeys`. No WinUI | Kit |
| `src/PlumCraft.Conformance/` | The conformance runner (contract 16.1) in C#, reading `cases.json` where it is: the step language, substitution, `$locate`, matchers and checks of `crates/editor-core/src/conformance.rs`; `KitDriver` (through the kit, as an app drives it) and `PortDriver` (a core's control port, JSON lines, `auth` first; `reports: false` for a running app that measures its own faces) | Kit |
| `tests/PlumCraft.Kit.Tests/` | xUnit: the models against the shared fixtures, the schema and live replies (strict), hostile input, the spike probe's flow, the hot calls, the document model, and the whole suite through the kit and over a port | Kit |
| `src/PlumCraft.App/` | The app, Plummer (WinUI 3, unpackaged and self-contained; `Plummer.exe`; slice 3: `Editors.cs`, the palette, field editor and name editor as native controls; `Canvas/CompositionCanvas.Drag.cs`, a drag's previews; `CanvasView` drives every drag through the kit's `DragDriver`). `App.xaml.cs`: one core per process, the core's own first document closed and a `document.open` per window, the status file scripts wait on. `DocWindow.cs`: a window per document, its content in the title bar over Mica, the menu bar and toolbar from the core's menu model, an InfoBar, a status line; every item dispatched by id. `Menus.cs` (MenuBar, MenuFlyout, the core's shortcuts), `Asks.cs` (pickers, ContentDialog, clipboard, Launcher, the canvas's picture), `Popouts.cs` (a window per pop-out, its bar: View As, the item stepper, Drag Out, Pin as A or B, Copy, Save As…, Share… disabled with the core's reason), `Viewers.cs` (slice 4: picture and SVG with the channel picker, Fit and 1:1, the readout and probe, detail on zoom; an array's pictures; text; scalars; table and numbers grid in a virtualised list paged and sorted by the core; the JSON outline in a TreeView and YAML; bytes; items; sound with waveform, spectrogram and MediaPlayer playback; what no viewer draws), `Inspector.cs` (slice 4: the parameters inspector, a pane docked beside the canvas), `CoreWindows.cs` (slice 4: Messages, the Cache Report, the graph's Parameters, value editors and Compare as windows of their own), `SettingsWindow.cs` (slice 4: a NavigationView of the core's settings sections), `Ui.cs` (the panels' shared pieces), `Appearance.cs` (`lifecycle.appearance`), `Script.cs` (`--then`, scripted steps for screenshots), `Bench.cs` (the budget run). `Canvas/`: `CompositionCanvas` (a SpriteVisual per face drawn offscreen on workers and copied into its CompositionDrawingSurface, a ShapeVisual per wire, frames and open groups, one transformed container, faces drawn again at the zoom's scale when a zoom settles, culling, the run wash and pulse as compositor animations, the selection ring, previews, the marquee; slice 4, `CompositionCanvas.Marks.cs`: net tags, an application's marks, the hover ring, the faces a marquee touches), `CanvasView` (input to `ui.gesture`; pan and zoom), `FaceLayout`, `FaceRenderer`, `FaceText` (the Swift faces' constants with DirectWrite), `Pictures` (icons and lent pictures). `Interop.cs` is the app's only `unsafe` code (the DXGI adapter query) | App |
| `src/PlumCraft.ConformanceCli/` | `plumcraft-conformance` in C#: the suite against a running shell's control port, the Swift command's flags (`--port --token-file --cases --doc --measured --engine --half --case --needing`), and `call` for one scripted request | Kit (a tool) |
| `src/PlumCraft.Probe/` | A console program: N0's flow and the boundary's timings (now also the kit's tests) | Spike, kept building |

Nothing here implements a user action: the kit dispatches core commands by id (`RunAsync`, `InvokeAsync`, `ActivateAsync` for a menu item), sends recognised input as `ui.gesture`, and the core decides what it is. Pan and zoom are the view's (contract 12.13).

## Building and testing (on the Windows 11 ARM64 test machine)

1. The core as a DLL, release, no debug info:

   ```powershell
   git config core.symlinks true   # before checkout; the repository has 45 symlinks (digest.rs, the Inter font, licences)
   $env:CARGO_PROFILE_RELEASE_DEBUG = '0'
   cargo build -p plumcraft-ffi -p plumcraft-text -p plumcraft-array --release --target aarch64-pc-windows-msvc
   ```

   This gives `target\aarch64-pc-windows-msvc\release\plumcraft_ffi.dll` (about 26 MB), which `Directory.Build.props` finds there (or pass `-p:PlumFfiDir=<folder>`), and the text and array engines' programs, which the app's build copies beside `Plummer.exe` so a person can host those engines as a separate process (B91; the audio engine is optional and not shipped).

2. The kit's tests, with the .NET 10 SDK alone (NuGet brings xUnit and the test SDK):

   ```powershell
   dotnet test tests\PlumCraft.Kit.Tests -c Release
   ```

   Set `PLUMCRAFT_TEST_SCRATCH` (and `TEMP`) to a scratch folder: every store, preferences file and root the cores touch goes under it, never a real one.

3. The probe, the app and the C# conformance command:

   ```powershell
   dotnet build src\PlumCraft.Probe -c Release
   dotnet build src\PlumCraft.App -c Release -p:Platform=ARM64
   dotnet build src\PlumCraft.ConformanceCli -c Release
   ```

   NuGet brings the Windows App SDK 2.6.5 and Win2D 1.4.0. The app is unpackaged and carries the Windows App SDK with it (`WindowsPackageType None`, `WindowsAppSDKSelfContained`): its folder runs as it is. `-p:Platform=x64` builds an x64 app; it needs an x64 core DLL under `target\x86_64-pc-windows-msvc\release` (`cargo build -p plumcraft-ffi --release --target x86_64-pc-windows-msvc`; it runs on an ARM64 machine under emulation).

4. **The launch as a person would** (the maintainer: "Add it to every shell's gate"), on the signed-in desktop of the test machine, after the build:

   ```powershell
   powershell -ExecutionPolicy Bypass -File person-check.ps1 [-Platform x64] [-Work <scratch folder>]
   ```

   It starts `Plummer.exe` with `--root` alone (a scratch folder; no file, no engine flag, no control port, no status file, no script steps) and uses it as a person does: real mouse and keys (`SendInput`), UI Automation to find and read what is on the screen (the canvas's faces and pins through its automation peer, `CanvasPeer.cs`), a screenshot after every step. It prints PASS or FAIL for each step with what it saw: the first window with an untitled graph; File › New Graph and closing that window; the Engine menu's engines (the conformance suite's probe from a scratch engines folder among them); Pick Engines Automatically turned off in Settings and Text hosted as a separate process, connected; Engine › Attach…'s form with each label at its own field; Install Engine…, disabled with its reason where uv isn't found and its form where it is; a node from the palette given a value in its field, a second wired to it by a drag, Run, and the value popped out from the output pin's menu; Save As to a scratch file, closing it and reopening it; File › Quit ending the app. From an SSH session it runs itself on the console desktop as a one-shot Interactive scheduled task and stops the app it started by its process id. Only two folders are redirected, through the environment a person's launch would also carry: the engines folder (`PLUMCRAFT_ENGINE_FOLDER`) and `TEMP`. What it cannot see: how things look (the screenshots are for a person), sound, and anything UI Automation doesn't expose.

## Running

- `PlumCraft.Probe.exe <scratch folder> [out.json]`: prints a line a check and exits 0 when all pass.
- `Plummer.exe [graph.json ...] [--root DIR] [--engine text|array] [--control-port 0 --token-file F] [--status F] [--appearance light|dark] [--top] [--size W,H] [--nodes N] [--view Z,X,Y] [--bench OUT --exit] [--then STEP;STEP]` (`Options.cs` says what each is). Every root the core writes goes under `--root` (default `%LOCALAPPDATA%\Plummer`). `--then` also takes slice 3's steps (press, move, release, click, double and right clicks, Escape, typing, New, Exit, adding and wiring nodes: `Script.cs`), which enter where real pointer input does.
- `plumcraft-conformance --port N --token-file F --doc D --measured --engine text --engine array --cases ..\..\crates\editor-core\fixtures\conformance\cases.json --half read`. Since slice 4 a measured run also runs the suite's `geometry` steps (the suite's measurements over the app's; `--own-geometry` skips them; `--has manifests` when the app's engines folder is the suite's, `PLUMCRAFT_ENGINE_FOLDER` set to `crates/editor-core/fixtures/conformance/engines`), its `hot` steps (`geometry.dragMove`) and its `media` steps (the port serves `media.get`), and the summary counts the steps run of the total.

A window started from an SSH session lands in session 0, where nobody sees it. The scripts in `claude-debug/native-n5/` (not tracked, except their Markdown) start it on the signed-in desktop through a one-shot Interactive scheduled task, screenshot that desktop the same way, inject mouse, touch and pen input there, and run the conformance suite from the Mac through an SSH-forwarded control port; `k1-*` there push this folder to the machine and run the kit's tests, `r2-*` build, launch, screenshot, probe, bench and run the suite against the app from the VM and from the Mac, and `r3-*` (slice 3) do the same for the editing build, inject real input through `SendInput` and synthetic pen and touch devices (`r3-send.ps1`, `r3-probe.sh`), and sync the worktree to the machine (`r3-sync.sh`); `r4-*` (slice 4) add scenes driven through the app's control port with screenshots between steps (`r4-scene.ps1`, `r4-look.sh`, steps files in `r4-steps/`). Set `PLUMMER_TRACE=1` to have the app log every exception as it is thrown.

## Not built

A share sheet (the app declares `platform.share: false`, so the core disables Share with its reason), documents' recent files, packaging (MSIX or an installer), a UI-test runner beyond the person check, inertia when panning, and any CI: CI stays Linux only. Section 20.7 of note 24 has the slice plan; section 21 what each slice did.
