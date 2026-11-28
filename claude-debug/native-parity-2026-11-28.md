---
title: "Native shells against the egui editor: what is left for parity"
date: 2026-11-28
---

# Native shells against the egui editor: what is left for parity

An audit of `feat/native-n0` at `7dbc954` (the same tree as `main`), made 2026-11-28 to answer the maintainer's question: "What is left on the roadmap to native app parity with the egui version? (I note we don't have send-to on ipad or mac)". The egui editor is the reference. The shells compared are the Mac, the iPad (and the iPhone where it differs) and Windows.

Read only: nothing was changed, built or launched for this audit. The evidence is the code, the conformance cases file, the shells' tests and person checks, and what `docs/STATUS.md`, `AI.md`, `docs/BUGS.md` (working tree), note 24 and the reports in `claude-debug/` record of earlier launches.

## How to read the marks

| Mark | Meaning |
|---|---|
| works (P) | A person check or a UI test launched the app and went down this path as a person does |
| works (C) | A conformance case passed against the launched app. Cases drive the core through the control port and check the core's answers, almost never what the shell painted |
| works (C) † | As works (C), but no person path to the command was found in the shell: only a script reaches it |
| built | Code is present (in the core's menus, gestures, settings or status line, or in the shell), with no launch evidence found |
| missing | No path in that shell |
| fails | Launched and failing |
| n/a | Not applicable, with the reason |

The conformance evidence per shell: the suite has 95 cases (51 read, 44 edit), unchanged since `984db78`.

- **Mac:** 95 of 95 (`AI.md`, FR6 slice 3, 2026-11-25).
- **iPad simulator:** 80 of 80. The 15 cases skipped need the test engine (`engine:ports`), `saves` or `install`, which the iPad's runner claims only with `PAD_PORTS=1` (`native/apple/PlumCraftPad/conformance.sh:116-146`).
- **Windows:** read half 51 of 51, edit half 42 of 44, at `984db78` on the x64 guest (`docs/STATUS.md:12`). The two failures are B176.
- **egui:** 95 of 95 (`AI.md`). egui's other evidence is `egui_kittest` runs on the real `EditorApp`, not a launched binary. Whether that counts as a launch is the maintainer's call (section 7).

No iPhone-only conformance run and no iPad device conformance run after B42 and B43 were found. The device UI tests ran 6 passed, 2 skipped.

## The answer: what is left

Findings first, then sizes (small: under a day of agent work; medium: a few days; large: more), then dependencies. Opinions are kept to section 8.

### (a) Missing in every native shell

1. **The trust review never happens.** `Session::set_trust_file` (`crates/session/src/lib.rs:334`) turns on "hold a graph from elsewhere until it is approved", and only the egui app calls it (`apps/plumcraft-editor/src/lib.rs:516`). Nothing in `crates/ffi` does, so on the Mac, iPad and Windows a workflow from elsewhere runs at once with no Review prompt. The shells' prompt drawers exist and are never reached, and the Mac's drops `params.detail` (`native/apple/PlumCraftMac/App/Asks.swift:195-212`).
   - Fix: the C ABI core sets the trust file from `dirs.prefs`, then one launch test per shell.
   - Size: small. Depends on nothing.
2. **An application's own commands, forms, menus and hooks cannot reach the native shells.** `plumcraft_ffi::app::App` has only `packs` and `openers` (`crates/ffi/src/app.rs:24-31`).
   - An application built on the shells keeps these in egui alone: its group export and delivery, publish, prune-after-save and hold-while-writing hooks, and its own curve and mask command ids.
   - Size: large (registration through the C ABI, shell drawing for its forms, per-command tests).
   - Depends on decision (e) 3.
3. **Four egui canvas actions are egui's own UI and never became core gestures or menu items:**
   - a face's "N more fields" footer (`node.fields`, `crates/ui/src/canvas.rs:1261`);
   - naming an output (`output.label`, `crates/ui/src/net_view.rs:545`);
   - dragging a net's tag (`net.move`, `wire.labelAt`, `net_view.rs:434-435`);
   - a bundle-bound row's Reset (`bundle.reset`, `canvas.rs:1145`).

   The native shells draw the Fields footer, but a tap there only watches the node: `FooterTap` has only `Watch` and `Save` (`crates/editor-core/src/geometry.rs:608-618`, `gesture.rs:572-578`). Size: small to medium, all in the core. Depends on nothing.
4. **A value's hover preview on a wire** (`view.peek`, `crates/ui/src/wire_view.rs:106`; kittest `hovering_a_wire_shows_its_value`). Size: medium. On the iPad it needs a replacement for hover (shell interface 8.10).
5. **Views egui has and no native shell has:**
   - XML outline, PDF pages, Document, Markdown (text in egui too), mesh/STL, and Find in the text viewer.
   - XML, PDF and Document are offered by the core but are not in the native list (`viewers.rs:222`), so the native shells show the engine's picture.
   - The turn field for `angle` could not be found anywhere.
   - Size: medium each (PDF on Apple through PDFKit is the smallest).
6. **Zoom keys on the Apple shells bypass `canvas.zoom`.** ⌘+, ⌘− and ⌘0 are Swift's own (`MainMenu.swift:231-238`, `Scene.swift:180`, `GraphViewController.swift:1104-1106`), which breaks golden rule 4. Windows sends them to the core. Size: small.
7. **Opening a bundle (`.zip`)** is missing on the Mac and iPad (no zip type declared, `GraphFiles.swift:28,58`). Windows has it built, never launched. Size: small.
8. **Send To as egui once had it does not exist anywhere today** (section 5.1). Each native shell has the system share sheet, or none on Windows. Nothing reads `shell.share_targets` (B184). Size: none until decision (e) 1.

Not gaps against egui, because egui lacks them too: an MCP server, a minimap, crash recovery, auto-update, a log viewer (all shells have Messages), and the pipekit/AMQP engine mode (refused by the core, `crates/host/src/manifest.rs:241`).

### (b) Missing on one platform

**Windows**

1. **B176:** a running node never shows `progress`, and two edit-half cases fail. Until it is fixed the N5 gate is not met and `run.cancel` has no passing case on Windows. Size: small to medium.
2. **The curves and mask editors:** only the core's JSON window (F59, F60). Size: large. Depends on decision (e) 4.
3. **Several pictures share no zoom or pan** (`Viewers.cs:631`), and B159's zoom is not kept from item to item. Size: medium.
4. **A share sheet:** `platform.share = false` (`native/windows/src/PlumCraft.App/App.xaml.cs:127-130`), and a `share` ask is still answered with the shell's own error (`Asks.cs:69`). Doc drift: `docs/shell-interface.md:180` still says the Windows app "keeps answering the `share` ask … unchanged". Size: medium (DataTransferManager).
5. **B151:** About's build is hard-coded to "n5-slice3" (`App.xaml.cs:121`). Size: small.
6. **B155 and B158**, a wire's route dragged: "Windows not done". Size: medium.
7. **Table column widths cannot be dragged.** Size: small.
8. **Files dropped in:** there is no `AllowDrop`. Size: small.
9. **The `.plum` file association** (FR2). Size: small, but it comes with packaging, (d) 3.

**iPhone**

10. **The curves and mask editors have no compact layout** (F59, F60 not met). Size: medium.

**iPad**

11. **The engines folder is not where a person can put a manifest.** No `dirs.engines` is set, so it sits outside Documents. Size: small.
12. **B137:** a chosen path is kept relative to "/" when the graph has no file. Size: small. Its shape is a decision.
13. **File › New and Open are swallowed** (`GraphViewController.swift:641-644`), and New Graph writes its template in Swift (`Documents.swift:128-139`), not through `graph.new`. Both are tied to the open `document_windows` decision. Size: medium.
14. **Files dropped in:** only the Files handoff. Size: small.

**Mac**

15. **Open Recent with no window open** goes through NSDocumentController, Clear Menu does nothing there, and a second AppKit recents list is kept (`MainMenu.swift:200-229`, `GraphDocument.swift:54`). Size: small.
16. **The shortcuts of commands with no menu place are unbound**, because shortcuts are bound from menu items, not from `menu::shortcut_table()` (`MainMenu.swift:301-343`). On the iPad, plain and Shift-only shortcuts are dropped too (`Menus.swift:215`). Size: small.
17. **The canvas is a single accessibility element**, with no nodes in it (the same on the iPad). Size: medium.

### (c) Present but never launched

These are built in the named shells, with no launch evidence found:

1. The sheet of items, `viewer.sheet` (F55): Mac, iPad, Windows.
2. Save As from a pop-out, `value.saveAs` (F64): Mac, iPad, Windows.
3. Attach to a running HTTP engine by URL (`engine.attach`): the form is opened and cancelled on the Mac and Windows, never connected. The iPad connects only through a manifest.
4. The uploads prompt (`allowUploads`): every shell.
5. The nested run and item steppers on the Apple shells. Windows saw them at `984db78`.
6. The chart's brush, zoom and readout under a hand: Mac and Windows. The iPad's brush is launched.
7. Windows's chart painters, colour-by-value tints and Compare's difference picture: cases pass, the painting was never seen. `Bench.Panels.cs`, which walks every viewer, has never been run.
8. The colour widget's native pickers (NSColorWell, UIColorPicker, Windows). The core's `row.set` passes everywhere.
9. Values dragged out of a pop-out (`value.export`): Mac, iPad, Windows. egui cannot drag at all.
10. Bundle export: all shells. Theme import and export: Apple.
11. Engine › Reconnect: Mac. Restart and backoff: Windows (F2 claimed on reading).
12. Open Recent: iPad and Windows. Its case needs `saves` on the iPad.
13. A second document scene on the iPad.
14. State restoration (Mac, iPad), VoiceOver and Narrator beyond labels, and a live high-contrast switch on Windows.
15. Cell text copied: all shells.
16. The commands marked "built" in the appendix's table: 52 on the Mac, 71 on the iPad, 53 on Windows.

### (d) Switch-over work besides features

1. **Mac notarization: never done.** The DMGs in `dist/apple` are `--local` builds with no secure timestamp and no stapled ticket. `native/apple/release.sh:154-161` prints the notarytool and stapler steps. Size: small, but it needs the maintainer's credentials.
2. **TestFlight: never done.** No `.ipa` has been made. It needs an App Store Connect record, a profile and the upload. Size: small to medium of the maintainer's time.
3. **Windows packaging and signing: not built.** The app is unpacked and self-contained (`WindowsPackageType None`, `PlumCraft.App.csproj:22-24`). The plan says "MSIX at N5's gate" (card 7), and there is no code signing. Size: medium. The `.plum` association, the icon and updates come with it.
4. **Auto-update: none on any shell**, egui included. Size: medium per platform. Depends on (d) 3 for Windows.
5. **Settings migration from egui: no code.**
   - egui keeps `prefs.json` under its own folder: `plumcraft-editor` in Application Support on the Mac, the config folder elsewhere.
   - The Mac and Windows shells keep theirs under `Plummer`. Card 20 chose separate files.
   - The format is the core's, so a one-time import of the preferences, recents and approvals would work.
   - The engines folder is already shared.
   - Size: small. Depends on decision (e) 5.
6. **No user guide for Plummer.** `docs/guide.md` is egui's. Size: medium.
7. **Gates still open:**
   - N4: a notarized build and a TestFlight build.
   - N5 (Windows): the suite passed 66 of 66 on 2026-11-22 but fails two cases now (B176); budgets on a real GPU were never measured (the test machines draw with WARP); MSIX is not built; egui's Windows place waits on this gate.
   - N6 (the research software on the shells): not started as a stage. Its curves and mask editors are done in Swift as Plummer's own. Its openers are done through `plumcraft_ffi::app::register`. Send To and its commands are not done.
   - Size: follows the items above.
8. **CI never builds Swift or C#,** by policy (no macOS CI). The C# models and runner could run on Linux .NET. Size: small, optional.
9. **The egui installers are stale.** The last ones shipped were 0.6.1; 0.7.0 and 0.8.0 shipped crates only. This matters for Linux, where egui stays.

**Open native bugs that bear on switch-over**, from `docs/BUGS.md` (working tree):

| Shell | Bugs |
|---|---|
| Mac and iPad | B184 (no Send To; `shell.share_targets` unread) |
| Mac | B163 (stale "N problems" line, a core fix awaiting a check), B170 (black band in a locked-screen screenshot, tooling), B85 (main-actor check crash in a scripted panel step), B88 (more-button case missed once), B124 (a stray preferences file from an old trial build) |
| iPad | B137 (path relative to "/"), B139 (audio bar overflows 375 points), B111 (never idle after typing a number), B112 to B114 (device test faults, explained, still open), B181 (no `.plum` document icon) |
| iPhone | B138 (a portrait test opens the palette), B140 (a table pop-out spreads across landscape width) |
| Windows | B176 (no progress state, the suite fails), B99 (dropped-wire palette case flaky), B143 (run-changed case flaky), B54 (one stowed-exception exit), B55 (VM hangs), B92 (sound plays twice), B96 (wire-tile moiré), B52 (kit-test flake, fix passed, not closed) |
| All native | B154 (views: Windows painters for slices 2 to 8 not seen), B180 (a multichoice shows a text box; which app is unknown) |

New, found by this audit and not yet in `docs/BUGS.md`:

- the trust review missing on every native shell, (a) 1;
- the footer taps, (a) 3;
- the Apple zoom keys, (a) 6;
- the Windows share doc drift, (b) 4.

### (e) Decisions only the maintainer can make

1. **What Send To means now (B184).** The spike's named targets were Photos, Files, Mail, Messages, AirDrop, Copy, Open in the default app and More…, offered for a result or for the whole graph as a bundle. The choice:
   - restore them through `shell.share_targets` and a target in the `share` ask;
   - or keep the system share sheet, which already reaches those services, and close B184. Today's egui has no Send To, so this is not a gap against egui.
2. **A share sheet on Windows,** or `platform.share = false` for good.
3. **Whether an application's commands, forms and hooks cross the C ABI** ((a) 2), or the application keeps those features egui-only.
4. **What Windows writes its value editors in** (note 24 section 12: "Still open").
5. **Preferences migrated from egui or kept apart** (card 20 chose "its own file").
6. **The iPad and `platform.document_windows`** (kept off "for now").
7. **Which egui-only views are floor:** XML, PDF, Document, mesh, text find, hover previews.
8. **MSIX or an installer for Windows, and which update mechanism on each platform.**
9. **Whether egui's kittest runs count as "launched"** for the reference column.
10. **Whether the B176 fix alone re-opens the N5 gate,** or a GPU machine is required first.

## 1. Commands and menus

The core defines **190 commands**:

- 153 in the session's registry (`crates/session/src/commands.rs:323-366`, 28 modules);
- 37 canvas commands (`crates/editor-core/src/canvas_cmds.rs:85`).

The menu bar's titles are File, Edit, View, Node, Run and then each spec's menu (Engine, Group, Help), from `menu.rs:444-519`. All three native shells draw the core's bar, toolbar and context menus from `menu.get`: `PlumCraftKit/DocumentModel.swift:401,506,522` and `PlumCraft.Kit/DocumentModel.cs:591,616,630`. So a command placed in a core menu is reachable by a person in every native shell. The full table, one row per command, is in the appendix.

**Counts per shell** (all 190 commands; "missing" includes 12 commands no shell reaches at all, such as `run.stats`, `view.list` and `labels.reload`):

| Shell | works | built | missing | fails |
|---|---|---|---|---|
| egui | 108 | 63 | 19 | 0 |
| Mac | 113 | 52 | 25 | 0 |
| iPad | 96 | 71 | 23 | 0 |
| Windows | 110 | 53 | 26 | 1 |

**What egui reaches and no native shell does:**

- `node.fields`, `output.label`, `net.move`, `wire.labelAt`, `bundle.reset` and `view.peek`, the person-facing gaps in (a) 3 and (a) 4;
- `engine.switch`, through egui's command line;
- `engine.add`, from tests only.

**What only the iPad has:** `graph.saveCopy`, `graph.duplicate`, `graph.rename` and `graph.move`, from its document browser. These are not gaps.

**The table's method:**

- Ids were read from the spec sources.
- A case "uses" a command when its id appears in the case.
- A shell "reaches" a command when the id is in the core's menus, gestures, settings model or status-line segments, or appears in the shell's own sources.
- Person evidence was added by hand from the person checks and UI tests named in the cells.
- A command reached only through another command's ask, such as `node.title` through Rename's text ask, shows as "another command or an ask".

### 1.1 Asks

| Ask kind | Mac (`PlumCraftMac/App/Asks.swift`) | iPad (`PlumCraftPad/App/Asks.swift`) | Windows (`PlumCraft.App/Asks.cs`) |
|---|---|---|---|
| `file.open` | NSOpenPanel. works (P): person check Open…, B133 Browse | document picker, copies into Picked Files, Photos or Files. works (P): `testBrowseForAPath`, `testBrowseOffersPhotosForAPicture` | common dialog. works (P): person check `path-browse`, `reopen` |
| `file.save` | NSSavePanel with a Format pop-up. works (P): person check Save As… (cancelled), F19 Save Result | an alert, then the export picker. works (P): `testTheSavesQuestionAndARecipe` | common dialog (B160). works (P): `save-as`, `save-result` |
| `folder.pick` | NSOpenPanel (directories). built | document picker. works (P): `testMaskAsksForAFolder` | built |
| `prompt` | NSAlert. built; the Review prompt is never raised, (a) 1 | UIAlertController. built | ContentDialog. built |
| `form` | the app's own sheet. works (P): Attach… and Install Engine… sheets, F19 options | an alert with fields and pick menus. built | ContentDialog. works (P): `attach-form`, `install-form` |
| `text` | a sheet with a field. built (the Rename cases are answered by script) | an alert. built | ContentDialog. built |
| `clipboard.set`, `clipboard.get` | NSPasteboard. built | UIPasteboard. built | Clipboard. built |
| `share` | NSSharingServicePicker. built: no launched test, and `crates/ffi/tests/share.rs` is not a launch | UIActivityViewController. built: `ViewerBarTests.swift:39` checks only that the button exists | n/a: `platform.share = false`; answers its own error |
| `open.external` | NSWorkspace: a URL, a named app, the default app, or Finder. built | a URL opens; a file goes to the share sheet ("Open in…"). built | Launcher. built |
| `screenshot` | the canvas's own drawing. works (C) | the canvas's own picture. works (C) | the canvas snapshot. works (C) |

An ask a script raises is answered by the script, and its UI is not shown (`Asks.swift`, note 24). So conformance cases are no evidence for an ask's UI. Only the person checks and UI tests above are.

### 1.2 Platform capabilities and switches

| Capability | Mac (`AppMain.swift:190-191`) | iPad (`AppMain.swift:475-476`) | Windows (`ShellConfig.cs:60-61`, `App.xaml.cs:127-130`) |
|---|---|---|---|
| `subprocess` | true | false (by nature) | true |
| `craft_apps` | true | false | false |
| `python` | true | false | true when uv is found |
| `multi_window` | true | true | true |
| `document_windows` | true (decision 20) | not declared (open) | true |
| `share` | default (true) | default (true) | **false** |
| `shell.share_targets` | not declared | not declared | not declared |

`ShellWords.unbuilt`, the list of things a shell marks "isn't available yet", is empty (`PlumCraftShell/ShellWords.swift:24`). Windows has no such list; its one such text is the share answer.

## 2. What egui does outside the core

| Item | egui | Core | Mac | iPad | Windows |
|---|---|---|---|---|---|
| Files dropped in | none | no command (a shell may call `graph.open`) | a workflow drop is built (`GraphDocument.swift:140-166`); a picture drop is missing | missing (Files handoff only) | missing |
| Values dragged out | none (winit cannot) | `value.export` | built | built | built |
| Menu shortcuts | `menu::shortcut_table()` | yes | works, but from menu items only (b) 16 | built; plain and Shift-only shortcuts dropped | works |
| Canvas keys (Tab, Space, F, Delete, arrows, Esc) | hard-coded in egui (`canvas.rs:633-646`, `edit_view.rs:570-650`) | Tab and F are the core's; Space, arrows and Esc are not | built, hard-coded | built, no Space | works for Tab, Delete and arrows; F built; Esc chosen in C# |
| Zoom and pan | the graph widget's own; ignores `canvas.zoom` | `canvas.zoom` | defect (a) 6; pinch and scroll built | defect (a) 6; pinch works (P) `testTouchGestures` | works (keys to `canvas.zoom`) |
| Minimap | none | none | n/a | n/a | n/a |
| Find in the graph or a viewer | palette search; text-viewer Find is egui's own | palette only | palette works; Find missing | palette works; Find missing | palette works; Find missing |
| Engine menu | the core's | yes | works (P) | works (P) `testAWarnedEngineIsMarkedInTheMenu` | works (P) `engine-menu` |
| Install / Attach | egui windows ending in `engine.install`, `engine.attach` | yes | Install works (a real install), Attach built | Install refused by design (P), Attach built | Install works (P), Attach built |
| Log window | none; Messages is `view.messages` | `view.messages`; `log.tail` for scripts | Messages works; the core's log never reaches the system log | Messages works | Messages works |
| Control channel | `--control`, `--control-token-file` | `control::step` | works (C) | works (C); LAN debug builds only | works (C) |
| MCP server | none | none | n/a | n/a | n/a |
| Command-line flags | `run`, `check`, `tidy`, `nodes`, engine flags, `--screenshot`, a `.zip` argument | headless commands | built (no `.zip`) | works (used by every UI test) | built; unknown flags ignored |
| Recents | core `graph.recent`, `graph.openRecent` (B166) | yes | works (P), with (b) 15 | built | built |
| Autosave | not used | `autosave.rs` | n/a (explicit Save, decision) | works (P) `testANewGraphIsMadeAndSavedInTheAppsFolder` | works (C) |
| Crash recovery | none | none | n/a | n/a | n/a |
| Trust prompt (Review) | works (kittest; wired at `apps/plumcraft-editor/src/lib.rs:515-517`) | `Prompt::Review`, `graph.trust` | **missing**, (a) 1 | **missing** | **missing** |
| Uploads prompt | drawn (`crates/ui/src/lib.rs:903`) | `Prompt::Uploads` | built | built | built |
| Bundles | `graph.bundle`; a `.zip` opens | yes | export built; opening missing | export built; opening missing | export built; opening built |
| Themes, text size, UI zoom | the core's settings model | `settings.rs:525-531` | works (C); import and export built | works (C) | works (P) `settings-auto-off` |
| Accessibility | accesskit; no `lifecycle.appearance` | `lifecycle.appearance` | contrast and motion built; the canvas is one element | contrast and motion built; VoiceOver labels partly | names through UI Automation work (P); Narrator built |
| Multi-window | one document per process; OS pop-out windows | `view.popout`, `document_windows` | works (P) File › New, pop-outs | built (scenes) | works (P) `file-new`, `pop-out` |
| About | egui window | `app.about` | works (P) | built, drawn in Swift | built; B151 not done |
| Acknowledgements | egui window | `app.acknowledgements` | works (C) | works (C) | works (C), seen at `984db78` |
| Tooltips and help | the core's hover text | the Help menu has About and Acknowledgements | works | built | works (P) |

## 3. Viewers and editors

Mac and iPad share `native/apple/PlumCraftFFI/Sources/PlumCraftShell/`, except for the picture, text and scalar views in each app's `App/Popouts.swift`. Windows is `native/windows/src/PlumCraft.App/`, and egui is `crates/ui/src/`. "works (kt)" marks egui driven in `egui_kittest`. Case numbers are positions in `crates/editor-core/fixtures/conformance/cases.json`.

| View or editor | egui | Mac | iPad (iPhone) | Windows |
|---|---|---|---|---|
| Picture: fit, 1:1, zoom, channel | works (kt) `viewers/wrap.rs:45` | works (C) `App/Popouts.swift:385`, cases 35, 70 | works (P) `testEveryViewerByATap` | works (C) `Viewers.cs:399` |
| Pixel probe, pinned, detail on zoom | works (kt) | works (C) case 70 | works (C) | works (C) |
| View modes (auto, color, stretch, clip, heat, labels, angle) | works (kt) `array_view.rs:65` | works (P) B154 scripted copy | works (P) `testViewModesAndHistograms` | works (P) person check `view-modes` |
| Turn field for `angle` | not found | missing | missing | missing |
| Per-channel histograms, Log, NaN counts | built | works (P) `Viewers.swift:1250` | works (P) | works (P) `view-modes` |
| Histogram of all values (FR6) | works (kt) case 95 | works (P) View As clicked | works (P) `testA3DArrayShowsAHistogramThroughViewAs` | works (C), seen at `984db78` |
| Several pictures with one zoom | built | works (P) | works (P) `testSliceTwoViews` | **missing** `Viewers.cs:631` |
| Zoom kept from item to item (B159) | built | works (P) | works (P) `testItemsKeepTheZoomAndView` | **missing** |
| Numbers grid, 64-column window | works (kt) | works (C) cases 57, 69 | works (C) | works (C) |
| Grid number format, colour by value | built | works (C) case 60 | works (P) `testSliceTwoViews` | works (C); tint never seen |
| Table sorted and paged by the core | works (kt); egui sorts itself and does not page | works (C) case 54 | works (P) | works (C) |
| Table column widths dragged | built | built | works (P) `testATableColumnIsWidenedByADrag` | **missing** |
| Plot (1-D series) | works (kt) | works (C) cases 63, 64 | works (P) `testAHistogramIsTheCoresChart` | works (C) |
| Table chart, six kinds, the choice kept | built (paints; no choice UI) | works (C) case 65 | works (P) `testATablesChartIsChosenFromItsMenuAndFollowsAnUndo` | works (C); painters not seen |
| Chart zoom, brush, readout | built (no interaction) | works (C) case 66 | works (P) brush; works (C) zoom and readout | built; works (C) case 66 |
| Time axes (dates) | built (unit tests) | built | built | built |
| Chart on a node's face | works (C) case 67 | works (C) | works (C) | works (C) |
| Text | works (kt) | works (C) | works (C) | works (P) `pop-out` |
| Text Find | built `basic.rs:120` | missing | missing | missing |
| Scalar, JSON tree, hex | works (kt) | works (C) | works (C) | works (C) |
| YAML | built | works (C) | works (P) | works (C) |
| SVG and its Source, SVG text, CSV text | works (kt) | works (C) cases 62, 71 | works (P) | works (C) |
| XML outline | built `viewers/xml.rs:229` | missing (engine picture) | missing | missing |
| PDF pages | built `viewers/pdf.rs:84` | missing (engine picture) | missing | missing |
| Document | built | missing | missing | missing |
| Mesh / STL | works (kt) `viewers/mesh.rs:255` | missing | missing | missing |
| Audio: waveform, spectrogram, play, seek, loop | works (kt) | works (P) person check `audio_viewer` | works (P) | works (C) case 56 |
| Item stepper (flat list) | works (kt) | works (P) | works (P) | works (P) `list-names` |
| Run and item steppers (nested lists) | works (kt) | built | built | works (C), seen |
| Sheet of items (F55) | works (kt) | built | built | built |
| Compare A/B | works (kt) | works (C) case 76 | works (C) | works (C) |
| Compare's exact difference, probe | works (kt) | works (C) case 61 | works (P) `testCompareDifference` | works (C); picture not seen |
| Pop-out windows | works (kt) | works (C) | works (P); iPhone `testCompactWindowsByATap` | works (P) `pop-out` |
| Recipe | works (kt) | works (P) F56 | works (P) | works (P) `recipe` |
| Save Result | works (kt) | works (P) F19 | works (P) | works (P) `save-result` |
| Save As from a pop-out (F64) | works (kt) | built | built | built |
| Hover preview of a wire's value | works (kt) | missing | missing | missing |
| Curves editor | the core's JSON window; commands works (kt) | works (P) `curves_step` (the drag SEE with the screen locked) | works (P) `testCurvesEditor`; **iPhone missing** | **missing** (JSON window) |
| Mask editor | the core's JSON window | works (P) `mask_step` | works (P) `testMaskEditor`, `testMaskAsksForAFolder`; **iPhone missing** | **missing** |
| The core's value window (`node.editor`) | works (kt) | works (C) cases 81, 93 | works (C) | works (P) `editor-row` |
| Path Browse | works (kt) | works (P) | works (P) | works (P) `path-browse` |
| Photos as a Browse source | n/a | n/a (the open panel lists Photos) | works (P) | n/a |
| Colour widget | built | built (NSColorWell) | built (UIColorPicker) | built |
| Number, toggle, choice, text, multiline, path widgets | works (kt) | works (P) typed field | works (P) `testInlineEdits` | works (P) |
| Wire slice editor | built `slice_view.rs:140` | not determined | not determined | not determined |

## 4. Engines

| Transport or management | egui | Mac | iPad | Windows |
|---|---|---|---|---|
| Compiled in | text, array, craft; audio runs as a process | works: text, array, audio (`crates/ffi/src/core.rs:863-867`) | works: text, array, audio, and an application's own | works: text, array, audio |
| A separate process | yes | works (P): started, killed, started again (F2, F35) | n/a: iPadOS starts no programs | works (P) `engine-separate-process` |
| HTTP attach by URL | yes | built (form only) | works (C) through a manifest, the test engine on the Mac's loopback (29 of 29); the form is built | built (form only) |
| pipekit / AMQP | refused by the core | refused | refused | refused |
| Install from a repository (`KNOWN_ENGINES`) | works (kt) | works (P), a real install | n/a: refused with the core's reason (P) | works (P), through uv |
| Engines folder and Reload | yes | works (C) with a scratch folder | scripts only; (b) 11 | works (C) |
| Python engines | `--python` | works (uv) | n/a | works (uv) |
| Restart and backoff | yes | works (P) F2; Reconnect never picked | n/a (in-process only) | built |
| Pick Engines Automatically | yes | built | built (the default) | works (P) |

What note 24 decides for the iPad: in-process engines only, any HTTP engine attachable, and array, audio and text compiled in (the maintainer: "Compile array,audio,text in"). F2 and F35 stay unmet there by nature.

## 5. The application's needs (the floor)

### 5.1 What Send To was

Send To was never a core command and never on `main`. It lived only on spike branches:

- plumcraft's `spike/ios` (`12ddb16`, 2026-11-17);
- an application's own iPad spike, built on the shells.

Neither branch is in the current tree.

**On the plumcraft spike:**

- `crates/ui/src/send_to.rs` held a `SendTo` trait (`targets`, `send(target, file, subject)`, `take_notice`) and a process-wide target list.
- egui drew a "Send To" submenu beside Save Result… and in File › Send To, with More… after a separator.
- It added two egui action ids, `run.sendTo` (a node's output) and `graph.sendTo` (the workflow).
- A result was saved through `run.save` into a temporary outbox, and the workflow went as a bundle.
- An application supplied the targets through `AppConfig.send_to`.

**The application's spike gave these targets:**

- iOS: Photos (add only), Files (export), Mail and Messages (the node's title as the subject), Copy, and More… (the share sheet).
- Mac: Mail, Messages, AirDrop, Photos, Copy, Open in the Default App, and More… (the sharing picker).

Its own notes say the targets were "not yet tried by hand". **Today's egui editor and the application's current egui app have no Send To.** `AppConfig` has no such field, and `git grep` finds no `send_to` in `crates/` or `apps/`. `docs/shell-interface.md:127,916` still describes `shell.share_targets` as its replacement, and nothing reads it (B184).

**In its place:**

- **Mac:** File › Share… and the pop-out's Share… go to `NSSharingServicePicker`, whose services include Mail, Messages, AirDrop and Photos. `value.open` gives Open with › an app or Choose an App… (`menu.rs:1196-1211`). `open.external` uses NSWorkspace.
- **iPad and iPhone:** File › Share…, the toolbar's Share and the pop-out's Share… go to `UIActivityViewController`. `open.external` sends a file to the share sheet ("Open in…").
- **None of the three Apple surfaces** has a named Photos, Mail or Files target; those are reached through the system sheet. None of them has been launched by a test.
- **Windows:** no share sheet (`platform.share = false`). Open with goes through `Launcher`.

### 5.2 The application built on the shells

Its Mac and iPad apps compile Plummer's shell and add only an app-library registration of engines and openers. So:

| Item | egui app | Mac | iPad |
|---|---|---|---|
| Curves and mask editors | its own egui editors | works through Plummer's general editors (its private cases) | works (its private cases, 3 of 3 and 4 of 4) |
| Its own curve and mask command ids | yes | missing (the core's ids instead) | missing |
| Clean Up Edit Files in the File menu | yes | built as the core's `mask.prune`, with no menu place | built |
| Hooks: prune after save, keep files a saved picture names, hold runs while files are written, flush before a bundle | yes | missing (a) 2 | missing |
| Group export and delivery to the palette | yes | missing | missing |
| Publish | yes | missing | missing (its plan says "only if wanted") |
| Open a picture as its recipe (opener) | yes | built | works (its private case) |
| Its engine in the Engine menu | yes | built | works (its UI test) |
| Never offer the array engine | the command line only | missing | missing |
| About with its engine's versions | yes | partly (build stamp only) | partly |
| Send To | none today | n/a (the share sheet) | n/a (the share sheet) |

## 6. Distribution and switch-over

| Item | State | Evidence |
|---|---|---|
| Mac signing, hardened runtime | ready: Developer ID, no entitlements, universal, engines inside | `native/apple/release.sh:1-33,101-153` |
| Mac notarization | never done | `dist/apple` DMGs carry no timestamp or ticket; `release.sh:154-161` |
| TestFlight | never done | no `.ipa`; `release.sh:164-201` |
| Mac App Store | declined (card 4A) | note 24 |
| Windows packaging | not built; unpacked folder, no signing | `PlumCraft.App.csproj:22-24`; `native/windows/README.md:72` |
| Windows architectures | ARM64 first; x64 built and launched on a guest; no real-GPU budgets | note 24 section 21 |
| Auto-update | none anywhere | — |
| Settings migration from egui | none; separate files by card 20 | `crates/editor-core/src/prefs.rs` |
| `.plum` association | Mac done with an icon; iPad and iPhone declared, no icon (B181); Windows none (FR2) | `PlumCraftMac/project.yml:46-62` |
| User guide | none for Plummer | `docs/guide.md` is egui's |
| egui releases | crates 0.8.0, Python SDK 0.3.0; installers last at 0.6.1 | `CHANGELOG.md` |
| CI | Linux only; the C ABI's headless conformance runs; Swift and C# never | `.github/workflows/ci.yml` |

## 7. What could not be determined

- **Whether the works (C) rows draw correctly.** Conformance checks the core's answers, not the pixels.
- **Whether egui's kittest runs count as launched** under the maintainer's rule.
- **Missing run records:**
  - an iPhone run of each UI test;
  - an iPad device conformance run after B42 and B43;
  - the per-case log of the `984db78` Windows run (only summaries exist).
- **Things nothing has launched:**
  - whether the Apple nested steppers work;
  - whether Open Recent works on the iPad (its case needs `saves`) and on Windows;
  - whether state restoration reopens graphs;
  - the uploads and trust prompts in any shell.
- **Not checked in the code:**
  - a turn field for `angle` anywhere;
  - a native wire-slice editor;
  - where the C ABI keeps approvals once a trust file is set;
  - whether the Mac's ⌘ shortcuts cover all of `shortcut_table()`.
- **The command table is a heuristic.** Its "reaches" test is a text search, so a command reached through data the shell does not name (a menu item built at run time, a status-line segment) is counted only when the core's own model names it. Its "works (C)" means the id appears in a passing case, not that every parameter was exercised.

## 8. Opinions (not findings)

- **The trust review is a safety gap,** not a parity nicety. In my view it should be fixed before any native build goes to anyone, and it is small.
- **B176 and Windows packaging are what stand between Windows and the N5 gate.** The Windows value editors are the largest single parity item left, and they wait on decision (e) 4.
- **Send To:** the system share sheets already reach the spike's targets on Apple. Unless the maintainer wants named one-tap targets, B184 can be closed as "replaced by Share…" once a launch test presses Share on each Apple shell.
- **Suggested order:**
  1. (a) 1, the trust review;
  2. B176;
  3. (a) 3 and (a) 6, the footer taps and the Apple zoom keys;
  4. a share launch test on Apple;
  5. notarization and TestFlight (the maintainer);
  6. Windows MSIX with the `.plum` association;
  7. the Windows editors;
  8. the application's command bridge, if decided;
  9. the egui-only views.

## Appendix: every command, by shell

Columns: the command; how a person reaches it ("menu", "gesture", "settings" and "status line" are the core's models, which every shell draws; "shell code" means a shell calls it itself); then egui, Mac, iPad, Windows; then the evidence (a case name, with how many more cases use it, and the person check or UI test).

| Command | Reach | egui | Mac | iPad | Windows | Evidence |
|---|---|---|---|---|---|---|
| `edit.undo` | menu | works (C) | works (P) | works (C) | works (C) | case "a drag moves an unselected face alone and leaves the selection" (+15); Mac: person check: Edit › Undo in the curves window (B150) |
| `edit.redo` | menu | works (C) | works (C) | works (C) | works (C) | case "a value typed into a row is checked, held to its range, and is one und" |
| `edit.copy` | menu | works (C) | works (C) | works (C) | works (C) | case "the same error twice is counted twice" (+1) |
| `edit.cut` | menu | built | built | built | built | none found |
| `edit.paste` | menu | built | built | built | built | none found |
| `edit.move` | gesture | works (C) | works (C) | works (C) | works (C) | case "a drag moves an unselected face alone and leaves the selection" (+3) |
| `edit.align` | menu | built | built | built | built | none found |
| `edit.distribute` | menu | built | built | built | built | none found |
| `graph.new` | menu | built | works (P) | works (P) | works (P) | Mac: person check: File › New Graph; iPad: UI test testANewGraphIsMadeAndSavedInTheAppsFolder; Windows: person check: file-new |
| `node.add` | menu | works (C) | works (P) | works (C) | works (P) | case "a shell's measurements are what the core hits" (+79); Mac: person check: palette adds a node; Windows: person check: palette-add |
| `node.remove` | menu | works (C) | works (C) | works (C) | works (C) | case "a context gesture gives the menu of what it is on" |
| `node.duplicate` | menu | built | built | built | built | none found |
| `node.title` | another command or an ask | works (C) † | works (C) † | works (C) † | works (C) † | case "a group is made, entered and edited inside, left, ungrouped, and undon" (+1) |
| `node.fields` | egui only | built | missing | missing | missing | none found |
| `node.face` | shell code | works (C) | works (C) | works (C) | works (C) | case "the parameters inspector lists every input, and a hidden one is set, s" |
| `node.parameters` | menu, gesture | works (C) | works (C) | works (C) | works (C) | case "a double tap opens what it is on" (+1) |
| `node.editor` | menu, gesture | works (C) | works (P) | works (P) | works (P) | case "a value with an editor promoted to a group opens the member's editor f" (+1); Mac: person check: S4a/S4b row opens the editor; iPad: UI test testCurvesEditor; Windows: person check: editor-row |
| `node.view` | menu, gesture | built | built | built | built | none found |
| `node.cache` | menu | built | built | built | built | none found |
| `node.engine` | menu | built | works (P) | built | built | Mac: person check: node menu Run on |
| `node.move` | script only | missing | missing | missing | missing | none found |
| `node.addWired` | script only | missing | missing | missing | missing | none found |
| `node.set` | shell code | works (C) | works (P) | works (P) | works (C) | case "the audio engine compiled in runs a tone through a gain to its peak an" (+31); Mac: person check: value typed on a face; iPad: UI test testInlineEdits |
| `wire.connect` | menu, gesture | works (C) | works (P) | works (C) | works (C) | case "a tap on a pin or a wire watches its value" (+44); Mac: person check: wire dragged |
| `wire.slice` | menu | works (C) | works (C) | built | works (C) | case "ports: a list, a wire's slice of it, and a path input" |
| `wire.reroute` | gesture | works (C) | works (C) | works (C) | works (C) | case "a wire's end picked up off an input is rerouted, or dropped" |
| `wire.route` | menu, gesture | works (C) | works (C) | works (C) | works (C) | case "a wire's right angles: drag an automatic corner, Make Right-Angled, Ro" (+2) |
| `wire.disconnect` | menu, gesture | works (C) | works (C) | works (C) | works (C) | case "a wire's end picked up off an input is rerouted, or dropped" (+1) |
| `node.expand` | menu | built | built | built | built | none found |
| `wire.label` | another command or an ask | works (C) † | works (C) † | works (C) † | works (C) † | case "a net is made, joined, renamed in place and left" |
| `wire.unlabel` | menu | works (C) | works (C) | works (C) | works (C) | case "a net is made, joined, renamed in place and left" |
| `output.label` | egui only | built | missing | missing | missing | none found |
| `net.rename` | another command or an ask | works (C) † | works (C) † | works (C) † | works (C) † | case "a net is made, joined, renamed in place and left" |
| `net.connect` | another command or an ask | works (C) † | works (C) † | works (C) † | works (C) † | case "a net is made, joined, renamed in place and left" |
| `net.move` | egui only | built | missing | missing | missing | none found |
| `wire.labelAt` | egui only | built | missing | missing | missing | none found |
| `frame.add` | menu | works (C) | works (C) | works (C) | works (C) | case "a frame is moved by its tab and sized by its handle" (+5) |
| `frame.set` | menu | works (C) | works (C) | works (C) | works (C) | case "a frame is made round a selection, renamed in place, resized, and grow" |
| `frame.remove` | menu | built | built | built | built | none found |
| `frame.move` | gesture | works (C) | works (C) | works (C) | works (C) | case "a frame is moved by its tab and sized by its handle" |
| `frame.resize` | gesture | works (C) | works (C) | works (C) | works (C) | case "a frame is moved by its tab and sized by its handle" (+1) |
| `graph.tidy` | menu | built | built | built | built | none found |
| `graph.open` | menu | works (C) | works (P) | built | works (P) | case "Clean Up Mask Files removes the mask files nothing reaches, after aski" (+1); Mac: person check: File › Open…; Windows: person check: reopen |
| `graph.save` | menu | works (C) | works (C) | built | works (C) | case "Clean Up Mask Files removes the mask files nothing reaches, after aski" (+1) |
| `graph.saveAs` | menu | works (C) | works (P) | built | works (P) | case "Clean Up Mask Files removes the mask files nothing reaches, after aski" (+2); Mac: person check: File › Save As… (panel cancelled); Windows: person check: save-as |
| `graph.saveCopy` | shell code | missing | missing | built | missing | none found |
| `graph.duplicate` | shell code | missing | missing | built | missing | none found |
| `graph.rename` | shell code | missing | missing | built | missing | none found |
| `graph.move` | shell code | missing | missing | built | missing | none found |
| `graph.bundle` | menu | built | built | built | built | none found |
| `graph.info` | another command or an ask | works (C) † | works (C) † | missing | works (C) † | case "a new graph is saved as .plum, and a .json graph opens and saves back " |
| `graph.fileTypes` | shell code | works (C) | works (C) | built | works (C) | case "a new graph is saved as .plum, and a .json graph opens and saves back " |
| `app.about` | menu | built | works (P) | built | built | Mac: person check: About step |
| `app.acknowledgements` | menu | works (C) | works (C) | works (C) | works (C) | case "the Acknowledgements window is the core's: opened, read, searched and " |
| `app.quit` | menu | built | works (P) | built | built | Mac: person check: Quit |
| `graph.insert` | menu | built | built | built | built | none found |
| `graph.insertGroup` | menu | built | built | built | built | none found |
| `app.settings` | menu, settings | works (C) | works (C) | works (C) | works (C) | case "settings are the core's: a setting set is read back, kept, and reset" |
| `view.textSize` | settings | built | built | built | built | none found |
| `view.zoom` | settings | built | built | built | built | none found |
| `theme.setToken` | settings | works (C) | works (C) | works (C) | works (C) | case "settings are the core's: a setting set is read back, kept, and reset" |
| `theme.setFruit` | settings | built | built | built | built | none found |
| `theme.pinFamily` | settings | built | built | built | built | none found |
| `theme.reset` | settings | built | built | built | built | none found |
| `theme.import` | settings | built | built | built | built | none found |
| `theme.export` | settings | built | built | built | built | none found |
| `labels.reload` | script only | missing | missing | missing | missing | none found |
| `labels.show` | script only | missing | missing | missing | missing | none found |
| `graph.port.name` | another command or an ask | works (C) † | works (C) † | works (C) † | works (C) † | case "the Cache Report and the graph's Parameters window are the core's: ope" |
| `graph.port.unname` | menu | built | built | built | built | none found |
| `graph.port.rename` | menu | works (C) | works (C) | works (C) | works (C) | case "the Cache Report and the graph's Parameters window are the core's: ope" |
| `graph.port.doc` | menu | built | built | built | built | none found |
| `graph.port.reorder` | menu | built | built | built | built | none found |
| `graph.params` | menu | built | built | built | built | none found |
| `view.params` | menu | works (C) | works (C) | works (C) | works (C) | case "the Cache Report and the graph's Parameters window are the core's: ope" (+1) |
| `group.create` | menu | works (C) | works (C) | works (C) | works (C) | case "a collapsed group's face is its group" (+7) |
| `group.ungroup` | menu | works (C) | works (C) | works (C) | works (C) | case "a group is made, entered and edited inside, left, ungrouped, and undon" |
| `group.rename` | another command or an ask | works (C) † | works (C) † | works (C) † | works (C) † | case "a group is made, entered and edited inside, left, ungrouped, and undon" |
| `group.promote` | menu | works (C) | works (C) | works (C) | works (C) | case "a value with an editor promoted to a group opens the member's editor f" (+2) |
| `group.editPromoted` | menu | works (C) | works (C) | works (C) | works (C) | case "promoted parameters sit under their node's title on the group's face, " |
| `group.unpromote` | menu | works (C) | works (C) | built | works (C) | case "a value with an editor promoted to a group opens the member's editor f" |
| `group.output` | menu | built | built | built | built | none found |
| `group.removeOutput` | menu | built | built | built | built | none found |
| `group.checkpoint` | menu | built | built | built | built | none found |
| `group.collapse` | menu, gesture | works (C) | works (C) | works (C) | works (C) | case "a group is made, entered and edited inside, left, ungrouped, and undon" (+1) |
| `group.move` | gesture | built | built | built | built | none found |
| `curves.edit` | shell code | works (C) | works (C) | works (P) | works (C) | case "the curves editor's commands edit an array node's curves, one undo ste"; iPad: UI test testCurvesEditor |
| `curves.reset` | shell code | missing | built | built | missing | none found |
| `curves.preset` | shell code | works (C) | works (C) | works (C) | works (C) | case "the curves editor's commands edit an array node's curves, one undo ste" |
| `curves.equalize` | shell code | works (C) | works (C) | works (C) | works (C) | case "the curves editor's commands edit an array node's curves, one undo ste" |
| `node.editorData` | shell code | works (C) | works (C) | works (C) | works (C) | case "the curves editor's commands edit an array node's curves, one undo ste" (+2) |
| `run.start` | menu | works (C) | works (P) | works (C) | works (C) | case "the audio engine compiled in runs a tone through a gain to its peak an" (+17); Mac: person check: Run |
| `run.stats` | script only | missing | missing | missing | missing | none found |
| `run.cacheReport` | settings, status line | works (C) | works (C) | works (C) | works (C) | case "the Cache Report and the graph's Parameters window are the core's: ope" (+1) |
| `run.cancel` | menu, status line | works (C) | works (P) | works (C) | fails (B176) | case "the engine's run changed while it goes: stale, cancel, latest wins, a "; Mac: person check: Run › Cancel Run (F36) |
| `run.auto` | menu, status line | works (C) | works (P) | works (C) | works (C) | case "the engine's run changed while it goes: stale, cancel, latest wins, a " (+1); Mac: person check: Run › Auto-Run (F30) |
| `run.watch` | menu, gesture | works (C) | works (C) | works (C) | works (C) | case "a tap on a pin or a wire watches its value" (+7) |
| `run.view` | shell code | works (C) | works (C) | works (C) | works (C) | case "an array's view is chosen through the core and its pictures change" |
| `run.item` | shell code | built | built | built | built | none found |
| `run.save` | menu, gesture | works (C) | works (P) | works (P) | works (P) | case "ports: Save Result writes a result node's value in each of its formats" (+1); Mac: person check: F19 Save Result; iPad: UI test testTheSavesQuestionAndARecipe (PAD_PORTS=1); Windows: person check: save-result |
| `graph.review` | script only | missing | missing | missing | missing | none found |
| `graph.trust` | script only | missing | missing | missing | missing | none found |
| `bundle.detach` | menu | built | built | built | built | none found |
| `bundle.reset` | egui only | built | missing | missing | missing | none found |
| `engine.switch` | egui only | built | missing | missing | missing | none found |
| `engine.add` | egui only | built | missing | missing | missing | none found |
| `engine.use` | menu | works (C) | works (P) | built | works (P) | case "ports: a variadic input, a choice's sub-port, and a choice this machin" (+8); Mac: person check: Engine menu pick; Windows: person check: engine-menu |
| `engine.auto` | settings | built | built | built | works (P) | Windows: person check: settings-auto-off |
| `engine.remove` | menu | built | built | built | built | none found |
| `engine.reconnect` | menu | built | built | built | built | none found |
| `engine.attach` | menu | built | works (P) | built | works (P) | Mac: person check: Engine › Attach… sheet (cancelled); Windows: person check: attach-form |
| `engine.manifests.reload` | menu | works (C) | works (C) | works (C) | works (C) | case "an engine a manifest in the engines folder names is offered, and Reloa" |
| `engine.clear` | menu | built | built | built | built | none found |
| `engine.install` | menu | works (C) | works (P) | works (C) | works (P) | case "Install Engine… is offered as the platform allows, by the core" (+1); Mac: person check: Install Engine… sheet (cancelled); Windows: person check: install-form / install |
| `engine.install.cancel` | menu | works (C) | works (C) | works (C) | works (C) | case "Install Engine… is offered as the platform allows, by the core" |
| `engine.list` | another command or an ask | works (C) † | works (C) † | works (C) † | works (C) † | case "Install Engine… is offered as the platform allows, by the core" (+1) |
| `view.runColors` | menu, settings | built | built | built | built | none found |
| `view.statusLine` | menu, settings | works (C) | works (C) | works (C) | works (C) | case "the status line: the core's model, after a run, a selection and an err" |
| `view.animateRun` | menu | built | built | built | built | none found |
| `view.animationSpeed` | menu, settings | works (C) | works (C) | works (C) | works (C) | case "settings are the core's: a setting set is read back, kept, and reset" |
| `view.messages` | menu, status line | works (C) | works (C) | works (C) | works (C) | case "the Messages window is the core's: opened, read and closed" (+2) |
| `view.popout` | menu, gesture | works (C) | works (P) | works (P) | works (P) | case "a double tap opens what it is on" (+22); Mac: person check: value popped out by a click on its pin; iPad: UI test testEveryViewerByATap; Windows: person check: pop-out |
| `view.theme` | settings | works (C) | works (C) | works (C) | works (C) | case "settings are the core's: a setting set is read back, kept, and reset" (+1) |
| `view.close` | shell code | works (C) | works (C) | works (C) | works (C) | case "a value popped out is a window of the core's: brought forward, viewed " (+18) |
| `view.list` | script only | missing | missing | missing | missing | none found |
| `view.viewer` | shell code | works (C) | works (C) | works (C) | works (C) | case "a JSON value popped out by a person is an outline the core opens and c" (+7) |
| `viewer.text` | shell code | works (C) | works (C) | works (C) | works (C) | case "a value popped out is a window of the core's: brought forward, viewed " |
| `view.peek` | egui only | built | missing | missing | missing | none found |
| `viewer.pin` | shell code | works (C) | works (C) | works (C) | works (C) | case "Compare's exact difference is the core's: its words, its numbers and i" (+1) |
| `viewer.unpin` | shell code | works (C) | works (C) | works (C) | works (C) | case "Compare's exact difference is the core's: its words, its numbers and i" (+1) |
| `viewer.compare` | shell code | works (C) | works (C) | works (P) | works (C) | case "Compare's exact difference is the core's: its words, its numbers and i" (+1); iPad: UI test CompareDifferenceTests |
| `viewer.probe` | shell code | built | built | built | built | none found |
| `viewer.zoom` | shell code | works (C) | works (C) | works (P) | works (C) | case "a value popped out is a window of the core's: brought forward, viewed " (+2); iPad: UI test PopoutZoomTests |
| `viewer.recipe` | shell code | works (C) | works (P) | works (P) | works (P) | case "ports: a value with a recipe offers Recipe, and one without doesn't"; Mac: person check: F56 Recipe; iPad: UI test testTheSavesQuestionAndARecipe (PAD_PORTS=1); Windows: person check: recipe |
| `viewer.sheet` | shell code | built | built | built | built | none found |
| `viewer.table` | shell code | works (C) | works (C) | works (P) | works (C) | case "a table popped out by a person is sorted and paged by the core" (+4); iPad: UI test TableColumnsTests |
| `viewer.tree` | shell code | works (C) | works (C) | works (C) | works (C) | case "a JSON value popped out by a person is an outline the core opens and c" (+1) |
| `viewer.audio` | shell code | works (C) | works (C) | works (C) | works (C) | case "a sound popped out by a person plays, pauses and seeks through the cor" |
| `viewer.picture` | shell code | works (C) | works (C) | works (C) | works (C) | case "a picture popped out by a person shows a channel, reads its own number" |
| `viewer.grid` | shell code | works (C) | works (C) | works (C) | works (C) | case "an array popped out by a person is a picture, and its numbers a grid p" (+2) |
| `viewer.hex` | shell code | works (C) | works (C) | works (C) | works (C) | case "a value read as what it isn't is refused with a reason, and the core s" |
| `viewer.array` | shell code | works (C) | works (C) | works (P) | works (P) | case "an array's view is chosen through the core and its pictures change"; iPad: UI test testViewModesAndHistograms; Windows: person check: view-modes |
| `viewer.chart` | shell code | works (C) | works (C) | works (P) | works (C) | case "a table's chart is chosen by command, kept in the graph and drawn from" (+2); iPad: UI test ChartChoicesTests |
| `viewer.derive` | script only | missing | missing | missing | missing | none found |
| `viewer.histogram` | another command or an ask | works (C) † | works (P) | works (P) | works (C) † | case "a 3-D array's port shows a histogram of all its values through View As"; Mac: View As › Histogram click (FR6 s3); iPad: UI test HistogramViewAsTests |
| `viewer.range` | shell code | works (C) | works (C) | works (C) | works (C) | case "a chart's range and the value's selection are the core's, and so is wh" |
| `viewer.select` | shell code | works (C) | works (C) | works (C) | works (C) | case "a chart's range and the value's selection are the core's, and so is wh" |
| `doc.open` | menu | built | built | built | built | none found |
| `doc.revert` | script only | missing | missing | missing | missing | none found |
| `value.open` | menu | built | built | built | built | none found |
| `wire.convert` | status line | built | built | built | built | none found |
| `wire.convertOffers` | script only | missing | missing | missing | missing | none found |
| `node.convert` | script only | missing | missing | missing | missing | none found |
| `value.copy` | shell code | works (C) | works (C) | works (C) | works (C) | case "a table popped out by a person is sorted and paged by the core" (+2) |
| `value.export` | shell code | works (C) | works (C) | works (C) | works (C) | case "an array popped out by a person is a picture, and its numbers a grid p" |
| `value.share` | shell code | built | built | built | built | none found |
| `value.saveAs` | shell code | built | built | built | built | none found |
| `selection.set` | gesture | works (C) | works (C) | works (C) | works (C) | case "a tap selects; Shift adds; primary takes away; empty canvas clears" (+7) |
| `selection.clear` | gesture | works (C) | works (C) | works (C) | works (C) | case "a tap selects; Shift adds; primary takes away; empty canvas clears" (+2) |
| `selection.all` | menu | built | built | built | built | none found |
| `net.select` | menu, gesture | built | built | built | built | none found |
| `edit.delete` | menu | built | built | built | built | none found |
| `canvas.focus` | gesture | works (C) | works (C) | works (C) | works (C) | case "a collapsed group's face is its group" (+1) |
| `canvas.fit` | menu | works (C) | works (C) | works (C) | works (C) | case "Select mode and Share are the core's commands, and the toolbar's state" |
| `canvas.zoom` | shell code | works (C) | works (C) | works (C) | works (C) | case "Zoom In, Zoom Out and Actual Size are the core's commands" |
| `canvas.palette` | menu, gesture | works (C) | works (P) | works (C) | works (P) | case "a new wire dropped on nothing opens the palette for what takes it; dis" (+4); Mac: person check: palette adds a node; Windows: person check: palette-add |
| `palette.choose` | shell code | works (C) | works (C) | works (C) | works (C) | case "a kind chosen from a dropped wire's palette is added with the wire, as" (+1) |
| `selection.nudge` | shell code | works (C) | works (C) | works (C) | works (C) | case "two drags are two undo steps; a held arrow key is one" (+1) |
| `run.selected` | menu | built | built | built | built | none found |
| `wire.square` | menu | works (C) | works (C) | works (C) | works (C) | case "a wire's right angles: drag an automatic corner, Make Right-Angled, Ro" |
| `canvas.reveal` | menu, gesture | built | built | built | built | none found |
| `geometry.set` | shell code | works (C) | works (C) | works (C) | works (C) | case "hostile gestures and queries are refused, and the core still serves" |
| `geometry.parts` | shell code | missing | built | built | built | none found |
| `row.edit` | gesture | works (C) | works (C) | works (C) | works (C) | case "ports: a path input's Browse asks for a file by its hints, and the ans" (+1) |
| `row.browse` | gesture | works (C) | works (P) | works (P) | works (P) | case "ports: a path input's Browse asks for a file by its hints, and the ans"; Mac: person check: B133 Browse; iPad: UI test testBrowseForAPath / PhotosTests; Windows: person check: path-browse |
| `mask.stroke` | shell code | works (C) | works (P) | works (P) | works (C) | case "the mask editor's strokes are files, one undo step each, and a graph w" (+1); Mac: person check: S4b drag on the mask canvas; iPad: UI test testMaskEditor |
| `mask.clear` | shell code | works (C) | works (C) | built | works (C) | case "the mask editor's strokes are files, one undo step each, and a graph w" |
| `mask.get` | shell code | works (C) | works (C) | built | works (C) | case "the mask editor's strokes are files, one undo step each, and a graph w" (+1) |
| `mask.prune` | another command or an ask | works (C) † | works (C) † | missing | works (C) † | case "Clean Up Mask Files removes the mask files nothing reaches, after aski" |
| `mask.folder` | script only | missing | missing | works (P) | missing | iPad: UI test testMaskAsksForAFolder |
| `row.set` | gesture | works (C) | works (C) | works (C) | works (C) | case "a value typed into a row is checked, held to its range, and is one und" (+4) |
| `canvas.selectMode` | menu | works (C) | works (C) | works (C) | works (C) | case "Select mode and Share are the core's commands, and the toolbar's state" |
| `graph.share` | menu | works (C) | works (C) | works (C) | works (C) | case "Select mode and Share are the core's commands, and the toolbar's state" |
| `graph.recent` | another command or an ask | works (C) † | works (C) † | missing | works (C) † | case "File › Open Recent lists the graphs saved, newest first, from the core" |
| `graph.openRecent` | menu | works (C) | works (P) | built | works (C) | case "File › Open Recent lists the graphs saved, newest first, from the core"; Mac: person check: Open Recent |
| `graph.clearRecent` | menu | works (C) | works (C) | built | works (C) | case "File › Open Recent lists the graphs saved, newest first, from the core" |
| `name.edit` | menu, gesture | works (C) | works (C) | works (C) | works (C) | case "a group is made, entered and edited inside, left, ungrouped, and undon" (+3) |
| `name.set` | shell code | works (C) | works (C) | works (C) | works (C) | case "a group is made, entered and edited inside, left, ungrouped, and undon" (+2) |
| `selection.align` | menu | built | built | built | built | none found |
| `selection.distribute` | menu | built | built | built | built | none found |
