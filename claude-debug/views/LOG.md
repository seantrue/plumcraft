# Track 3 overnight log (note 26: data views)

Worktree the views worktree (`.wt/plumcraft-views`), own cargo target `target/` in it, no debug info. Branches stacked from `origin/feat/native-n0` at `a0ec42b`.

## Step 0: review and plan (done)

- Note 26 sections 11 (capability review: every viewer, egui against Mac, iPad and Windows, with tables as tables and the interchange formats) and 12 (the revised slices and the eight assumed decisions). PDF regenerated (`docs/research/26-data-views.pdf`, untracked).
- Correction after building: the iPad's table cells already select their text (`textSelection`), so only Windows lacks cell copy.

## B161 (commit `1cf13dc` on `feat/views-s2`)

- The viewer bar is the widest of three arrangements that fits (`ViewThatFits`): one row; choosers and stepper over the actions; three rows.
- **The item stepper's code was moved** (not changed) into its own `itemStepper` part of `ViewerBar` in `PlumCraftShell/Viewers.swift`. The nested-lists agent changes the stepper: a merge there should take their stepper body into `itemStepper`.
- UI test `testTheViewerBarFitsANarrowWindow` on my own iPhone 17 Pro simulator: passed. Picture `claude-debug/views/b161-phone/b161-ui-bar.png` (looked at: two rows, everything inside the window).

## Slice 2 (in progress, `feat/views-s2`)

- Core: `viewer.grid {colored, format}`, `viewer.table {colored}` (state in `popout.get`); `viewer.data` grid page in the format with each cell's tint over the plane's range and the range's ends; table page tints numeric columns over each column's range. `format_value` moved from egui's grid to the core (egui re-exports it).
- Core: a pop-out's stamp now moves when a zoom is asked (`zoom_seq`): found when the shared-zoom tiles never heard a script's `viewer.zoom`. B159 worked only because a zoom past fit also asks for a rerun with detail. Conformance B159 case now checks the stamp.
- Apple: Colour by Value and Format controls over the grid and the table, tints on Mac `Table`, iOS `PageGrid` (grid and list). Several pictures share one zoom and view (`SharedZoomPictures`, `ZoomTile`): pinch and drag on any tile, Fit, 1:1, a tap reads and pins the probe, zoom asks taken.
- Tests so far: editor-core viewers unit test `colour_by_value_and_the_number_format`; session `colour_and_format_are_commands`; conformance "an array's numbers and a table's are tinted by value and formatted through the core" (C ABI array cases pass; Mac app 70 of 70, 8 skipped: the test engine program isn't built in my target); `swift test` 124 passed; workspace clippy clean; touched crates 932 passed, 3 failed: the two known engine-binary tests and `plum_open_returns_before_the_engine_and_holds_what_comes_meanwhile` (it starts the text engine as a process; the program isn't built in my fresh target), same cause.
- Mac launch 1 (`claude-debug/views/s2-mac/pictures-fit.png`): two pictures at fit 62.5%; the 200% ask was not heard (the stamp fault above). Rebuilding.
- Also found and fixed: a table-flagged array (the Histogram node's) was offered no Table viewer by the core (B152 covered TABLE values only), so View As was blank when it showed as a table; now Table is offered first for it (unit test `a_table_flagged_array_is_a_table_first`).
- Also fixed: the Colour by Value toggle showed no on state (the row's borderless button style overrode the toggle's look).
- Final launches of slice 2:
  - Mac scripted copy, control port only (`claude-debug/views/mac-launch.py`): two channel pictures at fit 62.5%, then `viewer.zoom {to: 4, at: [64, 96]}`: both at 200% centred (0.2513, 0.252) (`s2-mac-2/pictures-200.png`); grid at `%.3f`, colour, column window 128 (`s2-mac-3/grid-colour-format.png`); the histogram table, Table in View As, counts tinted (`s2-mac-2/table-colour.png`). Library unchanged each run. Looked at each.
  - iPad simulator (own, `views-ipad`): UI test `testSliceTwoViews` passed (`s2-pad/views-s2-ui-pictures-200.png`, `s2-pad/views-s2-ui-table-colour.png`); conformance read half 39 of 39, 1 skipped.
  - iPhone simulator (own, `views-phone`): `testSliceTwoViews` passed (`s2-phone/`); a first run exited 148 at once (a stop signal to the backgrounded job, not the test); rerun with stdin closed passed.
- Not built in slice 2: Compare's exact difference (slice 2b). Not checked: the Colour toggle's on look in an offscreen Mac picture (it looks the same on and off there); Windows (no C# yet for slice 2).

### The stamp fault in the merged B159 (the coordinator asked which test should have caught it)

B159 put `zoomAsk` in `popout.get` but left `zoom_seq` out of the pop-out's `stamp`, which is what tells a shell to read the pop-out again. Two merged tests could have caught it and didn't:

- **Conformance "a zoom asked of a pop-out reaches its shell once, with where to centre"** read `popout.get` directly after each `viewer.zoom`, so it saw `zoomAsk` whether or not a shell would ever have asked. It should have checked that the scene's stamp for the pop-out moved (now it checks `popout.get`'s `stamp` against the one saved before the ask).
- **iPad UI test `testItemsKeepTheZoomAndView`** and the Mac evidence zoomed a single picture to 200%, which is past its fit, so the core also asked for a rerun with more detail (`viewer.zoom` with a pixel scale sets `detail`); the rerun moved the stamp, and the shell heard the ask by that side effect. A zoom that asks for no detail (`in`, `out`, a centre alone, or two pictures as slice 2 has) was never heard. Found when slice 2's shared-zoom tiles didn't follow a script's `viewer.zoom`.

## Slice 2b (branch `feat/views-s2b`)

- Engine programs built once in my own target (`cargo build --bins`, and the test engine `plumcraft-ports` in release) so the engine-binary tests and the Mac's test-engine cases run.
- Slice 2b committed as `5476be1` on `feat/views-s2b`: the core computes Compare's difference (`compare.rs`) and puts its pictures in the media service (`MediaService::put`); Apple Compare has Difference. Mac conformance 79 of 79, 0 skipped (engine programs built); iPad conformance 41 of 41, 1 skipped; iPad UI test `testCompareDifference` passed; Mac launch `claude-debug/views/s2b-mac/compare-difference.png`, iPad `s2b-pad/views-s2b-ui-difference.png` (looked at both). The conformance case pins A before opening B: on the iPad a pop-out opening replaces the one before. iPhone run of `testCompareDifference`: not completed (the chained background job got a stop signal, 148, twice); not checked on the iPhone.
- Whole workspace (after `cargo clean --profile dev`, `CARGO_INCREMENTAL=0`): 1714 passed, 3 failed: `hidden_params_match_the_baseline` (the known baseline) and the two `hash_is_what_was_asked`, which refuse to run without the harness's `COMMONKIT_TEST_RUN` and a store root inside it; with those set both pass.

## Slice 4 (branch `feat/views-s4`)

- `plumcraft_proto::csv`: the text engine's CSV reader moved into the protocol crate (text engine wraps it), with limits (1,000,000 rows, 4,096 fields a row); in the hostile sweep (`csv_text_never_panics`, and every junk text through `read_csv`).
- `plumcraft-media`: a value typed `text/csv` is read as a table by that reader (`Table::from_csv`).
- Core registry: `text/csv` offered Table then Text; a TEXT starting with an `<svg` root offered SVG then Source. `popout.get` gives SVG text a `handle` of its own (the text inline, typed `image/svg+xml`, its picture started at 2048) and `svgSource: true`. Found on the Mac launch: without that handle the shells drew "no picture" (an inline TEXT has no full value).
- Tests: editor-core `interchange_formats_are_offered_as_what_they_are`, `a_csv_value_pages_as_a_table`; conformance "a CSV value is a table a viewer away from its text, and SVG text a picture" (C ABI with the SVG picture drawn through `media.get`, egui, Mac 80 of 80 with none skipped, iPad 41 of 41 with 1 skipped); touched crates 948 passed, 1 failed (`hash_is_what_was_asked`, the harness environment).
- Mac launch (`claude-debug/views/s4-mac-2/`): the CSV as a table and as text, the SVG text as a picture and as source; looked at each. My first launch script reused a name for a pop-out id, so its cleanup could not quit the app: I stopped that scripted copy by its PID, cleaned it, and the Library compare said unchanged.
- Assumption 3 refined: CSV is never sniffed; SVG text is recognised by its `<svg` root, since a TEXT can't be typed.
- Not checked: SVG text past the excerpt (the shell's Source still shows it; no picture); the egui editor doesn't offer these views (core additions).
- Slice 4 committed as `1de5fd5` on `feat/views-s4`.

## Slice 3 (branch `feat/views-s3`)

- iPad and iPhone table grid: a drag on a column's header edge sets its width (shell state, 13.2.1); adjustable by VoiceOver in 24-point steps. UI test `testATableColumnIsWidenedByADrag` passed on my iPad simulator (`claude-debug/views/s3-pad/views-s3-ui-column-widened.png`, looked at: the centre column widened, the count column kept).
- Windows: a table cell's text selects and copies (`IsTextSelectionEnabled`). Built on the Windows VM from a copy of `native/windows` in a folder of my own (`C:\work\views-s3`; the existing checkout untouched): `dotnet build` of PlumCraft.App, ARM64, 0 warnings, 0 errors. Built, not launched (no connected desktop overnight). Windows column widths by drag: not done.
