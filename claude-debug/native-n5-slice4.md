# N5 slice 4: panels and viewers (Plummer for Windows)

2026-11-21, branch `native/n5-panels`, from `feat/native-n0` at `6f2d2c6`, then fast-forwarded to the dev branch's `8820a35`. The slice's work is uncommitted. Note 24 section 21 has the summary. This report has the gate, the floor walk, the screenshots, what looked wrong, and what wasn't taken.

## Answer

Plummer for Windows now has every window and viewer the Apple shells have. Each is drawn natively from the core's model for it, and nothing is left on the "isn't built yet" banner: the banner's list (`Unbuilt` in `DocWindow.cs`) is gone.

- **The parameters inspector** is a pane docked at the right of the document's window.
- **The core's other panels** are windows of their own: Messages, the Cache Report, the graph's Parameters window, value editors and Compare (side by side and split).
- **Settings** is a window with a NavigationView, a page per section.
- **Every viewer is in a pop-out:** picture, SVG, an array's pictures and its numbers, text, scalar, table, JSON (YAML and the outline), bytes, items, sound, and what no viewer draws.
- **The canvas** draws net tags, the hover ring, the faces a marquee touches and waveform thumbnails.

**The suite skips no step.** Against the app it passes 53 of 53 cases with **729 of 729 steps run** from the C# runner on the VM. Before this slice a measured run skipped 18 steps (7 `geometry`, 4 `hot`, 5 `media`, 2 `reports`). Making them run took two small additions to the control port and a change to both runners. The C# runner also lacked the `holds` step form (B53, fixed). `dotnet test` passes 47 of 47.

**Not all of the gate was taken.** At 15:17 the Windows test VM's console session stopped starting programs. An app process, and every scheduled task's process after it, stuck where nothing could end them, and Windows logged a LiveKernelEvent at 15:31 (B55). As the brief says, I didn't restart the VM. Left untaken because of it:

- the x64 run;
- the Swift runner against the Windows app;
- repeated runs;
- the panel and 100,000-row table benchmarks (written, never run);
- three of the slice 3 screenshot retakes (`edit-number`, `edit-choice`, `context`).

## Gate

| Check | Where | Result |
|---|---|---|
| The suite, C# runner, against the app (ARM64), the read half | Windows test VM | 32 of 32 cases; 419 of 419 steps, none skipped (a mid-slice build, 14:3x) |
| The suite, C# runner, against the app (ARM64), the edit half | Windows test VM | 21 of 21 cases; 310 of 310 steps, none skipped (the same build) |
| The suite, C# runner, x64 under emulation | Windows test VM | **Not taken** (B55) |
| The suite, Swift runner on the Mac against the Windows app | Mac, forwarded port | **Not taken** (B55) |
| The suite against the Mac app (`conformance.sh all`, the Swift runner with this slice's changes) | Mac | 53 of 53 cases, 729 of 729 steps, none skipped; "the person's Library is unchanged" |
| `dotnet test` (final sources, after the merge) | Windows test VM | 47 of 47 (41 before, 6 new in `PanelsTests.cs`); one earlier run of the 6 new tests also passed |
| `cargo nextest run --all-features --no-fail-fast` | Mac | 1,569 of 1,570; the known `hidden_params_match_the_baseline` |
| `cargo test -p plumcraft-ffi --test conformance --test geometry` | Mac | 6 of 6; the port run now includes the media and `hot` steps |
| clippy `-D warnings`, `fmt --check`, `check-hash-gates.sh`, `check-core-toolkit-free.sh` | Mac | Clean |
| Python (`pytest -q`, `ruff check`) | Mac | 224 passed; clean |
| `build-xcframework.sh`, then `swift test` in `PlumCraftFFI` | Mac | Every run passes (31, 9, 14, 5 and 22 tests) |
| The VM's checkout against this worktree | Hashing (`r4-hash.sh`) | 639 of 639 files under `crates/` and `native/windows/` identical |
| `check-private-strings.sh` | Worktree | Right after the merge: "private strings: no match in 1242 tracked files". Again after the pattern file was extended at 18:08: 336 lines over 10 patterns, every one already on the dev branch (none in a line this slice added, and none in its new files: `r4-private.sh`); the files this slice touched that carry such lines are `docs/BUGS.md`, `docs/STATUS.md` and note 24, and I left those lines for the cleanup that owns them |

**Steps run of the suite's total.** Against the Windows app, 729 of 729. Before this slice, a measured run against an app skipped 18 of 729.

**How each skip was made to run:**

- **`media` (5 steps).** A C ABI core's control port now serves `media.get` (`crates/ffi/src/core.rs`). The C#, Swift and Rust port drivers say `media`.
- **`hot` (4 steps).** These are the typed drag move, `plum_gesture_move`. Its JSON form is now `geometry.dragMove {at, slop?}` (`crates/editor-core/src/control.rs`, `gesture::typed_json`), and port drivers call it.
- **`geometry` (7) and `only: "reports"` (2).** A measured driver over an app's port first waits until the app has measured those faces. It then reports the suite's measurements over them and waits until the core holds them. The app reports a face again only when its `rev` changes. `--own-geometry` keeps the old skip.
- **`holds` (3 steps, B53).** These were never skipped, but the C# runner wasn't checking them.

## The floor walk (plan section 9)

The core is the same C ABI core the Apple apps use. So for an item of the engine interface the core's tests and the suite are the evidence; for a UI item the evidence is the command, the test or a screenshot.

Counts: **77 met, 3 partly met (F16, F22, F53), 5 not met (F59, F60, F65, F66, F85).** Not met or partly met, one line each:

- **F16, path hints (partly).** A path is typed into a field. There is no Browse button: the core's path widget names no browse command for a shell to call.
- **F22, custom editors (partly).** The value editor window is built. No node of the text or array engines names an editor ("text.literal names no editor"), so it wasn't opened here.
- **F53, Compare (partly).** No exact difference (card 15).
- **F59 and F60, the curve and mask editors (not met).** These are the research software's editors (N6).
- **F65 and F66, plugin export and delivery (not met).** These are the research software's pack.
- **F85, local model management (not met).** This is the research software's CLI.

| F | Item | Windows | Evidence |
|---|---|---|---|
| F1 | Engine connection state | Met | The status line's engine dot and words from the feed's `engine` domain; conformance engine cases |
| F2 | Reconnect with backoff | Met | In the core (`plumcraft-session` links); the same C ABI core as the Apple apps |
| F3 | Liveness / health | Met | Core `ping`; `crates/engine` tests |
| F4 | Catalog fetch | Met | The palette lists every kind (`palette.items`, conformance) |
| F5 | Version gate | Met | Core; `crates/proto` hostile sweep and vectors |
| F6 | Engine identity, capabilities | Met | Core `caps`; the menu model's enabled states |
| F7 | Catalog hook | Met | Core |
| F8 | NodeDef core | Met | Faces from the core's scene; conformance |
| F9 | Wire types | Met | Wire drags snap, ring, dash through a Convert (slice 3); edit half |
| F10 | Families and icons | Met | Lucide icons on faces (slice 2); the palette by family |
| F11 | Port widget fields | Met | Every widget kind on the face (slice 3) and in the inspector (slice 4) |
| F12 | Socket modes | Met | Scene rows; the inspector says where a wired value comes from |
| F13 | Setting binding | Met | The inspector's sections (`parameters.get`) |
| F14 | Variadic inputs | Met | Core; scene rows |
| F15 | Choice-dependent sub-ports | Met | Core `node.set` replies; the choice editor |
| F16 | Path hints | Partly | A path is typed in a field on the face and in the inspector; no Browse button beside it (the Apple apps' path widget raises the file ask) |
| F17 | External ports | Met | Core |
| F18 | Machine-dependent choices | Met | Core; disabled choices with their reason in the ComboBox |
| F19 | Node flags | Met | Core |
| F20 | List typing | Met | Core; the item stepper (`run.item`) and the `items` viewer |
| F21 | Wire slices | Met | Slice… is a core command with a name in place (slice 3) |
| F22 | Custom editors | Partly | The value editor window is built (`node.editor`, `report.get {window: "editor"}`, JSON); no node of the text or array engines names an editor, so it was not opened here; an application's own editor kinds are not drawn |
| F23 | Codegen metadata | Met | Core passes `export` through |
| F24 | Expansions | Met | Core `node.expand`; the node menu |
| F25 | Style graphs | Met | Core reads templates |
| F26 | Run request | Met | Run on the toolbar and in the Run menu; conformance run cases |
| F27 | Targets | Met | Run Selected (`run.selected`) |
| F28 | Run identity, generation | Met | Core |
| F29 | Latest wins | Met | Core |
| F30 | Auto-run | Met | Core; the Run menu's check |
| F31 | Optimistic queued | Met | Run washes on faces (slice 2) |
| F32 | Run reply | Met | Core |
| F33 | Per-node report | Met | Face status and timing |
| F34 | Errors as data | Met | A failed node's face; the Messages window (slice 4, shot `messages-*`) |
| F35 | Failure stands in for a reply | Met | Core |
| F36 | Cancel | Met | Cancel on the toolbar (a filled square since slice 4) |
| F37 | Run summary for automation | Met | Control port `run.*`; conformance |
| F38 | Status record stream | Met | Core feed |
| F39 | States emitted | Met | Run washes; Colour Nodes While Running |
| F40 | Reducer rules | Met | Core |
| F41 | Canvas status display | Met | Face header status text |
| F42 | Per-node timing | Met | `cached · 0.21 ms` on faces (shots) |
| F43 | Parameter wires, bundles | Met | Core |
| F44 | Provenance | Met | Core |
| F45 | Stale tracking | Met | Core; faces dim stale |
| F46 | Text on nodes | Met | Face excerpts (shots) |
| F47 | Thumbnails in flight | Met | Picture previews (slice 2) and waveform thumbnails (slice 4) on faces |
| F48 | Texture cache | Met | The app's `Pictures`, keyed by what was asked |
| F49 | Inspect piggybacks on runs | Met | A watch pops the value out (contract 12.20) |
| F50 | Display payload | Met | `popout.get`; every viewer (slice 4) |
| F51 | View modes | Met | View As on every viewer's bar (`view.viewer`) |
| F52 | Pixel probe | Met | The readout (`viewer.data probe`) and a click's pinned probe (`viewer.probe`) |
| F53 | Compare | Partly | Pin A and B from a viewer's bar, side by side or split (`viewer.compare`, `compare.get`; shots `compare-*`); the exact difference is the egui editor's (card 15) |
| F54 | Tables and scalars | Met | The table viewer sorted and paged by the core; scalars |
| F55 | Sheet | Met | The `items` viewer (`viewer.sheet`); not shown with real content here (no engine here makes a list of pictures in one step) |
| F56 | Recipe display | Met | Recipe on every viewer's bar (`viewer.recipe`, its Copy `{copy: true}`); new in slice 4 (the Apple apps don't have it yet) |
| F57 | Picture window (T16) | Met | A pop-out window per value |
| F58 | Missing data is a state | Met | Viewers say what failed or is missing, with the reason |
| F59 | Curve editor | Not met | The research software's editor (N6) |
| F60 | Mask editor | Not met | The research software's editor (N6) |
| F61 | List mapping with axes | Met | Core |
| F62 | Vary nodes | Met | Core |
| F63 | Lineage | Met | Core |
| F64 | Save picture with recipe | Met | Save As… on every viewer's bar (`value.saveAs`; the engine writes the recipe) |
| F65 | Export group as plugin | Not met | `x.vi.export` is the research software's pack |
| F66 | Deliver plugin | Not met | As F65 |
| F67 | Plugin round trip | Met | Core expansions |
| F68 | Arrays never in messages | Met | Values lent by handle (`plum_blob`) |
| F69 | Content addressing | Met | Core, b3 |
| F70 | Hostile `.npy` and keys | Met | `crates/*/tests/hostile.rs` |
| F71 | Engine memo | Met | Core; the Cache Report (slice 4, shot `cache-*`) |
| F72 | Partial re-run | Met | Core |
| F73 | Preview latency path | Met | In-process engines over the C ABI |
| F74 | Full resolution on demand | Met | A zoom past the picture's pixels asks once (`viewer.zoom {to: "detail"}`, a picture file decoded larger, an SVG drawn larger) |
| F75 | Pictures as engine-side files | Met | Core |
| F76 | Workflow JSON is the payload | Met | Documents are workflow JSON |
| F77 | Recipes in pictures | Met | Engines write them on save |
| F78 | Bundles | Met | File › Export Bundle… from the core's menus, the file ask answered by FileSavePicker; not exercised by hand this slice |
| F79 | Insert graph or group | Met | File › Insert… from the core's menus |
| F80 | Engine-touching commands | Met | `enabled` from the core; Engine › Attach… and Install Engine… are the core's form ask, answered by a ContentDialog (shot `attach-*`) |
| F81 | Control channel | Met | The control port; the whole suite runs over it |
| F82 | Headless CLI catalog | Met | Core / CLI, not the app |
| F83 | Config by environment | Met | The core's config and the app's launch flags |
| F84 | Single-threaded engines | Met | Core |
| F85 | Local model management | Not met | The research software's CLI; not an app feature |

## Measurements

**None this slice.** `--bench OUT --bench-panels --nodes 500` (`Bench.Panels.cs`) is written, and builds. It would measure:

- each panel and each viewer opened at 500 nodes, timed from the core's command to the first frame that shows its content;
- a 100,000-row table's page scrolled a move a frame for the phase's seconds, three times, counting frames and late frames;
- five page turns through `viewer.table`.

It never ran: the VM's console session had stopped starting programs before it was reached (B55). The canvas budgets of slice 3 were not taken again either.

Two numbers did come from the kit tests and scenes, as glimpses rather than measurements:

- A table of 9 rows read, sorted and paged by the core in tens of milliseconds.
- A 160 × 120 Mandelbrot array's sound, picture and SVG computed in about 0.55 s.

## Built

### The kit (`native/windows/src/PlumCraft.Kit/Windows.cs`)

Platform-free models, each on its document model's synchronization context:

- `ParametersModel`;
- `PopoutModel` (every viewer command, and `viewer.data` pages waited on through the feed's `media`);
- `PopoutDiff`;
- `SettingsModel` and its items and sections (the section shown is the core's: `app.settings {tab}`);
- `ReportModel`;
- `CompareModel`;
- `MessagesModel`;
- `CoreWindowKeys`.

`Jsonv` gained nullable `Get`, `At` and `Items` so the core's models read cleanly.

**Tests (`PanelsTests.cs`), six against a real core:**

- the inspector's set, refusal, reset and Show on Face;
- a table sorted ascending and descending, then unsorted, through the core;
- another viewer chosen and the default back;
- a pin, then Compare on, split, a divider set, unpinned, and off;
- text wrap and monospace round trips;
- Settings read, a setting set and reset, an accent colour set and reset, Match the System offered and no text size, the section shown and Settings closed;
- the Cache Report, the graph's Parameters and Messages opened, read and closed;
- the pop-outs' diff.

### The app

- **`Inspector.cs`: the parameters inspector, as a docked pane.** The Mac's popover is an attached, transient surface inside the document's window. The Windows counterpart is a properties pane, as Visual Studio, Office and Paint have, and the pane stays while the person works on the canvas and never covers its node. A light-dismiss Flyout would close on the first click on the canvas, and a window of its own would fall behind the document's.
  - Each row kind gets a native control:
    - a number with a range: a Slider and a field, the field following the drag and the value set once when it ends;
    - a switch: a ToggleSwitch;
    - a choice: a ComboBox, with disabled choices and their reasons;
    - text: a TextBox;
    - several lines: a several-line TextBox;
    - a colour: a swatch with the system's ColorPicker;
    - an application's editor: the Value Editor window.
  - Reset and Show on Face are on each row; Show on Face is disabled on a wired input, with "A wired input always shows on the face".
  - The core's notes and refusals show under the title. Escape or the close button runs `node.parameters {}`.
- **`CoreWindows.cs`: Messages, the Cache Report, the graph's Parameters, value editors and Compare.** These are windows in the Windows 11 Settings look: cards, headings, Mica, a title bar that follows the app's appearance. The core opens and closes them. When the person closes one, the core's close command runs and the document's window comes forward.
  - Compare is a SelectorBar (Side by Side, Split). The split's divider is the shell's while dragged and is set in the core when the drag ends.
- **`SettingsWindow.cs`: Settings.** A NavigationView with a page per section of `settings.get`, each setting a card. The theme's ComboBox has Match the System. Text size is not offered.
- **`Viewers.cs` and `Popouts.cs`: the viewers.**
  - **Picture and SVG:** the channel picker (`viewer.picture`), Fit and 1:1 (`viewer.zoom`), the readout under the pointer (`viewer.data probe`), a click pinning the probe (`viewer.probe`), and detail asked for once a zoom passes the picture's pixels.
  - **An array's data:** its pictures; its numbers grid, with plane and page through the core and the 64-column window the shell's, as the Apple apps have it.
  - **Text** (wrap, monospace, the whole text) and **scalars**.
  - **Table:** a virtualised ListView under a header of the page's own widths, sorted and paged by the core.
  - **JSON:** YAML first, and the outline in a TreeView whose members the core opens and closes, with Copy Value on a right click.
  - **Bytes**, **items**, and **what no viewer draws**.
  - **Sound:** the core's waveform with the playhead, its spectrogram, and playback through Windows' MediaPlayer fed a 16-bit WAVE made from the lent samples. In a scripted run it is muted: the device is never opened and the clock runs silent. Play, pause, a seek, the level and loop are `viewer.audio`.
  - **The bar:** View As; a list's item stepper; Drag Out; Compare (Pin as A, Pin as B); Recipe; Copy; Save As…; Share…, disabled with the core's reason.
    - **Drag Out** is a real file drag. `DragStarting` takes a deferral, runs `value.export` and sets its file as a storage item. A drop into File Explorer was not tried (no hand on the VM).
    - **Recipe** (`viewer.recipe`, its Copy `{copy: true}`) is new on any shell's bar.
    - **Share…** takes its reason from `command.list`'s `disabledReason`.
- **The canvas, `CompositionCanvas.Marks.cs`:**
  - net tags at their pins;
  - an application's marks, when the scene has them (no engine here makes them, so none was seen);
  - the hover ring on a pin and the hovered wire drawn wider (the typed hit test);
  - the faces a marquee touches. These are the core's `touches`, asked over JSON at most ten times a second, always once more for the last point.
  - Waveform thumbnails on faces are in `CompositionCanvas.cs`.
- **Slice 3's leftovers:**
  - Cancel Run's icon is now a filled square.
  - The palette's section headings are secondary captions, not rows with a rule.
  - The faces a marquee touches are marked.
  - The screenshots that still said PlumCraft: `wire-bad`, `palette`, `edit-text` and `marquee` retaken. `edit-number`, `edit-choice` and `context` were not (B55).
- **`Bench.Panels.cs`:** the panels' bench (never run).
- **Scripts and status:**
  - `Script.cs` gains `hover:PLACE` and `item:COMMAND` (a menu item chosen by the person).
  - The status file reports the inspector, the core's windows, the viewers, the hover, tags and touches.
  - `PLUMMER_TRACE=1` logs every exception as it is thrown.
- **`unsafe` code** is unchanged: only `Interop.cs` in the app, and `Native.cs` and `PlumCore.cs` in the kit.

## Rust and contract changes

| Change | Where | The Apple shells | egui |
|---|---|---|---|
| A C ABI core's control port serves `media.get` (state and shape; the bytes stay `plum_blob`'s) | `crates/ffi/src/core.rs` (`replay`), `docs/control-protocol.md`, `docs/shell-interface.md` 16 | Their runner now says `media` over a port and runs those steps; the Mac app passes them | Unaffected (its own port is not a C ABI core's) |
| `geometry.dragMove {at, slop?}`: `plum_gesture_move`'s answer as JSON | `crates/editor-core/src/control.rs`, `gesture.rs` (`typed_json`), `docs/control-protocol.md`; hostile cases in `crates/ffi/tests/geometry.rs`; the Rust port driver uses it | The Swift port driver's `hotMove` calls it | Unaffected |
| Runners: step counts, the suite's geometry over a measured app's, `--own-geometry` | C#: `Runner.cs`, `Drivers.cs`, `Program.cs`. Swift: `Runner.swift`, `Drivers.swift`, `main.swift` | The Mac app passes 729 of 729 with it | Its runner (Rust) unchanged |
| `holds` in the C# runner (B53) | `native/windows/src/PlumCraft.Conformance/Runner.cs` | Already had it | Already had it |

There are no message type changes, so there was no schema change.

## What stays shell-side, and the clause allowing each

- **The view's pan and zoom** (the wheel, Ctrl+wheel, middle drag, one-finger pan, pinch). Contract 12.13.
- **Motion inside a viewer** (contract 13, "Where the line falls"):
  - scrolling a table or text;
  - a picture's pinch and Ctrl+wheel zoom;
  - a slider's or the divider's drag before it is set at its end;
  - the playing clock;
  - which 64 columns of a wide numbers grid show, as a horizontal scroll (the Apple apps keep it so; see decision 3).
- **The hover ring and the readout under the pointer.** These are reads, changing nothing: the typed hit test and `viewer.data probe` (contract 9, 13).
- **Draft text in a field before it commits:** the palette's search, a report field, an inspector field. The maintainer's "leave as is" for the palette; the same rule for the others.
- **The platform's chrome:** window placement, minimise, maximise, the title bar, and focus within a window (contract 12.13).
- **Opening the files named on the command line** (`document.open`, the platform's way of opening files).
- **Which key routes where.** For example, Escape on the canvas tries `run.cancel`, then `net.select`, then `selection.clear`. Each is the core's command; the order is contract 12.3's binding for Escape, read by the window.
- **Test hooks:** `--then`, `--bench`, `--bench-panels`.

## Screenshots

These are in `claude-debug/native-n5/shots-r4/`, light and dark, taken inside the VM. I looked at every one.

| File | What |
|---|---|
| `params-*` | The inspector on Every widget: a value set ("corgi", with Reset), `note` hidden from the face, the wired `text` input saying where its value comes from, Show on Face disabled on it |
| `settings-*`, `settings-colours-*`, `settings-running-*` | Settings: Appearance (Match the System, the accessibility settings), Colours of Paper or Dusk with contrast notes, Running |
| `messages-*` | Messages after a failed node ("1 error, in n3") |
| `cache-*` | The Cache Report after a run |
| `graphparams-*` | The graph's Parameters window with a named port (name, description, Go To, Remove) |
| `table-*` | A table sorted by `n` descending through the core |
| `yaml-*`, `tree-*` | A JSON value as YAML, then as the outline with two members opened by the core |
| `audio-*` | A sound: waveform, spectrogram, Play, Loop, the clock, the level, "muted (scripted run)" |
| `picture-*`, `picture-red-*` | A picture fitted, then its red channel |
| `svg-*` | An SVG drawn by the core at 2048 px |
| `array-*`, `grid-*` | An array's picture, then its numbers grid (columns 0 to 63 of 160) |
| `text-*`, `scalar-*` | Text with wrap off and monospaced; a word count as a scalar |
| `compare-side-*`, `compare-split-*` | Compare, side by side and split at 0.4 |
| `recipe-*` | The Recipe flyout over an array's pop-out ("No recipe…": the array engine makes none) |
| `waveform-*` | A waveform thumbnail on Array to Audio's face |
| `attach-*` | Engine › Attach… chosen by the person: the core's form in a ContentDialog |
| `canvas-tags-*` | A net's tags: "words ×2" at the driver, "words" at each use |
| `marquee-*` | A marquee under way, the face it touches ringed |
| `wire-bad-*`, `palette-*`, `edit-text-*` | Slice 3's shots retaken as Plummer; the palette's new headings |
| `editor-*`, `try1-*` | Not useful: no node here names an editor; a first trial |

Compared with the Mac's (`claude-debug/native-n4/s5-shots/mac-table-light.png` and the others there):

- The Windows table has no column dividers and no alternating rows. File Explorer's details view has neither, so this may be right for the platform.
- The Mac's bar shows fewer buttons (Copy, Share and Save, and its compare menu is a separate icon). Windows shows icons only, with tooltips.

**What looks wrong or un-native:**

1. **The bar's Compare icon** (four squares) doesn't say "compare", and the bar has no labels. Tooltips only.
2. **Compare's SelectorBar** reads as tabs rather than a segmented choice. WinUI 3 has no segmented control outside the Community Toolkit.
3. **Panel windows overlap the document's window** on the VM's 1280-pixel display: there is no room beside it.
4. **The Messages window** has no Mica (its list fills it). It looks plain next to the others.
5. **The graph's Parameters window's empty text** says "Long-press an input". These are the core's words, written for touch.
6. **A pop-out opens at 560 by about 440**, small for a picture or table.
7. **Monospaced text looks heavier than Consolas** usually does. Cascadia Mono is the first choice; I didn't check which face rendered.
8. **The palette still opens where it fits**, over the title bar near the window's top (left for the maintainer).
9. **"Activate Windows"** shows on the VM's desktop (its licence, not the app).

## Defects introduced and caught

| Defect | Caught by | Fixed |
|---|---|---|
| Panels in dark mode drew light cards: the theme brushes come from the app's resources, which followed light | The dark screenshots | The app's theme is the requested appearance |
| Panel and pop-out windows had light title bars in dark mode | The screenshots | `TitleBarTheme` from the appearance |
| The picture viewer showed nothing: `ScrollViewer.MinZoomFactor` 0.05 is refused (below 0.1), and the throw was caught and logged | `PLUMMER_TRACE`'s log | 0.1 |
| A fitted picture overflowed its window (a zoom set before layout) | The picture shot | Fitted by stretch, with zoom kept for 1:1 |
| The table's header and rows didn't line up (the list centred inside a wider column) | The table shot | Both left-aligned |
| The outline showed "Microsoft.UI.Xaml.Controls.TreeViewNode" for every member | The tree shot | An item template on the node's content |
| The marquee's touched faces never showed: the throttled ask sent a stale point and dropped the latest | The marquee shot, then the trace | The latest point is read after the wait |
| `run.start` called as a method in the kit tests | The first test run | `command.run` |
| Several build errors: an ambiguous `Border`, `FontFeatures` (no such property), a shadowed name | The build | Fixed |
| The first run of the read half against the app ended in a stowed exception (B54) | The suite | Not reproduced since; windows now drop late replies, and viewers catch and log their faults |

## Unverified

- **No VM run after the final build.** The final build (the outline's template, the table's alignment, waveform thumbnails, Recipe, title bars, the picture's fit) was used only for screenshots. The suite ran on a mid-slice build. The changes since were in drawing, not commands, but no run checked that.
- **The x64 build.** Not built or run this slice.
- **Every timing in the brief's "Numbers" list.**
- **Sound through the audio device.** Every run was scripted, so muted. Windows' MediaPlayer playing the WAVE was never heard.
- **Drag Out dropped into File Explorer.** Not tried.
- **Viewers not exercised with real content:** a value editor (no node names one), a list's `items` viewer, bytes (`hex`), and an application's marks (no engine here makes them).
- **Real hover with the mouse.** Hover was checked by the app's own entry point (`hover:` step; the status said `pin`), not by a mouse.

## VM and Mac

**The VM:**

- **Free space:** 65.87 GB before, 64.92 GB after.
- **The checkout:** `git checkout -- .`, `git clean -fd` of slice 3's copies, `git fetch origin feat/native-n0` and `git checkout --detach 6f2d2c6`, then `r4-sync.sh` (every file this worktree changed or added since `6f2d2c6`, the dev branch's merge included). Verified by hashing: 639 of 639 files under `crates/` and `native/windows/` the same on both sides. The core DLL was rebuilt after the merge.
- **Left behind:** no scheduled task of this slice. These processes could not be ended from the SSH session, and they are still running:
  - `Plummer.exe` (started 15:17);
  - the `cmd.exe`, PowerShell, `conhost.exe`, OpenConsole and Windows Terminal processes launched with it;
  - one `taskhostw.exe`.

  The console session starts no new program (B55). The VM was not restarted.

**The Mac:**

- **Free space:** 62 GB at the start, 25 GB at the end (other agents' builds included; the brief's floor, so no further build was started).
- **Real Library locations:** checked before and after the Mac app's suite run (`r4-mac-before.txt`, `r4-mac-after.txt`), and `conformance.sh`'s own check said "the person's Library is unchanged".
  - `com.swapwizard.plummer.mac`'s caches, logs and preferences were last modified at 17:50 to 17:51, before this slice's run.
  - `com.plumcraft.mac`'s were older still.
  - The real store and `Application Support/Plummer` were modified at 17:58:32, before my Mac app run. I didn't launch the Mac app then; my Python tests were running at about that time under the scratch environment, which refuses the real store. Another agent or a device pass may have written them.
  - Nothing in those locations changed during or after my run.

## Scripts (`claude-debug/native-n5/`, untracked)

| Script | What |
|---|---|
| `r4-*` | Copied from slice 3's `r3-*`, renamed |
| `r4-scene.ps1`, `r4-scene.sh`, `r4-look.sh`, `r4-steps/*.txt` | Scenes driven through the app's control port, with screenshots between steps, in light and dark |
| `r4-launch.ps1 -Trace` | Launches with `PLUMMER_TRACE` |
| `r4-hash.sh` | The VM's checkout against the worktree |
| `r4-private.sh` | The private-strings patterns against new files |
| `r4-kill.ps1`, `r4-launch0.ps1` | Tried when the console session stuck; neither helped |
| `r4-steps.py` | Counts the suite's steps by form |

Logs: `r4-nextest*.log`, `r4-clippy.log`, `r4-swift-test.txt`, `r4-mac-conform.log`, `r4-scenes-run*.log`, `r4-shots-run1.log`.

## Decisions for the maintainer

1. **The inspector as a docked pane** beside the canvas (Windows' properties-pane convention) rather than a popover-like flyout on the node or a window. Keep it?
2. **The suite's geometry over a measured app's, by default.** A measured run now reports the suite's measurements over the app's own for the `geometry` steps, so nothing is skipped; `--own-geometry` keeps the old skip. Keep it as the default for every runner?
3. **The numbers grid's 64-column window is the shell's,** as on the Apple apps, treated as a scroll. Make it the core's (`viewer.grid {cols}`), under "no exceptions"?
4. **Recipe on the viewer's bar exists only on Windows.** Add it to the Apple apps (F56 is "does not" there)?
5. **Sound plays through MediaPlayer from a 16-bit WAVE.** Accept that, or play float samples through AudioGraph (more code)?
6. **B55.** The Windows test VM's console session needs a person to sign out or restart it before N5 goes on. The untaken gate items (x64, the Swift runner against the Windows app, the benchmarks, three retakes) wait on it.
7. **B54.** Keep it open until repeated runs have been taken, or close it as not reproduced?
