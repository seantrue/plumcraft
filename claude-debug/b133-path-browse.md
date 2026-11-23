# B133: a path input's Browse, the core's, on every shell

*2026-11-23, branch `fix/b133-path-browse` from `origin/feat/native-n0` (2c3de06), uncommitted. No device. Windows adds its half on this branch.*

## Answer first

A path row now has a Browse button at the right end of its field, the same in every shell, and a tap on it opens the platform's own file dialog through the core. The Mac answers with its open and save panels (the real path). The iPad and iPhone copy the picked file into the app's Documents › Picked Files and name the copy. A folder is opened in place, as before. A may-be-new path uses the existing save alert. F16 is now met on the Mac and on the iPad.

The maintainer: "Yes, fix b133 on all systems." The copy-or-bookmark question was not answered separately. Copying is built and stays an open decision.

## The core (shared, for Windows too)

- **Where the button is:** `geometry::browse_rect(widget rect)`, the square at the widget rect's right end. Its side is the rect's height, at most half its width. A shell draws it on a row whose widget has `pathMode` and can be edited. It measures nothing new: the rect is the row's widget rect it already reports.
- **The gesture:** a tap inside that square is `selection.set` then `row.browse {node, port}`. A tap elsewhere on the field is `row.edit` as before.
- **The command `row.browse {node, port}`:**
  - It asks first, so the core invokes it from a gesture, `ui.invoke` and `command.run` alike (`control::asks_first`, the same rule as Save Result…'s `run.save {ask: true}`). Run any other way it is refused.
  - It closes any open field editor without its text.
  - It raises the existing path ask (`invoke::browse`), by the hint's mode:
    - a file the node reads: `file.open`
    - a file it writes: `file.save`
    - a folder: `folder.pick`
    - several files: `file.open {multiple}`
  - The ask carries purpose `path`, the hint's filters as `kinds`, the hint itself and the current value.
  - The answer is set with `node.set`: one undo step, "Set Value". Cancelled, nothing changes.
- **Locating it:** `geometry.locate {node, row, part: "browse"}` gives the button's middle.
- **egui:** keeps its own "…" button, which already called the same `invoke::browse`.
- **Contract:** `docs/shell-interface.md` covers the widget's Browse button, `row.browse` and the asks' purpose `path` (11.2). `docs/control-protocol.md` covers `row.browse` and the locate part.

## The shells

- **Mac:** the open or save panel as before; no Mac change was needed beyond drawing the button.
- **Drawing (shared Apple renderer):** the path field shows its text, a rule, and an accent "…" in the square. The field editor on a path row stops short of the square (`EditorPlacement`), so Browse can be tapped while the text is edited.
- **iPad and iPhone:** the document picker opens with `asCopy: true` for a path's file (`PickedFiles.keep`):
  - The copy goes into `Documents/Picked Files`, which the Files app shows as On My iPad › Plummer › Picked Files.
  - It keeps its own name, so the row shows a name the person recognises.
  - The same bytes already there are that file again; no second copy is made.
  - Another file of the same name is numbered, as Finder does: "notes 2.txt".
  - Earlier copies are left alone, since another graph may name them.
  - The core stores the copy's path relative to the workflow's folder when the workflow is in Documents. Otherwise the path is absolute; an app reinstall moves the container, so such a path would break (noted).
  - A folder (`dir`) is the picker in place, with access for the graph's life: it doesn't last across a relaunch (unchanged).
  - A may-be-new path (`save`) is the existing alert naming a file in the graph's folder, or in Documents.
  - The picker's state is in the app's status (`picking`), for tests.

## The keyboard's path (B129's area)

Browse while the path's text is being edited closes the field without its text, and the picker or panel comes. This was checked on the Mac (person check), on the iPad and on the iPhone (UI test), and in the conformance case (`view.edit` gone).

## Tests

- **Rust:** the conformance case "ports: a path input's Browse asks for a file by its hints, and the answer sets the value" passes on the core's own document, the C ABI and egui. Also new: `geometry::tests::a_path_rows_browse_button_is_the_square_at_its_right_end`.
- **Swift:** `PickedFileCopies` (by name, the same bytes reused, numbered, a missing source refused) and `EditorPlacement` (a path's field stops short of Browse).
- **iPad and iPhone simulators:** `testBrowseForAPath`, with the test engine over HTTP:
  - the button is pictured; a tap opens the picker for a copy, which is pictured and closed with its X;
  - Browse while the field is edited closes the field and the picker comes;
  - the iPhone is an iPhone 17 Pro at one step larger text;
  - nothing is picked: the simulator's picker has no file a test placed, so the copy itself is unit-tested.
- **Mac person check:** the open panel from the row's Browse, its folder read through accessibility and cancelled (never pressed); then Browse while editing; then the scripted answer names a scratch `.txt`, in one undo step, and a run reads its text.

## Gate

| Check | Expected | Took | Result |
|---|---|---|---|
| Rust gate: fmt, clippy, schema, hash and core gates, pytest, ruff, private strings | 10 min | 157 s | clean; nextest 1,647 of 1,648, the known `hidden_params_match_the_baseline` |
| swift test | 1 min | 34 s | passed |
| Mac gate twice, the second under 10 busy loops | 10 min | 600 s | 67 of 67 cases each, the new one included; person check 58 passed, 0 failed, 9 only seen. The screen was locked, so the panel's folder wasn't read through accessibility; the other B133 steps passed |
| iPad standard: device build, whole suite, UI tests | 5 min | 291 s | build passed; 59 of 59; UI 7 passed, 2 skipped (the test-engine tests) |
| iPad whole suite with the test engine | 1 min | 62 s, then 52 s | first 65 of 66: the new case expected an absolute path and the iPad stored it relative to `/` (B137, filed, not fixed; the case now checks the path's end). Then 66 of 66 |
| `testBrowseForAPath`, iPad and iPhone simulators | 2 min each | about 2 min each | passed on both after the picker's X was found (it is another process's view, tapped where it is drawn) |

Home listings around both Mac gates were unchanged. B137 was found on the way: on iOS a chosen path for a graph the core holds no file for is stored relative to `/`.

## Pictures

| What | Where |
|---|---|
| iPad: the path row with its Browse button | `claude-debug/n4-decided-2/shots-br7/b133-ui-path-row.png` |
| iPad: the picker | `claude-debug/n4-decided-2/shots-br4/b133-ui-picker.png` |
| iPhone: the row, editing, Browse beside the field | `claude-debug/n4-decided-2/shots-br9/b133-ui-editing.png` |
| iPhone: the picker | `claude-debug/n4-decided-2/shots-br8/b133-ui-picker.png` |
| Mac: the open panel from Browse | not taken: the screen was locked in both runs |

## What the Windows agent must do

1. **Draw the button.** On a row whose widget has `pathMode` (`open`, `save`, `dir`, `files`) and can be edited, draw a Browse button in the square at the right end of the widget's rect. Its side is the rect's height, at most half the rect's width (`geometry::browse_rect`). Use the platform's "…" or a folder glyph, with the path's text before it. Nothing new to measure or report: the row's widget rect is already in `geometry.set`'s `rows`.
2. **No tap handling of its own.** The tap goes through `ui.gesture` (`plum_gesture`) like every canvas tap. The core replies with `commands`: `selection.set`, then `row.browse` (`invoked`), and raises the ask.
3. **Answer the asks with purpose `path`.** The params are `{purpose: "path", title, kinds: [{label, extensions}], multiple, name?, hint: {mode, filters}, current, startIn}`.
   - `file.open`: FileOpenPicker with the kinds as filters (`multiple` → PickMultipleFilesAsync). Answer `{paths: [full paths]}`.
   - `file.save`: FileSavePicker, the name suggested. Answer `{path}`.
   - `folder.pick`: FolderPicker. Answer `{path}`.
   - Cancel answers with cancel.
   - Windows hands back real paths (`StorageFile.Path`), so no copy is needed.
4. **The field editor** on a path row stops short of the button (width minus the square), so Browse can be tapped while the text is edited. The core closes the editor when Browse is tapped.
5. **Person check (`person-check.ps1`):** add the test engine's Read Text File, click its Browse, see the open picker and cancel it (never press Open). Then click Browse again with the field open, answer the ask by script with a scratch `.txt`, check the value is set in one undo step, and check that a run reads it.
6. **Conformance:** the new case runs on the Windows driver where it measures rows (`only: "rows"`); nothing else changes.

## Brought up to the dev branch

The branch was brought up to `ede5705` (#175 app engines, #176 pack views) with no commit: a fast-forward, then the work put back with a three-way apply. Only `docs/STATUS.md` conflicted, and both entries are kept. This branch's bug was renumbered from B136 (taken in #176) to **B137**. The closing round after it (`claude-debug/n4-decided-2/b133c.sh`) took 21 minutes:

| Check | Took | Result |
|---|---|---|
| xcframework (macOS, iOS, simulator) | 41 s | built |
| Rust gate | 185 s | all clean except the known `hidden_params_match_the_baseline` (1649 of 1650) |
| swift test | 40 s | passed |
| Mac gate, twice (the second under load) | 627 s | 67 of 67 cases each; person check 58 passed, 0 failed (screen locked, so accessibility was only seen) |
| iPad standard: device build, whole suite, UI tests | 298 s | 59 of 59; UI 7 passed, 2 skipped; home listing unchanged |
| iPad whole suite with the test engine | 54 s | 66 of 66 |
| `testBrowseForAPath` on the iPhone, one step larger text | 34 s | passed |

## Open decisions

1. On iPadOS, keep copying the picked file into Picked Files (built), or keep a security-scoped bookmark to the original?
2. A copy's absolute path breaks if the app is reinstalled (the container moves) when the workflow isn't in the app's Documents. Store it relative to the Picked Files folder instead?

## Next step: Photos as well as Files (asked 2026-11-23, not built)

The controller asked for this on behalf of an application built on this shell. When the path's kinds are pictures, Browse should offer **Photos as well as Files** on the iPad and the iPhone. The picked picture would be copied into the app's container, the port's filter honoured, and the copy's path written into the node. It is not in what I built, and it is not small enough to add here:

- **The shell:** when the ask's `kinds` are all pictures, the iPad or iPhone answers the core's `file.open` (purpose `path`) with a choice of Photos or Files. Photos is a `PHPickerViewController` filtered to images (no photo-library permission is needed). Its item is loaded as a file (`loadFileRepresentation` for the filter's type, or `public.image`) and kept through `PickedFiles.keep`, the same naming and reuse as a Files pick. A photo has no name of its own, so it would be named from its suggested name or its date, plus the extension of its type. The answer is the copy's path, set as typing sets it.
- **The choice:** it is part of answering one ask, as the document picker's own navigation is, so it adds no core command. If the maintainer reads it as a separate user action (the command rule), the core's ask would carry a `sources` hint instead.
- **Tests:** a UI test on the simulator, whose photo library has its sample pictures (the picker is a remote view like the document picker's, so it is closed or chosen from at known places). A unit test for the photo's naming.
- **Cost:** about a day with the tests. The Mac needs nothing: its open panel already shows Photos in its sidebar.

## Unverified

- No device. Nothing was picked through the simulator's picker (the copy is unit-tested only).
- The Windows half isn't built.
