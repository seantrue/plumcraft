---
title: "N3 slice 4: the menu bar, the asks, run and watch, three viewers"
date: 2026-11-20
---

# N3 slice 4: the menu bar, the asks, run and watch, three viewers

Branch `native/n3-run-watch`, cut from `origin/feat/native-n0`. Nothing is committed; the changes are in the working tree for review. `AI.md` and `docs/BUGS.md` were not touched.

## Summary

All eight items were done. Item 7 (culling) is built and measured, but it was not shown to reduce hitches at 2,000 nodes on this machine.

1. **The menu bar** is the front document's menu model (`menu.get`), kept current by the feed's `menus` changes and validated item by item from the latest model. Items dispatch by id. A disabled item's tooltip is the core's reason. Undo and Redo carry the core's step names.
2. **Context menus:** a secondary click or Control-click on a node, wire, frame, group or the canvas shows the core's menu for it. Its items dispatch as the bar's do.
3. **The asks** are answered natively when the person started them: sheets on the window, NSPasteboard, NSWorkspace. An ask a script started is left for the script; no panel appears. `share` is refused with the reason. Saving and the edited dot follow NSDocument conventions, mapped onto the core's dirty state.
4. **Engines and running:** every document opens on the text engine with the array engine beside it. Their state shows in a toolbar item. Run, Run Selected and Cancel are in the toolbar and the Run menu. Run state is washed onto the canvas, with its pulse, hold and fade; this needed a core change. A node's error shows on its face and, whole, as its tooltip.
5. **Watch:** a Watched inspector panel per window, collapsible (View › Show Watched Panel, Option-Command-I, or the toolbar). It has native `picture`, `text.plain` and `scalar` viewers; other kinds say "no viewer yet". Faces draw their rgba8 previews. Pictures are decoded on a worker.
6. **`ui.viewport`:** nothing in this slice needed it, so it was left. Culling uses the canvas's own transform. The core's fit is a rect the shell fits itself. The palette isn't built.
7. **Culling** of off-screen face, wire and tag layers. Measured at 500 and 2,000 nodes (below).
8. **Conformance against the running app:** the read half passes 12 of 12 with **no skips**, and the edit half passes 12 of 12. This is one app run with both engines, and `conformance.sh` is still the one command.

## 1. The menu bar

`native/apple/PlumCraftMac/App/MainMenu.swift` (`MenuBar`).

- **Built from the model.** The application menu comes first, then the model's menus in its order (File, Edit, View, Node, Run, Group, Engine, Help), then Window before Help.
  - The bar is rebuilt only when the model's structure changes: ids, labels, commands, shortcuts or separators.
  - Otherwise each item's title, enabled state, check mark and tooltip are read from the latest model when AppKit validates it.
  - It follows the key window's document, or the first document opened while there is no key window.
- **Dispatch.** An item without `params` is `ui.invoke {command, by: "person"}`. One with `params` is `command.run`.
- **Roles are the Mac's own items:**
  - About, Settings… (Command-Comma, from the model's preferences item) and Quit go in the application menu, with Services, Hide, Hide Others and Show All.
  - New, Open… and Save use NSDocumentController's and NSDocument's actions, with Open Recent, Close, Revert to Saved and Save As….
  - Undo, Redo, Cut, Copy, Paste, Delete and Select All are the standard selectors, sent down the responder chain. An editing text field takes them. Otherwise the canvas answers them with the core's commands.
  - Window and Help are the system's. The model's Help › About moves to the application menu.
- **Undo and Redo: driven from the model, not the window's `UndoManager`.** The labels come from the feed's new `document` change (`undo: "Tidy"` becomes "Undo Tidy").
  - Why: the core owns the undo history. Scripts, gestures and other documents' requests change it too. A mirror stack of dummy actions in an `UndoManager` would drift from it.
  - Two things are lost: Shake to Undo (not a Mac thing) and the undo manager's own grouping of text-field edits. A text field still uses its own undo, because the actions go down the responder chain.
- **Items whose UI isn't built** do not run. They say so in a banner at the foot of the window for 5 seconds, for example "Settings isn't available yet in this app (Settings)." These are: Settings, the core's About window, Edit Parameters, value editors, pop-outs, Messages, the Cache Report, graph Parameters, the palette (Node › Search…), Attach…, Install Engine… and Compare.
- **Shortcuts.** A model shortcut becomes a key equivalent only when it has a modifier besides Shift. A plain key bound in the menu would take typing from a text field. So F (Fit), Backspace (Delete) and Escape are recognised by the canvas itself (12.3), along with the arrows (nudge, with a key press as one step).

**Shortcut conflicts and what I did:**

| Model shortcut | Conflict on the Mac | Done |
|---|---|---|
| View › Fit, F (slice 3 used Command-9) | A plain key can't be a menu equivalent without taking it from text fields | F handled by the canvas; Command-9 dropped; the menu item shows no shortcut |
| Edit › Delete, Backspace | Same | The canvas handles Delete and Forward Delete; the menu item shows none |
| Group, Command-G | The Mac convention is Find Next (there is no Find menu yet) | Kept as the model has it |
| Ungroup, Shift-Command-G | Find Previous (and Go to Folder in Finder) | Kept |
| Duplicate, Command-D | None in this app (NSDocument's Duplicate isn't offered) | Kept |
| Run, Command-Enter; Cancel Run, Command-Period | Command-Period is the Mac's cancel, which suits it | Kept (shown as Command-Return) |
| Zoom In, Zoom Out, Actual Size (Command-+, Command-minus, Command-0) | Not in the model: the view's own zoom, which the core never hears | Shell items appended to View, as in slice 3 |
| Full Screen | AppKit adds Enter Full Screen to the View menu itself (Control-Command-F, and the Globe-F key) | Slice 3's own item removed (it doubled AppKit's) |

### Menu differences from the egui editor

Compared in the `selection` state of N2's 124 dumps (`claude-debug/native-n2/s1-menus-after2/`). The Mac's tree is `claude-debug/native-n3/s4-menus-mac.txt` and the side-by-side is `claude-debug/native-n3/s4-menus-diff.md` (`s4-menu-diff.py`). The Mac tree was dumped as text by the app (`--dump-menus`), each item's state taken from the model. Menus were also photographed open.

| Menu | Difference |
|---|---|
| Application menu (Mac only) | About, Settings… (Command-Comma), Services, Hide, Hide Others, Show All, Quit. egui has Settings… and Quit at the foot of File and About in Help |
| File | Adds Open Recent, Close (Command-W, with AppKit's Close All on Option) and Revert to Saved; Settings… and Quit move to the application menu. Order and labels otherwise as the model's ("New Graph", "Open…", "Save", "Save As…", "Export Bundle…", "Insert Graph…", "Insert as Group…") |
| Edit | "Undo Tidy" (the step's name) where egui shows "Undo"; Delete shows no shortcut (egui shows Backspace); AppKit adds Writing Tools, AutoFill, Start Dictation… and Emoji & Symbols at the foot |
| View | Fit shows no shortcut (F is the canvas's); adds Zoom In, Zoom Out, Actual Size, Show Watched Panel (Option-Command-I) and AppKit's Enter Full Screen |
| Node | Search… shows no shortcut (egui shows Tab, which the Mac app doesn't bind: the palette isn't built) |
| Run, Group | The same items, labels and order; shortcuts in Mac form |
| Engine | The same items. This app's documents also have the array engine, so the model adds "Remove Array · Rust · in-process (mode 1)" and its separator |
| Help | Empty but for the system's search field (the model's About is in the application menu) |
| Window (Mac only) | Minimize, Zoom, Bring All to Front, the window list |
| Shortcut notation | The model's `primary` is Command here, Ctrl in egui's dump |
| Enabled state | Differs where the documents differ, not the mapping: egui's dump state had no engine connected (Run, Reconnect, Attach and others disabled), the Mac's has two; Animate Last Run is enabled after a run; Distribute shows enabled because the dump prints submenus without their state |

Note: the evidence launches ran in the background, so the app had no key window. Items that go down the responder chain (Close, Zoom In, Undo and others) then find no responder, and the screenshots show them disabled. The text dump prints the model's state instead, which is what a key window shows. I did not see the bar with the app frontmost.

## 2. Context menus

A secondary click (`rightMouseDown`) or Control-click sends `ui.gesture {kind: "context"}`, then `menu.get {context}` with the context the gesture's reply names. The result is shown as an NSMenu at the pointer (`ContextMenu` in `GraphDocument.swift`). Items dispatch by id through the same path as the bar. A disabled item carries its reason as a tooltip. Screenshot: `s4-context-node.png` (Replace's menu: Edit Parameters…, the id label, Rename…, View ›, Keep result ›, Watch text, Duplicate, Remove, Group this node, Collapse this node into a group).

## 3. The asks

`native/apple/PlumCraftMac/App/Asks.swift` (`AskPresenter`, one per window).

**Who an ask is for.** The person's asks are shown natively; a script's are left alone. For that the Mac app needed its own `ui.invoke` to raise asks for the person. Over the C ABI every `ui.invoke` used to raise them as a script's, so the shell couldn't tell its own menu's asks from a script's.

| Kind | Native answer | Notes |
|---|---|---|
| `file.open`, `folder.pick` | NSOpenPanel as a sheet on the window | Extensions from `kinds`; `startIn` honoured; `multiple` |
| `file.save` | NSSavePanel as a sheet | `name`, `kinds`, `startIn`. Screenshot `s4-save-sheet.png` (File › Save As… through the core, `graph.saveAs`) |
| `prompt` | NSAlert as a sheet; the Mac's words: Save, Don't Save (destructive), Cancel (Escape); Trust, Cancel | `unsavedChanges`, `reviewWorkflow`, `allowUploads` |
| `text` (rename, port and net names, slice) | NSAlert sheet with a text field | |
| `form` (command forms) | NSAlert sheet with a field per form field (a secure field for `secret`) | |
| `clipboard.set`, `clipboard.get` | NSPasteboard | Answered at once, person's only |
| `open.external` | NSWorkspace: a named app (path, bundle id or name), the default app, or Reveal in Finder (`app: "Finder"`) | URLs (http, https, mailto) open in the default app |
| `screenshot` | The canvas drawn offscreen as a PNG at the path asked (`source: "offscreen"`) | Answered whoever asked (`ui.screenshot` is always a script's) |
| `share` | **Refused:** `{error: "share isn't available yet in this app"}`, with a banner | No share targets are declared, so the core offers none |
| An unknown kind | Cancelled, with a banner | |

- One modal ask is shown at a time; the rest queue.
- A sheet whose ask the core ends first (answered over the port, withdrawn) is closed.
- A modal ask from a script shows a banner, "Waiting for a script to answer: …".

**Desktop note.** On the Mac (`subprocess: true`) the session starts other apps itself and raises `open.external` asks only through the launcher hook. So Open With and similar go through this ask path whenever the core routes them that way. I did not exercise Open With by hand.

### Saving and the edited state

- **Dirty state to change count.** The feed's `document` change carries `dirty`. The document calls `updateChangeCount(.changeDone)` when it becomes dirty and `.changeCleared` when it becomes clean, so the edited dot, the close sheet and Quit's review follow the core. Screenshot `s4-edited-title.png` shows the dot.
- **Save and Save As.** `save(to:ofType:for:completionHandler:)` sends `graph.save {path}` (the same file) or `graph.saveAs {path}` (another) to the core, which writes atomically. NSDocument then records the URL and clears the dot. NSDocument's own writing is never used.
- **Revert to Saved** is `graph.open {path, discard: true}`.
- **Close over unsaved changes** shows AppKit's own sheet (Save, Don't Save, Cancel; `s4-unsaved-sheet.png`). After the decision the core gets `document.close {discard: true}`. Quit reviews each document the AppKit way.
- **Autosave in place is off** (`autosavesInPlace` and `autosavesDrafts` false). Desktop keeps explicit Save (plan 6 and decision 7). Autosave would also make the core write files behind its own dirty state.
- **One wrinkle from the core:** the session keeps a document dirty after an undo back to where it was saved. The dot stays, unlike most Mac apps. That is the core's rule, not changed here.
- **A scripted launch's TERM** (`--scratch-state`) clears the change counts and quits without asking, because nobody is there to answer. Otherwise TERM is Command-Q and asks.

## 4. Engines and running

- **Engines.** Every document starts on the text engine with the array engine beside it, through the new config field `engines.also` (both compiled in; one catalogue behind the coordinator).
- **Engine state** is a toolbar item: a coloured dot and "Engines starting…", "Engines ready" or "Engine down", with the full label (and the last error when down) as its tooltip.
  - Why a toolbar item: the Mac has no status-bar convention for document windows. Xcode and others put activity in the unified toolbar, where it costs no canvas height and stays out of the way.
- **Run, Run Selected and Cancel** are toolbar buttons, enabled from the model's `run.start` and `run.cancel` items with the reason as tooltip. The Run menu has Run (Command-Return), Cancel Run (Command-Period) and Auto-Run. Escape on the canvas cancels a run in flight, else clears the selection.
  - **Run Selected** is shell-side: `run.start {targets: <the core's selection>}`. The registry has no "run selected" command. A core command would be better (decision 4 below).
- **Run state on the canvas.** Slice 3 found the C ABI's scenes had no run animation. Now the core's document keeps it (`Document::run_anim`, pumped every turn), and egui uses the same one.
  - A C ABI scene carries `anim.on` and each face's `shown` while the run shows. After the colours fade, `on` is false and no face is shown.
  - The canvas washes each face in its state's colour at the contract's amounts. Running pulses as a Core Animation animation.
  - Once no face shows waiting or running, the canvas holds for `hold_ms` and fades over `fade_ms` itself. The core drops `on` at about that time.
  - Screenshots: `s4-light-mid-run.png` (Slow running 33% with its progress bar, the rest queued, Fail in error, Lower case skipped) and `s4-light-after-run.png`.
- **A node's error** is its footer ("op error: this node always fails") and the header's red state. The whole message is the face's tooltip on hover, and the Watched panel says it when the node is watched (`s4-light-error.png`).

## 5. Watch and three viewers

`native/apple/PlumCraftMac/App/Watched.swift`, and `Pictures.swift` in the kit's canvas target (shared with iOS).

- **The panel** is an `NSSplitViewItem(inspectorWithViewController:)` on the window's right: collapsible, 260 to 640 points wide. It follows the core's `watched` (`ui.inspect`, read again when the watched face or the watched node changes). Clicking a pin or using Watch in a context menu watches, as the core decides.
- **`picture`** (`s4-light-watched-picture.png`):
  - The rgba8 rendition comes through `media.get {want: "rgba"}`, then `plum_blob`.
  - It is turned into a premultiplied CGImage on a worker, with one copy, so no frame decodes it, and cached by handle with a 128 MB budget.
  - It sits in a magnifying scroll view (pinch, Fit, 1:1), centred, with "160 × 120 px · rgba8 · image".
  - It is the engine's rendition, which is smaller than the value. Asking for `px` from the view's size is contract 10.4's `viewer.size`, not built.
- **`text.plain`** (`s4-light-watched-text.png`): a selectable NSTextView with a Monospaced checkbox. The excerpt shows first. When the text is longer than the excerpt, Show All reads the full value's bytes (`media.get {want: "bytes"}`, decoded off the main thread). "first N of M characters" says which.
- **`scalar`** (`s4-light-watched-scalar.png`): the value, large and formatted (grouping; a whole number shows no decimals), with its media.
- **Other kinds** show "‹viewer›: no viewer yet in this app." with the summary.
- **Previews on faces:** a face's rgba8 preview is a sublayer whose contents come from the same picture loader, once per handle, faded when the scene says so (the Mandelbrot and Encode Image faces in the screenshots).

## 6. `ui.viewport`

Not added. Nothing in this slice needs the core to know the view:

- Culling uses the canvas's own transform.
- The core's fit is a rect the shell fits itself.
- Reveal is refused over the C ABI as before.
- The palette isn't drawn.

It is needed when the palette opens at the view's centre, or when `ui.inspect`'s `canvas` should say what the Mac shows.

## 7. Culling

- **What it does.** Face, wire and net-tag layers whose frames are outside the visible rect, widened by half a view each side, are hidden (`isHidden`), so the compositor skips them.
- **What is kept alive:** every layer, its picture or path, its measurements and its geometry report. A face coming into view shows at once. Faces being dragged and their wires are never hidden.
- **When it runs:** when the scene changes, and when the view has moved a quarter of itself since the last time, so most frames of a pan cost one comparison.
- `LayerCanvas.culling = false` turns it off. `PLUMCRAFT_NO_CULL=1` does that in the bench for comparisons.
- Tested: a face 20,000 units away is hidden and its wire kept; panning there shows it with the same picture.

## 8. Conformance against the app

`native/apple/PlumCraftMac/conformance.sh [read|edit|all]`.

- It builds the app and the runner.
- It launches one app on scratch everything, with its control port, on the text engine and the array engine beside it.
- It runs the cases over the port, measuring the app's own faces, and quits the app by its PID.
- A `trap` kills the app on any way out.

| Half | Run | Passed | Skipped | Log |
|---|---|---|---|---|
| read | 12 | 12 | 0 | `claude-debug/native-n3/s4-conformance-read-final.txt` |
| edit (informational) | 12 | 12 | 0 | `claude-debug/native-n3/s4-conformance-edit.txt` |

- The run, text and scalar, and picture cases all pass on that one app.
- Settles that ran out of time: 0.
- The read half took 0.64 s.
- The app quit cleanly by TERM each time.

## Budgets (release, M4 Max, 120 Hz)

`native/apple/PlumCraftMac/bench.sh run|trace N M`. Results are in `claude-debug/native-n3/s4-*bench-*.json` and `s4-*trace-*-hitches.md`. The scene is `spike.scene` (text nodes, 1.6 wires a node).

| What | 500 nodes, both engines | 500, text alone | 2,000, both engines | 2,000, text alone, culled | 2,000, text alone, not culled | Slice 3 (500; 2,000) | Budget |
|---|---|---|---|---|---|---|---|
| Launch to first frame | 0.78 s | 0.28 s | 1.20 s | 0.91 s | 0.90 s | 0.50; 0.91 s | under 1 s |
| `document.open` to first frame | 435 ms | 167 ms | 1,075 ms | 790 ms | 777 ms | 169; 793 | |
| The core's `spike.scene` build | 284 ms | 23 ms | 548 ms | 276 ms | 270 ms | 22; 282 | |
| One-node change on its layer (kit), p50 | 6.2 ms | 6.0 | 35.5 | 35.8 | 31.2 | 5.9; 35.7 | 16 ms |
| The same through the feed, p50 | 18.2 ms | 17.7 | 36.1 | 36.9 | 30.7 | 18.4; 34.1 | 16 ms |
| Footprint after the run | 410 MB | 387 | 763 | 803 | 801 | 427; 811 | |
| Layers hidden while panning | 120 | 120 | 2,465 | 2,471 | 0 | none | |

**Reading the first rows.** Opening every document on two engines costs the core's first big edit about 260 ms here: `spike.scene` takes 284 ms against 23 ms with the text engine alone. The launch budget is therefore met with the text engine (0.28 s at 500) and missed with both (0.78 s at 500 is under the 1 s budget but slower; 1.2 s at 2,000 is over it). I did not profile why a two-engine catalogue slows that edit; the likely place is per-node lookups over a 111-kind catalogue. That cost is in the core, and decision 1 below is whether to keep the array engine in every document.

**Hitches (Instruments' Animation Hitches, ms of late frames a second):**

| Run | pan | pan at fit | zoom | drag |
|---|---|---|---|---|
| 500, culled, run 1 | 158.5 | 410.6 | 72.2 | 10.4 |
| 500, culled, run 2 | 31.2 | 48.0 | 27.8 | 4.2 |
| 2,000, culled | 415.8 | 285.7 | 544.2 | 16.7 |
| 2,000, not culled | 368.4 | 31.3 | 402.9 | 75.0 |
| Slice 3, 500, best of four | 20.8 | 4.2 | 28.5 | 6.3 |
| Slice 3, 2,000 | 371.4 | 594.0 | 328.5 | 20.9 |

**Culling did not measurably help at 2,000 nodes, and the machine's variance is as large as any effect.** Two runs at 500 differed up to eightfold (pan at fit: 410.6 against 48.0), as in slice 3. The "pan at fit" phase has everything on screen, so culling hides nothing there, yet it went from 285.7 culled to 31.3 not culled: that is noise. The budget (under 5 good, over 10 poor) is again neither shown met nor shown missed at 500. At 2,000 the canvas still bends.

What I'd look at next, untested:

- toggling `isHidden` on thousands of layers when the view jumps (a zoom out un-hides 2,000 at once);
- a simpler zoomed-out face, which 15.2 also named.

## Every Rust, C ABI, kit and contract change

**Rust:**

- `crates/editor-core/src/document.rs`: `Document::run_anim` (the run animation, now the document's), `Document::pump_run_anim` (called from `document::turn`), and `Document::shown_anim`. `Shell::scene` for the document (the C ABI's) passes the animation while it shows. `Document::invoke_by` and `Shell::invoke_by`.
- `crates/editor-core/src/run_anim.rs`: `plan(session, needed)`, moved from `crates/ui/src/lib.rs` (`run_anim_plan`).
- `crates/editor-core/src/control.rs`: `Shell::invoke_by()` (default `By::Script`); `ui.invoke` raises its asks for it.
- `crates/ui/src/lib.rs`, `crates/ui/src/control.rs`: the egui editor's own `run_anim` field is gone; it uses the document's (`self.doc.run_anim`), and its pump calls `Document::pump_run_anim`. `kittest_run_anim` (4 tests) passes unchanged.
- `crates/ffi/src/core.rs`:
  - `Config::also` (`engines.also`), checked against the platform, started beside the first engine in every document;
  - the feed's `document` domain (`note_document`, each turn, coalesced);
  - `ui.invoke {by}` taken off a `plum_request` before the dispatcher.
- New test file `crates/ffi/tests/run_watch.rs` (4 tests): the document domain (title, dirty, undo, redo, path after a save); `engines.also` (kinds of both engines; hostile `also` refused); the run animation in the scene while it shows and gone once faded, and never with Colour Nodes While Running off; `by: "person"` asks, `by: "script"`, hostile `by`, and the dispatcher refusing `by`.

**C ABI and control-port behaviour changes** (no export, struct or `PLUM_ABI_VERSION` change):

1. A C ABI document's scene carries the run animation while it shows (`anim.on`, faces' `shown`). Before, `on` was always false.
2. `plum_poll`'s `changed` carries `{doc, domain: "document", gen, title, document, path, dirty, undo, redo, lastError}` once a document is ready and whenever any of them changes.
3. `plum_request`'s `ui.invoke` takes `by: "person" | "script"`. The control channel's `ui.invoke` still refuses the field, which is unchanged behaviour.
4. The config takes `engines.also`.
5. The egui editor's run animation is the core's document's: same behaviour, and the same tests pass.

**Kit (`native/apple/PlumCraftFFI`):**

- Models: `Changed` gains `title`, `document`, `path`, `dirty`, `undo`, `redo`, `lastError`; new `DocumentState`.
- `DocumentModel`:
  - `state`, plus callbacks `onState`, `onMenus`, `onAsks`, `onWatched`, `onEngine`;
  - `activate(_:person:)` and `invoke(_:person:)`;
  - `EngineState` gains a public initializer.
- Canvas:
  - `Pictures` (new): rgba8 to CGImage on a worker, cached by handle.
  - `LayerCanvas`: `pictures`, previews on faces, the wash's hold and fade, culling (`culling`, `cull(force:)`, `hiddenCount`), `washed(nid:)`.
  - `CanvasView` (AppKit): context menus (`onContextMenu`), canvas keys (`onCanvasKey`: Delete, Escape, F, the arrows), the standard Edit actions with `validate`, and the error tooltip.
- Tests:
  - `DocumentChanges` (models) and `PictureBytes` (pictures);
  - `CullAndRun`, two tests on a core (culling, and the wash coming and going), 10 of 10 on repeat;
  - every test target now sets `COMMONKIT_STORE_ROOT`, `COMMONKIT_MARK_DIR`, `COMMONKIT_TEST_RUN` and `PLUMCRAFT_CRAFT_ROOT` to scratch, whatever the environment says (finding below).

**Mac app (`native/apple/PlumCraftMac`):**

- `MainMenu.swift` was rewritten (`MenuBar`), and `GraphDocument.swift` was rewritten (NSDocument saving, the split view, the toolbar, context menus, the banner).
- New files: `Asks.swift`, `Watched.swift`, `Script.swift`.
- `AppMain.swift`:
  - options `--engines-also`, `--then`, `--dump-menus`, `--hide-watched`;
  - `--scratch-state` refuses to start unless `COMMONKIT_STORE_ROOT` and `COMMONKIT_MARK_DIR` name somewhere other than the Mac's own store;
  - `NSTreatUnknownArgumentsAsOpen` is off;
  - scripted launches log errors instead of showing alerts;
  - TERM in a scripted launch quits without the unsaved review;
  - every copy writes its pid to `<status>.pids`.
- `conformance.sh`: one app with both engines; scratch environment including `COMMONKIT_TEST_RUN` and `PLUMCRAFT_CRAFT_ROOT`; a `trap` kill.
- `bench.sh`: scratch environment, a `trap` kill by the pids the app wrote, `s4-` names (`PREFIX`), `EXTRA` app arguments. It also clears copies Instruments leaves stopped: processes named exactly PlumCraft that weren't there before the trace and run this build's binary.

**Contract (`docs/shell-interface.md`):** 4.1 (`engines.also`, as built), 7.1 (the `document` domain, as built), 8.6 (the run animation over the C ABI, as built, and that `endedAt` isn't sent), 12.1 (`ui.invoke {by}`), 12.2 (the Mac's menu mapping). `docs/control-protocol.md` and `schema/shell1.json` don't change: no control-port method or schema type changed. Ledger: note 24 section 18 gains slice 4, and `docs/STATUS.md` an entry.

## Tests

| Suite | Result |
|---|---|
| `cargo fmt --all --check` | Clean |
| `cargo clippy --all-targets --all-features -- -D warnings` | Clean (`claude-debug/native-n3/s4-gate-clippy.log`) |
| `cargo nextest run --all-features --no-fail-fast` (the gate's environment, b3) | **1,466 of 1,467 passed**, 16 skipped; the one failure is the known local-only `plumcraft-craft::params_coverage hidden_params_match_the_baseline`. 1,463 before plus 4 new (`s4-gate-nextest.log`) |
| `scripts/check-hash-gates.sh`, `scripts/check-core-toolkit-free.sh` | ok, ok |
| `native/apple/build-xcframework.sh` | Built (all slices) |
| `swift build` and `swift test` (scratch environment) | Built with zero warnings; 55 tests run in four runners, all passed, the opt-in measurements skipped as before (`s4-swift-test.log`) |
| The Mac app, clean release build (`rm -rf build`) | Zero warnings (warnings are errors) (`s4-mac-clean-build.log`) |
| The app launches, opens, runs, watches, saves and quits | Yes, through `s4-drive.py` (light and dark); Save As… raised the save sheet and was cancelled over the port; the app's Save writes through the core (Rust test `run_watch` covers `graph.saveAs` and the path); every launch quit cleanly by TERM |
| `PlumCraftCanvas` for iOS device and simulator | Built, zero warnings |
| `conformance.sh` (read, final build) | 12 of 12, no skips |

**Not run in the app:** a Save that writes a file, done by a person in the save panel. I cancelled the panel each time rather than write outside scratch. The save path from NSDocument into the core was not exercised end to end: the panel's OK, then `graph.saveAs`, then the dot clearing.

## The maintainer's store, preferences and trusted files

- **Preferences and trusted files: unchanged.** SHA-256 of `prefs.json` and `trusted.json` for `plumcraft-editor`, the research software's app and a private batch app are the same before and after (`claude-debug/native-n3/s4-prefs-before.txt`, `s4-prefs-after.txt`).
- **The store (`/Volumes/<store>/store`): changed during the slice, and I believe not by me.**
  - Its listing differs (`s4-store-before.txt`, `s4-store-after.txt`): 91 files were written between 12:04 and 12:08. They are 10 MB `.npy` arrays under the keyspaces `vimparr`, `vimptup2` and `vimptup3`, plus `.usage.json`.
  - Those keyspaces are the research software's. PlumCraft's engines write `plumarr` and `plummemo` (checked in my scratch stores), and no `plum*` keyspace in the real store changed.
  - My arrays were at most 320 × 240.
  - Every launch, test and script I ran set `COMMONKIT_STORE_ROOT` and `COMMONKIT_MARK_DIR` to scratch, and the app now refuses a scripted launch otherwise.
  - Two Python processes were running on the machine. I can't prove the writer; please check what ran on the research software at 12:04.
- **A hazard found and fixed:** the login shell here exports `COMMONKIT_STORE_ROOT=/Volumes/<store>/store`, which wins over a config's `store_root`. Before this slice the kit's `swift test` set only preferences and passed `store_root`, so **earlier slices' `swift test` runs most likely used the maintainer's real store**. Every test target now overwrites the store, marks, test-run and craft-root variables with scratch paths.

## Final `pgrep`

```
$ pgrep -lx PlumCraft
$ pgrep -lx CanvasSpike
```

Both empty.

## Screenshots

All are under `claude-debug/native-n3/s4-shots/`, of the app's own windows (`screencapture -l`), on scratch preferences and store. The graph is `claude-debug/native-n3/s4-showcase.json` (the text engine's demo plus Mandelbrot and Encode Image), tidied and fitted.

| File | What |
|---|---|
| `s4-menu-file.png`, `s4-menu-edit.png`, `s4-menu-view.png`, `s4-menu-run.png` | The bar's menus open. Edit shows "Undo Tidy" disabled for the background-launch reason above |
| `s4-context-node.png` | A node's context menu (Replace) |
| `s4-light-before-run.png`, `s4-light-mid-run.png`, `s4-light-after-run.png` | Before, during (Slow running 33%, washes) and after a run |
| `s4-light-error.png` | The failing node watched: its error on the face and in the panel |
| `s4-light-watched-picture.png`, `s4-light-watched-text.png`, `s4-light-watched-scalar.png` | The three viewers |
| `s4-save-sheet.png` | File › Save As… through the core: the save sheet on the window |
| `s4-edited-title.png`, `s4-unsaved-sheet.png` | The edited dot; Close over unsaved changes |
| `s4-dark-*.png` | The same run and viewers in dark mode |

## Defects I introduced and caught

1. **A modal error dialog on the screen from an automated run, twice.** AppKit treated the words of `--engine text` and `--engines-also array` (in some orders) as files to open, and raised "could not be opened" as a modal alert. That stalled the launch, and TERM couldn't quit it. It happened in a `conformance.sh` run and in two probes, each up to about 60 s before the script killed the app by its PID. Caught by a `sample` of the hung app. Fixed: `NSTreatUnknownArgumentsAsOpen` is NO, and scripted launches log errors instead of presenting them. This is the same class as slice 3's `YES` defect.
2. **Menus held open on the screen for up to 20 s.** My first script waited with tasks inside a tracking menu, where neither tasks nor the main queue run, so the menu never closed until the app was killed. Fixed with run-loop timers in the common modes.
3. **Three stopped app copies left by Instruments**, for up to 7 minutes. `bench.sh`'s cleanup looked for the status file's pid, but the tracer's copies were stopped before they ran a line. They were killed by PID once found. `bench.sh` now clears them as described above, verified by the next trace.
4. **The menu bar stayed the no-document bar** in a launch with no key window. It now follows the first document too.
5. **Scripted TERM hung on the unsaved-changes review.** It now discards in scripted launches.
6. **Menu holds that were too short, and a step-index off by one, in the evidence script.** Caught by its time limits.
7. **The picture sat at the panel's bottom-left.** It is now centred with a centring clip view. Caught by a screenshot.
8. **The menu dump repeated AppKit's hidden alternates** (Dictation and Emoji three times), and my own Enter Full Screen doubled AppKit's. Hidden items are skipped in the dump, and my item was removed.
9. **A test assumption that undo back to the saved state clears `dirty`.** It doesn't, by the core's rule; the test was corrected.

## Decisions for the maintainer (look and behaviour)

1. **Both engines in every document** (`--engines-also array` by default). It needs no skips and lets a graph mix text and arrays. The cost is about 260 ms more on the first big edit (launch to first frame 0.78 s at 500 nodes against 0.28 s) and 1.2 s at 2,000. Alternatives: the text engine alone, with the array engine started when a graph names it (`engine.auto`), or fixing the two-engine edit cost in the core.
2. **Undo and Redo driven from the model, not `UndoManager`** (reasons above).
3. **Engine state as a toolbar item** with a dot.
4. **Run Selected is shell-side**, as `run.start {targets: selection}`. A core command (`run.selected`) would put it in the menu model for every shell.
5. **Items with no UI yet don't run**; they say so in a banner. The alternative is to run them and leave the core holding window state nothing shows.
6. **Plain-key shortcuts aren't menu key equivalents** (F, Backspace, Escape). The menu doesn't show F for Fit.
7. **The wash is a sublayer over the face's picture**, so it also tints the text a little. egui mixes the colour into the body only. A blend mode is possible on the Mac only.
8. **The panel shows the engine's rendition** (160 × 120 for a 320 × 240 value), not a picture sized to the view (`viewer.size`, not built).
9. **Undo back to the saved state keeps the dot** (the core's dirty rule).
10. **The Watched panel starts open** on every window.

## What slice 5 (the iPad app, device budgets, the N3 gate) should know

- `Pictures`, culling, the wash's fade and previews are in `PlumCraftCanvas` and compile for iOS. The menu bar, asks, panel and toolbar are AppKit, in the Mac app. The iPad needs its own `UIMenuBuilder` from `menu.get`, `UIDocumentPickerViewController` for the file asks, alerts, and a sheet or inspector for the Watched panel. The model state they need (`state`, `onAsks`, `onMenus`, `onWatched`, `onEngine`) is in `DocumentModel`.
- A shell's own `ui.invoke` must say `by: "person"`, or its asks come back as a script's.
- The C ABI scene's run animation needs nothing more on iOS: the wash and fade are in `LayerCanvas`.
- `conformance.sh`'s pattern (one app, both engines, a `trap`, status-file waits) carries to a simulator run. On a device the control port needs decision 10's local-network binding.
- The two-engine first-edit cost and the 2,000-node hitches are the open budget items. 500 nodes is where the gate reads.
- Run `swift test` and any app only with the scratch environment. The login shell names the real store.

## What I did not verify

- **Hands at the Mac:**
  - the menu bar with the app frontmost (key equivalents, validation of the responder-chain items);
  - a real right click and Control-click (only the script's equivalent ran);
  - the save panel's OK writing a file and the dot clearing;
  - Revert to Saved;
  - the Open panel from File › Open…;
  - Show All in the text viewer on a long text;
  - pinch zoom in the picture viewer;
  - the panel's collapse by the toolbar button with a person driving.
- **Open With and Reveal in Finder** through `open.external`: built, not exercised.
- **`form` and `text` sheets:** built; no command in this app's reach raised one in my runs, because rename items need text asks the core raises from context menus I did not choose.
- **The hitch budget on a quiet machine.**
- **Linux:** the Rust changes ran on the Mac only; the controller runs them on the Linux test host.
- **Who wrote the research software's files into the real store at 12:04 to 12:08.**
