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
- Slice 3 committed as `8cad79c` on `feat/views-s3`.

## Slice 5 (branch `feat/views-s5`): the plot on the chart engine

- Core `chart.rs`: what a table array plots as (points, segments, a histogram's columns, any (N, 2)), nice ticks, linear scales, one scale y down for positions whose spans agree, bars from zero, an even sample past 50,000 rows, the core's number text; the display list (plot rect, marks in the chart's points, axes, ticks, titles, grid, accessibility outline). `array.plot` offered first for such a table and in `NATIVE`; `viewer.data {part: "chart", width, height}`.
- Apple `ChartView.swift`: the Plot viewer asks for the list at its size and paints it with Canvas; accessibility label, value and an `AXChartDescriptor`.
- egui (decision 4A): the Plot viewer paints the same list (`viewers::plot::paint`); its egui_plot plot retired; its two kittests (`a_table_plots_its_points_and_segments`, `a_plots_marks_are_on_screen`) pass unchanged. The registry test no longer excludes `array.plot`.
- Tests: chart unit tests (kinds, ticks, a histogram's columns from zero, positions one scale y down, hostile tables); conformance "a table of positions or a histogram is a chart the core lays out" (C ABI, egui, Mac 81 of 81 with none skipped, iPad 42 of 42 with 1 skipped); Swift `ChartListTests` (kit 125 passed); iPad UI test `testAHistogramIsTheCoresChart`; crates editor-core, ui, ffi, media, proto in the gate environment: 832 passed, 0 failed; workspace clippy clean.
- Launched: Mac scripted copy (`claude-debug/views/s5-mac/histogram-plot.png`, `points-plot.png`), iPad simulator (`s5-pad/views-s5-ui-histogram.png`); looked at each: the histogram's bars from zero with nice ticks, the points to one scale y down.
- Changed expectation: B152's histogram (a TABLE that is a table-flagged array) is now Plot first, then Table, as egui offers it.
- Not done: Windows painter for the display list; 1-D series and hover previews (slice 6).

## Slices 6 and 7 (branch `feat/views-s6`): series, the six chart kinds, and the gaps

- Slice 6, series: a 1-D array is `array.plot` (`chart::Kind::Series`), a line of value by index reduced by M4 (each pixel column's first, lowest, highest and last value, so every spike stays). Hover and face previews deferred: a face's picture is the engine's thumbnail; a core-drawn one needs the list made a picture on the media workers and the scene asking for it (note 26 12.1). Audio's (frames, channels) waits with it.
- Slice 7, the six kinds: `viewer.chart {popout, mark, x, y, color, size, path, agg, stack, bins, clear}` (session `chart_cmds.rs`), each column checked against the value's and its role's type, kept in the graph's `meta.charts["node.port"]`, one undo step, outside the result hash. `chart_kinds.rs` lays out column (sum by category, side by side or stacked by colour; count with no y), histogram (bins, stacked by colour), line (rows of one x made one point by `agg`; split by colour; stacked as bands), treemap (squarified, labels where they fit naming the first level). Colours from a ten-colour palette, a legend. `table.chart` ("Chart") offered after Table for a TABLE, CSV or rows; with no choice the core picks a column chart of the first measure by the first category.
- Apple: `ChartChoices` (kind, X, Y, Colour; Groups and Size for a treemap) as menus, each `viewer.chart`; the painter draws colours, areas, labels, legend, and no axes for a treemap.
- egui: `TableChart` paints the core's list with the spec the session keeps (the pane is given it); egui now offers what the core does for CSV (Table, Chart, Text, read by the shared reader) and SVG text (the picture from the text); the registry test covers both.
- SVG text longer than its excerpt: the SDKs (Rust and Python) now send the whole TEXT as `full` when the excerpt doesn't hold it (before, a TEXT between 4,096 characters and the inline limit reached the editor only as its first 4,096); the core reads that value's bytes as SVG; `popout.get` says `svgSource`. `docs/interface.md` 10 has the SHOULD.
- Windows (built on the VM in `C:\work\views-s7`, my own copy; not launched): `ChartViewer` paints the display list with Win2D (marks, colours, areas, labels, legend, axes, the y title turned), with the table chart's choices as drop-down buttons; the table and grid have Colour by Value (cells tinted from the core's `tints`) and the grid its Format; Compare has Difference (the core's words and pictures, mean or each channel). `dotnet build` ARM64: 0 warnings, 0 errors.
- Found on the Mac launch, fixed: (1) the pop-out's stamp didn't move when only the chart's spec changed (it lives in the graph's meta), so no shell knew to draw again; the stamp now hashes the spec, and the conformance case checks it (it fails without the fix). (2) the Apple chart view keyed on `box.popout.window.stamp`, which isn't observed, and the value set again unchanged drew nothing: `ViewerBox.stamp`, set by both apps with the value, is observed and the chart and YAML views key on it. The test that should have caught (2): an iPad UI test choosing a kind through the control port and reading the app's status (`chart.kind`) after; the status now carries the kind drawn. (3) a line through a table with several rows an x zigzagged through them: rows of one x are now one point by `agg`.
- Launched: Mac scripted copy, control port only (`claude-debug/views/mac-launch-s7.py`): `s7-mac-5/` (auto column, stacked column, line, stacked line, histogram, treemap, after undo; the app's status names each kind drawn), `s7-mac/svg-long-text.png` (9,946 characters drawn as a picture), `s6-mac/series-line.png` (153,600 bytes as a line, 1,713 points sent). Looked at each. Library unchanged every run.
- After the launch, also: the line chart combined the rows of one x by `agg` (it zigzagged through them), the treemap's nested labels name their first level (`north · 611.84`), and a kind sent with an x that doesn't fit it drops that x (the shells now send the spec drawn with their one change; a histogram with no numeric x takes the first numeric column). Conformance covers both.
- iPad UI test `testATablesChartIsChosenFromItsMenuAndFollowsAnUndo`: the kind menu's Stacked Column (a person's tap), then `edit.undo` through the control port, the app's status naming the kind drawn each time. Passed on my iPad simulator (`s7-pad/`; deleted after). It is the test that would have caught fault (2).
- Gate: C ABI conformance 6 of 6 (none skipped), egui conformance, Mac conformance 83 of 83 none skipped (`s7-gate/mac-conf3.log`), iPad read half 44 of 44 with 1 skipped, Swift kit 126 passed, touched crates 1,258 passed 0 failed (gate environment), workspace clippy clean, fmt clean, python 249 passed and the known `test_hash_is_what_was_asked`.
- Disk fell to 7 GB during the gate (my target's debug profile had reached 45 GB); two Mac cases failed then with "the store declined the value". I cleaned my debug profile (`cargo clean --profile dev`, 44.5 GB), rebuilt the engine programs, and the Mac suite passed 83 of 83. Free afterwards: 48 GB.
- Not done this slice: hover and face previews (deferred, above); the iPhone run of `testCompareDifference`; a probe on the difference picture; Windows launched (built only).

## Slice 8, part 1 (branch `feat/views-s8`): the difference's probe, and the iPhone

- `viewer.compare {probe: [row, col] | null}` (session state, in the Compare stamp); `compare.get`'s `difference` carries `rows`, `cols` and, when set, `probe` (each channel's A, B and |A − B| in the core's number text; `outside` past the arrays). Apple: a tap on a difference picture probes there, the reading under the words, Clear. Windows: a tap on a difference picture (built on the VM, not launched).
- Tests: `compare.rs` unit test (`probe_json`); conformance "Compare's exact difference…" with probe steps (C ABI passed; Mac 83 of 83 none skipped); iPad UI `testCompareDifference` with a tap on the difference: passed on my iPad simulator (`s8-pad/views-s8-ui-difference-probe.png`).
- iPhone `testCompareDifference`, stdin closed, on my iPhone 17 Pro simulator: Compare, Difference and the probe all ran (`s8-phone/views-s8-ui-difference-probe.png`, looked at: "(127, 191) value 0 − 0 = 0"); the test failed only on my assertion of exactly (128, 192) for the centre tap. The assertion now allows a pixel or two; the iPhone run is repeated in the slice 8 gate.
